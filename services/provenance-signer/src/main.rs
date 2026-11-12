// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! provenance-signer subcommands:
//!   serve --config <file>
//!   gen-staging-chain --out <dir> --passphrase-file <file> [--with-intermediate]
//!   canary --config <file> [--external "<cmd>"]...
//!   reconcile --config <file>

use anyhow::{bail, Context as _};
use provenance_signer::app::{self, IndexGate};
use provenance_signer::config::Config;
use provenance_signer::keystore::KeyStore;
use provenance_signer::notify::{LogOnly, Notifier, Ntfy};
use provenance_signer::repo::Repo;
use provenance_signer::service::{system_clock, Service, Settings};
use provenance_signer::store::Store;
use provenance_signer::tsa::TsaConfig;
use provenance_signer::{canary, claim_signer, reconcile, staging_chain};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn args_all(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

fn notifier(cfg: &Config) -> Box<dyn Notifier> {
    match &cfg.ntfy_url {
        Some(u) => Box::new(Ntfy { url: u.clone() }),
        None => Box::new(LogOnly),
    }
}

/// `serving`: only the server clears the repository's staging leftovers (a tool run beside it must not).
fn build_service(cfg: &Config, serving: bool) -> anyhow::Result<Service> {
    let key = KeyStore::load(&cfg.key_path, &cfg.passphrase_path()?)
        .map_err(|e| anyhow::anyhow!("signing key: {e}"))?;
    let chain = claim_signer::load_chain(&cfg.chain_path).context("chain")?;
    let anchors = Some(std::fs::read_to_string(&cfg.trust_anchors_path).context("trust anchors")?);
    let allowlist = cfg
        .allowlist
        .iter()
        .filter_map(|a| fabstir_provenance::contract::parse_address(a))
        .collect();
    let settings = Settings {
        allowlist,
        informational_url: cfg.informational_url.clone(),
        reserve_min: cfg.reserve_min,
        reserve_max: cfg.reserve_max,
        reserve_recommended: cfg.reserve_recommended,
        min_bundle_version: cfg.min_bundle_version,
        reservations_per_minute: cfg.reservations_per_minute,
        chain_epoch: cfg.chain_epoch,
        trust_anchors: anchors,
        node_versions: cfg.node_versions.iter().cloned().collect(),
        requests_per_minute: cfg.requests_per_minute,
    };
    std::fs::create_dir_all(&cfg.data_dir)?;
    let store = Store::open(&cfg.data_dir.join("signer.sqlite"))?;
    let repo = if serving {
        Repo::open(&cfg.data_dir.join("repository"))?
    } else {
        Repo::open_shared(&cfg.data_dir.join("repository"))?
    };
    let tsa = TsaConfig {
        urls: cfg.tsa_urls.clone(),
        timeout: std::time::Duration::from_secs(cfg.tsa_timeout_secs),
    };
    Service::new(
        settings,
        store,
        repo,
        Arc::new(key),
        chain,
        tsa,
        system_clock(),
    )
}

async fn serve(cfg: Config) -> anyhow::Result<()> {
    harden_process()?;
    let svc = Arc::new(build_service(&cfg, true)?);
    let secret = std::fs::read(cfg.index_secret_file()?).context("index secret")?;
    let secret: Vec<u8> = String::from_utf8(secret)?.trim().as_bytes().to_vec();
    if secret.len() < 32 {
        bail!("index secret must be at least 32 bytes");
    }
    let allowed: Vec<IpAddr> = cfg
        .index_allowed_ips
        .iter()
        .map(|s| s.parse())
        .collect::<Result<_, _>>()
        .context("index_allowed_ips")?;
    let gate = IndexGate::new(secret, allowed);
    let api = tokio::net::TcpListener::bind(&cfg.listen_api).await?;
    let public = tokio::net::TcpListener::bind(&cfg.listen_public).await?;
    let index = tokio::net::TcpListener::bind(&cfg.listen_index).await?;
    tracing::info!(
        "provenance-signer: api {}, public {}, index {}",
        cfg.listen_api,
        cfg.listen_public,
        cfg.listen_index
    );
    // Retention: the log of requests is cut to `request_log_retention_days` now and once a day.
    let (pruner, days) = (svc.clone(), cfg.request_log_retention_days);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(86_400));
        loop {
            tick.tick().await;
            let p = pruner.clone();
            let done = tokio::task::spawn_blocking(move || {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                p.store
                    .prune_request_log(provenance_signer::store::request_log_cutoff(now, days))
            })
            .await;
            match done {
                Ok(Ok(n)) => tracing::info!(
                    "request log pruned to {days} days: audit={} reservations={} slots={}",
                    n.audit,
                    n.reservations,
                    n.slots
                ),
                Ok(Err(e)) => tracing::error!("request log prune failed: {e}"),
                Err(e) => tracing::error!("request log prune task failed: {e}"),
            }
        }
    });
    let a = axum::serve(api, app::api_router(svc.clone()).into_make_service());
    let p = axum::serve(public, app::public_router(svc.clone()).into_make_service());
    let i = axum::serve(
        index,
        app::index_router(svc.clone(), gate).into_make_service_with_connect_info::<SocketAddr>(),
    );
    tokio::select! {
        r = a => r?,
        r = p => r?,
        r = i => r?,
        _ = shutdown() => {}
    }
    Ok(())
}

