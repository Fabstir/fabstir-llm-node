// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Retention of delivered results. A video job's encrypted frames and manifest are written through the S5 bridge
//! to `home/ltx/job_<tag>/…` on the storage portal, for the client to collect. Nothing used to delete them. Once
//! at start and once a day this asks the bridge to delete the entries older than `LTX_RESULT_RETENTION_DAYS`
//! (default 30; 0 disables) and to release their data on the portal. The bridge owns the store and does the work
//! (`POST /s5/prune`); the node supplies the period and the names of the jobs it is working on, which are never
//! touched. Log lines carry counts only.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The tree a video job's results are written under.
pub const RESULT_PREFIX: &str = "home/ltx";
pub const DEFAULT_DAYS: u64 = 30;
/// At most this many job entries are deleted per run (the bridge enforces the same ceiling).
pub const MAX_ENTRIES_PER_RUN: u64 = 500;
/// One run's HTTP bound: a full run is a few thousand small calls from the bridge to the portal.
const RUN_BOUND: Duration = Duration::from_secs(1800);
const DAILY: Duration = Duration::from_secs(86_400);
/// When a run stopped at its ceiling the next comes sooner, so a backlog drains in hours, not months.
const BACKLOG: Duration = Duration::from_secs(3600);

/// `LTX_RESULT_RETENTION_DAYS`: `Ok(None)` = disabled (0), `Ok(Some(days))`, or an error that stops the node.
pub fn days_from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Option<u64>, String> {
    match get("LTX_RESULT_RETENTION_DAYS")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        None => Ok(Some(DEFAULT_DAYS)),
        Some(raw) => match raw.parse::<u64>() {
            Ok(0) => Ok(None),
            Ok(d) if d <= 3650 => Ok(Some(d)),
            _ => Err(
                "LTX_RESULT_RETENTION_DAYS must be a whole number of days from 0 (never delete) to 3650"
                    .to_string(),
            ),
        },
    }
}

fn live() -> &'static Mutex<HashMap<String, usize>> {
    static LIVE: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Held by a job from the moment its result path is chosen until the job ends, however it ends.
#[derive(Debug)]
pub struct InFlight(String);

/// Mark `job_tag` (the `<tag>` of `home/ltx/job_<tag>`) as being worked on.
pub fn hold(job_tag: &str) -> InFlight {
    if let Ok(mut g) = live().lock() {
        *g.entry(job_tag.to_string()).or_insert(0) += 1;
    }
    InFlight(job_tag.to_string())
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if let Ok(mut g) = live().lock() {
            match g.get_mut(&self.0) {
                Some(n) if *n > 1 => *n -= 1,
                _ => {
                    g.remove(&self.0);
                }
            }
        }
    }
}

/// The entry names of the jobs in flight, as the bridge sees them (`job_<tag>`), sorted.
pub fn in_flight_entries() -> Vec<String> {
    let mut out: Vec<String> = live()
        .lock()
        .map(|g| g.keys().map(|t| format!("job_{t}")).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// What one run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Counts {
    #[serde(default)]
    pub scanned: u64,
    #[serde(default)]
    pub deleted: u64,
    #[serde(default)]
    pub files: u64,
    #[serde(default)]
    pub released: u64,
    #[serde(default)]
    pub failed: u64,
    #[serde(default)]
    pub skipped: u64,
    /// The run stopped at its ceiling with older entries left.
    #[serde(default)]
    pub more: bool,
}

/// The request one run sends: the fixed tree, the period, the ceiling, and the jobs in flight.
pub fn request_body(days: u64, exclude: &[String]) -> serde_json::Value {
    serde_json::json!({
        "prefix": RESULT_PREFIX,
        "olderThanSecs": days.saturating_mul(86_400),
        "maxEntries": MAX_ENTRIES_PER_RUN,
        "exclude": exclude,
    })
}

/// One bounded run against the bridge at `bridge_url`.
pub async fn prune_once(
    http: &reqwest::Client,
    bridge_url: &str,
    days: u64,
) -> Result<Counts, String> {
    if days == 0 {
        return Err("retention is disabled".to_string());
    }
    let url = format!("{}/s5/prune", bridge_url.trim_end_matches('/'));
    let body = request_body(days, &in_flight_entries());
    let call = async {
        let res = http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("bridge unreachable ({})", kind_of(&e)))?;
        let status = res.status();
        if !status.is_success() {
            return Err(format!("bridge answered {status}"));
        }
        res.json::<Counts>()
            .await
            .map_err(|_| "bridge answer was not the expected counts".to_string())
    };
    tokio::time::timeout(RUN_BOUND, call)
        .await
        .map_err(|_| "run timed out".to_string())?
}

fn kind_of(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else {
        "request"
    }
}

/// Run now and then daily, for the life of the process. `days` of `None` starts nothing.
pub fn spawn(bridge_url: String, days: Option<u64>) -> Option<tokio::task::JoinHandle<()>> {
    let days = days?;
    Some(tokio::spawn(async move {
        let http = reqwest::Client::new();
        loop {
            let wait = match prune_once(&http, &bridge_url, days).await {
                Ok(c) => {
                    tracing::info!(
                        "ltx result retention ({days} days): scanned={} deleted={} files={} released={} failed={} skipped={} more={}",
                        c.scanned, c.deleted, c.files, c.released, c.failed, c.skipped, c.more
                    );
                    if c.more {
                        BACKLOG
                    } else {
                        DAILY
                    }
                }
                Err(why) => {
                    tracing::warn!("ltx result retention ({days} days): run failed: {why}");
                    DAILY
                }
            };
            tokio::time::sleep(wait).await;
        }
    }))
}
