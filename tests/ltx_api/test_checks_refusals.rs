// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N4/N5/N7 and invariants 1 and 3 at the prompt and input stages, driven through the real handler
//! and `run()` with a stub renderer and a stub check service. The client is told ONE sentence per code and never
//! which check fired (fix F12), so each test tells its exit apart by the code, the stage in the refusals record
//! and what the stub service and the stub renderer saw. `refused` (in `checks_renderer.rs`) asserts what every
//! refused job must show: the pending proof forfeited, the slot released, nothing rendered.

use super::checks_renderer::*;
use super::checks_svc::*;
use super::ltx_task_support::S5_ENV_LOCK;
use super::relight_stub::mp4_clip;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{CONTENT_REFUSED, MSG_PROMPT_TOO_LONG, MSG_REFUSED};
use serde_json::json;
use sha2::{Digest, Sha256};

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const EXR_MAGIC: &[u8] = &[0x76, 0x2f, 0x31, 0x01];

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The stage the one refusal on record names.
fn recorded_stage(rig: &Rig) -> String {
    let rec = rig.refusals();
    assert_eq!(rec.len(), 1, "one refusal on record: {rec:?}");
    rec[0]["stage"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_refused_prompt_ends_the_job_before_anything_is_staged() {
    // refusal ignored -> the job renders -> red; a refusal that does not forfeit the pending proof -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    rig.svc.set_text(POLICY);
    let cids = serve_blobs(&[png_bytes(1)]).await;
    refused(
        &server,
        &renderer,
        &still_job(&cids[0]),
        101,
        CONTENT_REFUSED,
        MSG_REFUSED,
    )
    .await;
    assert_eq!(
        renderer.count("upload"),
        0,
        "no input was uploaded after the refusal"
    );
    assert!(
        rig.svc.seen("png").is_empty(),
        "the job ended at its prompt: its still was never staged"
    );
    assert_eq!(
        rig.svc.texts.lock().unwrap().as_slice(),
        [PROMPT],
        "the service saw the prompt once"
    );
    let rec = rig.refusals();
    assert_eq!(recorded_stage(&rig), "c1");
    assert_eq!(rec[0]["session"], "s-refuse-101");
    assert_eq!(
        (&rec[0]["job"], &rec[0]["client"]),
        (&json!(101), &json!(ADDR))
    );
    assert_eq!(rec[0]["sealed"], false);
    assert_eq!(rec[0]["sha256"].as_str().unwrap(), sha(PROMPT.as_bytes()));
}

#[tokio::test]
async fn fixed_and_empty_prompts_are_not_sent_to_the_text_check() {
    // D7. Always calling the service -> a text call is logged -> red
    let rig = rig(Mode::Enforce).await;
    rig.svc.set_text(POLICY);
    let mut job = rig.checks.job("s", Some(1), None);
    rig.checks
        .check_prompt(&mut job, "the template's own words", true)
        .await
        .unwrap();
    rig.checks
        .check_prompt(&mut job, "  \n", false)
        .await
        .unwrap();
    assert_eq!(rig.svc.count("moderate-text"), 0);
    let long = "a".repeat(8001);
    let r = rig
        .checks
        .check_prompt(&mut job, &long, false)
        .await
        .unwrap_err();
    assert_eq!(
        (r.code, r.message.as_str()),
        ("VALIDATION_FAILED", MSG_PROMPT_TOO_LONG)
    );
    assert_eq!(
        rig.svc.count("moderate-text"),
        0,
        "an over-long prompt is refused before the call"
    );
    assert!(!rig.checks.breaker_open());
    assert!(
        rig.checks
            .check_prompt(&mut job, &"a".repeat(8000), false)
            .await
            .is_err(),
        "8000 characters are sent (and refused by the stub's rule)"
    );
    assert_eq!(rig.svc.count("moderate-text"), 1);
}

#[tokio::test]
async fn a_refused_still_never_reaches_the_renderer() {
    // invariant 3. The still not judged, or its refusal ignored -> an upload in the log -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    rig.svc.set_media("png", 0, POLICY);
    let still = png_bytes(2);
    let cids = serve_blobs(&[still.clone()]).await;
    refused(
        &server,
        &renderer,
        &still_job(&cids[0]),
        102,
        CONTENT_REFUSED,
        MSG_REFUSED,
    )
    .await;
    assert_eq!(recorded_stage(&rig), "c2");
    assert_eq!(renderer.count("upload"), 0);
    assert!(rig.files_in("in").is_empty() && rig.files_in("staging").is_empty());
    // what the service judged was this still, whole, at the service's own path to the staging area
    let seen = rig.svc.seen("png");
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].len, seen[0].sha256.as_str()),
        (still.len(), sha(&still).as_str())
    );
    assert!(seen[0]
        .path
        .starts_with(rig.dir("svc-view").to_str().unwrap()));
    assert_eq!(rig.svc.count("moderate outside-root"), 0);
}

