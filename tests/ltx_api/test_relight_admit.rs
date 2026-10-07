// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! VFX Passes (VP1.1) D8 + D3: GPU admission, the opt-in ComfyUI handshake, the relight `/free` before LTX jobs, the
//! relight-only `/interrupt` and the whole-job deadline — driven through `handle_encrypted_ltx_generate` + `run()`
//! against the stub sidecars (`relight_stub.rs`). Relight jobs carry a real clip served by the S5 blob stub, so a
//! misplaced admission (after `prepare_inputs`) WOULD show an `/upload/image` in the call log. Each test names the
//! mutation that turns it red.

use super::ltx_task_support::{mark_pending, pending_count, S5_ENV_LOCK};
use super::relight_stub::*;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::ltx::relight::{AdmitCfg, RelightPins};
use fabstir_llm_node::ltx::ComfyClient;
use std::sync::atomic::Ordering;
use std::sync::Arc;

async fn server(
    relight: Option<Arc<ComfyClient>>,
    ltx: Option<Arc<ComfyClient>>,
    cfg: AdmitCfg,
    pins: Option<RelightPins>,
) -> Arc<ApiServer> {
    let s = Arc::new(ApiServer::new_for_test());
    if let Some(c) = relight {
        s.set_relight_client(c).await;
    }
    if let Some(c) = ltx {
        s.set_ltx_client(c).await;
    }
    s.set_ltx_template_store(store()).await;
    s.set_admit_cfg(cfg).await;
    s.set_relight_pins(pins).await;
    s
}

fn assert_err(inner: &serde_json::Value, code: &str, starts: &str) {
    assert_eq!(inner["type"], "ltx_error", "{inner:?}");
    assert_eq!(inner["error"]["code"], code, "{inner:?}");
    let msg = inner["error"]["message"].as_str().unwrap();
    assert!(msg.starts_with(starts), "{msg}");
}

#[tokio::test]
async fn busy_for_the_whole_budget_refuses_before_any_upload_and_clears_the_pending() {
    // admission after prepare_inputs -> an upload in the log -> red; no busy check -> reaches /prompt -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (st, rc) = spawn_stub("relight", log.clone()).await;
    st.busy.store(true, Ordering::SeqCst);
    let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
    mark_pending(&srv, 901).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), Some(901), true).await;
    assert_err(&inner, "CAPACITY", "GPU_BUSY:");
    assert_eq!(st.calls("/upload/image"), 0);
    assert_eq!(st.calls("/prompt"), 0);
    assert_eq!(pending_count(&srv, 901).await, 0);
}

#[tokio::test]
async fn low_vram_refuses_before_any_upload() {
    // no VRAM threshold -> admitted -> uploads -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (st, rc) = spawn_stub("relight", log.clone()).await;
    st.vram_free.store(29 * GIB, Ordering::SeqCst);
    let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "CAPACITY", "GPU_BUSY:");
    assert_eq!(st.calls("/upload/image"), 0);
}

#[tokio::test]
async fn vram_lost_while_staging_is_caught_by_the_pre_submit_repoll() {
    // no re-poll before submit -> a /prompt in the log -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (st, rc) = spawn_stub("relight", log.clone()).await;
    st.vram_after_upload.store(GIB, Ordering::SeqCst);
    let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "CAPACITY", "GPU_BUSY:");
    assert_eq!(st.calls("/upload/image"), 1, "admitted, then staged");
    assert_eq!(st.calls("/prompt"), 0);
}

#[tokio::test]
async fn pin_mismatch_refuses_and_is_not_sticky() {
    // no pin check -> admitted -> red; a sticky refusal -> the corrected job is refused too -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (st, rc) = spawn_stub("relight", log.clone()).await;
    *st.pins.lock().unwrap() = serde_json::json!({"weights": {"Diffusion_Renderer_Inverse_Cosmos_7B/model.pt": "bb"}, "stack": "stack-1"});
    let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "SIDECAR_UNAVAILABLE", "SIDECAR_PIN_MISMATCH:");
    assert_eq!(st.calls("/upload/image"), 0);
    *st.pins.lock().unwrap() = pins_json();
    let _ = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_eq!(
        st.calls("/prompt"),
        1,
        "the corrected sidecar admits the next job"
    );
}

