// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 2.1 — seam-#1 server state: task_id↔job_id map, ingest-token auth,
//! and the frames match-state builder. (Endpoint tests live in test_moderate_frames.)

use fabstir_llm_node::api::server::ApiServer;

#[test]
fn apiserver_records_and_resolves_task_job() {
    let server = ApiServer::new_for_test();
    // Unknown task ⇒ None ⇒ /frames 404 ⇒ HOLD (fail-closed).
    assert_eq!(server.job_for_task("unknown"), None);
    server.record_task_job("task-a".into(), 42);
    assert_eq!(server.job_for_task("task-a"), Some(42));
    // R2-F5: a re-alias to a DIFFERENT job_id is rejected (original preserved).
    server.record_task_job("task-a".into(), 99);
    assert_eq!(
        server.job_for_task("task-a"),
        Some(42),
        "re-aliasing a task_id to a different job_id must be refused"
    );
    // Re-recording the SAME mapping is a harmless no-op.
    server.record_task_job("task-a".into(), 42);
    assert_eq!(server.job_for_task("task-a"), Some(42));
}

#[test]
fn verify_ingest_token_unset_rejects_all() {
    // R3-C1: an unset server token must reject EVERY request — never accept-all.
    let server = ApiServer::new_for_test(); // token defaults to None
    assert!(!server.verify_ingest_token("anything"));
    assert!(!server.verify_ingest_token(""));
    assert!(server.moderation_ingest_token().is_none());
}

#[test]
fn verify_ingest_token_matches_only_exact() {
    let mut server = ApiServer::new_for_test();
    server.set_ingest_token(Some("s3cr3t-shared".into()));
    assert!(server.verify_ingest_token("s3cr3t-shared"));
    assert!(!server.verify_ingest_token("wrong"));
    // An empty presented token is rejected even when a server token is set.
    assert!(!server.verify_ingest_token(""));
    assert_eq!(server.moderation_ingest_token(), Some("s3cr3t-shared"));
}

#[test]
fn verify_ingest_token_empty_server_token_rejects() {
    // An explicitly-empty configured token is treated as unset ⇒ reject-all.
    let mut server = ApiServer::new_for_test();
    server.set_ingest_token(Some(String::new()));
    assert!(!server.verify_ingest_token(""));
    assert!(!server.verify_ingest_token("anything"));
}

#[test]
fn frames_match_state_mirrors_asset_tuning() {
    // C5: the frames path uses the same PDQ max_distance (31) as the asset path.
    let server = ApiServer::new_for_test();
    let (snapshot, _ownhash, max_distance) = server.build_frames_match_state();
    assert_eq!(max_distance, 31);
    // R11: the PRODUCTION frames snapshot MUST be fail-closed (Unavailable) until the real
    // NCMEC list lands — a Loaded/available snapshot here would be a fail-open regression
    // (benign keyframes would clear). Asserted so it can't silently regress.
    assert!(
        snapshot.require_available().is_err(),
        "the production frames snapshot must be Unavailable (fail-closed)"
    );
}
