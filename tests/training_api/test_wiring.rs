// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T3.5 wiring pieces: the TD15 boot orphan sweep, the node-side template
//! loader (numeric authoring rule + self-computed templateHash), and the
//! capacity-hint route (`GET /v1/training/capacity` — 404 when disabled,
//! `{available}` when wired) through the REAL router.

use std::sync::Arc;

use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::training::core::load_training_template;
use fabstir_llm_node::training::staging::sweep_orphan_job_dirs;
use tower::util::ServiceExt;

use super::support::{
    fixture, make_deps, model_id, passing_snapshot, CountBehaviour, MockSessions, ScanBehaviour,
};

// --- TD15 boot sweep ---

#[test]
fn boot_sweep_removes_only_job_dirs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("job-7/slice-0")).unwrap();
    std::fs::create_dir_all(dir.path().join("job-8")).unwrap();
    std::fs::create_dir_all(dir.path().join("not-a-job")).unwrap();
    std::fs::write(dir.path().join("job-file"), b"a file, not a dir").unwrap();
    let swept = sweep_orphan_job_dirs(dir.path());
    assert_eq!(swept, 2);
    assert!(!dir.path().join("job-7").exists());
    assert!(!dir.path().join("job-8").exists());
    assert!(
        dir.path().join("not-a-job").exists(),
        "non-job dirs survive"
    );
    assert!(dir.path().join("job-file").exists(), "files survive");
    assert_eq!(sweep_orphan_job_dirs(&dir.path().join("absent")), 0);
}

// --- the template loader ---

fn template_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "training-template-v1",
        "templateId": "train-qlora-qwen38-27b-v1",
        "base": { "tokenizerSha256": "0xab", "files": [],
                   "baseServingModelId": "0x00000000000000000000000000000000000000000000000000000000000000ba" },
        "method": { "ranks": [8, 16], "alphas": [16, 32], "seqLens": [2048], "packing": "cross-boundary-v1" },
        "bounds": { "maxEpochs": 5, "maxTotalTokens": 15_000_000u64 },
        "sliceTokens": 1_000_000u64,
        "countingRecipe": "count-v1",
        "specialsPerSample": 1
    })
}

#[test]
fn loader_types_the_template_and_computes_the_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.json");
    let value = template_json();
    std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
    let template = load_training_template(&path).expect("loads");
    assert_eq!(template.template_id, "train-qlora-qwen38-27b-v1");
    assert_eq!(template.ranks, vec![8, 16]);
    assert_eq!(template.max_total_tokens, 15_000_000);
    assert_eq!(template.slice_tokens, 1_000_000);
    assert_eq!(template.lrs, None);
    // templateHash is COMPUTED from canonical bytes — independently verify.
    let canonical = fabstir_llm_node::checkpoint::delta::sort_json_keys(&value).to_string();
    use tiny_keccak::{Hasher, Keccak};
    let mut keccak = Keccak::v256();
    let mut out = [0u8; 32];
    keccak.update(canonical.as_bytes());
    keccak.finalize(&mut out);
    assert_eq!(template.template_hash, format!("0x{}", hex::encode(out)));
}

#[test]
fn loader_enforces_the_numeric_authoring_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.json");
    // Float → the proven canonical-divergence class.
    let mut with_float = template_json();
    with_float["bounds"]["lrFloor"] = serde_json::json!(2e-5);
    std::fs::write(&path, serde_json::to_string(&with_float).unwrap()).unwrap();
    let err = load_training_template(&path).unwrap_err();
    assert!(err.contains("numeric rule"), "{err}");
    // > u64 integer — must arrive as RAW text (serde_json cannot even build
    // it via json!): the parser falls back to f64, exactly the divergence
    // class the rule bans.
    let raw_big = serde_json::to_string(&template_json()).unwrap().replace(
        "\"maxEpochs\":5",
        "\"maxEpochs\":5,\"huge\":18446744073709551616",
    );
    std::fs::write(&path, raw_big).unwrap();
    let err2 = load_training_template(&path).unwrap_err();
    assert!(err2.contains("numeric rule"), "{err2}");
    // Missing pinned key.
    let mut missing = template_json();
    missing["bounds"]
        .as_object_mut()
        .unwrap()
        .remove("maxTotalTokens");
    std::fs::write(&path, serde_json::to_string(&missing).unwrap()).unwrap();
    assert!(load_training_template(&path).is_err());

    // The counting fields are REQUIRED, not defaulted. A template without
    // specialsPerSample must fail at boot: the alternative is a client
    // guessing 0, mis-counting every sample, and finding out as a
    // DECLARED_TOKENS_MISMATCH on a job the customer has already funded.
    for key in ["specialsPerSample", "countingRecipe"] {
        let mut dropped = template_json();
        dropped.as_object_mut().unwrap().remove(key);
        std::fs::write(&path, serde_json::to_string(&dropped).unwrap()).unwrap();
        assert!(
            load_training_template(&path).is_err(),
            "a template missing {key} must not load"
        );
    }
}