#[tokio::test]
async fn unset_pins_refuse_every_relight_job() {
    // pins optional -> admitted -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let (st, rc) = spawn_stub("relight", Log::default()).await;
    let srv = server(Some(rc), None, test_cfg(), None).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "SIDECAR_UNAVAILABLE", "SIDECAR_PIN_MISMATCH:");
    assert!(inner["error"]["message"]
        .as_str()
        .unwrap()
        .contains("no pins configured"));
    assert_eq!(st.calls("/upload/image"), 0);
}

#[tokio::test]
async fn unreachable_relight_stats_refuse_before_any_upload() {
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let srv = server(Some(dead_client()), None, test_cfg(), Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "SIDECAR_UNAVAILABLE", "relight sidecar unreachable");
}

#[tokio::test]
async fn sidecar_gpu_busy_execution_error_maps_to_capacity() {
    // no mapping -> GENERATION_FAILED -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let (st, rc) = spawn_stub("relight", Log::default()).await;
    *st.ws_error.lock().unwrap() = Some("GPU_BUSY: 12 GiB free, 28 GiB needed".into());
    let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_eq!(st.calls("/prompt"), 1);
    assert_eq!(inner["error"]["code"], "CAPACITY", "{inner:?}");
    assert!(inner["error"]["message"]
        .as_str()
        .unwrap()
        .contains("GPU_BUSY: 12 GiB free"));
}

#[tokio::test]
async fn ltx_job_frees_the_relight_sidecar_first() {
    // no /free -> red; /free after the prompt -> order -> red
    let log = Log::default();
    let (_rs, rc) = spawn_stub("relight", log.clone()).await;
    let (_ls, lc) = spawn_stub("ltx", log.clone()).await;
    let srv = server(Some(rc), Some(lc), test_cfg(), Some(pins())).await;
    let _ = accept_and_run(&srv, &ltx_job(&store()), None, false).await;
    let l = log.lock().unwrap().clone();
    let free = l
        .iter()
        .position(|e| e.starts_with("relight /free"))
        .expect("relight /free sent");
    let prompt = l
        .iter()
        .position(|e| e == "ltx /prompt")
        .expect("ltx /prompt sent");
    assert!(free < prompt, "{l:?}");
    assert!(
        !l.iter()
            .any(|e| e == "ltx /interrupt" || e == "relight /interrupt"),
        "{l:?}"
    );
}

#[tokio::test]
async fn ltx_job_proceeds_when_the_relight_sidecar_is_down() {
    // every /free error refuses -> red
    let log = Log::default();
    let (ls, lc) = spawn_stub("ltx", log.clone()).await;
    let srv = server(Some(dead_client()), Some(lc), test_cfg(), Some(pins())).await;
    let _ = accept_and_run(&srv, &ltx_job(&store()), None, false).await;
    assert_eq!(ls.calls("/prompt"), 1);
}

#[tokio::test]
async fn ltx_job_refused_when_relight_free_fails_or_stalls() {
    // every error = proceed -> red; Other (timeout) = proceed -> red
    for (status, delay) in [(500u16, 0u64), (200, 10_000)] {
        let log = Log::default();
        let (rs, rc) = spawn_stub("relight", log.clone()).await;
        rs.free_status.store(status, Ordering::SeqCst);
        rs.free_delay_ms.store(delay, Ordering::SeqCst);
        let (ls, lc) = spawn_stub("ltx", log.clone()).await;
        let srv = server(Some(rc), Some(lc), test_cfg(), Some(pins())).await;
        let inner = accept_and_run(&srv, &ltx_job(&store()), None, false).await;
        assert_err(&inner, "CAPACITY", "GPU_BUSY:");
        assert_eq!(ls.calls("/prompt"), 0, "status {status} delay {delay}");
    }
}

#[tokio::test]
async fn handshake_on_queues_frees_and_polls_comfyui() {
    // handshake ignored -> no /queue or /free on ComfyUI -> red; low ComfyUI VRAM ignored -> admitted -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (rs, rc) = spawn_stub("relight", log.clone()).await;
    let (ls, lc) = spawn_stub("ltx", log.clone()).await;
    ls.vram_free.store(GIB, Ordering::SeqCst);
    let cfg = AdmitCfg {
        comfy_handshake: true,
        ..test_cfg()
    };
    let srv = server(Some(rc), Some(lc), cfg, Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "CAPACITY", "GPU_BUSY:");
    assert!(ls.calls("/queue") >= 1);
    let l = log.lock().unwrap().clone();
    let free = l
        .iter()
        .find(|e| e.starts_with("ltx /free"))
        .expect("ComfyUI /free sent");
    assert!(
        free.contains("\"unload_models\":true") && free.contains("\"free_memory\":true"),
        "{free}"
    );
    assert!(
        ls.stats_calls.load(Ordering::SeqCst) >= 1,
        "ComfyUI vram_free polled"
    );
    assert_eq!(rs.calls("/upload/image"), 0);
}

