// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks, node 8.65.3 (D5): the result of a Cut-out (Alpha) job is not sent to the check service ONLY when
//! the template is `ltx-alpha-hdr` or `ltx-alpha-chain-hdr`, the prompt is exactly empty, the mode is `enforce`
//! and every input was checked and passed in this job. The service here refuses ANY result it is shown (only the
//! listed inputs and the probe clip pass), so a result that reaches it is a refusal. Each test names its mutation.

use super::checks_chain::*;
use super::checks_renderer::{run_job, serve_blobs, spawn_renderer, Ran, Renderer};
use super::checks_svc::*;
use super::ltx_task_support::{pending_count, S5_ENV_LOCK};
use super::relight_stub::mp4_clip;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{
    CHECK_UNAVAILABLE, CONTENT_REFUSED, FIXED_GRAPH_TEMPLATES, MSG_REFUSED,
};
use fabstir_llm_node::ltx::provenance::SELF_TEST_MP4;
use fabstir_llm_node::ltx::TemplateStore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) const ADDR: &str = "0xd00d000000000000000000000000000000000007";
pub(super) const ILLEGAL: Reply = Reply::Verdict("BLOCK_ILLEGAL", Some("csam"));
pub(super) const HDR: &str = "ltx-alpha-hdr";
pub(super) const CHAIN: &str = "ltx-alpha-chain-hdr";

pub(super) fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(super) fn store50() -> Arc<TemplateStore> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v50");
    Arc::new(TemplateStore::new(dir).unwrap())
}

