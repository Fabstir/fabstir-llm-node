// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use ethers::signers::Signer;
use fabstir_llm_node::{
    api::{ApiConfig, ApiServer},
    contracts::{
        checkpoint_manager::CheckpointManager,
        model_registry::ModelRegistryClient,
        tx_queue::{TransactionQueue, TxQueueConfig},
        Web3Client, Web3Config,
    },
    inference::{EngineConfig, LlmEngine, ModelConfig},
    model_validation::ModelValidator,
    p2p::{Node, NodeEvent},
    p2p_config::NodeConfig,
};
use std::{env, path::PathBuf, sync::Arc, time::Duration};

/// SIGINT (Ctrl-C) and SIGTERM (`docker stop`, a CVM stop) as streams, installed
/// as the first thing `node_main` does, on EVERY node, and owned by
/// [`stop_watchdog`] on its own thread for the life of the process. Two reasons it is unconditional: on the attested path
/// a stop during the boot window (policy fetch, a long container download,
/// decrypt, model load) must purge a plaintext already on tmpfs instead of
/// taking the default disposition; and inside the container the node is PID 1
/// of its namespace, where the kernel DROPS every signal that has no handler
/// (`pid_namespaces(7)`), so without the streams a plain node would ignore
/// `docker stop` until the daemon's SIGKILL.
struct StopSignals {
    term: tokio::signal::unix::Signal,
    int: tokio::signal::unix::Signal,
}

impl StopSignals {
    fn install() -> std::io::Result<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Ok(Self {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
        })
    }

    /// Resolves on the next SIGINT or SIGTERM.
    async fn recv(&mut self) {
        tokio::select! {
            _ = self.term.recv() => {}
            _ = self.int.recv() => {}
        }
    }
}

/// Where the process is, for [`stop_watchdog`]: before serving (the attested
/// load, the model load, P2P, API, registration, sidecars, a refusal pause) or
/// serving (the orderly shutdown belongs to main).
const PHASE_STARTING: u8 = 0;
const PHASE_SERVING: u8 = 1;

/// The one place stop signals are received. First signal: while STARTING,
/// unlink every plaintext the attested loader has on disk in any state (the
/// loader registers each file BEFORE decrypting it, so a decrypt in progress on
/// the main thread, the hash step, a cached or an already-mapped file are all
/// reachable; never overwrite, a mapping may be open) and exit at once, with
/// no hand-off to main: main may be inside a minutes-long synchronous stretch
/// (the decrypt, the model load) and a `docker stop` gives it ten seconds. A
/// decrypt still writing continues into an unlinked inode until the process
/// ends. While SERVING, hand the signal to main's orderly shutdown and give it
/// ORDERLY_BOUND to finish, else exit the same emergency way. Second signal,
/// whenever: exit now (0). The streams live here for the whole
/// process, because tokio never uninstalls a handler and a dropped stream
/// would swallow every later signal.
async fn stop_watchdog(
    mut stop: StopSignals,
    phase: Arc<std::sync::atomic::AtomicU8>,
    loader: LoaderSlot,
    notify: Arc<tokio::sync::Notify>,
) {
    // Unlink FIRST, log AFTER, and only through `raw_stderr` (a bare write(2),
    // no Rust lock): the main thread may be blocked inside its own `println!`
    // or `eprintln!` on a stalled log pipe (Docker log driver back-pressure
    // during the long "Loading model" stretch), and this exit must never wait
    // on either lock before the plaintext is gone, nor after.
    let unlink = |slot: &LoaderSlot| -> usize {
        let l = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
        l.map(|l| l.unlink_live_plaintexts()).unwrap_or(0)
    };
    stop.recv().await;
    if phase.load(std::sync::atomic::Ordering::SeqCst) != PHASE_SERVING {
        let n = unlink(&loader);
        raw_stderr(&format!(
            "⏹️  Stop signal during start-up; not starting the node.{}\n",
            if n > 0 {
                format!(" Unlinked {n} attested plaintext file(s).")
            } else {
                String::new()
            }
        ));
        exit_now(0);
    }
    // A permit is stored if main is not yet waiting, so the signal is never
    // lost across the SERVING transition. main's orderly path ends in
    // `exit_now(0)` itself; if it has not ended the process within
    // ORDERLY_BOUND (main still inside a stalled post-API setup step, or a
    // drain that will not finish), exit here the emergency way, inside the
    // service manager's grace. A second signal does the same at once.
    notify.notify_one();
    let why = tokio::select! {
        _ = stop.recv() => "Second stop signal",
        _ = tokio::time::sleep(ORDERLY_BOUND) => "Orderly shutdown did not complete in time",
    };
    let n = unlink(&loader);
    raw_stderr(&format!(
        "⏹️  {why}: exiting now.{}\n",
        if n > 0 {
            format!(" Unlinked {n} attested plaintext file(s).")
        } else {
            String::new()
        }
    ));
    // Exit 0: a requested stop, however abrupt, must not read as a failure to
    // `restart: on-failure`, or the policy would resurrect the node and re-run
    // the whole attested boot.
    exit_now(0);
}

/// How long the watchdog gives main's orderly shutdown (API drain bound 5 s +
/// P2P leave) before it ends the process itself; inside docker's 10 s grace.
const ORDERLY_BOUND: std::time::Duration = std::time::Duration::from_secs(8);

