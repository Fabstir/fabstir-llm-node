// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content-check settings and start-up rules (D12). There is no silent default: whenever video generation is
//! configured the mode must be stated, and a mode that cannot do its work stops the node.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// `LTX_CONTENT_CHECKS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// No check code runs (hosts outside the MVP, D20).
    Off,
    /// Run and log, refuse nothing; rejected after `shadow_until`.
    Shadow,
    /// Run and refuse (D1).
    Enforce,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Shadow => "shadow",
            Mode::Enforce => "enforce",
        }
    }
}

/// One renderer's input and output folders as the node sees them (D9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererDirs {
    pub input: PathBuf,
    pub output: PathBuf,
}

/// D11: one check's limit and a job's shared budget.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(120);
pub const JOB_BUDGET: Duration = Duration::from_secs(300);
/// This many service-class failures in a row, across jobs, open the breaker whatever the probe says.
pub const FAILURES_TO_TRIP: usize = 3;
/// The waiting period after such a trip.
pub const BREAKER_HOLD: Duration = Duration::from_secs(300);
/// D8: the staging area's size.
pub const STAGING_CAP_BYTES: u64 = 512 << 20;

#[derive(Debug, Clone)]
pub struct ChecksConfig {
    pub mode: Mode,
    /// Last day (UTC) on which `shadow` still accepts video jobs.
    pub shadow_until: Option<chrono::NaiveDate>,
    pub socket: Option<PathBuf>,
    /// The staging directory as the node sees it, and as the service sees it (D8).
    pub staging_node: Option<PathBuf>,
    pub staging_service: Option<PathBuf>,
    pub staging_cap_bytes: u64,
    pub comfy_dirs: Option<RendererDirs>,
    pub relight_dirs: Option<RendererDirs>,
    /// Compressed secp256k1 public key the strongest-signal items are sealed to (D15).
    pub evidence_pubkey: Option<[u8; 33]>,
    pub evidence_dir: Option<PathBuf>,
    pub refusals_file: Option<PathBuf>,
    /// The service bundle hash expected at deploy, without the `sha256:` prefix, lower case.
    pub bundle_hash: Option<String>,
    pub alert_url: Option<String>,
    pub check_timeout: Duration,
    pub job_budget: Duration,
    /// How often an open breaker re-probes the service.
    pub probe_interval: Duration,
    /// After a trip on consecutive failures (the probe was passing all along), the breaker stays open at least
    /// this long, so it does not open and close with every probe.
    pub breaker_hold: Duration,
}

impl ChecksConfig {
    /// The `off` configuration: nothing set, nothing runs.
    pub fn off() -> Self {
        ChecksConfig {
            mode: Mode::Off,
            shadow_until: None,
            socket: None,
            staging_node: None,
            staging_service: None,
            staging_cap_bytes: STAGING_CAP_BYTES,
            comfy_dirs: None,
            relight_dirs: None,
            evidence_pubkey: None,
            evidence_dir: None,
            refusals_file: None,
            bundle_hash: None,
            alert_url: None,
            check_timeout: CHECK_TIMEOUT,
            job_budget: JOB_BUDGET,
            probe_interval: Duration::from_secs(15),
            breaker_hold: BREAKER_HOLD,
        }
    }

    pub fn from_env(ltx_configured: bool, relight_configured: bool) -> Result<Self, String> {
        Self::from_lookup(ltx_configured, relight_configured, |k| {
            std::env::var(k).ok()
        })
    }