#[tokio::test]
async fn handshake_off_never_touches_comfyui_for_a_relight_job() {
    // handshake always on -> ComfyUI sees /queue or /free -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (_rs, rc) = spawn_stub("relight", log.clone()).await;
    let (ls, lc) = spawn_stub("ltx", log.clone()).await;
    let srv = server(Some(rc), Some(lc), test_cfg(), Some(pins())).await;
    let _ = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert!(
        !log.lock().unwrap().iter().any(|e| e.starts_with("ltx ")),
        "{:?}",
        log.lock().unwrap()
    );
    assert_eq!(ls.stats_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn relight_interrupt_after_a_watch_error_and_after_a_delivery_error_exit() {
    // no interrupt -> red; interrupt before submit (unconditional) -> the refused case below -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    for ws_error in [Some("boom".to_string()), None] {
        let (st, rc) = spawn_stub("relight", Log::default()).await;
        *st.ws_error.lock().unwrap() = ws_error.clone();
        let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
        let _ = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
        assert_eq!(st.calls("/prompt"), 1);
        assert_eq!(st.calls("/interrupt"), 1, "ws_error {ws_error:?}");
    }
    // refused at admission (nothing submitted) -> no interrupt
    let (st, rc) = spawn_stub("relight", Log::default()).await;
    st.busy.store(true, Ordering::SeqCst);
    let srv = server(Some(rc), None, test_cfg(), Some(pins())).await;
    let _ = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_eq!(st.calls("/interrupt"), 0);
}

#[tokio::test]
async fn a_render_past_the_whole_job_deadline_is_abandoned() {
    // no deadline check after watch -> the job runs on to "no frames"/storage -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let (st, rc) = spawn_stub("relight", Log::default()).await;
    st.finish_after_ms.store(2_500, Ordering::SeqCst);
    let cfg = AdmitCfg {
        deadline_secs: 2,
        ..test_cfg()
    };
    let srv = server(Some(rc), None, cfg, Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "GENERATION_FAILED", "DEADLINE:");
}

#[tokio::test]
async fn a_sidecar_still_hashing_counts_as_busy_then_admits() {
    // 503 treated as unreachable or as an instant refusal -> red; 503 never cleared -> CAPACITY
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let (st, rc) = spawn_stub("relight", Log::default()).await;
    st.stats_503.store(true, Ordering::SeqCst);
    let srv = server(Some(rc.clone()), None, test_cfg(), Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "CAPACITY", "GPU_BUSY:");
    assert!(
        inner["error"]["message"]
            .as_str()
            .unwrap()
            .contains("hashing"),
        "{inner:?}"
    );
    assert_eq!(st.calls("/upload/image"), 0);
    // hashing finishes 0.5 s into the next job's budget: admitted
    let flip = st.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        flip.stats_503.store(false, Ordering::SeqCst);
    });
    let _ = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_eq!(st.calls("/prompt"), 1);
}

#[tokio::test]
async fn handshake_waits_for_a_busy_comfyui_queue_and_never_frees_it() {
    // /free sent while ComfyUI is still running a job -> red
    let _env = S5_ENV_LOCK.lock().await;
    let clip = serve_clip(145).await;
    let log = Log::default();
    let (rs, rc) = spawn_stub("relight", log.clone()).await;
    let (ls, lc) = spawn_stub("ltx", log.clone()).await;
    ls.queue_busy.store(true, Ordering::SeqCst);
    let cfg = AdmitCfg {
        comfy_handshake: true,
        ..test_cfg()
    };
    let srv = server(Some(rc), Some(lc), cfg, Some(pins())).await;
    let inner = accept_and_run(&srv, &relight_job(&store(), 145, &clip), None, false).await;
    assert_err(&inner, "CAPACITY", "GPU_BUSY:");
    assert!(ls.calls("/queue") >= 2, "polled within the budget");
    assert_eq!(ls.calls("/free"), 0);
    assert_eq!(rs.calls("/upload/image"), 0);
}
