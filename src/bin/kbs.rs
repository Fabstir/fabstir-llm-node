// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! `fabstir-kbs` (design §1, §13): `serve` runs the broker; the provider tooling
//! (`keyring`, `policy sign`, `seal`, `reseal`) lands in `kbs::tools`, the
//! policy schema 3 tooling (`authority`, `profile`, `policy migrate-v2`,
//! `--authority`) in `kbs::tools_authority`.
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
use fabstir_llm_node::kbs::tools_authority as ta;
use fabstir_llm_node::tee::host_profile::{
    parse_bundle, AuthorityBundle, AuthorityManifest, HostProfile, HostProfileSet,
    SignedAuthorityManifest, SignedHostProfileSet,
};
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
    /// Host authority documents (schema 3): the root-signed manifest and the bundle.
    Authority {
        #[command(subcommand)]
        cmd: AuthorityCmd,
    },
    /// Host profiles (schema 3): built from PREDICTED values; a capture only confirms.
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
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
        /// The authority bundle the policy will be judged under: refuse a root, minimum or
        /// class it cannot meet.
        #[arg(long)]
        authority: Option<PathBuf>,
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
        /// The authority bundle the policy will be judged under: refuse a root, minimum or
        /// class it cannot meet.
        #[arg(long)]
        authority: Option<PathBuf>,
    },
    /// Migrate a signed schema-2 policy: an unsigned v3 policy + an unsigned host profile.
    MigrateV2 {
        #[arg(long = "in", value_name = "IN")]
        input: PathBuf,
        /// The provider the v2 policy must be signed by; checked before any field is read.
        #[arg(long)]
        expected_provider: String,
        #[arg(long)]
        root: String,
        #[arg(long)]
        class: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        policy_not_before: u64,
        #[arg(long)]
        policy_expiry: u64,
        #[arg(long)]
        profile_not_before: u64,
        #[arg(long)]
        profile_expiry: u64,
        #[arg(long)]
        policy_version: u32,
        #[arg(long)]
        min_manifest_version: u32,
        #[arg(long)]
        min_threshold: u32,
        #[arg(long)]
        min_profile_epoch: u64,
        #[arg(long, action = clap::ArgAction::Set)]
        require_pins: bool,
        #[arg(long)]
        evidence: String,
        #[arg(long)]
        policy_out: PathBuf,
        #[arg(long)]
        profile_out: PathBuf,
    },
}