/// Run [`stop_watchdog`] on its OWN OS thread with its own single-threaded
/// runtime (and therefore its own signal driver), so it can never be starved:
/// the node runtime's workers can all be blocked at once by synchronous work
/// (N concurrent non-streaming decodes on an N-vCPU box hold every worker on
/// the engine's mutex), and a watchdog task sharing those workers would then
/// never be polled, leaving a `docker stop` to the daemon's SIGKILL. tokio's
/// signal registry is process-global: every stream on any runtime is woken.
fn spawn_stop_watchdog(
    phase: Arc<std::sync::atomic::AtomicU8>,
    loader: LoaderSlot,
    notify: Arc<tokio::sync::Notify>,
) -> Result<()> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::io::Result<()>>();
    std::thread::Builder::new()
        .name("stop-watchdog".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            rt.block_on(async move {
                let stop = match StopSignals::install() {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                stop_watchdog(stop, phase, loader, notify).await;
            });
        })?;
    // Do not proceed until the handlers are installed: from here on a signal
    // is never dropped.
    ready_rx.recv().map_err(|_| {
        anyhow::anyhow!("stop watchdog thread died before installing its handlers")
    })??;
    Ok(())
}

/// Exit code for a refused TEE configuration / attested load (EX_CONFIG).
const EXIT_TEE_REFUSED: i32 = 78;
/// How long a refused attested load waits before exiting, so that a
/// container restart policy cannot hot-loop a permanent refusal (each attempt
/// re-downloads the container, burns a broker nonce, collects GPU evidence and
/// decrypts). The composes also cap restarts (`restart: on-failure:5`, guard).
const REFUSAL_PAUSE: std::time::Duration = std::time::Duration::from_secs(20);

/// Exit with `EXIT_TEE_REFUSED` after `REFUSAL_PAUSE`. A stop during the pause
/// ends it at once: the phase is STARTING, so the watchdog exits the process.
async fn exit_refused() -> ! {
    // Main-thread caller: flushing stdout here is safe (it owns any partial line).
    use std::io::Write;
    let _ = std::io::stdout().flush();
    eprintln!(
        "   Pausing {}s before exit {EXIT_TEE_REFUSED} so a restart policy cannot hot-loop this refusal.",
        REFUSAL_PAUSE.as_secs()
    );
    tokio::time::sleep(REFUSAL_PAUSE).await;
    // `exit_now`: one of these refusals follows a FAILED model load, after
    // `LlamaBackend::init` may have created a CUDA context; atexit's teardown
    // could hang and turn the 78 into the daemon's 137.
    exit_now(EXIT_TEE_REFUSED)
}

/// Exit from the watchdog thread WITHOUT running atexit handlers: the main
/// thread may be inside a synchronous model load (CUDA on the GPU compose),
/// and `process::exit` would run the CUDA runtime's teardown against a context
/// another thread is actively using, which can hang until the service
/// manager's SIGKILL. Nothing needs a destructor here, and NOTHING is flushed:
/// stdout is line-buffered (every complete line already landed), stderr is
/// unbuffered, and a flush would only be a lock acquisition the watchdog must
/// never wait on. Its own log lines go through `raw_stderr`.
fn exit_now(code: i32) -> ! {
    // No flush of either stream: stdout is line-buffered (every complete line
    // already landed) and stderr is unbuffered, so a flush would only be a lock
    // acquisition, and the watchdog must never wait on a lock a stalled main
    // thread holds. Its own log lines go through `raw_stderr`.
    // SAFETY: `_exit` only terminates the process; no Rust state is touched after it.
    unsafe { libc::_exit(code) }
}

use fabstir_llm_node::tee::raw_stderr;

/// Parse `API_PORT` / `P2P_PORT` and probe-bind the API port and the three
/// P2P ports (TCP p, TCP p+1, UDP p+2), releasing them at once. A failure here
/// is the same failure `ApiServer::new` / `Node::new` would hit minutes later,
/// after the attested load; catching it first makes it cost nothing. The
/// probe-then-real-bind gap is a benign race (the real bind reports the same
/// error, just later).
fn preflight_ports(api_port: &str, p2p_port: &str) -> Result<()> {
    let api: u16 = api_port
        .parse()
        .map_err(|_| anyhow::anyhow!("API_PORT must be a port number, got {api_port:?}"))?;
    let p2p: u16 = p2p_port
        .parse()
        .map_err(|_| anyhow::anyhow!("P2P_PORT must be a port number, got {p2p_port:?}"))?;
    let p2p_hi = p2p
        .checked_add(2)
        .ok_or_else(|| anyhow::anyhow!("P2P_PORT {p2p} + 2 exceeds 65535"))?;
    for (what, port) in [
        ("API_PORT", api),
        ("P2P_PORT", p2p),
        ("P2P_PORT+1", p2p + 1),
    ] {
        std::net::TcpListener::bind(("0.0.0.0", port))
            .map_err(|e| anyhow::anyhow!("{what}: cannot bind TCP {port}: {e}"))?;
    }
    std::net::UdpSocket::bind(("0.0.0.0", p2p_hi))
        .map_err(|e| anyhow::anyhow!("P2P_PORT+2: cannot bind UDP {p2p_hi}: {e}"))?;
    Ok(())
}

/// The attested loader, published by the load as soon as it exists so the
/// watchdog's emergency exit can reach plaintexts the load has not handed to
/// `main` yet.
type LoaderSlot = Arc<std::sync::Mutex<Option<Arc<fabstir_llm_node::tee::EncryptedModelLoader>>>>;