// --- the capacity-hint route through the real router ---

async fn capacity_status_and_body(server: Arc<ApiServer>) -> (u16, serde_json::Value) {
    let router = ApiServer::create_router(server);
    let response = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/training/capacity")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn capacity_hint_is_404_when_disabled_and_available_when_wired() {
    let server = Arc::new(ApiServer::new_for_test());
    let (status, _body) = capacity_status_and_body(server.clone()).await;
    assert_eq!(status, 404, "training disabled → 404 (interface v0.3.1)");

    // Wire deps over a healthy mock sidecar: available.
    let fx = fixture(None).await;
    let sessions = MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    };
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    server.set_training_deps(Arc::new(h.deps)).await;
    let (status2, body2) = capacity_status_and_body(server.clone()).await;
    assert_eq!(status2, 200);
    assert_eq!(body2["available"], true, "{body2}");

    // A dead sidecar socket → wired but NOT available (200/false, not 404).
    let fx2 = fixture(None).await;
    let sessions2 = MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    };
    let mut h2 = make_deps(
        &fx2,
        sessions2,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    h2.deps.trainer = Arc::new(
        fabstir_llm_node::training::trainer_client::TrainerClient::new(
            std::path::PathBuf::from("/nonexistent/trainer.sock"),
            std::time::Duration::from_millis(200),
        ),
    );
    server.set_training_deps(Arc::new(h2.deps)).await;
    let (status3, body3) = capacity_status_and_body(server).await;
    assert_eq!(status3, 200);
    assert_eq!(body3["available"], false, "{body3}");
}

#[test]
fn loader_requires_a_wellformed_base_serving_model_id() {
    // T5.3 prerequisite. E.2's serve-back gate compares the session's model
    // against the template's `baseServingModelId`, and `stage` takes it as a
    // parameter — but until now the loader never read it, so the caller had
    // nothing to pass. It is REQUIRED and shape-checked here rather than
    // defaulted, because the failure a default would produce is silent: the
    // gate would compare against an empty string and refuse every honest
    // serve-back session, or worse, accept the wrong base.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.json");

    // Happy: read, lower-cased, 0x-prefixed.
    let mut upper = template_json();
    upper["base"]["baseServingModelId"] = serde_json::json!(
        "0x00000000000000000000000000000000000000000000000000000000000000BA"
    );
    std::fs::write(&path, serde_json::to_string(&upper).unwrap()).unwrap();
    let template = load_training_template(&path).expect("loads");
    assert_eq!(
        template.base_serving_model_id,
        "0x00000000000000000000000000000000000000000000000000000000000000ba",
        "the pin must be normalised so E.2's comparison is not case-dependent"
    );

    // Absent → boot fails, not a paying customer's session init.
    let mut missing = template_json();
    missing["base"]
        .as_object_mut()
        .unwrap()
        .remove("baseServingModelId");
    std::fs::write(&path, serde_json::to_string(&missing).unwrap()).unwrap();
    let err = load_training_template(&path).unwrap_err();
    assert!(err.contains("baseServingModelId"), "{err}");

    // Wrong shape → refused. A short or non-hex id is not a bytes32 model id,
    // and would silently never match any session's model.
    // The 64-char non-hex case is the one that pins `is_ascii_hexdigit`:
    // without it every bad input here is already caught by the length check,
    // so the hex predicate could be deleted with this row still green.
    let long_non_hex = format!("0x{}", "z".repeat(64));
    for bad in ["0xdeadbeef", "not-hex-at-all", "", long_non_hex.as_str()] {
        let mut wrong = template_json();
        wrong["base"]["baseServingModelId"] = serde_json::json!(bad);
        std::fs::write(&path, serde_json::to_string(&wrong).unwrap()).unwrap();
        let err = load_training_template(&path).unwrap_err();
        assert!(
            err.contains("bytes32"),
            "{bad:?} must be refused as a malformed bytes32, got: {err}"
        );
    }
}
