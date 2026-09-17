// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 (P2.4) — NVIDIA GPU evidence collection on the node.
//!
//! The node COLLECTS GPU evidence and never verifies it. Collection is NVIDIA's
//! own code (`nv-local-gpu-verifier`, `cc_admin.collect_gpu_evidence_remote`),
//! which is Python and needs `pynvml` against the guest driver, so it runs as a
//! subprocess: `python3 collect_gpu_evidence.py <nonce_hex>` (shipped in the
//! Phala image, `deployment/phala/collect_gpu_evidence.py`) prints one JSON
//! object in the shape Phala's reference node ships to its relying party:
//!
//! ```text
//! {"nonce": "<hex>", "evidence_list": [{"certificate", "evidence", "arch"}], "arch": "HOPPER"}
//! ```
//!
//! The 32-byte challenge nonce goes in verbatim; nvtrust embeds it in the
//! hardware-signed attestation report, and the broker's NRAS verification of
//! that report against the same nonce is the whole cross-binding to the CPU
//! quote (see `types::report_data`).
//!
//! **Test mode.** `TEE_GPU_EVIDENCE=canned` makes the script return nvtrust's
//! `test_no_gpu` sample evidence, whose report carries nvtrust's FIXED nonce,
//! not ours. The payload is labelled `"canned": true` and this module refuses a
//! payload whose label disagrees with the configured mode in either direction,
//! so a canned payload can never leave a node that thinks it is collecting real
//! evidence, and a real payload is never mislabelled as canned. Only a broker
//! started with `KBS_GPU_EVIDENCE=canned` (test keyring) accepts the label.
//! Gate A-19 keeps the flag out of the GPU compose.

use crate::tee::types::{TeeError, TeeResult};
use rand::RngCore;
use std::path::PathBuf;
use std::time::Duration;

/// Where the collector script lives in the Phala image.
pub const DEFAULT_SCRIPT: &str = "/usr/local/bin/collect_gpu_evidence.py";
/// Env var naming the script (set by the image; overridable for tests).
pub const SCRIPT_ENV: &str = "TEE_GPU_EVIDENCE_SCRIPT";
/// Env var selecting the mode: unset / `real` / `canned`.
pub const MODE_ENV: &str = "TEE_GPU_EVIDENCE";

/// Real hardware evidence, or nvtrust's canned sample (test keyring only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuEvidenceMode {
    Real,
    Canned,
}

impl GpuEvidenceMode {
    /// Parse the env var's spelling; anything but unset/`real`/`canned` is an
    /// error, never a default.
    pub fn parse(s: Option<&str>) -> TeeResult<Self> {
        match s.map(str::trim) {
            None | Some("") | Some("real") => Ok(Self::Real),
            Some("canned") => Ok(Self::Canned),
            Some(other) => Err(TeeError::GpuEvidence(format!(
                "{MODE_ENV} must be unset, 'real' or 'canned'; got {other:?}"
            ))),
        }
    }

    fn env_value(self) -> &'static str {
        match self {
            Self::Real => "real",
            Self::Canned => "canned",
        }
    }
}

/// Runs the collector script and validates its output.
#[derive(Debug, Clone)]
pub struct GpuEvidenceCollector {
    python: PathBuf,
    script: PathBuf,
    mode: GpuEvidenceMode,
    timeout: Duration,
}

impl GpuEvidenceCollector {
    pub fn new(
        python: impl Into<PathBuf>,
        script: impl Into<PathBuf>,
        mode: GpuEvidenceMode,
        timeout: Duration,
    ) -> Self {
        Self {
            python: python.into(),
            script: script.into(),
            mode,
            timeout,
        }
    }

