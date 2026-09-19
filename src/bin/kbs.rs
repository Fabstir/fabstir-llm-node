// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! `fabstir-kbs` (design §1, §13): `serve` runs the broker; the provider tooling
//! (`keyring`, `policy sign`, `seal`, `reseal`) lands in `kbs::tools`.
//!
//! Exit codes (design D17): 78 for a config, keyring, permission or mode refusal
//! (permanent; the unit has `RestartPreventExitStatus=78`); 1 for any other start
//! failure (bind, IO), which restarts.

use clap::{Parser, Subcommand};
use fabstir_llm_node::kbs::config::{KbsConfig, EXIT_REFUSED};
use fabstir_llm_node::kbs::egress::{EgressClient, EgressOptions};
use fabstir_llm_node::kbs::keyring::Keyring;
use fabstir_llm_node::kbs::routes::{serve, AppState};
use fabstir_llm_node::kbs::tools;
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::Policy;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "fabstir-kbs", version = fabstir_llm_node::version::VERSION, about = "Fabstir Phase 5 key broker")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the broker (configuration from `KBS_*`).
    Serve,
    /// Keyring maintenance (run as the service user: the file must stay 0600 and owned by it).
    Keyring {
        #[command(subcommand)]
        cmd: KeyringCmd,
    },
    /// Provider policy tooling.
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },
    /// Seal a plaintext model into a container bound to a signed policy (streaming).
    Seal {
        #[arg(long)]
        model: PathBuf,
        #[arg(long)]
        dek_file: PathBuf,
        #[arg(long)]
        model_id: String,
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// The provider address the policy must be signed by (the keyring's `provider`).
        #[arg(long)]
        provider: String,
    },
    /// Re-seal an existing container under a NEW signed policy with the same DEK (streaming).
    Reseal {
        #[arg(long = "in", value_name = "IN")]
        input: PathBuf,
        #[arg(long)]
        dek_file: PathBuf,
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// The provider address the policy must be signed by (the keyring's `provider`).
        #[arg(long)]
        provider: String,
    },
}

