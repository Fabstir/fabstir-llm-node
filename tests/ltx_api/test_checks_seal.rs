// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N7/N8 and invariants 4 and 8, through the real handler: the strongest signal is sealed to the
//! evidence key before its staged copy is deleted, opens with the private key and nothing else, and the user sees
//! the same refusal as for any other rule; and no log line, alert, error frame, refusals record or sealed file
//! shows prompt text. Each test names the mutation that turns it red.

use super::checks_renderer::*;
use super::checks_svc::*;
use super::ltx_task_support::{mark_pending, pending_count, S5_ENV_LOCK};
use super::relight_stub::mp4_clip;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{seal, CONTENT_REFUSED, MSG_PAUSED, MSG_REFUSED};
use sha2::{Digest, Sha256};

const ADDR: &str = "0x5ea1ed0000000000000000000000000000000004";
const ILLEGAL: Reply = Reply::Verdict("BLOCK_ILLEGAL", Some("csam_suspected"));

fn sealed_files(rig: &Rig) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(rig.dir("evidence"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sealed"))
        .collect()
}

#[tokio::test]
async fn the_strongest_signal_is_sealed_and_looks_like_any_other_refusal() {
    // invariant 8. Sealing after the staged copy is dropped, to another key, or skipped -> red;
    // a distinct message for this rule -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    server.set_content_checks(rig.checks.clone());
    let clip = mp4_clip(121);
    rig.svc.set_media("mp4", 0, ILLEGAL);
    let cids = serve_blobs(&[clip.clone()]).await;
    mark_pending(&server, 201).await;
    let ran = run_job(
        &server,
        &clip_job(&cids[0], false),
        "s-seal",
        Some(201),
        Some(ADDR),
        true,
    )
    .await;
    assert_eq!(ran.code(), CONTENT_REFUSED);
    assert_eq!(
        ran.message(),
        MSG_REFUSED,
        "the same words as any other refusal: no rule and no stage on the wire"
    );
    assert_eq!(pending_count(&server, 201).await, 0);
    assert_eq!(renderer.count("upload") + renderer.count("prompt"), 0);

    let files = sealed_files(&rig);
    assert_eq!(files.len(), 1, "one sealed item");
    let raw = std::fs::read(&files[0]).unwrap();
    assert!(
        !raw.windows(8).any(|w| w == b"isomiso2"),
        "the clip is not readable on the host"
    );
    let (record, item) = seal::open(&rig.secret, &raw).expect("opens with the private key");
    assert_eq!(item, clip, "the sealed item is the refused clip, whole");
    assert_eq!(record["session"], "s-seal");
    assert_eq!(
        (&record["job"], &record["stage"], &record["ext"]),
        (
            &serde_json::json!(201),
            &serde_json::json!("c3"),
            &serde_json::json!("mp4")
        )
    );
    assert_eq!(record["verdict"], "BLOCK_ILLEGAL");
    assert_eq!(
        record["sha256"].as_str().unwrap(),
        hex::encode(Sha256::digest(&clip))
    );
    let other: [u8; 32] = k256::SecretKey::random(&mut rand::thread_rng())
        .to_bytes()
        .into();
    assert!(seal::open(&other, &raw).is_err(), "nothing else opens it");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777,
        0o600
    );

    assert!(
        rig.files_in("staging").is_empty(),
        "the staging copy is gone after sealing"
    );
    let alerts = rig.alerts.all();
    assert!(alerts
        .iter()
        .any(|(k, d)| k == "sealed" && d.contains("job=201") && d.contains("stage=c3")));
    let rec = rig.refusals();
    assert_eq!(
        (rec.len(), &rec[0]["sealed"]),
        (1, &serde_json::json!(true))
    );
}