#[derive(Subcommand)]
enum AuthorityCmd {
    /// Sign a manifest as the root (or attach an external signature).
    Sign {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, required_unless_present = "signature_file")]
        key_file: Option<PathBuf>,
        /// An externally produced 65-byte signature (hex), e.g. from a hardware wallet.
        #[arg(long, conflicts_with = "key_file")]
        signature_file: Option<PathBuf>,
        #[arg(long, required_unless_present = "first")]
        previous: Option<PathBuf>,
        #[arg(long, conflicts_with = "previous")]
        first: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// Assemble a bundle (step rule + the highest-signed ledger in ~/.fabstir-authority).
    Assemble {
        #[arg(long)]
        manifest_signed: PathBuf,
        #[arg(long)]
        set_signed: PathBuf,
        #[arg(long, required_unless_present = "first")]
        previous: Option<PathBuf>,
        #[arg(long, conflicts_with = "previous")]
        first: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// Check a bundle offline and read-only; exit 1 on a refusal, 3 on warnings only (2 is a
    /// usage error).
    Check {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        policy: Vec<PathBuf>,
        #[arg(long)]
        previous: Option<PathBuf>,
        #[arg(long)]
        warn_days: Option<u64>,
        #[arg(long)]
        replay: Vec<PathBuf>,
        #[arg(long)]
        hwm_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// A new UNPINNED profile from predicted values (dstack-mr, image digest, compose rule).
    New {
        #[arg(long)]
        id: String,
        #[arg(long)]
        class: String,
        #[arg(long)]
        not_before: u64,
        #[arg(long)]
        expiry: u64,
        #[arg(long)]
        evidence: String,
        /// The vendor's EAT `hwmodel` string (repeat for several).
        #[arg(long, required = true)]
        hwmodel: Vec<String>,
        #[arg(long)]
        min_driver_version: Option<String>,
        #[arg(long)]
        min_vbios_version: Option<String>,
        /// dstack-mr output: {mrtd, rtmr0, rtmr1, rtmr2}.
        #[arg(long)]
        measurements: PathBuf,
        #[arg(long)]
        os_image_digest: String,
        #[arg(long)]
        compose_file: PathBuf,
        /// A served attestation JSON (or app-compose JSON) from an earlier deploy.
        #[arg(long)]
        compose_reference: PathBuf,
        #[arg(long)]
        env_names: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Pin app_id (out of band) and the Phala KMS key provider, once a capture confirms every row.
    Pin {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        capture: PathBuf,
        #[arg(long)]
        app_id: String,
        /// Replace the committed Phala KMS key provider (prints the STOP rule).
        #[arg(long)]
        key_provider_override: Option<String>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Byte-compare a profile with the predicted registers, image digest and compose hash.
    Check {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        measurements: PathBuf,
        #[arg(long)]
        os_image_digest: String,
        #[arg(long)]
        compose_file: PathBuf,
        #[arg(long)]
        compose_reference: PathBuf,
        #[arg(long)]
        env_names: PathBuf,
    },
    /// Assemble an unsigned profile set.
    Assemble {
        #[arg(long)]
        root: String,
        #[arg(long)]
        epoch: u64,
        #[arg(long)]
        issued_at: u64,
        #[arg(long, required_unless_present = "first")]
        previous: Option<PathBuf>,
        #[arg(long, conflicts_with = "previous")]
        first: bool,
        #[arg(long, required = true)]
        profile: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Sign a set as a manifest signer; `--append` co-signs an already signed, identical set.
    Sign {
        #[arg(long)]
        set: PathBuf,
        #[arg(long)]
        key_file: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        append: Option<PathBuf>,
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
        Cmd::Authority { cmd } => run_authority(cmd),
        Cmd::Profile { cmd } => run_profile(cmd),
        Cmd::Seal {
            model,
            dek_file,
            model_id,
            policy,
            out,
            provider,
            authority,
        } => run_seal(
            &model,
            &dek_file,
            &model_id,
            &policy,
            &out,
            Some(&provider),
            authority.as_deref(),
        ),
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

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_file(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// A long-term signing key (provider, root or signer): zeroised like every DEK.
fn read_key(path: &Path) -> Result<k256::ecdsa::SigningKey, String> {
    let key_hex = zeroize::Zeroizing::new(
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
    );
    tools::signing_key_from_hex(&key_hex).map_err(|e| e.to_string())
}

fn read_bundle(path: &Path) -> Result<AuthorityBundle, String> {
    parse_bundle(&read_file(path)?, &path.display().to_string()).map_err(|e| e.to_string())
}

/// `--previous <bundle>` (verified) or `--first`.
fn prior(previous: Option<PathBuf>, first: bool) -> Result<ta::Prior, String> {
    match (previous, first) {
        (Some(p), false) => ta::read_previous(&read_file(&p)?, &p.display().to_string())
            .map(|b| ta::Prior::Previous(Box::new(b)))
            .map_err(|e| e.to_string()),
        (None, true) => Ok(ta::Prior::First),
        _ => Err("exactly one of --previous and --first".into()),
    }
}

fn write_json<T: serde::Serialize>(out: &Path, v: &T) -> Result<(), String> {
    tools::write_new(
        out,
        &serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn run_policy(cmd: PolicyCmd) -> Result<(), String> {
    match cmd {
        PolicyCmd::MigrateV2 {
            input,
            expected_provider,
            root,
            class,
            id,
            policy_not_before,
            policy_expiry,
            profile_not_before,
            profile_expiry,
            policy_version,
            min_manifest_version,
            min_threshold,
            min_profile_epoch,
            require_pins,
            evidence,
            policy_out,
            profile_out,
        } => {
            let args = ta::MigrateArgs {
                expected_provider,
                root,
                class,
                id,
                policy_not_before,
                policy_expiry,
                profile_not_before,
                profile_expiry,
                policy_version,
                min_manifest_version,
                min_threshold,
                min_profile_epoch,
                require_pins,
                evidence,
            };
            let (policy, profile) =
                ta::migrate_v2(&read_file(&input)?, &args).map_err(|e| e.to_string())?;
            ta::write_pair(
                &policy_out,
                &serde_json::to_vec_pretty(&policy).map_err(|e| e.to_string())?,
                &profile_out,
                &serde_json::to_vec_pretty(&profile).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            println!(
                "model_id={}\nprofile={}\nprofile_min_driver_version={}",
                hex::encode(policy.model_id),
                profile.id,
                profile.gpu.min_driver_version.as_deref().unwrap_or("none")
            );
            Ok(())
        }
        PolicyCmd::Sign {
            policy,
            key_file,
            encrypted_ref,
            out,
            authority,
        } => {
            let policy: Policy = read_json(&policy)?;
            if let Some(b) = authority {
                ta::check_policy_against_bundle(&policy, &read_bundle(&b)?)
                    .map_err(|e| e.to_string())?;
            }
            let sk = read_key(&key_file)?;
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
    authority: Option<&Path>,
) -> Result<(), String> {
    let dek = read_dek(dek_file)?;
    let id: [u8; 32] = hex::decode(model_id)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or("model_id must be 64 hex")?;
    let signed = read_signed_policy(policy)?;
    if let Some(b) = authority {
        ta::check_policy_against_bundle(&signed.policy, &read_bundle(b)?)
            .map_err(|e| e.to_string())?;
    }
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

fn run_authority(cmd: AuthorityCmd) -> Result<(), String> {
    match cmd {
        AuthorityCmd::Sign {
            manifest,
            key_file,
            signature_file,
            previous,
            first,
            out,
        } => {
            let m: AuthorityManifest = read_json(&manifest)?;
            let prior = prior(previous, first)?;
            let sk;
            let signer = match (key_file, signature_file) {
                (Some(k), None) => {
                    sk = read_key(&k)?;
                    ta::ManifestSigner::Key(&sk)
                }
                (None, Some(f)) => ta::ManifestSigner::External(
                    ta::read_signature_file(&read_file(&f)?).map_err(|e| e.to_string())?,
                ),
                _ => return Err("exactly one of --key-file and --signature-file".into()),
            };
            let signed = ta::sign_manifest(m, signer, &prior).map_err(|e| e.to_string())?;
            write_json(&out, &signed)?;
            let digest = fabstir_llm_node::tee::host_profile::document_digest(&signed.manifest)
                .map_err(|e| e.to_string())?;
            println!(
                "root={}\nmanifest_version={}\nmanifest_digest={}",
                signed.manifest.root,
                signed.manifest.manifest_version,
                hex::encode(digest)
            );
            Ok(())
        }
        AuthorityCmd::Assemble {
            manifest_signed,
            set_signed,
            previous,
            first,
            out,
        } => {
            let m: SignedAuthorityManifest = read_json(&manifest_signed)?;
            let s: SignedHostProfileSet = read_json(&set_signed)?;
            let prior = prior(previous, first)?;
            let dir = ta::default_ledger_dir().ok_or("HOME is not set (the ledger lives there)")?;
            let (bundle, entry) =
                ta::assemble_to(&out, m, s, &prior, &dir).map_err(|e| e.to_string())?;
            println!(
                "root={}\nmanifest_version={}\nepoch={}\nmanifest_digest={}\nset_digest={}\nserve_as={}.json\nledger={}",
                entry.root,
                entry.manifest_version,
                entry.epoch,
                entry.manifest_digest,
                entry.set_digest,
                fabstir_llm_node::kbs::authority::root_key(&bundle.manifest.manifest.root),
                dir.join(format!(
                    "{}.json",
                    fabstir_llm_node::kbs::authority::root_key(&entry.root)
                ))
                .display()
            );
            Ok(())
        }
        AuthorityCmd::Check {
            bundle,
            policy,
            previous,
            warn_days,
            replay,
            hwm_dir,
        } => {
            let bytes = read_file(&bundle)?;
            let previous = previous.map(|p| read_file(&p)).transpose()?;
            let mut policies = Vec::new();
            for p in &policy {
                policies.push((p.display().to_string(), read_file(p)?));
            }
            let name = bundle
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let report = ta::authority_check(&ta::CheckInputs {
                bundle: &bytes,
                bundle_name: &name,
                policies,
                previous: previous.as_deref(),
                warn_days,
                replays: replay,
                hwm_dir,
                now: ta::now().map_err(|e| e.to_string())?,
            });
            for l in &report.lines {
                println!("{l}");
            }
            for w in &report.warnings {
                println!("warning: {w}");
            }
            for f in &report.failures {
                println!("refused: {f}");
            }
            let (result, code) = if !report.failures.is_empty() {
                ("refused", 1)
            } else if !report.warnings.is_empty() {
                ("warn", 3)
            } else {
                ("ok", 0)
            };
            println!("result={result}");
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
    }
}

/// `profile new`/`check`'s compose prediction from the three compose inputs.
fn predict(
    compose_file: &Path,
    reference: &Path,
    env_names: &Path,
) -> Result<(String, String), String> {
    let compose = std::fs::read_to_string(compose_file)
        .map_err(|e| format!("{}: {e}", compose_file.display()))?;
    let names = ta::env_names(
        &std::fs::read_to_string(env_names).map_err(|e| format!("{}: {e}", env_names.display()))?,
    )
    .map_err(|e| e.to_string())?;
    ta::predict_compose_hash(&read_file(reference)?, &compose, &names).map_err(|e| e.to_string())
}

/// The platform fields the prediction inherits from the reference (read them:
/// the reference must be the authority's own known-good deployment).
fn print_reference(reference: &Path) -> Result<(), String> {
    for f in ta::reference_platform_fields(&read_file(reference)?).map_err(|e| e.to_string())? {
        println!("reference.{f}");
    }
    Ok(())
}

fn run_profile(cmd: ProfileCmd) -> Result<(), String> {
    match cmd {
        ProfileCmd::New {
            id,
            class,
            not_before,
            expiry,
            evidence,
            hwmodel,
            min_driver_version,
            min_vbios_version,
            measurements,
            os_image_digest,
            compose_file,
            compose_reference,
            env_names,
            out,
        } => {
            let (served, predicted) = predict(&compose_file, &compose_reference, &env_names)?;
            let p = ta::new_profile(ta::NewProfile {
                id,
                class,
                not_before,
                expiry,
                evidence,
                hwmodels: hwmodel,
                min_driver_version,
                min_vbios_version,
                measurements: read_json(&measurements)?,
                os_image_digest,
                compose_hash: predicted.clone(),
            })
            .map_err(|e| e.to_string())?;
            write_json(&out, &p)?;
            println!(
                "profile={}\nserved_compose_hash={served}\ncompose_hash={predicted}\npinned=false",
                p.id
            );
            print_reference(&compose_reference)?;
            Ok(())
        }
        ProfileCmd::Pin {
            profile,
            capture,
            app_id,
            key_provider_override,
            out,
        } => {
            let p: HostProfile = read_json(&profile)?;
            let id = ta::capture_identity(&read_file(&capture.join("request.json"))?)
                .map_err(|e| e.to_string())?;
            let pinned = ta::pin_profile(p, &id, &app_id, key_provider_override.as_deref())
                .map_err(|e| e.to_string())?;
            if let Some(notice) = pinned.notice {
                eprintln!("fabstir-kbs: {notice}");
            }
            write_json(&out, &pinned.profile)?;
            println!("profile={}\npinned=true", pinned.profile.id);
            Ok(())
        }
        ProfileCmd::Check {
            profile,
            measurements,
            os_image_digest,
            compose_file,
            compose_reference,
            env_names,
        } => {
            let p: HostProfile = read_json(&profile)?;
            let (_, predicted) = predict(&compose_file, &compose_reference, &env_names)?;
            let rows =
                ta::check_profile(&p, &read_json(&measurements)?, &os_image_digest, &predicted);
            if !rows.is_empty() {
                return Err(format!(
                    "profile {} differs from the prediction: {}",
                    p.id,
                    rows.join("; ")
                ));
            }
            println!("profile={}\nprofile_check=ok", p.id);
            print_reference(&compose_reference)?;
            Ok(())
        }
        ProfileCmd::Assemble {
            root,
            epoch,
            issued_at,
            previous,
            first,
            profile,
            out,
        } => {
            let prior = prior(previous, first)?;
            let mut profiles = Vec::new();
            for p in &profile {
                profiles.push(read_json::<HostProfile>(p)?);
            }
            let s = ta::assemble_set(&root, epoch, issued_at, &prior, profiles)
                .map_err(|e| e.to_string())?;
            write_json(&out, &s)?;
            println!(
                "root={}\nepoch={}\nprofiles={}",
                s.root,
                s.epoch,
                s.profiles.len()
            );
            Ok(())
        }
        ProfileCmd::Sign {
            set,
            key_file,
            manifest,
            out,
            append,
        } => {
            let s: HostProfileSet = read_json(&set)?;
            let m: SignedAuthorityManifest = read_json(&manifest)?;
            let append = append
                .map(|a| read_json::<SignedHostProfileSet>(&a))
                .transpose()?;
            let sk = read_key(&key_file)?;
            let signed = ta::sign_set(s, &sk, &m, append).map_err(|e| e.to_string())?;
            write_json(&out, &signed)?;
            println!(
                "epoch={}\nsignatures={}",
                signed.set.epoch,
                signed.signatures.len()
            );
            Ok(())
        }
    }
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
        // Policy schema 3 (D10, R3): the HWM and the archive must persist, or a
        // rollback after a restart would go unnoticed.
        (cfg.hwm_dir(), "hwm dir"),
        (cfg.archive_dir(), "archive dir"),
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
    // Reconcile every HWM with its archive (D10) BEFORE logging it: a crash
    // between an archive write and its HWM write, or a lost / stale HWM, is
    // walked forward through the re-verified archive (CRITICAL log per step);
    // an entry that does not verify, or a stray file, refuses to start (78).
    match state.authority.reconcile_with_archive() {
        Ok(done) => {
            for line in done {
                tracing::error!("CRITICAL: {line}");
            }
        }
        Err(e) => refuse("hwm", e),
    }
    // Every root's high-water mark at start (D10): after a backup restore the
    // operator compares these with the served bundles' digests.
    match state.authority.hwm().roots() {
        Ok(roots) => {
            for root in roots {
                match state.authority.hwm().read(&root) {
                    Ok(Some(h)) => tracing::info!(
                        root,
                        manifest_version = h.manifest_version,
                        epoch = h.epoch,
                        manifest_digest = &h.manifest_digest[..16],
                        set_digest = &h.set_digest[..16],
                        "authority high-water mark"
                    ),
                    Ok(None) => {}
                    Err(e) => refuse("hwm", e),
                }
            }
        }
        Err(e) => refuse("hwm dir", e),
    }
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
