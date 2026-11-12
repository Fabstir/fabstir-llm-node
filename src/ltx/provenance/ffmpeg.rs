// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! D3: the fingerprint of a delivered picture, computed by the PINNED ffmpeg's MPEG-7 `signature` filter.
//!
//! The binary path comes from `PROVENANCE_FFMPEG` and must match [`FFMPEG_SHA256`] (the static ffmpeg 7.0.2
//! GPL build AP0.1b measured with; `vf_signature` needs `--enable-gpl`, so it ships as a separate executable
//! the node runs, never links). The subprocess follows `src/tee/gpu_evidence.rs`: empty environment, null
//! stdin, timeout, kill on drop, a per-call scratch directory. A missing binary, a wrong hash, a non-zero
//! exit or an empty signature is an error: the job fails closed (D3, T8).

use anyhow::{anyhow, bail, Context, Result};
use fabstir_provenance::fingerprint::{parse_ffmpeg_xml, SignatureSet, FFMPEG_FILTER};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ENV: &str = "PROVENANCE_FFMPEG";
/// sha256 of `ffmpeg-7.0.2-amd64-static/ffmpeg` (johnvansickle release, AP0.1b).
pub const FFMPEG_SHA256: &str = "e7e7fb30477f717e6f55f9180a70386c62677ef8a4d4d1a5d948f4098aa3eb99";
pub const TIMEOUT: Duration = Duration::from_secs(300);

/// Remove scratch directories a killed process left behind (they hold decrypted outputs). Only those older
/// than an hour: a live fingerprint (at most [`TIMEOUT`]) of another process sharing the temp dir is left alone.
/// Called once, at node startup.
pub fn sweep_scratch() {
    let cutoff = std::time::SystemTime::now() - Duration::from_secs(3600);
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t < cutoff)
                .unwrap_or(false);
            if old && e.file_name().to_string_lossy().starts_with("prov-fp-") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Ffmpeg {
    pub path: PathBuf,
}

impl Ffmpeg {
    /// From `PROVENANCE_FFMPEG`, checked: exists, pinned hash, has the `signature` filter (D18).
    pub async fn from_env() -> Result<Ffmpeg> {
        let p = std::env::var_os(ENV).ok_or_else(|| anyhow!("{ENV} is not set"))?;
        let f = Ffmpeg {
            path: PathBuf::from(p),
        };
        f.check().await?;
        Ok(f)
    }

    pub async fn check(&self) -> Result<()> {
        let bytes = tokio::fs::read(&self.path)
            .await
            .with_context(|| format!("{ENV}={}", self.path.display()))?;
        let h = hex::encode(Sha256::digest(&bytes));
        if h != FFMPEG_SHA256 {
            bail!("{ENV} sha256 {h} is not the pinned {FFMPEG_SHA256}");
        }
        let out = self
            .command(&["-hide_banner", "-h", "filter=signature"], Path::new("/"))
            .output();
        let out = tokio::time::timeout(Duration::from_secs(30), out)
            .await
            .map_err(|_| anyhow!("ffmpeg -h timed out"))??;
        if !out.status.success()
            || !String::from_utf8_lossy(&out.stdout).contains("MPEG-7 video signature")
        {
            bail!("ffmpeg has no signature filter");
        }
        Ok(())
    }

    fn command(&self, args: &[&str], cwd: &Path) -> tokio::process::Command {
        let mut c = tokio::process::Command::new(&self.path);
        c.args(args)
            .env_clear()
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        c
    }

    /// The signature set of a whole mp4 (no duration limit: the filter writes its file on the natural end of input).
    pub async fn fingerprint(&self, mp4: &[u8]) -> Result<SignatureSet> {
        let dir = tempfile::Builder::new()
            .prefix("prov-fp-")
            .tempdir()
            .context("scratch dir")?;
        let d = dir.path();
        if !d
            .to_string_lossy()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
        {
            bail!("scratch path not safe for an ffmpeg filter argument");
        }
        let input = d.join("in.mp4");
        let xml = d.join("sig.xml");
        tokio::fs::write(&input, mp4).await?;
        let filter = FFMPEG_FILTER.replace("{out}", &xml.to_string_lossy());
        let args = [
            "-nostdin",
            "-v",
            "error",
            "-i",
            &input.to_string_lossy(),
            "-vf",
            &filter,
            "-f",
            "null",
            "-",
        ]
        .map(|s| s.to_string());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = tokio::time::timeout(TIMEOUT, self.command(&args, d).output())
            .await
            .map_err(|_| anyhow!("ffmpeg timed out"))??;
        if !out.status.success() {
            bail!(
                "ffmpeg exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .last()
                    .unwrap_or("")
            );
        }
        let text = tokio::fs::read_to_string(&xml)
            .await
            .context("ffmpeg wrote no signature file")?;
        Ok(parse_ffmpeg_xml(&text)?)
    }
}