#[tokio::test]
async fn no_record_of_a_refused_prompt_shows_its_text() {
    // invariant 4. Logging the prompt, putting it in an alert, the error frame or the refusals file -> red;
    // sealing the text in clear -> red
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let (_renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    server.set_content_checks(rig.checks.clone());
    rig.svc.set_text(ILLEGAL);
    mark_pending(&server, 211).await;
    let first = run_job(&server, &t2v_job(), "s-text", Some(211), Some(ADDR), true).await;
    assert_eq!(
        (first.code(), first.message()),
        (CONTENT_REFUSED, MSG_REFUSED)
    );

    // the sealed prompt opens to exactly the text, for the named person only
    let files = sealed_files(&rig);
    assert_eq!(files.len(), 1);
    let raw = std::fs::read(&files[0]).unwrap();
    let (record, item) = seal::open(&rig.secret, &raw).unwrap();
    assert_eq!(
        (item.as_slice(), &record["ext"]),
        (PROMPT.as_bytes(), &serde_json::json!("txt"))
    );
    assert_eq!(
        record["text"],
        serde_json::json!({"termRule": true, "explicit": false, "minor": true})
    );

    // two more refusals (a weaker rule): the third alerts and pauses the address (D13)
    rig.svc
        .set_text(Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit")));
    for jid in [212, 213] {
        mark_pending(&server, jid).await;
        let ran = run_job(
            &server,
            &t2v_job(),
            &format!("s-text-{jid}"),
            Some(jid),
            Some(ADDR),
            true,
        )
        .await;
        assert_eq!(
            ran.message(),
            first.message(),
            "every rule reads the same to the user"
        );
    }
    // a NEW session of the same address is paused too: the count follows the client address, not the session
    let paused = run_job(
        &server,
        &t2v_job(),
        "s-text-214",
        Some(214),
        Some(ADDR),
        false,
    )
    .await;
    assert_eq!((paused.accepted, paused.message()), (false, MSG_PAUSED));
    assert_eq!(sealed_files(&rig).len(), 1, "the weaker rule is not sealed");
    let alerts = rig.alerts.all();
    assert_eq!(
        alerts
            .iter()
            .filter(|(k, _)| k == "repeated-refusals")
            .count(),
        1
    );
    assert!(alerts
        .iter()
        .any(|(k, d)| k == "repeated-refusals" && d.contains(ADDR) && d.contains("refusals24h=3")));

    // nowhere on the host does the text appear
    let log_text = logs.text();
    assert!(
        log_text.contains("ltx-check") && log_text.contains("stage=c1"),
        "the capture sees the check's lines"
    );
    let refusals_raw = std::fs::read_to_string(rig.dir("evidence").join("refusals.jsonl")).unwrap();
    assert_eq!(refusals_raw.lines().count(), 3);
    let alert_text: String = alerts.iter().map(|(k, d)| format!("{k} {d}\n")).collect();
    let frames = format!("{}{}", first.terminal, paused.terminal);
    for needle in [PROMPT, "lighthouse keeper", "storm outside"] {
        for (place, text) in [
            ("log", &log_text),
            ("alerts", &alert_text),
            ("refusals file", &refusals_raw),
            ("error frames", &frames),
        ] {
            assert!(!text.contains(needle), "{place} shows prompt text");
        }
        assert!(
            !raw.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "the sealed file shows it"
        );
    }
    assert!(
        !log_text.contains(rig.dir("staging").to_str().unwrap()),
        "no staged path is logged"
    );
}

#[tokio::test]
async fn a_lone_or_policy_signal_is_refused_but_never_sealed() {
    // D15. Sealing on BLOCK_UNRESOLVED or BLOCK_POLICY -> a sealed file appears -> red
    let _env = S5_ENV_LOCK.lock().await;
    for (jid, reply) in [
        (221u64, Reply::Verdict("BLOCK_UNRESOLVED", None)),
        (
            222,
            Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit")),
        ),
    ] {
        let rig = rig(Mode::Enforce).await;
        let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
        let server = server_for(client).await;
        server.set_content_checks(rig.checks.clone());
        rig.svc.set_media("png", 0, reply);
        let cids = serve_blobs(&[png_bytes(9)]).await;
        mark_pending(&server, jid).await;
        let ran = run_job(
            &server,
            &still_job(&cids[0]),
            "s-weak",
            Some(jid),
            Some(ADDR),
            true,
        )
        .await;
        assert_eq!((ran.code(), ran.message()), (CONTENT_REFUSED, MSG_REFUSED));
        assert_eq!(renderer.count("upload"), 0);
        assert!(sealed_files(&rig).is_empty());
        assert!(!rig
            .alerts
            .kinds()
            .iter()
            .any(|k| k == "sealed" || k == "seal-failed"));
        assert_eq!(rig.refusals()[0]["sealed"], false);
    }
}