#[tokio::test]
async fn a_refused_clip_never_reaches_the_renderer() {
    // invariant 3. The clip not judged, or its refusal ignored -> an upload in the log -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    rig.svc
        .spare_probe
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.svc.set_media("mp4", 0, POLICY);
    let clip = mp4_clip(121);
    let cids = serve_blobs(&[clip.clone()]).await;
    refused(
        &server,
        &renderer,
        &clip_job(&cids[0], false),
        103,
        CONTENT_REFUSED,
        MSG_REFUSED,
    )
    .await;
    assert_eq!(recorded_stage(&rig), "c3");
    assert_eq!(renderer.count("upload"), 0);
    assert!(
        rig.files_in("staging").is_empty(),
        "the staged copy is deleted after the answer"
    );
    let seen = rig.svc.seen("mp4");
    assert_eq!(seen.len(), 1, "the clip was judged once, whole");
    assert_eq!(&seen[0].head[4..8], b"ftyp", "an mp4 reached the service");
    assert_eq!(
        (seen[0].len, seen[0].sha256.as_str()),
        (clip.len(), sha(&clip).as_str())
    );
}

/// Run the 121-frame deep job; returns the sha256 of each frame the service judged, in order.
async fn deep_job(rig: &Rig, jid: u64) -> (Vec<Seen>, Ran, std::sync::Arc<Renderer>) {
    let (renderer, server) = setup(rig).await;
    let frames: Vec<Vec<u8>> = (0..121).map(exr_frame).collect();
    let manifest_cid = serve_deep(&frames).await;
    let before = rig.svc.seen("png").len();
    let ran = run_job(
        &server,
        &clip_job(&manifest_cid, true),
        &format!("s-deep-{jid}"),
        Some(jid),
        Some(ADDR),
        false,
    )
    .await;
    (rig.svc.seen("png")[before..].to_vec(), ran, renderer)
}

#[tokio::test]
async fn a_refused_deep_frame_ends_the_job_and_the_frames_already_uploaded_are_gone() {
    // invariant 3. No folder delete (or the folder noted after the loop) -> uploaded EXR files stay -> red;
    // the raw EXR handed to the service instead of the PNG -> the magic assertion -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    rig.svc.set_media("png", 2, POLICY); // the third frame this job judges
    let (judged, ran, renderer) = deep_job(&rig, 104).await;
    assert_eq!(
        (ran.code(), ran.message()),
        (CONTENT_REFUSED, MSG_REFUSED),
        "{:?}",
        ran.terminal
    );
    assert_eq!(recorded_stage(&rig), "c4");
    assert_eq!(renderer.count("prompt"), 0, "nothing was rendered");
    assert_eq!(judged.len(), 3, "the job stopped at the third judged frame");
    for f in &judged {
        assert!(
            f.head.starts_with(PNG_MAGIC) && !f.head.starts_with(EXR_MAGIC),
            "each frame reaches the service as a PNG, never as the EXR: {:?}",
            f.head
        );
    }
    let uploaded = renderer.count("upload");
    assert!(
        (2..120).contains(&uploaded),
        "the frames before the refused one had been uploaded, and none after it: {uploaded}"
    );
    assert!(
        rig.files_in("in").is_empty(),
        "and they are gone afterwards"
    );
}

#[tokio::test]
async fn two_jobs_on_the_same_deep_input_judge_different_frames() {
    // fix F6. A fixed sampling pattern (multiples of the fps) -> the two lists are equal -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (first, ran, _r) = deep_job(&rig, 105).await;
    assert_eq!(ran.message(), "no frames produced", "{:?}", ran.terminal);
    let (second, _, _r) = deep_job(&rig, 106).await;
    // the first, the last, and one from each of the six seconds in between
    assert_eq!((first.len(), second.len()), (8, 8));
    let shas = |v: &[Seen]| v.iter().map(|f| f.sha256.clone()).collect::<Vec<_>>();
    assert_eq!(
        (shas(&first)[0].as_str(), shas(&first)[7].as_str()),
        (shas(&second)[0].as_str(), shas(&second)[7].as_str()),
        "the first and the last frame are always judged"
    );
    assert_ne!(
        shas(&first),
        shas(&second),
        "which frames are judged in between is drawn afresh for every job"
    );
}