    /// From the image's environment: `TEE_GPU_EVIDENCE_SCRIPT` (default
    /// [`DEFAULT_SCRIPT`]) and `TEE_GPU_EVIDENCE` (default real). Fails if the
    /// script is missing, so a mis-built image is refused at startup, not at the
    /// first attestation.
    pub fn from_env() -> TeeResult<Self> {
        let script = std::env::var(SCRIPT_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SCRIPT));
        if !script.is_file() {
            return Err(TeeError::GpuEvidence(format!(
                "collector script not found at {} (set {SCRIPT_ENV})",
                script.display()
            )));
        }
        let mode = GpuEvidenceMode::parse(std::env::var(MODE_ENV).ok().as_deref())?;
        if mode == GpuEvidenceMode::Canned {
            tracing::warn!(
                target: "tee",
                "CRITICAL: {MODE_ENV}=canned — GPU evidence will be nvtrust's canned sample with a FIXED nonce; \
                 only a test-keyring broker accepts it. Never on the GPU CVM."
            );
        }
        Ok(Self::new("python3", script, mode, Duration::from_secs(120)))
    }

    pub fn mode(&self) -> GpuEvidenceMode {
        self.mode
    }

    /// Collect evidence for `nonce`. Returns the payload bytes exactly as the
    /// script printed them (the broker forwards them to NRAS byte-for-byte), after
    /// checking they are one JSON object with our nonce, a non-empty
    /// `evidence_list`, and a `canned` label consistent with the mode.
    pub async fn collect(&self, nonce: &[u8; 32]) -> TeeResult<Vec<u8>> {
        let nonce_hex = hex::encode(nonce);
        // nvtrust's config.py removes and re-creates `verifier.log` in the CWD at
        // import time, so two overlapping collections in one directory would race
        // on it; each run gets its own scratch directory, removed afterwards. The
        // name is per CALL (random), never derived from the nonce: two concurrent
        // collections for the same nonce would otherwise delete each other's CWD
        // (converge round 8: the suite's shared test nonce hit exactly that).
        let mut rnd = [0u8; 8];
        rand::rngs::OsRng.fill_bytes(&mut rnd);
        let scratch = std::env::temp_dir().join(format!(
            "tee-gpu-evidence-{}-{}",
            std::process::id(),
            hex::encode(rnd)
        ));
        std::fs::create_dir_all(&scratch).map_err(|e| {
            TeeError::GpuEvidence(format!("scratch dir {}: {e}", scratch.display()))
        })?;
        let result = self.run(&nonce_hex, &scratch).await;
        let _ = std::fs::remove_dir_all(&scratch);
        result
    }

    async fn run(&self, nonce_hex: &str, scratch: &std::path::Path) -> TeeResult<Vec<u8>> {
        let mut cmd = tokio::process::Command::new(&self.python);
        cmd.arg(&self.script)
            .arg(nonce_hex)
            // A third-party Python process (nvtrust + pynvml) gets an EMPTY
            // environment plus exactly what it needs: never HOST_PRIVATE_KEY, the
            // broker URL or anything else the node holds.
            .env_clear()
            .env(
                "PATH",
                std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()),
            )
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env(MODE_ENV, self.mode.env_value())
            // A writable per-call CWD (see `collect`): a read-only or non-root
            // container CWD would otherwise fail every attestation with an
            // import-time PermissionError from nvtrust's log file.
            .current_dir(scratch)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let output = tokio::time::timeout(self.timeout, cmd.output())
            .await
            .map_err(|_| {
                TeeError::GpuEvidence(format!("collector timed out after {:?}", self.timeout))
            })?
            .map_err(|e| TeeError::GpuEvidence(format!("spawn {}: {e}", self.python.display())))?;

        // The script prints its NVML pre-flight (cc_enabled / ppcie / devtools)
        // and every refusal reason on stderr; keep it in the log either way.
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines().filter(|l| !l.trim().is_empty()) {
            tracing::info!(target: "tee", "gpu-evidence: {line}");
        }
        if !output.status.success() {
            let reason = stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            return Err(TeeError::GpuEvidence(format!(
                "collector exited {}: {reason}",
                output.status
            )));
        }

        // The payload is the LAST non-empty stdout line. The script routes
        // nvtrust's own logging (a StreamHandler on stdout) to stderr before the
        // import, but a library that prints via a saved handle would still land
        // ahead of the JSON; taking the last line makes that harmless either way.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let payload_line = stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .ok_or_else(|| TeeError::GpuEvidence("collector printed nothing".into()))?
            .trim()
            .to_string();
        let value: serde_json::Value = serde_json::from_str(&payload_line)
            .map_err(|e| TeeError::GpuEvidence(format!("collector output is not JSON: {e}")))?;
        let got_nonce = value.get("nonce").and_then(|v| v.as_str()).unwrap_or("");
        if got_nonce.to_ascii_lowercase() != nonce_hex {
            return Err(TeeError::GpuEvidence(
                "collector payload nonce is not the challenge nonce".into(),
            ));
        }
        let n = value
            .get("evidence_list")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if n == 0 {
            return Err(TeeError::GpuEvidence(
                "collector payload has no evidence".into(),
            ));
        }
        let labelled_canned = value
            .get("canned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        match (self.mode, labelled_canned) {
            (GpuEvidenceMode::Real, true) => {
                return Err(TeeError::GpuEvidence(
                    "collector returned CANNED evidence in real mode; refusing".into(),
                ))
            }
            (GpuEvidenceMode::Canned, false) => {
                return Err(TeeError::GpuEvidence(
                    "canned mode but the payload is not labelled canned; refusing".into(),
                ))
            }
            (GpuEvidenceMode::Canned, true) => tracing::warn!(
                target: "tee",
                "CRITICAL: GPU evidence is CANNED (nvtrust sample, fixed nonce); test keyring only"
            ),
            (GpuEvidenceMode::Real, false) => {}
        }
        Ok(payload_line.into_bytes())
    }
}