#[derive(Subcommand)]
enum KeyringCmd {
    /// Add an entry. `--generate` writes a fresh DEK to `--dek-out` (mode 0600); it is never printed.
    Add {
        #[arg(long)]
        model_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long, conflicts_with = "generate")]
        dek_file: Option<PathBuf>,
        #[arg(long, requires = "dek_out")]
        generate: bool,
        #[arg(long)]
        dek_out: Option<PathBuf>,
        #[arg(long)]
        test: bool,
        #[arg(long, default_value_t = 1)]
        min_policy_version: u32,
        #[arg(long, default_value = "")]
        note: String,
        /// The keyring file (default `$KBS_KEYRING_FILE` or `/var/lib/fabstir-kbs/keyring.json`).
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Validate a keyring file with the broker's rules.
    Check {
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum PolicyCmd {
    /// Sign a policy JSON (the `Policy` object) as the provider; prints the signer address and policy_hash.
    Sign {
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        key_file: PathBuf,
        #[arg(long)]
        encrypted_ref: String,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() {
    // Plain journald lines (design §2: no json feature in this crate's
    // tracing-subscriber); RUST_LOG is honoured. Stderr, never stdout: the tooling's
    // `signer=… policy_hash=…` line is parsed by scripts, and journald reads both.
    // (`fmt::init()` minus its stdout writer: the same `Targets` filter it builds
    // from RUST_LOG when the `env-filter` feature is off, default `info`.)
    {
        use tracing_subscriber::{filter::Targets, layer::SubscriberExt, util::SubscriberInitExt};
        let info = || Targets::new().with_default(tracing::level_filters::LevelFilter::INFO);
        let targets = match std::env::var("RUST_LOG") {
            // A typo in the env file must not silence the broker (the CRITICAL lines
            // are how a test keyring or a memo-served pass is noticed): say so and
            // fall back to `info`, not to "nothing".
            Ok(var) => var.parse::<Targets>().unwrap_or_else(|e| {
                eprintln!("fabstir-kbs: ignoring RUST_LOG={var:?}: {e}; logging at info");
                info()
            }),
            Err(_) => info(),
        };
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_max_level(tracing::level_filters::LevelFilter::TRACE)
            .finish()
            .with(targets)
            .init();
    }
    let cli = Cli::parse();
    let r = match cli.cmd {
        Cmd::Serve => {
            run_serve();
            Ok(())
        }
        Cmd::Keyring { cmd } => run_keyring(cmd),
        Cmd::Policy { cmd } => run_policy(cmd),
        Cmd::Seal {
            model,
            dek_file,
            model_id,
            policy,
            out,
            provider,
        } => run_seal(&model, &dek_file, &model_id, &policy, &out, Some(&provider)),
        Cmd::Reseal {
            input,
            dek_file,
            policy,
            out,
            provider,
        } => run_reseal(&input, &dek_file, &policy, &out, Some(&provider)),
    };
    if let Err(e) = r {
        eprintln!("fabstir-kbs: {e}");
        std::process::exit(1);
    }
}

/// The unit's `EnvironmentFile`; `sudo -u fabstir-kbs …` strips the process
/// environment, so the tooling reads the same file the broker will.
const ENV_FILE: &str = "/etc/fabstir-kbs/env";

/// A `KBS_*` value from the process environment, else from `KBS_ENV_FILE` /
/// `/etc/fabstir-kbs/env` (`KEY=VALUE` lines, `#` comments, optional quotes).
fn kbs_setting(name: &str) -> Option<String> {
    if let Some(v) = std::env::var_os(name) {
        return Some(v.to_string_lossy().into_owned());
    }
    let file = std::env::var_os("KBS_ENV_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(ENV_FILE));
    let text = std::fs::read_to_string(file).ok()?;
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == name)
        .map(|(_, v)| v.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|v| !v.is_empty())
}

/// The same resolution as `KbsConfig`: `--file`, else `KBS_KEYRING_FILE`, else
/// `$KBS_DATA_DIR/keyring.json`, else the default data dir; the env vars come from
/// the process or the unit's env file.
fn keyring_path(file: Option<PathBuf>) -> PathBuf {
    file.or_else(|| kbs_setting("KBS_KEYRING_FILE").map(PathBuf::from))
        .unwrap_or_else(|| {
            kbs_setting("KBS_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/var/lib/fabstir-kbs"))
                .join("keyring.json")
        })
}

fn read_dek(path: &Path) -> Result<zeroize::Zeroizing<[u8; 32]>, String> {
    let text = zeroize::Zeroizing::new(
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
    );
    let s = text.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    let v = zeroize::Zeroizing::new(hex::decode(s).map_err(|_| "dek file is not hex".to_string())?);
    if v.len() != 32 {
        return Err("dek file is not 32 bytes".to_string());
    }
    let mut dek = zeroize::Zeroizing::new([0u8; 32]);
    dek.copy_from_slice(&v);
    Ok(dek)
}

fn read_signed_policy(path: &Path) -> Result<SignedModelPolicy, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn run_keyring(cmd: KeyringCmd) -> Result<(), String> {
    match cmd {
        KeyringCmd::Add {
            model_id,
            provider,
            dek_file,
            generate,
            dek_out,
            test,
            min_policy_version,
            note,
            file,
        } => {
            let (dek, generated): (zeroize::Zeroizing<[u8; 32]>, Option<PathBuf>) = if generate {
                let out = dek_out.ok_or("--generate needs --dek-out")?;
                let d = zeroize::Zeroizing::new(tools::generate_dek());
                // create_new: an existing DEK file is never truncated.
                let hex_dek = zeroize::Zeroizing::new(hex::encode(*d));
                tools::write_0600_new(&out, hex_dek.as_bytes()).map_err(|e| e.to_string())?;
                (d, Some(out))
            } else {
                (
                    read_dek(&dek_file.ok_or("--dek-file or --generate is required")?)?,
                    None,
                )
            };
            let path = keyring_path(file);
            let n = match tools::keyring_add(
                &path,
                &model_id,
                &provider,
                &dek,
                test,
                min_policy_version,
                &note,
            ) {
                Ok(n) => n,
                Err(e) => {
                    // A refused add must not leave an orphan DEK on disk.
                    if let Some(out) = &generated {
                        let _ = std::fs::remove_file(out);
                    }
                    return Err(e.to_string());
                }
            };
            if let Some(out) = &generated {
                eprintln!("fabstir-kbs: fresh DEK written to {} (mode 0600; use it for `seal`, then shred it)", out.display());
            }
            eprintln!(
                "fabstir-kbs: {} now holds {n} entr{}",
                path.display(),
                if n == 1 { "y" } else { "ies" }
            );
            Ok(())
        }
        KeyringCmd::Check { file } => {
            let path = keyring_path(file);
            let n = tools::keyring_check(&path).map_err(|e| e.to_string())?;
            eprintln!("fabstir-kbs: {} valid, {n} entries", path.display());
            Ok(())
        }
    }
}

fn run_policy(cmd: PolicyCmd) -> Result<(), String> {
    match cmd {
        PolicyCmd::Sign {
            policy,
            key_file,
            encrypted_ref,
            out,
        } => {
            let bytes = std::fs::read(&policy).map_err(|e| format!("{}: {e}", policy.display()))?;
            let policy: Policy =
                serde_json::from_slice(&bytes).map_err(|e| format!("policy: {e}"))?;
            // The provider's long-term signing key: zeroised like every DEK.
            let key_hex = zeroize::Zeroizing::new(
                std::fs::read_to_string(&key_file)
                    .map_err(|e| format!("{}: {e}", key_file.display()))?,
            );
            let sk = tools::signing_key_from_hex(&key_hex).map_err(|e| e.to_string())?;
            let (signed, signer) =
                tools::sign_policy(&policy, &encrypted_ref, &sk).map_err(|e| e.to_string())?;
            let hash = tools::policy_hash_of(&signed).map_err(|e| e.to_string())?;
            tools::write_new(
                &out,
                &serde_json::to_vec_pretty(&signed).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            println!(
                "signer={signer}\npolicy_hash={}\npolicy_version={}",
                hex::encode(hash),
                signed.policy.policy_version
            );
            Ok(())
        }
    }
}

fn run_seal(
    model: &Path,
    dek_file: &Path,
    model_id: &str,
    policy: &Path,
    out: &Path,
    provider: Option<&str>,
) -> Result<(), String> {
    let dek = read_dek(dek_file)?;
    let id: [u8; 32] = hex::decode(model_id)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or("model_id must be 64 hex")?;
    let signed = read_signed_policy(policy)?;
    let len = tools::seal(model, out, &dek, id, &signed, provider).map_err(|e| e.to_string())?;
    let hash = tools::policy_hash_of(&signed).map_err(|e| e.to_string())?;
    println!(
        "sealed {len} bytes into {} under policy_hash={}",
        out.display(),
        hex::encode(hash)
    );
    Ok(())
}

fn run_reseal(
    input: &Path,
    dek_file: &Path,
    policy: &Path,
    out: &Path,
    provider: Option<&str>,
) -> Result<(), String> {
    let dek = read_dek(dek_file)?;
    let signed = read_signed_policy(policy)?;
    let len = tools::reseal(input, out, &dek, &signed, provider).map_err(|e| e.to_string())?;
    let hash = tools::policy_hash_of(&signed).map_err(|e| e.to_string())?;
    println!(
        "resealed {len} plaintext bytes into {} under policy_hash={}",
        out.display(),
        hex::encode(hash)
    );
    Ok(())
}

fn refuse(what: &str, e: impl std::fmt::Display) -> ! {
    eprintln!("fabstir-kbs: refusing to start: {what}: {e}");
    std::process::exit(EXIT_REFUSED);
}

fn run_serve() {
    let cfg = match KbsConfig::from_env() {
        Ok(c) => c,
        Err(e) => refuse("config", e),
    };
    let keyring = match Keyring::load(&cfg.keyring_file, cfg.test_keyring_required()) {
        Ok(k) => k,
        Err(e) => refuse("keyring", e),
    };
    // A permission refusal (the unit's ReadWritePaths not matching KBS_DATA_DIR)
    // is permanent: exit 78, never a silently dead memo/capture.
    for (dir, what) in [
        (cfg.memo_dir(), "memo dir"),
        (cfg.capture_dir(), "capture dir"),
    ] {
        if let Err(e) = fabstir_llm_node::kbs::config::probe_writable(&dir) {
            refuse(what, e);
        }
    }
    let egress = match EgressClient::new(cfg.allowed_hosts(), EgressOptions::default()) {
        Ok(c) => c,
        Err(e) => refuse("egress client", e),
    };
    let class = keyring.class().map(|c| c.wire()).unwrap_or("mixed");
    tracing::info!(
        version = fabstir_llm_node::version::VERSION,
        keyring = class,
        entries = keyring.len(),
        gpu_evidence = ?cfg.gpu_evidence,
        cpu_evidence = ?cfg.cpu_evidence,
        "fabstir-kbs starting"
    );
    if class == "test" {
        tracing::error!(
            "CRITICAL: TEST keyring loaded; every release is labelled test_release: true"
        );
    }
    let state = match AppState::new(cfg, keyring, egress) {
        Ok(s) => Arc::new(s),
        Err(e) => refuse("keyring", e),
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("fabstir-kbs: runtime: {e}");
            std::process::exit(1);
        }
    };
    let result = rt.block_on(async move {
        let shutdown = async {
            // systemd stop/restart sends SIGTERM; SIGINT is the terminal. Either
            // drains in-flight requests (a burned nonce + a paid NRAS round trip).
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            tracing::info!("fabstir-kbs: shutdown signal");
        };
        serve(state, shutdown).await
    });
    if let Err(e) = result {
        eprintln!("fabstir-kbs: serve: {e}");
        std::process::exit(1);
    }
}
