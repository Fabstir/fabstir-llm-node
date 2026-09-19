// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Broker configuration (design §2): every `KBS_*` variable, read once at start,
//! malformed = refuse to start (exit 78, `RestartPreventExitStatus=78`).
//!
//! [`KbsConfig::from_map`] is the seam the tests drive; [`KbsConfig::from_env`]
//! feeds it the process environment. The broker never reads `HOST_TEE_ENABLED`,
//! `TEE_*` or the node's env.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

/// Exit status for a config, keyring, permission or mode refusal (design D17).
/// Every other start failure exits 1 and restarts.
pub const EXIT_REFUSED: i32 = 78;

/// Intel's two hosts dcap-qvl may reach beside the configured PCCS: the keyless
/// PCS and the root certificate's CRL distribution point (design §12).
pub const INTEL_PCS_HOST: &str = "api.trustedservices.intel.com";
pub const INTEL_CRL_DP_HOST: &str = "certificates.trustedservices.intel.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuEvidenceMode {
    Real,
    Canned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuEvidenceMode {
    Real,
    Simulator,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("config: {0}")]
pub struct ConfigError(pub String);

/// The resolved configuration (design §2 table). Durations are already `Duration`s;
/// URLs already parsed and policy-checked (https, no userinfo).
#[derive(Debug, Clone)]
pub struct KbsConfig {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
    pub keyring_file: PathBuf,
    pub policy_dir: PathBuf,
    pub gpu_evidence: GpuEvidenceMode,
    pub cpu_evidence: CpuEvidenceMode,
    pub pccs_url: Url,
    pub nras_gpu_url: Url,
    pub nras_jwks_url: Url,
    pub nras_issuer: String,
    pub nras_claims_version: String,
    pub nonce_ttl: Duration,
    pub nonce_cap: usize,
    pub nonce_per_source_cap: usize,
    pub request_concurrency: usize,
    pub inflight_per_source_cap: usize,
    pub request_wall: Duration,
    pub pccs_timeout: Duration,
    pub nras_timeout: Duration,
    pub jwks_timeout: Duration,
    pub collateral_memo_fresh: Duration,
    pub max_body_bytes: usize,
    pub capture_max: usize,
    pub capture_preverify_max: usize,
}

impl KbsConfig {
    /// Read the process environment (only `KBS_*` names are consulted).
    pub fn from_env() -> Result<Self, ConfigError> {
        let map: HashMap<String, String> = std::env::vars_os()
            .filter_map(|(k, v)| {
                let k = k.to_str()?;
                if !k.starts_with("KBS_") {
                    return None;
                }
                Some((k.to_string(), v.to_string_lossy().into_owned()))
            })
            .collect();
        Self::from_map(&map)
    }

    /// Build from a name → value map (the test seam). Every rule of design §2:
    /// malformed or out-of-range values refuse; defaults apply to absent names.
    pub fn from_map(map: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let get = |k: &str| map.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());

        let listen: SocketAddr = get("KBS_LISTEN")
            .unwrap_or("127.0.0.1:3030")
            .parse()
            .map_err(|e| ConfigError(format!("KBS_LISTEN: {e}")))?;

