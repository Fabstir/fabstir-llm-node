// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks (N10) test support: the rig (a configured checker over temporary folders with the stub service
//! behind it) and a log capture. The service's view of the staging area is a DIFFERENT path from the node's (a
//! symlink to the same folder), as on a host where the two containers mount it at different places: a node that
//! hands the service its own path is refused by the stub.
#![allow(dead_code)]

use super::checks_svc::{spawn_service, CheckSvc, BUNDLE};
use fabstir_llm_node::ltx::checks::alerts::RecordingAlerts;
use fabstir_llm_node::ltx::checks::config::{ChecksConfig, Mode, RendererDirs};
use fabstir_llm_node::ltx::checks::staging::Staging;
use fabstir_llm_node::ltx::checks::{seal, ContentChecks};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A configured checker over temporary folders, with the stub service behind it.
pub struct Rig {
    pub tmp: tempfile::TempDir,
    pub svc: Arc<CheckSvc>,
    pub checks: ContentChecks,
    pub alerts: Arc<RecordingAlerts>,
    /// The evidence private key (never given to the node side).
    pub secret: [u8; 32],
}

impl Rig {
    pub fn dir(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }
    /// Files left in a folder (recursive, regular files and symlinks).
    pub fn files_in(&self, name: &str) -> Vec<String> {
        fn walk(d: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    walk(&e.path(), out);
                } else {
                    out.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
        let mut out = vec![];
        walk(&self.dir(name), &mut out);
        out
    }
    /// Whether the staging area is empty, allowing for a recovery probe in flight: once a breaker opens, the
    /// node re-probes in the background and its built-in clip sits in staging until that probe is answered or
    /// times out (2 s here). A job's own staged item is gone before the job ends, so a leak never drains.
    pub async fn staging_drained(&self) -> bool {
        for _ in 0..60 {
            if self.files_in("staging").is_empty() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }
    /// The refusals file, one record per line.
    pub fn refusals(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir("evidence").join("refusals.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    pub fn alert_count(&self, kind: &str) -> usize {
        self.alerts.kinds().iter().filter(|k| *k == kind).count()
    }
}

/// The rig's settings: 2 s per check, 5 s for a job's prompt and inputs, no background re-probe inside a test's
/// lifetime. `svc-view` is the service's path to the staging folder.
pub fn rig_config(tmp: &std::path::Path, mode: Mode, pubkey: [u8; 33]) -> ChecksConfig {
    for d in ["staging", "in", "out", "evidence"] {
        std::fs::create_dir_all(tmp.join(d)).unwrap();
    }
    std::os::unix::fs::symlink(tmp.join("staging"), tmp.join("svc-view")).unwrap();
    let mut cfg = ChecksConfig::off();
    cfg.mode = mode;
    cfg.shadow_until = (mode == Mode::Shadow).then(|| chrono::Utc::now().date_naive());
    cfg.socket = Some(tmp.join("mod.sock"));
    cfg.staging_node = Some(tmp.join("staging"));
    cfg.staging_service = Some(tmp.join("svc-view"));
    cfg.comfy_dirs = Some(RendererDirs {
        input: tmp.join("in"),
        output: tmp.join("out"),
    });
    cfg.evidence_pubkey = Some(pubkey);
    cfg.evidence_dir = Some(tmp.join("evidence"));
    cfg.refusals_file = Some(tmp.join("evidence").join("refusals.jsonl"));
    cfg.bundle_hash = Some(BUNDLE.to_string());
    cfg.check_timeout = Duration::from_secs(2);
    cfg.job_budget = Duration::from_secs(5);
    cfg.probe_interval = Duration::from_secs(3600);
    cfg
}

/// Build the rig; `ready` runs the start-up probe so the breaker is closed. `staging` adjusts the staging area
/// (the slow-write seam).
pub async fn rig_full(
    mode: Mode,
    ready: bool,
    tweak: impl FnOnce(&mut ChecksConfig),
    staging: impl FnOnce(Staging) -> Staging,
) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let secret: [u8; 32] = k256::SecretKey::random(&mut rand::thread_rng())
        .to_bytes()
        .into();
    let mut cfg = rig_config(tmp.path(), mode, seal::public_key_of(&secret).unwrap());
    tweak(&mut cfg);
    cfg.verify_access().expect("the rig's folders are usable");
    let svc = spawn_service(tmp.path().join("mod.sock"), tmp.path().join("svc-view"));
    let alerts = Arc::new(RecordingAlerts::default());
    let checks = ContentChecks::with_staging(cfg, alerts.clone(), staging);
    if ready {
        assert!(
            checks.probe_now().await,
            "the start-up probe passes against the stub"
        );
    }
    Rig {
        tmp,
        svc,
        checks,
        alerts,
        secret,
    }
}

pub async fn rig_with(mode: Mode, ready: bool, tweak: impl FnOnce(&mut ChecksConfig)) -> Rig {
    rig_full(mode, ready, tweak, |s| s).await
}

pub async fn rig(mode: Mode) -> Rig {
    rig_with(mode, true, |_| {}).await
}

/// Capture this test thread's log lines (the test runtime is single-threaded, so every task logs here). Which
/// check ended a job is told to the log, not to the client: tests read it here.
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Logs {
    /// Start capturing; keep the guard alive for the test.
    pub fn capture() -> (Logs, tracing::subscriber::DefaultGuard) {
        // Tracing caches, per call site and for the whole process, whether anyone listens. A capture lives on
        // one test thread only, so a call site first reached on another thread while no capture existed could be
        // cached as unheard. A process-wide listener that discards everything keeps every call site live.
        static LISTENING: std::sync::Once = std::sync::Once::new();
        LISTENING.call_once(|| {
            let quiet = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_writer(std::io::sink)
                .finish();
            let _ = tracing::subscriber::set_global_default(quiet);
        });
        let logs = Logs::default();
        let sink = logs.clone();
        let sub = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || sink.clone())
            .finish();
        (logs, tracing::subscriber::set_default(sub))
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
    /// Whether a check's log line names this stage and this ending (`verdict=NONE cause=Timeout`,
    /// `refused=BLOCK_POLICY`, ...).
    pub fn has(&self, stage: &str, ending: &str) -> bool {
        self.text().lines().any(|l| {
            l.contains("ltx-check") && l.contains(&format!("stage={stage} ")) && l.contains(ending)
        })
    }
}