/// A real clip that is NOT the probe clip (the service stub spares the probe).
pub(super) fn preview() -> Vec<u8> {
    let mut b = SELF_TEST_MP4.to_vec();
    b.extend_from_slice(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']);
    b
}

pub(super) fn job(
    template: &str,
    hash: Option<&str>,
    prompt: &str,
    cids: &[String],
    extra: &Value,
) -> Value {
    let st = store50();
    let mut wire = json!({
        "action": "ltx_generate", "requestId": "r-fg", "templateId": template,
        "templateHash": hash.map(str::to_string).unwrap_or_else(|| st.template_hash(template).unwrap().to_string()),
        "prompt": prompt, "seed": "42", "frames": 121, "fps": 24,
        "resolution": {"w": 1920, "h": 1088}, "lora": format!("{template}@v2"),
        "output": "exr-sequence", "videos": cids
    });
    for (k, v) in extra.as_object().into_iter().flatten() {
        wire[k] = v.clone();
    }
    wire
}

pub(super) struct Run {
    pub(super) rig: Rig,
    pub(super) renderer: Arc<Renderer>,
    pub(super) chain: Arc<Chain>,
    pub(super) ran: Ran,
    pub(super) cm: Arc<fabstir_llm_node::contracts::checkpoint_manager::CheckpointManager>,
}

/// One job through the real handler and task. `inputs` are the job's clips in order; `spared` are the ones the
/// service passes; `tweak` adjusts the rig's service before the run.
pub(super) async fn go(
    mode: Mode,
    template: &str,
    prompt: &str,
    inputs: &[Vec<u8>],
    spared: &[usize],
    hash: Option<&str>,
    jid: u64,
) -> Run {
    go_with(
        mode,
        template,
        prompt,
        inputs,
        spared,
        hash,
        jid,
        &json!({}),
    )
    .await
}

/// [`go`] with extra wire fields (`strength` and the like) merged into the job.
#[allow(clippy::too_many_arguments)]
pub(super) async fn go_with(
    mode: Mode,
    template: &str,
    prompt: &str,
    inputs: &[Vec<u8>],
    spared: &[usize],
    hash: Option<&str>,
    jid: u64,
    extra: &Value,
) -> Run {
    let rig = rig(mode).await;
    go_on(rig, template, prompt, inputs, spared, hash, jid, extra).await
}

/// [`go_with`] on a rig that already exists (its checks, and so its cleared-results record, carry over).
#[allow(clippy::too_many_arguments)]
pub(super) async fn go_on(
    rig: Rig,
    template: &str,
    prompt: &str,
    inputs: &[Vec<u8>],
    spared: &[usize],
    hash: Option<&str>,
    jid: u64,
    extra: &Value,
) -> Run {
    let mut shas: Vec<String> = spared.iter().map(|i| sha(&inputs[*i])).collect();
    if extra.get("sparePreview").is_some() {
        shas.push(sha(&preview())); // the service passes the result too
    }
    *rig.svc.block_all_but.lock().unwrap() = Some((shas, ILLEGAL));
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    *renderer.output.lock().unwrap() = Some(preview());
    let server = Arc::new(ApiServer::new_for_test());
    server.set_ltx_client(client).await;
    server.set_ltx_template_store(store50()).await;
    server.set_content_checks(rig.checks.clone());
    install_signing_provenance(&server).await;
    let st = store50();
    let model =
        fabstir_llm_node::ltx::template::expected_session_model(st.entry(template), template);
    let (chain, url) = spawn_chain(model).await;
    let cm = install_checkpoint_manager(&server, &url).await;
    let cids = serve_blobs(inputs).await;
    let wire = job(template, hash, prompt, &cids, extra);
    let ran = run_job(&server, &wire, "s-fg", Some(jid), Some(ADDR), false).await;
    if ran.accepted {
        assert_eq!(pending_count(&server, jid).await, 0);
    }
    Run {
        rig,
        renderer,
        chain,
        ran,
        cm,
    }
}

pub(super) fn source() -> Vec<u8> {
    mp4_clip(145)
}
pub(super) fn previous() -> Vec<u8> {
    mp4_clip(146)
}

pub(super) fn assert_skipped(r: &Run, logs: &Logs, template: &str, n: usize) {
    assert_eq!(
        r.ran.terminal["type"], "ltx_complete",
        "{:?}",
        r.ran.terminal
    );
    assert_eq!(r.renderer.count("prompt"), 1);
    let seen: Vec<String> = r
        .rig
        .svc
        .seen("mp4")
        .into_iter()
        .map(|f| f.sha256)
        .collect();
    assert!(!seen.contains(&sha(&preview())), "the matte was sent");
    assert_eq!(
        r.rig.svc.count("moderate mp4"),
        1 + n,
        "the probe and the inputs"
    );
    assert!(logs.text().contains(&format!(
        "verdict=SKIPPED_FIXED_GRAPH template={template} reason=\"empty prompt, inputs checked\" inputs={n}"
    )));
    assert!(r.rig.refusals().is_empty());
    assert_eq!(r.chain.claimed_tokens().len(), 1, "billed and proved");
}

pub(super) fn assert_checked(r: &Run, logs: &Logs) {
    assert_eq!(
        (r.ran.code(), r.ran.message()),
        (CONTENT_REFUSED, MSG_REFUSED),
        "{:?}",
        r.ran.terminal
    );
    assert_eq!(r.renderer.count("prompt"), 1, "rendered, then judged");
    let seen: Vec<String> = r
        .rig
        .svc
        .seen("mp4")
        .into_iter()
        .map(|f| f.sha256)
        .collect();
    assert!(seen.contains(&sha(&preview())), "the result reached c5");
    assert_eq!(r.rig.refusals()[0]["stage"], json!("c5"));
    assert!(!logs.text().contains("SKIPPED_FIXED_GRAPH"));
    assert!(r.chain.raw_txs.lock().unwrap().is_empty(), "nothing billed");
}

#[tokio::test]
async fn an_empty_prompt_cutout_is_not_sent_to_c5_and_completes() {
    // The branch removed from the handler (every route to `check_result`) -> CONTENT_REFUSED -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let r = go(Mode::Enforce, HDR, "", &[source()], &[0], None, 701).await;
    assert_skipped(&r, &logs, HDR, 1);
}

#[tokio::test]
async fn an_empty_prompt_chain_part_with_both_clips_checked_is_not_sent_to_c5() {
    // `CHAIN` missing from FIXED_GRAPH_TEMPLATES -> the matte is judged -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let ins = [source(), previous()];
    let r = go(Mode::Enforce, CHAIN, "", &ins, &[0, 1], None, 702).await;
    assert_skipped(&r, &logs, CHAIN, 2);
}

#[tokio::test]
async fn a_one_character_prompt_is_sent_to_c5_on_both_templates() {
    // `prompt.is_empty()` dropped from the rule -> the job completes unjudged -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let r = go(Mode::Enforce, HDR, "x", &[source()], &[0], None, 703).await;
    assert_checked(&r, &logs);
    let (logs, _c) = Logs::capture();
    let ins = [source(), previous()];
    let r = go(Mode::Enforce, CHAIN, "x", &ins, &[0, 1], None, 704).await;
    assert_checked(&r, &logs);
}

#[tokio::test]
async fn a_whitespace_prompt_is_a_prompt_the_graph_would_receive_it() {
    // `prompt.trim().is_empty()` in the rule -> the job completes unjudged -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let r = go(Mode::Enforce, HDR, "  \n", &[source()], &[0], None, 705).await;
    assert_checked(&r, &logs);
}

#[tokio::test]
async fn a_refused_source_still_refuses_the_job_before_any_work() {
    // The input check skipped for the alpha templates -> a render -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let r = go(Mode::Enforce, HDR, "", &[source()], &[], None, 706).await;
    assert_eq!(
        (r.ran.code(), r.ran.message()),
        (CONTENT_REFUSED, MSG_REFUSED)
    );
    assert_eq!(r.renderer.count("prompt") + r.renderer.count("upload"), 0);
    assert_eq!(r.rig.refusals()[0]["stage"], json!("c3"));
    assert!(
        logs.has("c3", "verdict=BLOCK_ILLEGAL frames=1 top=0.870")
            || logs.text().contains("stage=c3")
    );
    assert!(!logs.text().contains("SKIPPED_FIXED_GRAPH"));
    assert!(r.chain.raw_txs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_chain_part_with_a_refused_previous_part_is_refused() {
    // The previous part left out of the input check -> a render, then skipped -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let ins = [source(), previous()];
    let r = go(Mode::Enforce, CHAIN, "", &ins, &[0], None, 707).await;
    assert_eq!(
        (r.ran.code(), r.ran.message()),
        (CONTENT_REFUSED, MSG_REFUSED)
    );
    assert_eq!(r.renderer.count("prompt"), 0, "refused before any work");
    assert_eq!(r.rig.refusals()[0]["stage"], json!("c3"));
    assert!(!logs.text().contains("SKIPPED_FIXED_GRAPH"));
}

#[tokio::test]
async fn another_template_with_an_empty_prompt_is_sent_to_c5() {
    // A rule keyed on the empty prompt alone -> the edit result is skipped -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let r = go(
        Mode::Enforce,
        "ltx-edit-hdr",
        "",
        &[source()],
        &[0],
        None,
        708,
    )
    .await;
    assert_checked(&r, &logs);
}

#[tokio::test]
async fn an_unavailable_input_check_ends_the_job_and_skips_nothing() {
    // An input counted as checked without a verdict -> a render and a skip -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    *rig.svc.fail_all.lock().unwrap() = Some(Reply::NotJson);
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    *renderer.output.lock().unwrap() = Some(preview());
    let server = Arc::new(ApiServer::new_for_test());
    server.set_ltx_client(client).await;
    server.set_ltx_template_store(store50()).await;
    server.set_content_checks(rig.checks.clone());
    install_signing_provenance(&server).await;
    let st = store50();
    let model = fabstir_llm_node::ltx::template::expected_session_model(st.entry(HDR), HDR);
    let (_chain, url) = spawn_chain(model).await;
    install_checkpoint_manager(&server, &url).await;
    let cids = serve_blobs(&[source()]).await;
    let ran = run_job(
        &server,
        &job(HDR, None, "", &cids, &json!({})),
        "s-un",
        Some(713),
        Some(ADDR),
        false,
    )
    .await;
    assert_ne!(ran.terminal["type"], "ltx_complete", "{:?}", ran.terminal);
    assert_eq!(ran.code(), CHECK_UNAVAILABLE, "{:?}", ran.terminal);
    assert_eq!(renderer.count("prompt"), 0, "no render");
    assert!(!logs.text().contains("SKIPPED_FIXED_GRAPH"));
}

#[test]
fn only_the_two_alpha_templates_have_opted_in_and_the_handler_has_one_call() {
    // A third template on the list, or a second call site -> red
    assert_eq!(FIXED_GRAPH_TEMPLATES, &[HDR, CHAIN]);
    let handler: String = include_str!("../../src/api/websocket/handlers/ltx.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(
        handler.matches("result_of_fixed_graph_template(").count(),
        1
    );
    assert!(handler.contains("checks.result_of_fixed_graph_template(&cjob,&job)"));
    let checks = include_str!("../../src/ltx/checks/mod.rs");
    assert_eq!(checks.matches("fixed_graph_inputs_checked(").count(), 2);
    assert!(!checks.contains("pub fn fixed_graph_inputs_checked"));
}