fn main() -> Result<()> {
    // `block_on` runs `node_main` on this (main) thread. The stop watchdog does
    // NOT depend on these workers (it has its own thread and runtime, see
    // `spawn_stop_watchdog`); two workers minimum is simply a floor for the
    // node's own tasks on a 1-vCPU CVM or under a compose `cpus:` limit.
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(2);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<()> {
    let result = node_main().await;
    if result.is_err() && fabstir_llm_node::tee::attested_model_id().is_some() {
        // A `?` failure after the attested decrypt (P2P/API start, an address
        // parse): `node_main`'s locals are already dropped, so the plaintext is
        // unlinked (`Drop for AttestedLoad`). Pause like a refusal, so a capped
        // restart policy does not re-fetch, re-attest and re-decrypt five times
        // back to back. A stop during the pause is handled by the watchdog:
        // at once while the phase is STARTING (every `?` today is before the
        // SERVING store), or within its 8 s bound if a fallible step is ever
        // added after it.
        // The cause first (a stop during the pause ends the process before
        // anything after the sleep), then the pause.
        if let Err(e) = &result {
            eprintln!("❌ Attested node failed to start: {e:#}");
        }
        eprintln!(
            "   Pausing {}s before exit {EXIT_TEE_REFUSED} so a restart policy cannot hot-loop it.",
            REFUSAL_PAUSE.as_secs()
        );
        tokio::time::sleep(REFUSAL_PAUSE).await;
        // Exit like a refusal (78, documented) and via `_exit`: the model is
        // loaded (a CUDA context may exist) and returning would run atexit's
        // teardown under it and then drop the runtime.
        exit_now(EXIT_TEE_REFUSED);
    }
    result
}

async fn node_main() -> Result<()> {
    // Stop watchdog FIRST, before anything slow or blocking (tracing init, the
    // engine's backend/CUDA init, the attested load): the node is PID 1 of its
    // container namespace, where an unhandled SIGTERM is dropped, so from the
    // first instruction on a `docker stop` must find a handler. See
    // `StopSignals` / `stop_watchdog`.
    let phase = Arc::new(std::sync::atomic::AtomicU8::new(PHASE_STARTING));
    let loader_slot: LoaderSlot = Arc::default();
    let stop_notify = Arc::new(tokio::sync::Notify::new());
    spawn_stop_watchdog(
        Arc::clone(&phase),
        Arc::clone(&loader_slot),
        Arc::clone(&stop_notify),
    )?;

    // Initialize tracing subscriber for logging
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    tracing_subscriber::fmt::init();

    println!("🚀 Starting Fabstir LLM Node...\n");
    println!("📦 BUILD VERSION: {}", fabstir_llm_node::version::VERSION);
    println!("📅 Build Date: {}", fabstir_llm_node::version::BUILD_DATE);
    println!();

    // Parse environment variables for configuration
    // Trimmed ONCE here: the preflight and every later consumer (multiaddr
    // parses, the API listen address) must see the same string.
    let p2p_port = env::var("P2P_PORT")
        .unwrap_or_else(|_| "9000".to_string())
        .trim()
        .to_string();
    let api_port = env::var("API_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .trim()
        .to_string();
    let model_path = env::var("MODEL_PATH")
        .unwrap_or_else(|_| "./models/tiny-vicuna-1b.q4_k_m.gguf".to_string());
    let gpu_layers = env::var("GPU_LAYERS")
        .unwrap_or_else(|_| "35".to_string())
        .parse::<usize>()
        .unwrap_or(35); // Default to GPU acceleration

    // Configure and initialize inference engine
    println!("🧠 Initializing LLM inference engine...");

    // Read batch size from environment variable
    let batch_size = env::var("LLAMA_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(2048);

    // Read max context length from environment variable
    let max_context_length = env::var("MAX_CONTEXT_LENGTH")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8192);

    // Read KV cache type from environment variable (sets both K and V)
    let kv_cache_type = env::var("KV_CACHE_TYPE").ok();

    let engine_config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length,
        gpu_layers,
        thread_count: 8,
        batch_size,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 4,
        model_eviction_policy: "lru".to_string(),
        kv_cache_type_k: kv_cache_type.clone(),
        kv_cache_type_v: kv_cache_type,
    };

    let mut llm_engine = LlmEngine::new(engine_config).await?;
    println!("✅ Inference engine initialized");

    // ========================================================================
    // Model Authorization Validation (Phase 2.2 - v8.14.0)
    // ========================================================================
    // If REQUIRE_MODEL_VALIDATION=true, validate model before loading.
    // Default is false (disabled) for v8.14.0 gradual rollout.
    let model_path_buf = PathBuf::from(&model_path);
    let mut semantic_model_id: Option<ethers::types::H256> = None;

    // Same truthiness (trimmed, case-insensitive) as tee::live's reading of the
    // variable, so the banner and the attested path's refusal never disagree.
    let validation_enabled = env::var("REQUIRE_MODEL_VALIDATION")
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("true") || v == "1"
        })
        .unwrap_or(false);

    // DISABLE_LLM turns this into a sidecar-only node (e.g. a dedicated LTX/ComfyUI
    // box): skip the LLM GGUF load entirely so no VRAM is spent on inference the host
    // isn't serving. Downstream already tolerates an unloaded engine (the load-failure
    // path continues with an empty model_id), so this is the same state, reached on
    // purpose. Sidecars, the API and the WS/LTX pipeline all still start.
    // Same truthiness (trimmed, case-insensitive) as tee::live's reading of it.
    let disable_llm = env::var("DISABLE_LLM")
        .map(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("true") || v == "1"
        })
        .unwrap_or(false);

    // ========================================================================
    // Phase 5 — the attested load path (TEE). Decided before any model load:
    // HOST_TEE_ENABLED=true means the model MUST come from an attested decrypt
    // (challenge → dstack quote + GPU evidence → key broker → DEK → tmpfs) or
    // the node does not start. This is the guarantee behind the `tee-attested`
    // advert. See `tee::live` for the full env contract.
    // ========================================================================
    // Cheap, deterministic configuration errors FIRST, before anything that
    // costs a container download, a broker nonce, a decrypt and a model load:
    // an unparsable port or an API/P2P port already bound would otherwise be
    // discovered only after all of that, five times over under the restart cap.
    if let Err(e) = preflight_ports(&api_port, &p2p_port) {
        eprintln!("❌ {e}");
        if fabstir_llm_node::tee::host_tee_enabled() {
            // Attested node: the paused, documented 78, so the restart cap
            // does not spin.
            exit_refused().await;
        }
        // Plain node: the same code and no pause, as the later bind error gave
        // (78 is reserved for TEE refusals).
        std::process::exit(1);
    }

    // The load publishes its loader to the watchdog (spawned at the top of
    // `node_main`) as soon as it exists, so a stop at any point of the load
    // (download, decrypt, hash step) unlinks whatever is on disk and exits
    // without waiting for main. The orchestration's own guard still covers
    // every `Err` return of the load.
    let attested = {
        let slot = Arc::clone(&loader_slot);
        fabstir_llm_node::tee::live::AttestedLoad::from_env_with_loader_hook(move |l| {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(l));
        })
        .await
    };
    let attested = match attested {
        Ok(a) => a,
        Err(e) => {
            eprintln!("❌ TEE attested load refused: {e}");
            if fabstir_llm_node::tee::host_tee_enabled() {
                eprintln!("   HOST_TEE_ENABLED=true nodes never start without an attested model.");
            } else {
                eprintln!("   This is a plain node (HOST_TEE_ENABLED is not true); remove the TEE_* variables or set the flag.");
            }
            exit_refused().await;
        }
    };

    // Plain-model validation only applies to a plain MODEL_PATH; the attested path
    // binds the decrypted weights to the on-chain hash itself (TEE_EXPECTED_MODEL_SHA256).
    if validation_enabled && !disable_llm && attested.is_none() {
        println!("🔒 Model validation ENABLED - validating before loading...");

        // Check if HOST_PRIVATE_KEY is available (needed for host address)
        match env::var("HOST_PRIVATE_KEY") {
            Ok(host_private_key) => {
                // Get required contract addresses from environment
                let model_registry_addr = env::var("CONTRACT_MODEL_REGISTRY")
                    .unwrap_or_else(|_| "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2".to_string());
                let node_registry_addr = env::var("CONTRACT_NODE_REGISTRY")
                    .unwrap_or_else(|_| "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22".to_string());
                let rpc_url = env::var("BASE_SEPOLIA_RPC_URL")
                    .or_else(|_| env::var("RPC_URL"))
                    .unwrap_or_else(|_| "https://sepolia.base.org".to_string());

                // Parse addresses
                let model_registry_address: ethers::types::Address = model_registry_addr
                    .parse()
                    .expect("Invalid MODEL_REGISTRY address");
                let node_registry_address: ethers::types::Address = node_registry_addr
                    .parse()
                    .expect("Invalid NODE_REGISTRY address");

                // Extract host address from private key
                let wallet: ethers::signers::LocalWallet =
                    host_private_key.parse().expect("Invalid HOST_PRIVATE_KEY");
                let host_address = ethers::signers::Signer::address(&wallet);

                println!(
                    "   Host address: 0x{}",
                    hex::encode(host_address.as_bytes())
                );
                println!("   Model registry: {}", model_registry_addr);
                println!("   Node registry: {}", node_registry_addr);

                // Initialize Web3 provider for validation
                let provider =
                    ethers::providers::Provider::<ethers::providers::Http>::try_from(&rpc_url)
                        .expect("Failed to create provider");
                let provider = Arc::new(provider);

                // Create ModelRegistryClient
                match ModelRegistryClient::new(
                    provider.clone(),
                    model_registry_address,
                    Some(node_registry_address),
                )
                .await
                {
                    Ok(model_registry_client) => {
                        let model_registry = Arc::new(model_registry_client);

                        // Create dummy Web3Client (for ModelValidator interface)
                        // Note: We only need the provider for validation queries
                        let web3_config = Web3Config {
                            rpc_url: rpc_url.clone(),
                            chain_id: 84532,
                            private_key: Some(host_private_key.clone()),
                            ..Default::default()
                        };

                        match Web3Client::new(web3_config).await {
                            Ok(web3_client) => {
                                let web3_client = Arc::new(web3_client);

                                // Create ModelValidator
                                let validator = ModelValidator::new(
                                    model_registry.clone(),
                                    node_registry_address,
                                    web3_client,
                                );

                                // Build dynamic model map from contract
                                println!("📋 Building dynamic model map from contract...");
                                if let Err(e) = validator.build_model_map().await {
                                    eprintln!("❌ Failed to build model map: {}", e);
                                    eprintln!("   Cannot validate model without contract access.");
                                    std::process::exit(1);
                                }

                                // Validate model at startup
                                match validator
                                    .validate_model_at_startup(&model_path_buf, host_address)
                                    .await
                                {
                                    Ok(model_id) => {
                                        println!(
                                            "✅ Model authorization verified: 0x{}",
                                            hex::encode(&model_id.0)
                                        );
                                        semantic_model_id = Some(model_id);
                                    }
                                    Err(e) => {
                                        eprintln!("❌ Model validation FAILED: {}", e);
                                        eprintln!("");
                                        eprintln!("   Your MODEL_PATH does not match a model you're registered for.");
                                        eprintln!("   Either:");
                                        eprintln!(
                                            "     1. Register this model in NodeRegistry contract"
                                        );
                                        eprintln!("     2. Change MODEL_PATH to a model you're registered for");
                                        eprintln!("     3. Disable validation: REQUIRE_MODEL_VALIDATION=false");
                                        eprintln!("");
                                        std::process::exit(1);
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!(
                                    "❌ Failed to initialize Web3Client for validation: {}",
                                    e
                                );
                                eprintln!("   Cannot validate model without contract access.");
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to initialize ModelRegistryClient: {}", e);
                        eprintln!("   Cannot validate model without contract access.");
                        std::process::exit(1);
                    }
                }
            }
            Err(_) => {
                eprintln!("❌ REQUIRE_MODEL_VALIDATION=true but HOST_PRIVATE_KEY not set!");
                eprintln!(
                    "   Model validation requires HOST_PRIVATE_KEY to determine host address."
                );
                eprintln!("   Either:");
                eprintln!("     1. Set HOST_PRIVATE_KEY environment variable");
                eprintln!("     2. Disable validation: REQUIRE_MODEL_VALIDATION=false");
                std::process::exit(1);
            }
        }
    } else if let Some(a) = &attested {
        // The attested path's validation is the hash binding done inside
        // `prepare_attested_model`; `tee::live` refuses REQUIRE_MODEL_VALIDATION=true
        // without TEE_EXPECTED_MODEL_SHA256, so "enabled" here always means "bound".
        if validation_enabled {
            println!(
                "🔒 Model validation ENABLED (attested path): plaintext bound to TEE_EXPECTED_MODEL_SHA256 for model {}",
                hex::encode(a.prepared.model_id)
            );
            println!(
                "   Note: the NodeRegistry host-authorisation check of the plain path is NOT run here; \
                 an unregistered (host, model) pair is simply never routed sessions"
            );
        } else {
            println!("ℹ️  Model validation DISABLED on the attested path (TEE_EXPECTED_MODEL_SHA256 binding only if set)");
        }
    } else {
        println!("ℹ️  Model validation DISABLED (set REQUIRE_MODEL_VALIDATION=true to enable)");
    }

    // ========================================================================
    // Load the GGUF model (after validation)
    // ========================================================================
    let mut model_id = String::new();

    let (load_path, encrypted) = match &attested {
        Some(a) => {
            println!(
                "🔐 Attested model decrypted to {} (model {}{})",
                a.path.display(),
                hex::encode(a.prepared.model_id),
                if a.test_release {
                    "; TEST-keyring release"
                } else {
                    ""
                }
            );
            (a.path.clone(), true)
        }
        None => (model_path_buf, false),
    };

    if disable_llm {
        println!(
            "🚫 DISABLE_LLM set — skipping LLM model load (sidecar-only node; \
             LLM inference disabled, sidecars/API still active)"
        );
    } else if load_path.exists() {
        println!("📦 Loading model: {}", load_path.display());
        let model_config = ModelConfig {
            model_path: load_path,
            model_type: "llama".to_string(),
            context_size: max_context_length,
            gpu_layers,
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
            chat_template: None, // Use model's default chat template
            encrypted,           // true iff the weights came from the attested decrypt above
        };

        // Pass semantic_model_id if validation was performed
        // Note: In Phase 4, load_model will accept this parameter
        let _ = semantic_model_id; // Suppress unused warning until Phase 4

        match llm_engine.load_model(model_config).await {
            Ok(id) => {
                model_id = id.clone();
                println!("✅ Model loaded successfully (ID: {})", id);
                println!("   GPU layers: {}", gpu_layers);
                println!("   Context size: {} tokens", max_context_length);
                println!("   Batch size: {} tokens", batch_size);
                if semantic_model_id.is_some() {
                    println!(
                        "   Contract model ID: 0x{}",
                        hex::encode(&semantic_model_id.unwrap().0[..8])
                    );
                }
            }
            Err(e) => {
                eprintln!("❌ Failed to load model: {}", e);
                if let Some(a) = &attested {
                    // The invariant behind the `tee-attested` advert: an attested node
                    // has a loaded attested model or does not run. Wipe the plaintext
                    // and exit; never boot into "inference won't work" while advertising.
                    a.release();
                    eprintln!("   Attested model failed to load: plaintext released, exiting.");
                    exit_refused().await;
                }
                eprintln!("   The node will start but inference won't work.");
            }
        }
    } else {
        if let Some(a) = &attested {
            a.release();
            eprintln!(
                "❌ Attested model path vanished before load: {}",
                load_path.display()
            );
            exit_refused().await;
        }
        eprintln!("⚠️  Model file not found at: {}", model_path);
        eprintln!("   Please ensure the GGUF model file exists.");
        return Err(anyhow::anyhow!("Model file not found"));
    }

    // Configure P2P node
    println!("\n📡 Configuring P2P networking...");
    let node_config = NodeConfig {
        listen_addresses: vec![
            format!("/ip4/0.0.0.0/tcp/{}", p2p_port).parse()?,
            format!("/ip4/0.0.0.0/tcp/{}", p2p_port.parse::<u16>()? + 1).parse()?,
            format!("/ip4/0.0.0.0/udp/{}/quic-v1", p2p_port.parse::<u16>()? + 2).parse()?,
        ],
        capabilities: vec![
            "llama".to_string(),
            "vicuna".to_string(),
            "tiny-vicuna".to_string(),
            "inference".to_string(),
        ],
        enable_mdns: true,
        enable_auto_reconnect: true,
        ..Default::default()
    };

    // Create and start P2P node
    let mut p2p_node = Node::new(node_config).await?;
    let peer_id = p2p_node.peer_id();
    println!("✅ P2P node created with ID: {}", peer_id);

    let mut event_receiver = p2p_node.start().await;
    println!("✅ P2P node started");

    // Wait for listeners to be established
    tokio::time::sleep(Duration::from_millis(500)).await;
    let listeners = p2p_node.listeners();
    for addr in &listeners {
        println!("   Listening on: {}", addr);
    }

    // Configure and start API server
    println!("\n🌐 Starting API server...");
    let api_config = ApiConfig {
        listen_addr: format!("0.0.0.0:{}", api_port),
        enable_websocket: true,
        cors_allowed_origins: vec!["*".to_string()],
        ..Default::default()
    };

    // Create API server and pass the loaded model ID
    let api_server = ApiServer::new(api_config).await?;

    // Moderation enforce-coupling visibility (3.1.1): make fail-closed-but-
    // misconfigured states loud at startup, so an operator can't silently run with the
    // gate off or with /v1/moderate/frames rejecting every POST.
    if !api_server.moderation_enforce() {
        tracing::warn!("⚠️ MODERATION_ENFORCE is OFF — the transcode moderation gate is NOT enforced (dark-launch). Set MODERATION_ENFORCE=true at go-live once seam-#1 ingest is wired.");
    }
    if api_server.moderation_ingest_token().is_none() {
        tracing::warn!("⚠️ MODERATION_INGEST_TOKEN is unset — POST /v1/moderate/frames will REJECT every request (401). Set it on every node the transcoder POSTs to, independent of MODERATION_ENFORCE.");
    }

    api_server.set_engine(Arc::new(llm_engine)).await;
    // The node is LIVE from here (API accepting, P2P started, the model
    // served), so a stop from now on takes the orderly path (API drain, P2P
    // leave, plaintext unlink) rather than the start-up `_exit`. A signal that
    // lands during the optional setup that follows (embeddings, sidecars, LTX
    // bundle publish, training) is kept as a permit and honoured at the serving
    // wait IF main gets there within the watchdog's ORDERLY_BOUND (8 s from the
    // signal); a setup step slower than that ends in the watchdog's emergency
    // exit instead (unlink + `_exit(0)`, no drain), by design.
    phase.store(PHASE_SERVING, std::sync::atomic::Ordering::SeqCst);
    api_server
        .set_default_model_id(if model_id.is_empty() {
            "tiny-vicuna".to_string()
        } else {
            model_id
        })
        .await;

    // Initialize Embedding Model Manager for /v1/embed endpoint
    println!("🧠 Initializing embedding model manager...");

    // Create default embedding model config for all-MiniLM-L6-v2
    let embedding_configs = vec![fabstir_llm_node::embeddings::EmbeddingModelConfig {
        name: "all-MiniLM-L6-v2".to_string(),
        model_path: "./models/all-MiniLM-L6-v2-onnx/model.onnx".to_string(),
        tokenizer_path: "./models/all-MiniLM-L6-v2-onnx/tokenizer.json".to_string(),
        dimensions: 384,
    }];

    match fabstir_llm_node::embeddings::EmbeddingModelManager::new(embedding_configs).await {
        Ok(manager) => {
            let manager = Arc::new(manager);
            api_server
                .set_embedding_model_manager(manager.clone())
                .await;
            println!("✅ Embedding model manager initialized");

            // List available models
            let models = manager.list_models();
            if !models.is_empty() {
                println!("   Available embedding models:");
                for model in models {
                    println!("     - {} ({}D)", model.name, model.dimensions);
                }
            }
        }
        Err(e) => {
            println!("⚠️  Failed to initialize embedding model manager: {}", e);
            println!("   /v1/embed endpoint will return 503 Service Unavailable");
            println!("   This is optional - node will continue without embeddings");
        }
    }

    // Initialize Vision Model Manager for /v1/ocr and /v1/describe-image endpoints
    println!("👁️  Initializing vision model manager...");

    let ocr_model_path =
        env::var("OCR_MODEL_PATH").unwrap_or_else(|_| "./models/paddleocr-onnx".to_string());
    let florence_model_path =
        env::var("FLORENCE_MODEL_PATH").unwrap_or_else(|_| "./models/florence-2-onnx".to_string());

    let vlm_endpoint = env::var("VLM_ENDPOINT").ok();
    let vlm_model_name = env::var("VLM_MODEL_NAME").ok();

    if let Some(ref endpoint) = vlm_endpoint {
        println!("🔭 VLM endpoint configured: {}", endpoint);
    } else {
        println!("   No VLM_ENDPOINT set, using ONNX vision models only");
    }

    let vision_config = fabstir_llm_node::vision::VisionModelConfig {
        ocr_model_dir: Some(ocr_model_path),
        florence_model_dir: Some(florence_model_path),
        vlm_endpoint,
        vlm_model_name,
    };

    match fabstir_llm_node::vision::VisionModelManager::new(vision_config).await {
        Ok(manager) => {
            let manager = Arc::new(manager);
            api_server.set_vision_model_manager(manager.clone()).await;
            println!("✅ Vision model manager initialized");

            // List available vision models
            let models = manager.list_models();
            if !models.is_empty() {
                println!("   Available vision models:");
                for model in models {
                    let status = if model.available { "✓" } else { "✗" };
                    println!("     {} {} ({})", status, model.name, model.model_type);
                }
            } else {
                println!("   No vision models loaded");
                println!("   /v1/ocr and /v1/describe-image will return 503");
            }
        }
        Err(e) => {
            println!("⚠️  Failed to initialize vision model manager: {}", e);
            println!("   /v1/ocr and /v1/describe-image endpoints will return 503");
            println!("   This is optional - node will continue without vision models");
        }
    }

    // Initialize Diffusion Client (v8.16.0+ - image generation)
    // Optional: requires DIFFUSION_ENDPOINT env var
    let diffusion_endpoint = env::var("DIFFUSION_ENDPOINT").ok();
    let diffusion_model_name =
        env::var("DIFFUSION_MODEL_NAME").unwrap_or_else(|_| "flux2-klein-4b".to_string());

    if let Some(ref endpoint) = diffusion_endpoint {
        match fabstir_llm_node::diffusion::DiffusionClient::new(endpoint, &diffusion_model_name) {
            Ok(client) => {
                let client = Arc::new(client);
                api_server.set_diffusion_client(client).await;
                println!(
                    "🎨 Diffusion sidecar configured: endpoint={}, model={}",
                    endpoint, diffusion_model_name
                );
            }
            Err(e) => {
                println!("⚠️  Failed to create diffusion client: {}", e);
                println!("   /v1/images/generate will return 503");
            }
        }
    } else {
        println!("   No DIFFUSION_ENDPOINT set — /v1/images/generate will return 503");
    }

    // Initialize Transcoder Client (v8.25.0+ - transcoding sidecar)
    // Optional: requires TRANSCODER_ENDPOINT and FABSTIR_TRANSCODER_JWT env vars
    let transcoder_endpoint = env::var("TRANSCODER_ENDPOINT").ok();
    let transcoder_jwt = env::var("FABSTIR_TRANSCODER_JWT").ok();

    if let (Some(ref endpoint), Some(ref jwt_token)) = (&transcoder_endpoint, &transcoder_jwt) {
        match fabstir_llm_node::transcoder::TranscoderClient::new(endpoint, jwt_token) {
            Ok(client) => {
                let client = Arc::new(client);
                api_server.set_transcoder_client(client).await;
                println!("🎬 Transcoder sidecar configured: endpoint={}", endpoint);
            }
            Err(e) => {
                println!("⚠️  Failed to create transcoder client: {}", e);
                println!("   Transcoding will return 503");
            }
        }
    } else {
        println!("   No TRANSCODER_ENDPOINT set — transcoding will return 503");
    }

    // Initialize LTX 2.3 generation sidecar (ComfyUI). Optional: requires COMFY_URL.
    let comfy_url = env::var("COMFY_URL").ok();
    if let Some(ref url) = comfy_url {
        let template_dir = env::var("TEMPLATE_DIR").unwrap_or_else(|_| "./templates".to_string());
        match fabstir_llm_node::ltx::ComfyClient::new(url) {
            Ok(client) => {
                api_server.set_ltx_client(Arc::new(client)).await;
                match fabstir_llm_node::ltx::TemplateStore::new(&template_dir) {
                    Ok(store) => {
                        api_server.set_ltx_template_store(Arc::new(store)).await;
                        println!(
                            "🎬 LTX sidecar configured: endpoint={}, templates={}",
                            url, template_dir
                        );
                    }
                    Err(e) => {
                        println!(
                            "⚠️  LTX template store failed to load from {}: {}",
                            template_dir, e
                        );
                        println!("   ltx_generate will reject (no pinned templates)");
                    }
                }
            }
            Err(e) => {
                println!("⚠️  Failed to create LTX client: {}", e);
                println!("   ltx_generate will return 503");
            }
        }
    } else {
        println!("   No COMFY_URL set — ltx_generate will return 503");
    }

    // Initialize Web Search Service (v8.7.0+)
    // Enabled by default - DuckDuckGo requires no API key
    // Set WEB_SEARCH_ENABLED=false to disable
    println!("🔍 Initializing web search service...");
    let search_config = fabstir_llm_node::search::SearchConfig::from_env();
    if search_config.enabled {
        let search_service = Arc::new(fabstir_llm_node::search::SearchService::new(search_config));
        // Convert &str to owned Strings before moving the Arc
        let providers: Vec<String> = search_service
            .available_providers()
            .iter()
            .map(|s| s.to_string())
            .collect();
        api_server.set_search_service(search_service).await;
        println!("✅ Web search service initialized (enabled by default)");
        println!("   Available providers: {}", providers.join(", "));
        println!("   /v1/search endpoint enabled");
        println!("   Inference with web_search=true is supported");
    } else {
        println!("ℹ️  Web search explicitly disabled (WEB_SEARCH_ENABLED=false)");
    }

    // Initialize Web3 and CheckpointManager if HOST_PRIVATE_KEY is available
    if let Ok(host_private_key) = env::var("HOST_PRIVATE_KEY") {
        println!("🔗 Initializing Web3 client for checkpoint submission...");

        // Load RPC URL from env or use default
        let rpc_url = env::var("RPC_URL").unwrap_or_else(|_| {
            "https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR".to_string()
        });

        let web3_config = Web3Config {
            rpc_url,
            chain_id: 84532, // Base Sepolia
            private_key: Some(host_private_key),
            ..Default::default()
        };

        match Web3Client::new(web3_config.clone()).await {
            Ok(mut web3_client) => {
                // Initialize transaction queue for nonce collision prevention
                let mut _tx_queue = TransactionQueue::new(TxQueueConfig::default());
                if let Some(ref pk) = web3_config.private_key {
                    if let Ok(wallet) = pk.parse::<ethers::signers::LocalWallet>() {
                        let wallet = wallet.with_chain_id(web3_config.chain_id);
                        let signer =
                            std::sync::Arc::new(ethers::middleware::SignerMiddleware::new(
                                web3_client.provider.as_ref().clone(),
                                wallet,
                            ));
                        let sender = _tx_queue.start_chain(
                            web3_config.chain_id,
                            signer,
                            web3_client.provider.clone(),
                        );
                        web3_client.set_tx_queue_sender(sender);
                        println!(
                            "🔗 Transaction queue initialized for chain {}",
                            web3_config.chain_id
                        );
                    }
                }

                let web3_client = Arc::new(web3_client);
                match CheckpointManager::new(web3_client).await {
                    Ok(checkpoint_manager) => {
                        api_server
                            .set_checkpoint_manager(Arc::new(checkpoint_manager))
                            .await;
                        println!("✅ Checkpoint manager initialized - payments enabled!");
                        // With S5 now available, publish the LTX allow-list bundle (if the
                        // sidecar is configured) so clients can fetch + authenticate it;
                        // the logged bundleCID is what registerNode advertises / the E2E uses.
                        let _ = api_server.publish_ltx_bundle().await;
                    }
                    Err(e) => {
                        println!("⚠️  Failed to initialize checkpoint manager: {}", e);
                        println!("   Node will run but automatic checkpoint submission disabled");
                    }
                }
            }
            Err(e) => {
                println!("⚠️  Failed to initialize Web3 client: {}", e);
                println!("   Node will run but automatic checkpoint submission disabled");
            }
        }
    } else {
        println!("ℹ️  HOST_PRIVATE_KEY not set - checkpoint submission disabled");
        println!("   To enable payments, set HOST_PRIVATE_KEY environment variable");
    }

    // Training M0 (T4.5): gated on TRAIN_ENABLED; needs the checkpoint
    // manager above for chain reads/settles.
    fabstir_llm_node::training::chain::wire_training_from_env(&api_server).await;

    // The API server is already running in the background (started in new())
    // We don't need to call run() or spawn a task

    println!("✅ API server started on http://0.0.0.0:{}", api_port);

    // Print node information
    let separator = "=".repeat(60);
    println!("\n{}", separator);
    println!("🎉 Fabstir LLM Node is running with REAL inference!");
    println!("{}", separator);
    println!("Peer ID:        {}", peer_id);
    println!(
        "P2P Ports:      {}-{}",
        p2p_port,
        p2p_port.parse::<u16>()? + 2
    );
    println!("API Port:       {}", api_port);
    match &attested {
        Some(a) => println!(
            "Model:          attested {} (plaintext {})",
            hex::encode(a.prepared.model_id),
            a.path.display()
        ),
        None => println!(
            "Model:          {}",
            model_path.split('/').last().unwrap_or("unknown")
        ),
    }
    println!("GPU Layers:     {}", gpu_layers);
    println!("\nAPI Endpoints:");
    println!("  Health:       http://localhost:{}/health", api_port);
    println!("  Models:       http://localhost:{}/v1/models", api_port);
    println!(
        "  Inference:    POST http://localhost:{}/v1/inference",
        api_port
    );
    println!(
        "  Embed:        POST http://localhost:{}/v1/embed",
        api_port
    );
    println!("  OCR:          POST http://localhost:{}/v1/ocr", api_port);
    println!(
        "  Describe:     POST http://localhost:{}/v1/describe-image",
        api_port
    );
    println!("  WebSocket:    ws://localhost:{}/v1/ws", api_port);
    println!("\nTest with curl:");
    println!(
        "  curl -X POST http://localhost:{}/v1/inference \\",
        api_port
    );
    println!("    -H 'Content-Type: application/json' \\");
    println!("    -d '{{");
    println!("      \"model\": \"tiny-vicuna\",");
    println!("      \"prompt\": \"What is the capital of France?\",");
    println!("      \"max_tokens\": 50,");
    println!("      \"temperature\": 0.7");
    println!("    }}'");
    println!("\nPress Ctrl+C to shutdown...");
    println!("{}\n", separator);

    // Handle P2P events in background
    let event_handle = tokio::spawn(async move {
        while let Some(event) = event_receiver.recv().await {
            match event {
                NodeEvent::ConnectionEstablished { peer_id } => {
                    println!("📌 New peer connected: {}", peer_id);
                }
                NodeEvent::ConnectionClosed { peer_id } => {
                    println!("📤 Peer disconnected: {}", peer_id);
                }
                NodeEvent::DiscoveryEvent(e) => {
                    println!("🔍 Discovery: {:?}", e);
                }
                _ => {}
            }
        }
    });

    // Wait for a shutdown signal: SIGINT (Ctrl-C) or SIGTERM, which is what
    // `docker stop` and a CVM stop send. The watchdog owns the streams and hands
    // the first signal here since the phase became SERVING (a signal that
    // landed before this wait is kept as a permit); a second signal is its own
    // "exit now".
    stop_notify.notified().await;

    println!("\n⏹️  Shutting down...");

    // Cleanup. The API is told to stop and given a bounded drain
    // (api::server::SHUTDOWN_DRAIN). Whether the drain waits on upgraded
    // WebSocket sessions depends on hyper's upgrade handling; either way the
    // bound applies, `drained` may come back false with a session still open,
    // and a generation may still be decoding after this returns (it ends with
    // the process below). Then the node leaves the network.
    let drained = api_server.shutdown().await;
    if !drained {
        println!("⚠️  API server still draining after the shutdown bound; continuing");
    }
    p2p_node.shutdown().await;
    event_handle.abort();
    if let Some(a) = &attested {
        // Attested path: UNLINK the tmpfs plaintext (never zero it in place — a
        // generation still running would decode garbage from under its mmap and
        // stream/track those tokens; see AttestedLoad::detach_for_exit), then
        // exit at once so that generation ends with the process. The pages are
        // freed with the mapping; tmpfs has no medium to scrub.
        match a.detach_for_exit() {
            Ok(()) => println!("🔐 Attested model plaintext unlinked"),
            Err(e) => eprintln!("⚠️  could not unlink the attested plaintext: {e}"),
        }
        println!("👋 Goodbye!");
        // Exit rather than return. Returning would drop the runtime, which
        // cancels every async task (the WS session that would have tracked the
        // tokens included) but then waits for an orphaned blocking decode to
        // run out its max_tokens with nobody reading. Same billing outcome as
        // the plain node's return (tokens since the last checkpoint of a session
        // in flight at shutdown are lost on both paths; pre-existing), without
        // the wait. `exit_now`, not `process::exit`: a generation may still be
        // decoding on the GPU, and atexit's CUDA teardown under it can hang.
        exit_now(0);
    }

    println!("👋 Goodbye!");
    // Same as the attested branch: returning would drop the node runtime,
    // which waits for any blocking decode still running to reach max_tokens
    // with nobody reading (the WS task that would have tracked the tokens is
    // cancelled). The watchdog lives on its own thread and runtime, so it is
    // not what keeps this honest; the wait is. Exit instead.
    exit_now(0)
}