        let data_dir = PathBuf::from(get("KBS_DATA_DIR").unwrap_or("/var/lib/fabstir-kbs"));
        if !data_dir.is_dir() {
            return Err(ConfigError(format!(
                "KBS_DATA_DIR {} is not a directory",
                data_dir.display()
            )));
        }
        let keyring_file = get("KBS_KEYRING_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("keyring.json"));
        let policy_dir = get("KBS_POLICY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("public").join("policies"));
        if !policy_dir.is_dir() {
            // A typo here would otherwise boot cleanly and answer `no_provider` to
            // every node, after each one's nonce was burned.
            return Err(ConfigError(format!(
                "KBS_POLICY_DIR {} is not a directory",
                policy_dir.display()
            )));
        }

        let gpu_evidence = match get("KBS_GPU_EVIDENCE").unwrap_or("real") {
            "real" => GpuEvidenceMode::Real,
            "canned" => GpuEvidenceMode::Canned,
            other => {
                return Err(ConfigError(format!(
                    "KBS_GPU_EVIDENCE must be real|canned, got {other:?}"
                )))
            }
        };
        let cpu_evidence = match get("KBS_CPU_EVIDENCE").unwrap_or("real") {
            "real" => CpuEvidenceMode::Real,
            "simulator" => CpuEvidenceMode::Simulator,
            other => {
                return Err(ConfigError(format!(
                    "KBS_CPU_EVIDENCE must be real|simulator, got {other:?}"
                )))
            }
        };

        let pccs_url = parse_https_url(
            "KBS_PCCS_URL",
            get("KBS_PCCS_URL").unwrap_or("https://pccs.phala.network"),
        )?;
        let nras_gpu_url = parse_https_url(
            "KBS_NRAS_GPU_URL",
            get("KBS_NRAS_GPU_URL").unwrap_or("https://nras.attestation.nvidia.com/v3/attest/gpu"),
        )?;
        let nras_jwks_url = parse_https_url(
            "KBS_NRAS_JWKS_URL",
            get("KBS_NRAS_JWKS_URL")
                .unwrap_or("https://nras.attestation.nvidia.com/.well-known/jwks.json"),
        )?;
        let nras_issuer = get("KBS_NRAS_ISSUER")
            .unwrap_or("https://nras.attestation.nvidia.com")
            .to_string();
        let nras_claims_version = get("KBS_NRAS_CLAIMS_VERSION").unwrap_or("2.0").to_string();
        if crate::kbs::nras_claims::table_for(&nras_claims_version).is_none() {
            // Refused at start, not after a burned nonce and a paid NRAS round trip.
            return Err(ConfigError(format!(
                "KBS_NRAS_CLAIMS_VERSION {nras_claims_version:?} has no claim table in this build"
            )));
        }

        let nonce_ttl_secs = parse_u64_in(
            "KBS_NONCE_TTL_SECS",
            get("KBS_NONCE_TTL_SECS"),
            300,
            300,
            3600,
        )?;
        // A full sweep walks the map under the store's lock; 100 000 keeps that bounded.
        let nonce_cap =
            parse_u64_in("KBS_NONCE_CAP", get("KBS_NONCE_CAP"), 10_000, 1, 100_000)? as usize;
        let nonce_per_source_cap = parse_u64_in(
            "KBS_NONCE_PER_SOURCE_CAP",
            get("KBS_NONCE_PER_SOURCE_CAP"),
            64,
            1,
            100_000,
        )? as usize;
        if nonce_per_source_cap > nonce_cap {
            return Err(ConfigError(
                "KBS_NONCE_PER_SOURCE_CAP must not exceed KBS_NONCE_CAP".into(),
            ));
        }
        let request_concurrency = parse_u64_in(
            "KBS_REQUEST_CONCURRENCY",
            get("KBS_REQUEST_CONCURRENCY"),
            8,
            1,
            1024,
        )? as usize;
        let inflight_per_source_cap = parse_u64_in(
            "KBS_INFLIGHT_PER_SOURCE_CAP",
            get("KBS_INFLIGHT_PER_SOURCE_CAP"),
            2,
            1,
            1024,
        )? as usize;
        if inflight_per_source_cap > request_concurrency {
            return Err(ConfigError(
                "KBS_INFLIGHT_PER_SOURCE_CAP must not exceed KBS_REQUEST_CONCURRENCY".into(),
            ));
        }
        let request_wall = parse_u64_in(
            "KBS_REQUEST_WALL_SECS",
            get("KBS_REQUEST_WALL_SECS"),
            170,
            1,
            3600,
        )?;
        let pccs_timeout = parse_u64_in(
            "KBS_PCCS_TIMEOUT_SECS",
            get("KBS_PCCS_TIMEOUT_SECS"),
            15,
            1,
            3600,
        )?;
        let nras_timeout = parse_u64_in(
            "KBS_NRAS_TIMEOUT_SECS",
            get("KBS_NRAS_TIMEOUT_SECS"),
            60,
            1,
            3600,
        )?;
        let jwks_timeout = parse_u64_in(
            "KBS_JWKS_TIMEOUT_SECS",
            get("KBS_JWKS_TIMEOUT_SECS"),
            10,
            1,
            3600,
        )?;
        let collateral_memo_fresh = parse_u64_in(
            "KBS_COLLATERAL_MEMO_FRESH_SECS",
            get("KBS_COLLATERAL_MEMO_FRESH_SECS"),
            86_400,
            0,
            30 * 86_400,
        )?;
        let max_body_bytes = parse_u64_in(
            "KBS_MAX_BODY_BYTES",
            get("KBS_MAX_BODY_BYTES"),
            1_048_576,
            1024,
            64 * 1_048_576,
        )? as usize;
        let capture_max =
            parse_u64_in("KBS_CAPTURE_MAX", get("KBS_CAPTURE_MAX"), 100, 0, 100_000)? as usize;
        let capture_preverify_max = parse_u64_in(
            "KBS_CAPTURE_PREVERIFY_MAX",
            get("KBS_CAPTURE_PREVERIFY_MAX"),
            20,
            0,
            100_000,
        )? as usize;

        // Names we do not know are a misconfiguration, not a silent no-op
        // (`KBS_ENV_FILE` is the tooling's; the server ignores it).
        for k in map.keys() {
            if k.starts_with("KBS_") && !KNOWN.contains(&k.as_str()) && k != "KBS_ENV_FILE" {
                return Err(ConfigError(format!("unknown variable {k}")));
            }
        }

        Ok(Self {
            listen,
            data_dir,
            keyring_file,
            policy_dir,
            gpu_evidence,
            cpu_evidence,
            pccs_url,
            nras_gpu_url,
            nras_jwks_url,
            nras_issuer,
            nras_claims_version,
            nonce_ttl: Duration::from_secs(nonce_ttl_secs),
            nonce_cap,
            nonce_per_source_cap,
            request_concurrency,
            inflight_per_source_cap,
            request_wall: Duration::from_secs(request_wall),
            pccs_timeout: Duration::from_secs(pccs_timeout),
            nras_timeout: Duration::from_secs(nras_timeout),
            jwks_timeout: Duration::from_secs(jwks_timeout),
            collateral_memo_fresh: Duration::from_secs(collateral_memo_fresh),
            max_body_bytes,
            capture_max,
            capture_preverify_max,
        })
    }

    /// Any test evidence mode forces the TEST keyring (design D3).
    pub fn test_keyring_required(&self) -> bool {
        self.gpu_evidence == GpuEvidenceMode::Canned
            || self.cpu_evidence == CpuEvidenceMode::Simulator
    }

    pub fn memo_dir(&self) -> PathBuf {
        self.data_dir.join("memo")
    }

    pub fn capture_dir(&self) -> PathBuf {
        self.data_dir.join("capture")
    }

    /// The egress allow-list (design §12): the configured PCCS/NRAS/JWKS hosts plus
    /// Intel's PCS and CRL-distribution-point hosts, each with the port the
    /// configured URL names (443 when unstated).
    pub fn allowed_hosts(&self) -> Vec<(String, u16)> {
        let mut v = vec![
            host_port(&self.pccs_url),
            host_port(&self.nras_gpu_url),
            host_port(&self.nras_jwks_url),
            (INTEL_PCS_HOST.to_string(), 443),
            (INTEL_CRL_DP_HOST.to_string(), 443),
        ];
        v.sort();
        v.dedup();
        v
    }
}

/// Create `dir` if absent and prove it is writable (a temp file created and
/// removed). The unit's `ReadWritePaths` must match `KBS_DATA_DIR`; a mismatch
/// would otherwise let the broker start with a silently dead memo and capture.
pub fn probe_writable(dir: &std::path::Path) -> Result<(), ConfigError> {
    std::fs::create_dir_all(dir).map_err(|e| ConfigError(format!("{}: {e}", dir.display())))?;
    let probe = tempfile::NamedTempFile::new_in(dir)
        .map_err(|e| ConfigError(format!("{} is not writable: {e}", dir.display())))?;
    probe
        .close()
        .map_err(|e| ConfigError(format!("{} is not writable: {e}", dir.display())))?;
    Ok(())
}

fn host_port(u: &Url) -> (String, u16) {
    (
        u.host_str().unwrap_or_default().to_ascii_lowercase(),
        u.port().unwrap_or(443),
    )
}

/// `https://` with a host, no userinfo, no fragment (design §12 rules for egress URLs).
fn parse_https_url(name: &str, raw: &str) -> Result<Url, ConfigError> {
    let u = Url::parse(raw).map_err(|e| ConfigError(format!("{name}: {e}")))?;
    if u.scheme() != "https" {
        return Err(ConfigError(format!("{name}: scheme must be https")));
    }
    if u.host_str().map(|h| h.is_empty()).unwrap_or(true) {
        return Err(ConfigError(format!("{name}: missing host")));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(ConfigError(format!("{name}: userinfo is not allowed")));
    }
    if u.fragment().is_some() {
        return Err(ConfigError(format!("{name}: fragment is not allowed")));
    }
    Ok(u)
}

fn parse_u64_in(
    name: &str,
    raw: Option<&str>,
    default: u64,
    lo: u64,
    hi: u64,
) -> Result<u64, ConfigError> {
    let v = match raw {
        None => default,
        Some(s) => s
            .parse::<u64>()
            .map_err(|e| ConfigError(format!("{name}: {e}")))?,
    };
    if v < lo || v > hi {
        return Err(ConfigError(format!(
            "{name} must be within {lo}..={hi}, got {v}"
        )));
    }
    Ok(v)
}

const KNOWN: &[&str] = &[
    "KBS_LISTEN",
    "KBS_DATA_DIR",
    "KBS_KEYRING_FILE",
    "KBS_POLICY_DIR",
    "KBS_GPU_EVIDENCE",
    "KBS_CPU_EVIDENCE",
    "KBS_PCCS_URL",
    "KBS_NRAS_GPU_URL",
    "KBS_NRAS_JWKS_URL",
    "KBS_NRAS_ISSUER",
    "KBS_NRAS_CLAIMS_VERSION",
    "KBS_NONCE_TTL_SECS",
    "KBS_NONCE_CAP",
    "KBS_NONCE_PER_SOURCE_CAP",
    "KBS_REQUEST_CONCURRENCY",
    "KBS_INFLIGHT_PER_SOURCE_CAP",
    "KBS_REQUEST_WALL_SECS",
    "KBS_PCCS_TIMEOUT_SECS",
    "KBS_NRAS_TIMEOUT_SECS",
    "KBS_JWKS_TIMEOUT_SECS",
    "KBS_COLLATERAL_MEMO_FRESH_SECS",
    "KBS_MAX_BODY_BYTES",
    "KBS_CAPTURE_MAX",
    "KBS_CAPTURE_PREVERIFY_MAX",
];
