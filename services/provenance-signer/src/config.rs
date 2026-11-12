// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Signer configuration (JSON file). Secrets are never in it: the key passphrase and the detector's index
//! secret are systemd credentials read from `$CREDENTIALS_DIRECTORY` (or explicit paths in tests).

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Host API (reserve, sign, sign-sequence, limits): behind TLS termination.
    pub listen_api: String,
    /// Public read-only repository listener (`GET /v1/manifests/{id}`).
    pub listen_public: String,
    /// Internal index listener for the detector (firewalled to its IP; shared secret).
    pub listen_index: String,
    pub data_dir: PathBuf,
    pub key_path: PathBuf,
    /// File holding the key passphrase (default: `$CREDENTIALS_DIRECTORY/signing-key-passphrase`).
    pub key_passphrase_path: Option<PathBuf>,
    pub chain_path: PathBuf,
    /// Bumped at every certificate swap (nodes redo their placeholder when it moves).
    pub chain_epoch: u64,
    /// Trust anchors (PEM): our root (verify_after_sign) plus the TSAs' roots (the canary's `timeStamp.trusted`).
    pub trust_anchors_path: PathBuf,
    /// Exact node versions allowed into the claim (the node's `crate::version::VERSION` of each release).
    pub node_versions: Vec<String>,
    /// Authenticated requests per host per minute, all routes.
    #[serde(default = "default_requests")]
    pub requests_per_minute: u64,
    pub tsa_urls: Vec<String>,
    #[serde(default = "default_tsa_timeout")]
    pub tsa_timeout_secs: u64,
    /// Allow-listed host addresses (0x-hex): Fabstir-vetted hosts only.
    pub allowlist: Vec<String>,
    pub informational_url: String,
    #[serde(default = "default_reserve_min")]
    pub reserve_min: usize,
    #[serde(default = "default_reserve_max")]
    pub reserve_max: usize,
    #[serde(default = "default_reserve_recommended")]
    pub reserve_recommended: usize,
    /// Bundles before this leaked ComfyUI metadata (v48): refused at reserve.
    #[serde(default = "default_min_bundle")]
    pub min_bundle_version: u32,
    /// Reservations per host per minute (T9: host concurrency x the largest Batch burst).
    #[serde(default = "default_rate")]
    pub reservations_per_minute: u64,
    /// Days the log of requests is kept (the `audit`, `reservations` and `slots` tables: which host asked for
    /// which signature). Older rows are deleted at start-up and once a day. The repository of records is never
    /// pruned. Default 365.
    #[serde(default = "default_request_log_retention_days")]
    pub request_log_retention_days: u64,
    /// Detector source IPs allowed on the index listener.
    pub index_allowed_ips: Vec<String>,
    /// File holding the index secret (default: `$CREDENTIALS_DIRECTORY/index-secret`).
    pub index_secret_path: Option<PathBuf>,
    /// ntfy topic URL for alarms (canary, reconciliation, signer health).
    pub ntfy_url: Option<String>,
    /// Reconciliation: JSON-RPC endpoint, JobMarketplace address, blocks per window.
    pub reconcile: Option<ReconcileConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileConfig {
    pub rpc_url: String,
    pub job_marketplace: String,
    #[serde(default = "default_window_blocks")]
    pub window_blocks: u64,
    #[serde(default = "default_chunk_blocks")]
    pub chunk_blocks: u64,
    /// Proofs and signatures straddling the window edges, unpaid probes and jobs that failed after signing
    /// make small differences normal; only a larger difference alarms.
    #[serde(default = "default_tolerance")]
    pub tolerance: u64,
}

fn default_tsa_timeout() -> u64 {
    10
}
fn default_reserve_min() -> usize {
    9_216
}
fn default_reserve_max() -> usize {
    16_384
}
fn default_reserve_recommended() -> usize {
    12_000
}
fn default_min_bundle() -> u32 {
    49
}
fn default_request_log_retention_days() -> u64 {
    365
}
/// Shorter than this would cut into live reservations and the daily reconciliation's window.
pub const MIN_REQUEST_LOG_RETENTION_DAYS: u64 = 7;
fn default_rate() -> u64 {
    120
}
fn default_requests() -> u64 {
    1_200
}
fn default_tolerance() -> u64 {
    2
}
fn default_window_blocks() -> u64 {
    43_200
}
fn default_chunk_blocks() -> u64 {
    5_000
}

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Config> {
        let c: Config = serde_json::from_slice(&std::fs::read(path)?)?;
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.reserve_min <= self.reserve_recommended
                && self.reserve_recommended <= self.reserve_max,
            "reserve range"
        );
        anyhow::ensure!(
            !self.tsa_urls.is_empty(),
            "at least one TSA (two in production)"
        );
        anyhow::ensure!(!self.allowlist.is_empty(), "empty allow-list");
        for a in &self.allowlist {
            anyhow::ensure!(
                fabstir_provenance::contract::parse_address(a).is_some(),
                "bad allow-list address {a}"
            );
        }
        anyhow::ensure!(
            self.informational_url.starts_with("https://"),
            "informational_url must be https"
        );
        anyhow::ensure!(!self.node_versions.is_empty(), "node_versions is empty");
        anyhow::ensure!(
            self.request_log_retention_days >= MIN_REQUEST_LOG_RETENTION_DAYS,
            "request_log_retention_days must be at least {MIN_REQUEST_LOG_RETENTION_DAYS}"
        );
        anyhow::ensure!(
            !self.index_allowed_ips.is_empty(),
            "index_allowed_ips is empty (the detector could never query)"
        );
        if let Some(r) = &self.reconcile {
            anyhow::ensure!(
                r.chunk_blocks >= 1 && r.window_blocks >= 1,
                "reconcile window and chunk must be >= 1"
            );
        }
        Ok(())
    }

    fn credential(explicit: &Option<PathBuf>, name: &str) -> anyhow::Result<PathBuf> {
        if let Some(p) = explicit {
            return Ok(p.clone());
        }
        let dir = std::env::var_os("CREDENTIALS_DIRECTORY").ok_or_else(|| {
            anyhow::anyhow!(
                "no {name}: set CREDENTIALS_DIRECTORY (systemd LoadCredentialEncrypted=)"
            )
        })?;
        Ok(PathBuf::from(dir).join(name))
    }

    pub fn passphrase_path(&self) -> anyhow::Result<PathBuf> {
        Self::credential(&self.key_passphrase_path, "signing-key-passphrase")
    }

    pub fn index_secret_file(&self) -> anyhow::Result<PathBuf> {
        Self::credential(&self.index_secret_path, "index-secret")
    }
}