    /// Pure: the settings come through `get`, so every start-up rule is testable without the process environment.
    /// `ltx_configured` = a ComfyUI or relight sidecar is configured; `relight_configured` = the relight one is.
    pub fn from_lookup(
        ltx_configured: bool,
        relight_configured: bool,
        get: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let val = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let mut cfg = Self::off();
        if !ltx_configured {
            // no video generation on this node: the mode is not asked for and nothing runs
            return Ok(cfg);
        }
        // A renderer's folders count only as a PAIR. `COMFY_OUTPUT_DIR` on its own predates this release (the
        // production compose file has always set it), so alone it changes nothing; `enforce` requires both below.
        cfg.comfy_dirs = dir_pair(&val, "COMFY_INPUT_DIR", "COMFY_OUTPUT_DIR")?;
        cfg.relight_dirs = dir_pair(&val, "RELIGHT_INPUT_DIR", "RELIGHT_OUTPUT_DIR")?;
        cfg.mode = match val("LTX_CONTENT_CHECKS").as_deref() {
            Some("off") => Mode::Off,
            Some("shadow") => Mode::Shadow,
            Some("enforce") => Mode::Enforce,
            Some(other) => {
                return Err(format!(
                    "LTX_CONTENT_CHECKS={other:?} is not one of off, shadow, enforce"
                ))
            }
            None => {
                return Err(
                    "LTX_CONTENT_CHECKS must be set (off, shadow or enforce) whenever video generation is configured"
                        .to_string(),
                )
            }
        };
        if cfg.mode == Mode::Off {
            cfg.check_distinct(false)?;
            return Ok(cfg);
        }
        cfg.alert_url = val("LTX_CHECKS_ALERT_URL");
        if let Some(u) = &cfg.alert_url {
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return Err("LTX_CHECKS_ALERT_URL must be an http(s) URL".to_string());
            }
        }
        if let Some(raw) = val("LTX_CHECKS_STAGING_MAX_BYTES") {
            cfg.staging_cap_bytes = raw
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or("LTX_CHECKS_STAGING_MAX_BYTES must be a positive number of bytes")?;
        }
        let mode = cfg.mode.as_str();
        let need =
            |k: &str| val(k).ok_or_else(|| format!("LTX_CONTENT_CHECKS={mode} requires {k}"));
        // both modes run the checks, so both need the service and the shared staging area
        cfg.socket = Some(PathBuf::from(need("LTX_CHECKS_SOCKET")?));
        cfg.staging_node = Some(abs_dir(
            "LTX_CHECKS_STAGING_DIR",
            need("LTX_CHECKS_STAGING_DIR")?,
        )?);
        cfg.staging_service = Some(abs_dir(
            "LTX_CHECKS_STAGING_DIR_SERVICE",
            need("LTX_CHECKS_STAGING_DIR_SERVICE")?,
        )?);
        cfg.bundle_hash = val("LTX_CHECKS_BUNDLE_HASH")
            .map(|h| normalise_bundle_hash(&h))
            .transpose()?;
        cfg.evidence_pubkey = val("LTX_EVIDENCE_PUBKEY")
            .map(|k| super::seal::parse_public_key(&k))
            .transpose()?;
        cfg.evidence_dir = val("LTX_EVIDENCE_DIR")
            .map(|d| abs_dir("LTX_EVIDENCE_DIR", d))
            .transpose()?;
        // absolute, or the overlap rule below could not place it
        cfg.refusals_file = val("LTX_REFUSALS_FILE")
            .map(|f| abs_dir("LTX_REFUSALS_FILE", f))
            .transpose()?
            .or_else(|| cfg.evidence_dir.as_ref().map(|d| d.join("refusals.jsonl")));
        match cfg.mode {
            Mode::Shadow => {
                let raw = need("LTX_CONTENT_CHECKS_SHADOW_UNTIL")?;
                cfg.shadow_until = Some(
                    chrono::NaiveDate::parse_from_str(&raw, "%Y-%m-%d").map_err(|_| {
                        "LTX_CONTENT_CHECKS_SHADOW_UNTIL must be a date, YYYY-MM-DD".to_string()
                    })?,
                );
            }
            Mode::Enforce => {
                need("COMFY_INPUT_DIR")?;
                need("COMFY_OUTPUT_DIR")?;
                if relight_configured {
                    // D9 for the second renderer: without its folders a refused relight job's files would stay
                    need("RELIGHT_INPUT_DIR")?;
                    need("RELIGHT_OUTPUT_DIR")?;
                }
                need("LTX_EVIDENCE_PUBKEY")?;
                need("LTX_EVIDENCE_DIR")?;
                need("LTX_CHECKS_BUNDLE_HASH")?;
            }
            Mode::Off => {}
        }
        cfg.check_distinct(false)?;
        Ok(cfg)
    }

    /// The named folders this configuration uses.
    fn folders(&self) -> Vec<(&'static str, &Path)> {
        let mut out: Vec<(&'static str, &Path)> = Vec::new();
        if let Some(d) = &self.staging_node {
            out.push(("LTX_CHECKS_STAGING_DIR", d));
        }
        if let Some(d) = &self.evidence_dir {
            out.push(("LTX_EVIDENCE_DIR", d));
        }
        if let Some(d) = &self.comfy_dirs {
            out.push(("COMFY_INPUT_DIR", &d.input));
            out.push(("COMFY_OUTPUT_DIR", &d.output));
        }
        if let Some(d) = &self.relight_dirs {
            out.push(("RELIGHT_INPUT_DIR", &d.input));
            out.push(("RELIGHT_OUTPUT_DIR", &d.output));
        }
        out
    }

    /// Fix F9: the folders must be distinct and none inside another (the node sweeps one, deletes job files from
    /// others and keeps evidence in another: an overlap would have one of those jobs destroy another's files).
    /// The refusals file may live in the evidence directory (its default) but in none of the others. `real`
    /// compares the paths after resolving symlinks (start-up, when they exist); otherwise as written.
    pub fn check_distinct(&self, real: bool) -> Result<(), String> {
        let view = |p: &Path| -> PathBuf {
            if real {
                p.canonicalize().unwrap_or_else(|_| lexical(p))
            } else {
                lexical(p)
            }
        };
        let folders: Vec<(&str, PathBuf)> = self
            .folders()
            .into_iter()
            .map(|(n, p)| (n, view(p)))
            .collect();
        for (i, (a_name, a)) in folders.iter().enumerate() {
            for (b_name, b) in &folders[i + 1..] {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err(format!(
                        "{a_name} and {b_name} overlap: each must be a folder of its own, none inside another"
                    ));
                }
            }
        }
        if let Some(file) = &self.refusals_file {
            // the file itself may not exist yet: resolve its folder
            let file = match (file.parent(), file.file_name()) {
                (Some(dir), Some(name)) => view(dir).join(name),
                _ => view(file),
            };
            for (name, dir) in &folders {
                if *name != "LTX_EVIDENCE_DIR" && file.starts_with(dir) {
                    return Err(format!(
                        "LTX_REFUSALS_FILE lies inside {name}: it would be swept or deleted there"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Whether `shadow` has run past its end date (video jobs are then rejected, D12).
    pub fn shadow_expired(&self, today: chrono::NaiveDate) -> bool {
        self.mode == Mode::Shadow && self.shadow_until.is_some_and(|until| today > until)
    }

    /// Start-up access checks (D9, D12): every folder this mode writes to must exist and be writable, or the node
    /// refuses to start. Creates the evidence directory (owner only) when it is missing.
    pub fn verify_access(&self) -> Result<(), String> {
        // Fix F11: job-end deletion follows the folder settings, not the mode; so whenever a renderer's folders
        // are set (in ANY mode) the node must be able to delete there, or it does not start.
        for (name, dirs) in [("COMFY", &self.comfy_dirs), ("RELIGHT", &self.relight_dirs)] {
            if let Some(d) = dirs {
                writable(&d.input)
                    .map_err(|e| format!("{name}_INPUT_DIR {}: {e}", d.input.display()))?;
                writable(&d.output)
                    .map_err(|e| format!("{name}_OUTPUT_DIR {}: {e}", d.output.display()))?;
            }
        }
        if self.mode != Mode::Off {
            if let Some(d) = &self.staging_node {
                writable(d).map_err(|e| format!("LTX_CHECKS_STAGING_DIR {}: {e}", d.display()))?;
            }
            if let Some(d) = &self.evidence_dir {
                create_private_dir(d)
                    .and_then(|_| writable(d))
                    .map_err(|e| format!("LTX_EVIDENCE_DIR {}: {e}", d.display()))?;
            }
        }
        // again with symlinks resolved, now that the folders exist
        self.check_distinct(true)
    }
}

/// `p` without `.` and with `..` resolved as written (no filesystem access).
fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn dir_pair(
    val: &impl Fn(&str) -> Option<String>,
    input: &str,
    output: &str,
) -> Result<Option<RendererDirs>, String> {
    match (val(input), val(output)) {
        (Some(i), Some(o)) => Ok(Some(RendererDirs {
            input: abs_dir(input, i)?,
            output: abs_dir(output, o)?,
        })),
        _ => Ok(None),
    }
}

fn abs_dir(name: &str, raw: String) -> Result<PathBuf, String> {
    let p = PathBuf::from(raw);
    if !p.is_absolute() {
        return Err(format!("{name} must be an absolute path"));
    }
    Ok(p)
}

/// `sha256:<64 hex>` or bare 64 hex, compared lower case without the prefix.
pub fn normalise_bundle_hash(raw: &str) -> Result<String, String> {
    let h = raw.trim().to_ascii_lowercase();
    let h = h.strip_prefix("sha256:").unwrap_or(&h).to_string();
    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(
            "a bundle hash must be 64 hex characters (optionally prefixed sha256:)".to_string(),
        );
    }
    Ok(h)
}

fn create_private_dir(d: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !d.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(d)
            .map_err(|e| e.to_string())?;
    }
    std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())
}

/// Prove write access by creating and removing a file.
pub fn writable(d: &Path) -> Result<(), String> {
    if !d.is_dir() {
        return Err("is not a directory".to_string());
    }
    let probe = d.join(format!(".write-probe-{}", uuid::Uuid::new_v4()));
    std::fs::write(&probe, b"").map_err(|e| format!("is not writable ({e})"))?;
    std::fs::remove_file(&probe).map_err(|e| format!("cannot delete in it ({e})"))
}