/// No core dumps and no ptrace/proc-mem reads of this process: the sealed key and its wrap key live in
/// memory (C2PA Level 1 O.2 defence in depth; the unit file repeats this with LimitCORE=0).
fn harden_process() -> anyhow::Result<()> {
    unsafe {
        let zero = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::setrlimit(libc::RLIMIT_CORE, &zero) != 0 {
            bail!("setrlimit(RLIMIT_CORE, 0) failed");
        }
        if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 {
            bail!("prctl(PR_SET_DUMPABLE, 0) failed");
        }
    }
    Ok(())
}

async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let config = || -> anyhow::Result<Config> {
        Config::load(&PathBuf::from(arg(&args, "--config").context("--config")?))
    };
    match cmd {
        "serve" => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(serve(config()?)),
        "gen-staging-chain" => {
            let out = PathBuf::from(arg(&args, "--out").context("--out")?);
            let pass =
                std::fs::read(arg(&args, "--passphrase-file").context("--passphrase-file")?)?;
            let mut pass = zeroize::Zeroizing::new(pass);
            while pass.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                pass.pop();
            }
            let c = staging_chain::generate(
                args.iter().any(|a| a == "--with-intermediate"),
                &pass,
                397,
                None,
            )?;
            staging_chain::write(&out, &c)?;
            println!(
                "staging chain written to {} (ee.key.enc.pem is encrypted; CA keys discarded)",
                out.display()
            );
            Ok(())
        }
        "canary" => {
            let cfg = config()?;
            let svc = build_service(&cfg, false)?;
            let anchors = svc
                .settings
                .trust_anchors
                .clone()
                .context("canary needs trust_anchors_path")?;
            let rep = canary::run(
                &svc,
                &anchors,
                &args_all(&args, "--external"),
                &cfg.data_dir.join("canary"),
                notifier(&cfg).as_ref(),
            );
            println!(
                "canary: ok={} state={} missing={:?} failures={:?}",
                rep.ok, rep.validation_state, rep.missing, rep.failures
            );
            if rep.ok {
                Ok(())
            } else {
                bail!("canary failed")
            }
        }
        "reconcile" => {
            let cfg = config()?;
            let rc = cfg
                .reconcile
                .clone()
                .context("config has no reconcile section")?;
            let tolerance = rc.tolerance;
            let store = Store::open(&cfg.data_dir.join("signer.sqlite"))?;
            let counter = reconcile::RpcCounter {
                rpc_url: rc.rpc_url,
                job_marketplace: rc.job_marketplace,
                window_blocks: rc.window_blocks,
                chunk_blocks: rc.chunk_blocks,
            };
            let hosts: Vec<String> = cfg
                .allowlist
                .iter()
                .map(|h| h.trim_start_matches("0x").to_ascii_lowercase())
                .collect();
            let m = reconcile::reconcile_with_tolerance(
                &counter,
                |a, b| store.signatures_per_host(a, b),
                &hosts,
                tolerance,
                notifier(&cfg).as_ref(),
            )?;
            println!("reconcile: {} mismatch(es)", m.len());
            if m.is_empty() {
                Ok(())
            } else {
                bail!("mismatch")
            }
        }
        _ => {
            eprintln!("usage: provenance-signer serve|gen-staging-chain|canary|reconcile ...");
            std::process::exit(2)
        }
    }
}
