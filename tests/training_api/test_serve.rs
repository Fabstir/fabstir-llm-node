// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T5: serve-back (interface E.1/E.2, TD9) — the session-scoped adapter's
//! verification chain, 0600 staging, PER-SESSION isolation, and eviction.
//!
//! The isolation row is the one TD9 exists for: the v0.1 model-keyed design
//! would have applied one user's private adapter to every concurrent
//! session on the same base model.

use std::collections::HashMap;

use fabstir_llm_node::training::serve::{AdapterRegistry, LoraRequest, ServeError};
use sha2::{Digest, Sha256};

use super::support::{encrypt_blob, sha256_hex, spawn_s5};

const BASE_MODEL: &str = "0xbase00000000000000000000000000000000000000000000000000000000beef";

/// Build an adapter artifact-manifest-v1 + its blobs on a fresh mock S5.
/// `include_gguf` false = the TD12 gguf-failure shape (safetensors only).
/// `tamper` mutates exactly one claim.
struct AdapterFixture {
    base_url: String,
    manifest_cid: String,
    manifest_sha256: String,
    gguf_bytes: Vec<u8>,
}

async fn adapter_fixture(include_gguf: bool, tamper: Option<&str>) -> AdapterFixture {
    let safetensors = vec![0xA5u8; 3072];
    let gguf = vec![0x6Fu8; 2048];
    let mut store = HashMap::new();

    let (st_cid, st_dl, st_ct) = encrypt_blob(&safetensors);
    store.insert(st_dl, st_ct);
    let (gg_cid, gg_dl, gg_ct) = encrypt_blob(&gguf);
    store.insert(gg_dl, gg_ct);

    let gguf_claim = if tamper == Some("file-sha") {
        sha256_hex(b"a different file entirely")
    } else {
        sha256_hex(&gguf)
    };
    let mut files = vec![serde_json::json!({
        "name": "adapter_model.safetensors",
        "sha256": sha256_hex(&safetensors),
        "sizeBytes": safetensors.len() as u64,
        "shards": [{ "cid": st_cid, "sha256": sha256_hex(&safetensors), "sizeBytes": safetensors.len() as u64 }],
    })];
    if include_gguf {
        files.push(serde_json::json!({
            "name": "adapter.gguf",
            "sha256": gguf_claim,
            "sizeBytes": gguf.len() as u64,
            "shards": [{ "cid": gg_cid, "sha256": sha256_hex(&gguf), "sizeBytes": gguf.len() as u64 }],
        }));
    }
    let manifest = serde_json::json!({
        "schema": if tamper == Some("schema") { "artifact-manifest-v2" } else { "artifact-manifest-v1" },
        "kind": if tamper == Some("kind") { "checkpoint" } else { "adapter" },
        "files": files,
    });
    let stored =
        fabstir_llm_node::training::attestation::canonical_manifest_bytes(&manifest).into_bytes();
    let manifest_sha256 = if tamper == Some("manifest-sha") {
        sha256_hex(b"not the manifest")
    } else {
        format!("0x{}", hex::encode(Sha256::digest(&stored)))
    };
    let (m_cid, m_dl, m_ct) = encrypt_blob(&stored);
    store.insert(m_dl, m_ct);

    AdapterFixture {
        base_url: spawn_s5(store).await,
        manifest_cid: m_cid,
        manifest_sha256,
        gguf_bytes: gguf,
    }
}

/// A manifest whose NUMBERS are hostile: the declared file size and/or shard
/// count are attacker-chosen. Blobs are never hosted — a conforming node must
/// refuse before it fetches anything.
async fn hostile_fixture(size_bytes: u64, shard_count: usize) -> AdapterFixture {
    let filler = vec![0x11u8; 64];
    let (cid, _dl, _ct) = encrypt_blob(&filler);
    let per_shard = if shard_count > 0 {
        size_bytes / shard_count as u64
    } else {
        size_bytes
    };
    let shards: Vec<serde_json::Value> = (0..shard_count)
        .map(|_| serde_json::json!({ "cid": cid, "sha256": sha256_hex(&filler), "sizeBytes": per_shard }))
        .collect();
    let manifest = serde_json::json!({
        "schema": "artifact-manifest-v1",
        "kind": "adapter",
        "files": [{
            "name": "adapter.gguf",
            "sha256": sha256_hex(&filler),
            "sizeBytes": size_bytes,
            "shards": shards,
        }],
    });
    let stored =
        fabstir_llm_node::training::attestation::canonical_manifest_bytes(&manifest).into_bytes();
    let manifest_sha256 = format!("0x{}", hex::encode(Sha256::digest(&stored)));
    let (m_cid, m_dl, m_ct) = encrypt_blob(&stored);
    AdapterFixture {
        base_url: spawn_s5(HashMap::from([(m_dl, m_ct)])).await,
        manifest_cid: m_cid,
        manifest_sha256,
        gguf_bytes: filler,
    }
}

/// A TWO-shard adapter whose SECOND shard's manifest claim is a lie — the
/// per-shard verification loop is the only thing that catches it.
async fn multishard_fixture(tamper_second: bool) -> AdapterFixture {
    let part_a = vec![0x01u8; 1024];
    let part_b = vec![0x02u8; 1024];
    let whole: Vec<u8> = part_a.iter().chain(part_b.iter()).copied().collect();
    let mut store = HashMap::new();
    let (cid_a, dl_a, ct_a) = encrypt_blob(&part_a);
    let (cid_b, dl_b, ct_b) = encrypt_blob(&part_b);
    store.insert(dl_a, ct_a);
    store.insert(dl_b, ct_b);
    let claim_b = if tamper_second {
        sha256_hex(b"not shard b")
    } else {
        sha256_hex(&part_b)
    };
    let manifest = serde_json::json!({
        "schema": "artifact-manifest-v1",
        "kind": "adapter",
        "files": [{
            "name": "adapter.gguf",
            "sha256": sha256_hex(&whole),
            "sizeBytes": whole.len() as u64,
            "shards": [
                { "cid": cid_a, "sha256": sha256_hex(&part_a), "sizeBytes": part_a.len() as u64 },
                { "cid": cid_b, "sha256": claim_b, "sizeBytes": part_b.len() as u64 },
            ],
        }],
    });
    let stored =
        fabstir_llm_node::training::attestation::canonical_manifest_bytes(&manifest).into_bytes();
    let manifest_sha256 = format!("0x{}", hex::encode(Sha256::digest(&stored)));
    let (m_cid, m_dl, m_ct) = encrypt_blob(&stored);
    store.insert(m_dl, m_ct);
    AdapterFixture {
        base_url: spawn_s5(store).await,
        manifest_cid: m_cid,
        manifest_sha256,
        gguf_bytes: whole,
    }
}

fn request(fx: &AdapterFixture, file: &str) -> LoraRequest {
    LoraRequest {
        manifest_cid: fx.manifest_cid.clone(),
        manifest_sha256: fx.manifest_sha256.clone(),
        file: file.to_string(),
    }
}

#[tokio::test]
async fn happy_stage_verifies_writes_0600_and_registers() {
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let staged = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("stages");
    assert_eq!(staged.session_id, "session-A");
    assert_eq!(staged.file, "adapter.gguf");
    assert!(staged.path.ends_with("adapters/session-A/adapter.gguf"), "{:?}", staged.path);
    // The staged bytes ARE the artifact.
    assert_eq!(std::fs::read(&staged.path).unwrap(), fx.gguf_bytes);
    // 0600 (TD9: the adapter is private user property on a shared box).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&staged.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "staged adapter must be 0600, was {mode:o}");
    }
    assert_eq!(registry.adapter_for("session-A"), Some(staged));
}

#[tokio::test]
async fn wrong_manifest_sha_is_integrity() {
    let fx = adapter_fixture(true, Some("manifest-sha")).await;
    let dir = tempfile::tempdir().unwrap();
    match AdapterRegistry::new()
        .stage(
            &fx.base_url,
            dir.path(),
            "s",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .unwrap_err()
    {
        ServeError::Integrity(detail) => assert!(detail.contains("manifestSha256"), "{detail}"),
        other => panic!("expected Integrity, got {other:?}"),
    }
}

#[tokio::test]
async fn wrong_file_sha_is_integrity_and_stages_nothing() {
    let fx = adapter_fixture(true, Some("file-sha")).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    match registry
        .stage(
            &fx.base_url,
            dir.path(),
            "s",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .unwrap_err()
    {
        ServeError::Integrity(detail) => assert!(detail.contains("adapter.gguf"), "{detail}"),
        other => panic!("expected Integrity, got {other:?}"),
    }
    assert!(registry.adapter_for("s").is_none(), "a failed stage registers nothing");
    assert!(
        !dir.path().join("adapters/s/adapter.gguf").exists(),
        "no half-written adapter survives"
    );
}

#[tokio::test]
async fn wrong_base_serving_model_is_refused() {
    // E.2: "The session's model must equal the template's baseServingModelId".
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    match AdapterRegistry::new()
        .stage(
            &fx.base_url,
            dir.path(),
            "s",
            "0xsomeothermodel000000000000000000000000000000000000000000000000",
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .unwrap_err()
    {
        ServeError::Validation(detail) => assert!(
            detail.to_lowercase().contains("base"),
            "must name the base pin: {detail}"
        ),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[tokio::test]
async fn missing_gguf_refuses_serve_back() {
    // TD12/E.1(b): a run whose GGUF conversion failed ships safetensors-only
    // and CANNOT serve back — the manifest must actually carry the file.
    let fx = adapter_fixture(false, None).await;
    let dir = tempfile::tempdir().unwrap();
    match AdapterRegistry::new()
        .stage(
            &fx.base_url,
            dir.path(),
            "s",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .unwrap_err()
    {
        ServeError::Validation(detail) => assert!(detail.contains("adapter.gguf"), "{detail}"),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[tokio::test]
async fn wrong_manifest_kind_or_schema_is_refused() {
    for tamper in ["kind", "schema"] {
        let fx = adapter_fixture(true, Some(tamper)).await;
        let dir = tempfile::tempdir().unwrap();
        let result = AdapterRegistry::new()
            .stage(
                &fx.base_url,
                dir.path(),
                "s",
                BASE_MODEL,
                BASE_MODEL,
                &request(&fx, "adapter.gguf"),
            )
            .await;
        assert!(
            matches!(result, Err(ServeError::Validation(_))),
            "{tamper}: {result:?}"
        );
    }
}

#[tokio::test]
async fn adapters_are_isolated_per_session() {
    // THE TD9 row: two concurrent sessions on the SAME base model; only the
    // session that staged an adapter can see one. (The v0.1 model-keyed
    // design would have applied A's private adapter to B.)
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let staged_a = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("A stages");
    // B runs on the same base model and staged nothing.
    assert_eq!(registry.adapter_for("session-A"), Some(staged_a.clone()));
    assert_eq!(
        registry.adapter_for("session-B"),
        None,
        "a concurrent session on the same base MUST NOT see another session's adapter"
    );
    // B stages its OWN adapter: each still resolves to its own file.
    let fx_b = adapter_fixture(true, None).await;
    let staged_b = registry
        .stage(
            &fx_b.base_url,
            dir.path(),
            "session-B",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx_b, "adapter.gguf"),
        )
        .await
        .expect("B stages");
    assert_ne!(staged_a.path, staged_b.path, "per-session staging paths");
    assert_eq!(registry.adapter_for("session-A").unwrap().path, staged_a.path);
    assert_eq!(registry.adapter_for("session-B").unwrap().path, staged_b.path);
}

#[tokio::test]
async fn eviction_deregisters_and_deletes_the_file() {
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let staged = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("stages");
    assert!(staged.path.exists());
    registry.evict("session-A").await;
    assert!(registry.adapter_for("session-A").is_none(), "deregistered");
    assert!(!staged.path.exists(), "the staged adapter FILE must be deleted");
    // Evicting an unknown session is a no-op, not a panic.
    registry.evict("never-existed").await;
}

// ---------------------------------------------------------------------------
// T5 round-1 security rows. The first cut of serve.rs treated wire strings and
// a client-authored manifest as trusted; these pin every fix.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn traversal_session_ids_are_refused() {
    // F2: `""` made the staged dir the SHARED adapters/ root, so eviction
    // deleted every concurrent session's private adapter; `".."` reached the
    // staging root itself; an absolute id escaped entirely.
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    for bad in ["", "..", "/etc/cron.d", "a/b", "with\0nul", "."] {
        let result = registry
            .stage(
                &fx.base_url,
                dir.path(),
                bad,
                BASE_MODEL,
                BASE_MODEL,
                &request(&fx, "adapter.gguf"),
            )
            .await;
        assert!(
            matches!(result, Err(ServeError::Validation(_))),
            "session id {bad:?} must be refused, got {result:?}"
        );
    }
    // Nothing was created outside a proper per-session directory.
    assert!(!dir.path().join("adapters").join("adapter.gguf").exists());
}

#[tokio::test]
async fn traversal_or_foreign_file_names_are_refused() {
    // F1: `request.file` is a wire string and the manifest is CLIENT-AUTHORED,
    // so a matching entry proves nothing — `Path::join` with an absolute or
    // `..` name gave an arbitrary file write as the node user.
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    for bad in [
        "../../../../etc/cron.d/pwn",
        "/etc/cron.d/pwn",
        "adapter_model.safetensors", // real entry, but not the M0 artifact
    ] {
        let result = registry
            .stage(
                &fx.base_url,
                dir.path(),
                "session-A",
                BASE_MODEL,
                BASE_MODEL,
                &request(&fx, bad),
            )
            .await;
        let detail = match result {
            Err(ServeError::Validation(detail)) => detail,
            other => panic!("file {bad:?} must be refused as Validation, got {other:?}"),
        };
        // Round-2 R2-10: asserting only the VARIANT let the two traversal
        // cases pass for the manifest-lookup reason ("carries no ..."), so
        // `safe_component(&request.file)` was deletable with this row green.
        if bad.contains('/') {
            assert!(
                detail.contains("single normal path component"),
                "file {bad:?} must be refused by component validation, not by a later \
                 lookup: {detail}"
            );
        }
    }
    assert!(!std::path::Path::new("/etc/cron.d/pwn").exists());
}

#[tokio::test]
async fn one_adapter_per_session_a_restage_is_refused() {
    // F3: `insert` silently replaced the entry AND overwrote the file, so two
    // clients presenting one session id swapped private adapters.
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let first = registry
        .stage(&fx.base_url, dir.path(), "s", BASE_MODEL, BASE_MODEL, &request(&fx, "adapter.gguf"))
        .await
        .expect("first stages");
    let fx2 = adapter_fixture(true, None).await;
    let second = registry
        .stage(&fx2.base_url, dir.path(), "s", BASE_MODEL, BASE_MODEL, &request(&fx2, "adapter.gguf"))
        .await;
    assert!(
        matches!(second, Err(ServeError::Validation(_))),
        "a re-stage must be refused (E.2 one adapter per session), got {second:?}"
    );
    // The FIRST session's adapter is untouched.
    assert_eq!(registry.adapter_for("s"), Some(first));
}

#[tokio::test]
async fn evicting_one_session_leaves_another_intact() {
    // The blast radius the traversal bug had: eviction must remove exactly
    // one session's directory.
    let fx_a = adapter_fixture(true, None).await;
    let fx_b = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let a = registry
        .stage(&fx_a.base_url, dir.path(), "A", BASE_MODEL, BASE_MODEL, &request(&fx_a, "adapter.gguf"))
        .await
        .unwrap();
    let b = registry
        .stage(&fx_b.base_url, dir.path(), "B", BASE_MODEL, BASE_MODEL, &request(&fx_b, "adapter.gguf"))
        .await
        .unwrap();
    registry.evict("A").await;
    assert!(!a.path.exists(), "A's adapter is gone");
    assert!(b.path.exists(), "B's adapter MUST survive A's eviction");
    assert_eq!(registry.adapter_for("B"), Some(b));
}

#[tokio::test]
async fn oversized_or_overcounted_manifests_are_refused_before_fetch() {
    // F4/F5: every number is attacker-chosen. A `sizeBytes` of u64::MAX made
    // `Vec::with_capacity` abort the PROCESS before a byte was fetched.
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    for (label, size, shard_count) in [
        ("u64-max", u64::MAX, 1usize),
        ("over-cap", 4 * 1024 * 1024 * 1024u64, 1),
        ("zero", 0u64, 1),
        // 2000/100 divides exactly, so the sum matches and the SHARD CAP is
        // the only thing left standing (round-2 R2-10: at 2048 the sum check
        // fired first and the cap could be deleted with this row still green).
        ("too-many-shards", 2000u64, 100),
    ] {
        let hostile = hostile_fixture(size, shard_count).await;
        let result = registry
            .stage(
                &hostile.base_url,
                dir.path(),
                "s",
                BASE_MODEL,
                BASE_MODEL,
                &request(&hostile, "adapter.gguf"),
            )
            .await;
        match result {
            Err(ServeError::Validation(detail)) => assert!(
                !detail.contains("already has an adapter"),
                "{label} was refused for the WRONG reason (a stranded reservation \
                 from the previous iteration, not its own bound): {detail}"
            ),
            other => panic!("{label} must be refused as Validation, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_tampered_second_shard_is_caught_by_the_per_shard_check() {
    // Round-1 falsifiability gap: every fixture had ONE shard, so the whole
    // per-shard verification loop could be deleted and the suite stayed green.
    let fx = multishard_fixture(true).await;
    let dir = tempfile::tempdir().unwrap();
    match AdapterRegistry::new()
        .stage(&fx.base_url, dir.path(), "s", BASE_MODEL, BASE_MODEL, &request(&fx, "adapter.gguf"))
        .await
        .unwrap_err()
    {
        ServeError::Integrity(detail) => {
            assert!(detail.contains("shard 1"), "must name the tampered shard: {detail}")
        }
        other => panic!("expected Integrity, got {other:?}"),
    }
}

#[tokio::test]
async fn a_preexisting_loose_file_cannot_keep_its_mode() {
    // F5: with create+truncate an existing 0644 file kept 0644 and the
    // "never world-readable" claim was false. The write is now create_new
    // into a temp + rename, so the mode is always fresh.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fx = adapter_fixture(true, None).await;
        let dir = tempfile::tempdir().unwrap();
        let session_dir = dir.path().join("adapters").join("s");
        std::fs::create_dir_all(&session_dir).unwrap();
        let victim = session_dir.join("adapter.gguf");
        std::fs::write(&victim, b"pre-existing").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
        let staged = AdapterRegistry::new()
            .stage(&fx.base_url, dir.path(), "s", BASE_MODEL, BASE_MODEL, &request(&fx, "adapter.gguf"))
            .await
            .expect("stages over the leftover");
        let mode = std::fs::metadata(&staged.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "a leftover file must not keep its loose mode, was {mode:o}");
        assert_eq!(std::fs::read(&staged.path).unwrap(), fx.gguf_bytes);
    }
}

#[tokio::test]
async fn the_base_pin_is_checked_before_any_fetch() {
    // The docstring's ordering claim, pinned: point the fetch at an
    // unreachable base — a refusal proves nothing was fetched.
    let fx = adapter_fixture(true, None).await;
    let mut req = request(&fx, "adapter.gguf");
    req.manifest_cid = fx.manifest_cid.clone();
    let dir = tempfile::tempdir().unwrap();
    let result = AdapterRegistry::new()
        .stage(
            "http://127.0.0.1:9", // discard port: any fetch fails
            dir.path(),
            "s",
            "0xsomeothermodel000000000000000000000000000000000000000000000000",
            BASE_MODEL,
            &req,
        )
        .await;
    match result {
        Err(ServeError::Validation(detail)) => {
            assert!(detail.to_lowercase().contains("base"), "{detail}")
        }
        other => panic!("base pin must refuse BEFORE the fetch, got {other:?}"),
    }
}

#[test]
fn lora_request_deserialises_from_the_e2_wire_shape() {
    // E.2's literal block. The rename attributes are the whole reason the
    // type exists and no row exercised them.
    let parsed: LoraRequest = serde_json::from_str(
        r#"{ "manifestCID": "uABC", "manifestSha256": "0xdead", "file": "adapter.gguf" }"#,
    )
    .expect("E.2 wire shape must deserialise");
    assert_eq!(parsed.manifest_cid, "uABC");
    assert_eq!(parsed.manifest_sha256, "0xdead");
    assert_eq!(parsed.file, "adapter.gguf");
    // Rust-side names must NOT be accepted in their place.
    assert!(serde_json::from_str::<LoraRequest>(
        r#"{ "manifest_cid": "uABC", "manifest_sha256": "0xdead", "file": "adapter.gguf" }"#
    )
    .is_err());
}

/// A manifest whose shard sizes are chosen to WRAP when summed. `size_bytes`
/// is the (small, in-range) file claim; `shard_sizes` are the per-shard lies.
async fn wrapping_fixture(size_bytes: u64, shard_sizes: &[u64]) -> AdapterFixture {
    let filler = vec![0x22u8; 64];
    let (cid, _dl, _ct) = encrypt_blob(&filler);
    let shards: Vec<serde_json::Value> = shard_sizes
        .iter()
        .map(|n| serde_json::json!({ "cid": cid, "sha256": sha256_hex(&filler), "sizeBytes": n }))
        .collect();
    let manifest = serde_json::json!({
        "schema": "artifact-manifest-v1",
        "kind": "adapter",
        "files": [{
            "name": "adapter.gguf",
            "sha256": sha256_hex(&filler),
            "sizeBytes": size_bytes,
            "shards": shards,
        }],
    });
    let stored =
        fabstir_llm_node::training::attestation::canonical_manifest_bytes(&manifest).into_bytes();
    let manifest_sha256 = format!("0x{}", hex::encode(Sha256::digest(&stored)));
    let (m_cid, m_dl, m_ct) = encrypt_blob(&stored);
    AdapterFixture {
        base_url: spawn_s5(HashMap::from([(m_dl, m_ct)])).await,
        manifest_cid: m_cid,
        manifest_sha256,
        gguf_bytes: filler,
    }
}

#[tokio::test]
async fn shard_sizes_that_wrap_u64_are_refused() {
    // Round-2 R2-1 (HIGH): `shards.iter().map(|s| s.size_bytes).sum()` over
    // attacker u64s. Release builds do not check overflow, so 2^63 + 2^63 +
    // 1024 wrapped to 1024 and MATCHED the declared `sizeBytes` of 1024. The
    // equality gate passed, and because the per-shard pre-fetch gate then
    // used `shard.size_bytes` as its OWN ceiling, an 8 GiB blob was admitted
    // and downloaded before a single hash was checked — the OOM the module
    // header claims to have closed. Two shards of 2^63 also make the file
    // claim satisfiable by no honest manifest, which is the point: it is a
    // pure arithmetic attack on the binding between the two checks.
    let hostile = wrapping_fixture(1024, &[1u64 << 63, 1u64 << 63, 1024]).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let result = registry
        .stage(
            &hostile.base_url,
            dir.path(),
            "s",
            BASE_MODEL,
            BASE_MODEL,
            &request(&hostile, "adapter.gguf"),
        )
        .await;
    match result {
        Err(ServeError::Validation(detail)) => assert!(
            detail.contains("outside 1..="),
            "must be refused by the PER-SHARD bound, not incidentally: {detail}"
        ),
        other => panic!("wrapping shard sizes must be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_failed_stage_releases_its_reservation() {
    // Round-2 R2-3: the one-per-session check became a reservation taken
    // under the lock BEFORE the fetch. Without the release-on-drop guard, any
    // failed stage would strand its session id as permanently unusable — and
    // worse, every later attempt would be refused with a Validation error
    // that LOOKS like a legitimate one-per-session refusal.
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let bad = adapter_fixture(true, Some("file-sha")).await;
    let first = registry
        .stage(
            &bad.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&bad, "adapter.gguf"),
        )
        .await;
    assert!(matches!(first, Err(ServeError::Integrity(_))), "{first:?}");
    assert_eq!(registry.adapter_for("session-A"), None, "a failure must register nothing");

    // The same id must still be stageable.
    let good = adapter_fixture(true, None).await;
    let staged = registry
        .stage(
            &good.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&good, "adapter.gguf"),
        )
        .await
        .expect("a failed attempt must not strand the session id");
    assert_eq!(registry.adapter_for("session-A"), Some(staged));
}

#[tokio::test]
#[cfg(unix)]
async fn staged_directories_are_0700() {
    // Round-1 F11 shipped with no falsifiable row (round-2 R2-9):
    // `create_dir_all` leaves 0755, letting any local user enumerate live
    // session ids and adapter names on the shared box the premise is about.
    use std::os::unix::fs::PermissionsExt;
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    let staged = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("stages");
    let session_dir = staged.path.parent().unwrap();
    for target in [session_dir, session_dir.parent().unwrap()] {
        let mode = std::fs::metadata(target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{target:?} must be 0700, was {mode:o}");
    }
}

#[tokio::test]
async fn the_boot_sweep_clears_orphan_adapter_dirs() {
    // Round-1 F10's boot sweep also shipped with no row (round-2 R2-9). A
    // crash leaves customers' private weights on disk; the sweep is the only
    // thing that removes them, and TD15 runs it on both roots at startup.
    let root = tempfile::tempdir().unwrap();
    let adapters = root.path().join("adapters");
    std::fs::create_dir_all(adapters.join("dead-session-1")).unwrap();
    std::fs::create_dir_all(adapters.join("dead-session-2")).unwrap();
    std::fs::write(adapters.join("dead-session-1").join("adapter.gguf"), b"private").unwrap();
    std::fs::write(adapters.join("not-a-dir"), b"stray").unwrap();

    let swept = fabstir_llm_node::training::serve::sweep_orphan_adapter_dirs(root.path());
    assert_eq!(swept, 2, "both orphan session dirs must be swept");
    assert!(!adapters.join("dead-session-1").exists());
    assert!(!adapters.join("dead-session-2").exists());
    assert!(adapters.join("not-a-dir").exists(), "the sweep removes DIRS, not stray files");
}

#[tokio::test]
#[cfg(unix)]
async fn a_symlink_at_the_destination_is_replaced_not_followed() {
    // Round-2 R2-11 recorded that the tmp+rename write path had no
    // falsifiable row of its own, on the reasoning that its only property is
    // crash-atomicity. It has a second, testable one: `rename` REPLACES a
    // symlink at the destination, whereas round-1's `create(true).truncate(true)`
    // on the destination FOLLOWED it and wrote the adapter's bytes into
    // whatever file the link pointed at — outside the staging root, as the
    // node user. This row is red under that revert and green under the
    // current path.
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("victim.txt");
    std::fs::write(&outside, b"untouched").unwrap();
    let session_dir = dir.path().join("adapters").join("session-A");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::os::unix::fs::symlink(&outside, session_dir.join("adapter.gguf")).unwrap();

    let registry = AdapterRegistry::new();
    let staged = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("stages over the planted link");
    assert_eq!(
        std::fs::read(&outside).unwrap(),
        b"untouched",
        "the symlink target outside the session dir must never be written"
    );
    assert!(!std::fs::symlink_metadata(&staged.path).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&staged.path).unwrap(), fx.gguf_bytes);
}

/// An adapter fixture whose S5 holds the FIRST request (the manifest fetch)
/// until released, so a test can act while `stage` is provably between taking
/// its reservation and committing it.
async fn gated_adapter_fixture() -> (
    AdapterFixture,
    std::sync::Arc<tokio::sync::Notify>,
    std::sync::Arc<tokio::sync::Notify>,
) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let gguf = vec![0x6Fu8; 2048];
    let mut store = HashMap::new();
    let (gg_cid, gg_dl, gg_ct) = encrypt_blob(&gguf);
    store.insert(gg_dl, gg_ct);
    let manifest = serde_json::json!({
        "schema": "artifact-manifest-v1",
        "kind": "adapter",
        "files": [{
            "name": "adapter.gguf",
            "sha256": sha256_hex(&gguf),
            "sizeBytes": gguf.len() as u64,
            "shards": [{ "cid": gg_cid, "sha256": sha256_hex(&gguf), "sizeBytes": gguf.len() as u64 }],
        }],
    });
    let stored =
        fabstir_llm_node::training::attestation::canonical_manifest_bytes(&manifest).into_bytes();
    let manifest_sha256 = format!("0x{}", hex::encode(Sha256::digest(&stored)));
    let (m_cid, m_dl, m_ct) = encrypt_blob(&stored);
    store.insert(m_dl, m_ct);

    let arrived = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let first = Arc::new(AtomicBool::new(true));
    let store = Arc::new(store);
    let (a, r, f) = (arrived.clone(), release.clone(), first.clone());

    let app = axum::Router::new().route(
        "/s5/blob/:cid",
        axum::routing::get(move |axum::extract::Path(cid): axum::extract::Path<String>| {
            let (store, a, r, f) = (store.clone(), a.clone(), r.clone(), f.clone());
            async move {
                if f.swap(false, Ordering::SeqCst) {
                    a.notify_one();
                    r.notified().await;
                }
                match store.get(&cid) {
                    Some(bytes) => (axum::http::StatusCode::OK, bytes.clone()),
                    None => (axum::http::StatusCode::NOT_FOUND, Vec::new()),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    (
        AdapterFixture {
            base_url: format!("http://127.0.0.1:{port}"),
            manifest_cid: m_cid,
            manifest_sha256,
            gguf_bytes: gguf,
        },
        arrived,
        release,
    )
}

#[tokio::test]
async fn an_evict_during_staging_cancels_it_and_leaves_nothing_behind() {
    // Round-3 F1, in the round-2 fix code: `evict` did
    // `remove(session_id).flatten()`, which for an IN-FLIGHT reservation
    // (value `None`) removed the KEY and deleted no FILE. The stage then
    // committed an adapter for a session that had already ended — private
    // weights with nothing left to evict them, surviving until the boot sweep
    // — and in that window a second session could claim the freed id and have
    // its entry overwritten by the first stage's commit, which is exactly the
    // TD9 isolation failure the reservation was added to close.
    use std::sync::Arc;
    let (fx, arrived, release) = gated_adapter_fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(AdapterRegistry::new());

    let (task_registry, root, base, req) = (
        registry.clone(),
        dir.path().to_path_buf(),
        fx.base_url.clone(),
        request(&fx, "adapter.gguf"),
    );
    let task = tokio::spawn(async move {
        task_registry
            .stage(&base, &root, "session-A", BASE_MODEL, BASE_MODEL, &req)
            .await
    });

    // The manifest fetch is held, so the reservation is taken and the commit
    // has not happened: precisely the window.
    arrived.notified().await;
    registry.evict("session-A").await;
    release.notify_one();

    let result = task.await.unwrap();
    assert!(
        matches!(result, Err(ServeError::Cancelled(_))),
        "a stage overtaken by eviction must report cancellation, got {result:?}"
    );
    assert_eq!(registry.adapter_for("session-A"), None, "nothing may stay registered");
    assert!(
        !dir.path().join("adapters").join("session-A").exists(),
        "a cancelled stage must leave no private weights behind"
    );

    // Round 4: without this, deleting the cancelled branch's key removal
    // survives — the id would stay Reserved forever and every later stage on
    // it would be refused with a message that reads like a legitimate
    // one-per-session refusal, which is the exact failure the reservation
    // guard exists to prevent.
    let fresh = adapter_fixture(true, None).await;
    registry
        .stage(
            &fresh.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fresh, "adapter.gguf"),
        )
        .await
        .expect("a cancelled stage must release the session id, not strand it");
}

#[tokio::test]
async fn a_required_adapter_that_is_not_staged_refuses_rather_than_serving_the_base_model() {
    // T5.3 round-1 F1(b): the first wiring cached a raw path in the
    // connection, so `adapter_for` ended up with ZERO production callers and
    // the one component that knows an eviction happened was never consulted.
    // Resolution goes back through the registry, and a session that ASKED for
    // an adapter and has none must fail its request — answering from the base
    // model would hand the customer the wrong weights on a paid session, and
    // do it invisibly.
    use fabstir_llm_node::training::serve::SessionAdapter;
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();

    // An ordinary session resolves to no adapter, and that is not an error.
    assert_eq!(registry.resolve(&SessionAdapter::None), Ok(None));

    // A session that asked but never staged is REFUSED, not silently served.
    let want = SessionAdapter::Required("session-A".to_string());
    assert!(
        matches!(registry.resolve(&want), Err(ServeError::Validation(_))),
        "a Required adapter with nothing staged must refuse"
    );

    // Once staged it resolves to the staged path.
    let staged = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "session-A",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("stages");
    assert_eq!(registry.resolve(&want), Ok(Some(staged.path.clone())));

    // And after eviction it refuses again, rather than returning a path whose
    // bytes are gone or, worse, have been replaced.
    registry.evict("session-A").await;
    assert!(
        matches!(registry.resolve(&want), Err(ServeError::Validation(_))),
        "an evicted adapter must refuse, not fall back to the base model"
    );
}

/// T5.3 round-1 F1 (CRITICAL), through the REAL router and a REAL WebSocket.
///
/// The first wiring evicted on the connection variable `session_id`, which is
/// assigned from the OUTER, UNENCRYPTED json at the top of both init branches,
/// before any gate, on a route that carries no auth. So anyone could open a
/// socket, name a session id, close, and delete that customer's adapter — then
/// stage their own bytes at the same path, which the victim's next request
/// would load, because adapters are read per request and the sha256 was
/// checked at stage time. Session ids are job ids, published on-chain.
///
/// Eviction is now keyed on `staged_sid`, set only by a stage that actually
/// succeeded on THIS connection, so a connection that staged nothing evicts
/// nothing.
#[tokio::test]
async fn a_connection_that_staged_nothing_cannot_evict_another_sessions_adapter() {
    use fabstir_llm_node::api::server::ApiServer;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::Message;

    let fx = super::support::fixture(None).await;
    let harness = super::support::make_deps(
        &fx,
        super::support::MockSessions {
            snapshot: Ok(super::support::passing_snapshot()),
            model: super::support::model_id(0xAA),
            dispute: 30,
        },
        super::support::ScanBehaviour::Cleared,
        super::support::CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(harness.deps);
    let server = Arc::new(ApiServer::new_for_test());
    server.set_training_deps(deps.clone()).await;

    // A victim session stages its private adapter.
    let afx = adapter_fixture(true, None).await;
    let staging = tempfile::tempdir().unwrap();
    let victim = deps
        .adapters
        .stage(
            &afx.base_url,
            staging.path(),
            "1234",
            BASE_MODEL,
            BASE_MODEL,
            &request(&afx, "adapter.gguf"),
        )
        .await
        .expect("victim stages");
    assert!(victim.path.exists());

    // Serve the real router.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // An unrelated, unauthenticated connection names the victim's session.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    // Drain the welcome frame so the handler is definitely running.
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;
    ws.send(Message::Text(
        serde_json::json!({ "type": "session_init", "session_id": "1234" }).to_string(),
    ))
    .await
    .unwrap();
    // Wait for the ack, which proves the message was processed and therefore
    // that `session_id` has been assigned on the server side.
    let _ack = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;
    ws.close(None).await.ok();
    drop(ws);

    // The close path runs immediately once the socket drops; poll a generous
    // window so the vulnerable version has every chance to delete the file.
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            deps.adapters.adapter_for("1234").is_some(),
            "an unrelated connection deregistered another session's adapter"
        );
        assert!(
            victim.path.exists(),
            "an unrelated connection DELETED another session's private weights"
        );
    }
}

/// T5.3 round-2 F1-R2 (CRITICAL), through the real router.
///
/// Round 1 stopped EVICTION keying on the unauthenticated connection variable
/// `session_id` but left RESOLUTION keyed on it, which was worse. An attacker
/// sent an `encrypted_session_init` whose OUTER `session_id` named the victim
/// and whose inner payload carried any `lora` at all. Their own stage was
/// refused (the victim already held that key), so nothing of theirs was
/// staged — but `session_adapter` became `Required(victim)`, and their next
/// prompt resolved straight to the victim's private adapter and was answered
/// through the victim's fine-tune, on the attacker's own paid session.
///
/// The registry key is now minted server-side and never read from the wire,
/// so naming a session id buys nothing.
#[tokio::test]
async fn an_encrypted_init_naming_another_sessions_id_cannot_resolve_its_adapter() {
    use fabstir_llm_node::api::server::ApiServer;
    use fabstir_llm_node::crypto::{derive_shared_key, encrypt_with_aead};
    use futures_util::{SinkExt, StreamExt};
    use k256::ecdsa::{signature::Signer, SigningKey};
    use k256::SecretKey;
    use rand::rngs::OsRng;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::Message;

    // The victim's adapter, staged under the id the attacker will name.
    let fx = super::support::fixture(None).await;
    let harness = super::support::make_deps(
        &fx,
        super::support::MockSessions {
            snapshot: Ok(super::support::passing_snapshot()),
            model: super::support::model_id(0xAA),
            dispute: 30,
        },
        super::support::ScanBehaviour::Cleared,
        super::support::CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(harness.deps);
    let afx = adapter_fixture(true, None).await;
    let staging = tempfile::tempdir().unwrap();
    deps.adapters
        .stage(
            &afx.base_url,
            staging.path(),
            "victim-1234",
            BASE_MODEL,
            BASE_MODEL,
            &request(&afx, "adapter.gguf"),
        )
        .await
        .expect("victim stages");

    // A node key, so the encrypted branch is reachable.
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv: [u8; 32] = node_secret.to_bytes().into();
    let mut server = ApiServer::new_for_test();
    server.set_node_private_key_for_test(node_priv);
    let server = Arc::new(server);
    server.set_training_deps(deps.clone()).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // The attacker's encrypted init: outer session_id = the victim's, inner
    // `lora` deliberately junk so nothing of the attacker's can ever stage.
    let client_secret = SecretKey::random(&mut OsRng);
    let shared = derive_shared_key(
        node_secret.public_key().to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();
    let inner = serde_json::json!({
        "jobId": "9999",
        "modelName": "qwen3.8-27b",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 904,
        "lora": {
            "manifestCID": "u-not-a-real-capability",
            "manifestSha256": "0xdead",
            "file": "adapter.gguf",
        },
    });
    let nonce = [9u8; 24];
    let ciphertext =
        encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
    let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
    let mut sig = [0u8; 65];
    sig[..64].copy_from_slice(&signature.to_bytes());

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;
    ws.send(Message::Text(
        serde_json::json!({
            "type": "encrypted_session_init",
            "session_id": "victim-1234",
            "payload": {
                "ephPubHex": format!("0x{}", hex::encode(client_secret.public_key().to_sec1_bytes())),
                "ciphertextHex": format!("0x{}", hex::encode(&ciphertext)),
                "nonceHex": format!("0x{}", hex::encode(nonce)),
                "signatureHex": format!("0x{}", hex::encode(sig)),
                "aadHex": "",
            },
        })
        .to_string(),
    ))
    .await
    .unwrap();

    // Then a prompt on that same connection.
    ws.send(Message::Text(
        serde_json::json!({
            "type": "inference",
            "request": {
                "model": "qwen3.8-27b",
                "prompt": "who are you",
                "max_tokens": 8,
                "session_id": "victim-1234",
            },
        })
        .to_string(),
    ))
    .await
    .unwrap();

    // Collect frames until we see the refusal, or time out.
    let mut refusal = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await
        else {
            break;
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if value["type"] == "error" {
            let body = value["message"]
                .as_str()
                .or_else(|| value["error"].as_str())
                .unwrap_or_default()
                .to_string();
            // The staging failure frame is expected and is not the refusal.
            if value["code"] == "LORA_STAGING_FAILED" {
                continue;
            }
            refusal = Some(body);
            break;
        }
    }

    let refusal = refusal.expect("the prompt must be answered with an error, not content");
    assert!(
        refusal.contains("asked for a LoRA adapter and none is staged"),
        "the attacker's prompt must be refused because ITS OWN adapter is not staged. \
         Anything else — an engine error, or content — means resolution reached the \
         victim's adapter. Got: {refusal}"
    );
    // And the victim is untouched.
    assert!(deps.adapters.adapter_for("victim-1234").is_some());
}

/// T5.3 round-3 R3-1, through the real router: a re-init that FAILS must not
/// downgrade a live serve-back session to the base model.
///
/// Round 2's fix evicted the previous adapter and cleared the refusal at the
/// top of every init branch, before any gate. But `job_id` is only reassigned
/// inside the decrypt's `Ok` arm and the old session key stays live in the key
/// store, so ONE malformed re-init deleted the adapter, cleared the refusal,
/// and every later prompt was answered from the BASE MODEL and billed to the
/// old job, silently. That is the exact fail-open the `Required` state exists
/// to prevent, introduced by the fix for a fail-closed one.
///
/// This is also the only row that drives a SUCCESSFUL stage through the
/// router, so it covers the whole init → stage → registry path as well.
#[tokio::test]
async fn a_failed_reinit_refuses_prompts_rather_than_falling_back_to_the_base_model() {
    use fabstir_llm_node::api::server::ApiServer;
    use fabstir_llm_node::crypto::{derive_shared_key, encrypt_with_aead};
    use futures_util::{SinkExt, StreamExt};
    use k256::ecdsa::{signature::Signer, SigningKey};
    use k256::SecretKey;
    use rand::rngs::OsRng;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::Message;

    // The session's on-chain model must equal the template's serve-back pin,
    // or the stage is refused on the base check before it reaches anything
    // else. `fixture_template()` pins 0x00..00ba.
    let mut chain_model = [0u8; 32];
    chain_model[31] = 0xba;

    let fx = super::support::fixture(None).await;
    let afx = adapter_fixture(true, None).await;
    let mut harness = super::support::make_deps(
        &fx,
        super::support::MockSessions {
            snapshot: Ok(super::support::passing_snapshot()),
            model: chain_model,
            dispute: 30,
        },
        super::support::ScanBehaviour::Cleared,
        super::support::CountBehaviour::Tokens(9),
    );
    // The router stages from `deps.s5_base`, which defaults to the DATASET
    // fixture; point it at the adapter's mock S5.
    harness.deps.s5_base = afx.base_url.clone();
    let deps = Arc::new(harness.deps);

    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv: [u8; 32] = node_secret.to_bytes().into();
    let mut server = ApiServer::new_for_test();
    server.set_node_private_key_for_test(node_priv);
    let server = Arc::new(server);
    server.set_training_deps(deps.clone()).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let seal = |inner: serde_json::Value, nonce_len: usize| {
        let client_secret = SecretKey::random(&mut OsRng);
        let shared = derive_shared_key(
            node_secret.public_key().to_sec1_bytes().as_ref(),
            &client_secret.to_bytes(),
        )
        .unwrap();
        let nonce = [5u8; 24];
        let ciphertext =
            encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
        let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
        let mut sig = [0u8; 65];
        sig[..64].copy_from_slice(&signature.to_bytes());
        serde_json::json!({
            "type": "encrypted_session_init",
            "session_id": "job-777",
            "payload": {
                "ephPubHex": format!("0x{}", hex::encode(client_secret.public_key().to_sec1_bytes())),
                "ciphertextHex": format!("0x{}", hex::encode(&ciphertext)),
                // A short nonce is refused deterministically, with no crypto
                // setup, which is exactly the "re-init that fails" case.
                "nonceHex": format!("0x{}", hex::encode(&nonce[..nonce_len])),
                "signatureHex": format!("0x{}", hex::encode(sig)),
                "aadHex": "",
            },
        })
    };

    let good_inner = serde_json::json!({
        "jobId": "777",
        "modelName": "qwen3.8-27b",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 904,
        "lora": {
            "manifestCID": afx.manifest_cid,
            "manifestSha256": afx.manifest_sha256,
            "file": "adapter.gguf",
        },
    });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;

    // 1. A good init: the adapter stages for real through the router.
    ws.send(Message::Text(seal(good_inner, 24).to_string()))
        .await
        .unwrap();
    let mut staged_ok = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if !deps.adapters.is_empty_for_test() {
            staged_ok = true;
            break;
        }
    }
    assert!(
        staged_ok,
        "the router must stage the adapter for a well-formed init; without that this row \
         proves nothing about what a FAILED re-init does to it"
    );

    // 2. A malformed re-init: 23-byte nonce, refused before decrypt.
    ws.send(Message::Text(
        seal(serde_json::json!({ "jobId": "777" }), 23).to_string(),
    ))
    .await
    .unwrap();

    // 3. A prompt. It must be REFUSED, not answered from the base model.
    // The prompt carries IMAGES and an id. Images route to the VLM sidecar and
    // bill on-chain in the CALLER, ahead of the streaming function, so a gate
    // that sat below them would let the work happen before refusing (round-3
    // R3-3); and the id lets this row require the refusal to be correlated and
    // followed by a stream_end (round-4 F-R4-1), without which the SDK's
    // pending promise for it never settles.
    ws.send(Message::Text(
        serde_json::json!({
            "type": "inference",
            "id": "req-42",
            "images": ["data:image/png;base64,iVBORw0KGgo="],
            "request": {
                "model": "qwen3.8-27b",
                "prompt": "who are you",
                "max_tokens": 8,
                "session_id": "job-777",
            },
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut refusal = None;
    let mut refusal_id = serde_json::Value::Null;
    let mut saw_stream_end_for_req = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await
        else {
            break;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if value["type"] == "stream_end" && value["id"] == "req-42" {
            saw_stream_end_for_req = true;
            if refusal.is_some() {
                break;
            }
            continue;
        }
        if value["type"] != "error" {
            continue;
        }
        let code = value["code"].as_str().unwrap_or_default().to_string();
        // The nonce rejection is expected and is not the answer we want.
        if code == "INVALID_NONCE_SIZE" {
            continue;
        }
        refusal_id = value["id"].clone();
        refusal = Some((
            code,
            value["message"]
                .as_str()
                .or_else(|| value["error"].as_str())
                .unwrap_or_default()
                .to_string(),
        ));
        if saw_stream_end_for_req {
            break;
        }
    }

    let (code, message) = refusal.expect(
        "after a failed re-init the prompt must be refused. Silence here means it was \
         answered from the base model and billed to the old job",
    );
    // `LORA_NOT_STAGED` specifically, not merely a message that mentions the
    // adapter (round-4 F-R4-7): the streaming function's own refusal produces
    // the same words under `INFERENCE_FAILED`, so a looser assertion stayed
    // green with the pre-vision gate deleted, leaving the thing it exists for
    // unpinned.
    assert_eq!(
        code, "LORA_NOT_STAGED",
        "the refusal must come from the gate AHEAD of the vision work, got \
         code={code:?} message={message:?}"
    );
    assert!(message.contains("asked for a LoRA adapter"), "{message}");
    assert_eq!(refusal_id, "req-42", "the refusal must correlate with the request");
    assert!(
        saw_stream_end_for_req,
        "a refusal with no stream_end leaves the SDK's promise for this id unsettled, \
         which the customer sees as a hang rather than an error"
    );
}

#[tokio::test]
async fn the_node_refuses_beyond_its_live_adapter_cap() {
    // Round-3 R3-4: per-connection minted keys removed the incidental cap the
    // session-keyed design gave, so one client holding many sockets could fill
    // the staging volume an adapter at a time.
    use fabstir_llm_node::training::serve::ADAPTER_MAX_LIVE;
    let fx = adapter_fixture(true, None).await;
    let dir = tempfile::tempdir().unwrap();
    let registry = AdapterRegistry::new();
    for n in 0..ADAPTER_MAX_LIVE {
        registry
            .stage(
                &fx.base_url,
                dir.path(),
                &format!("key-{n}"),
                BASE_MODEL,
                BASE_MODEL,
                &request(&fx, "adapter.gguf"),
            )
            .await
            .unwrap_or_else(|e| panic!("stage {n} within the cap must succeed: {e}"));
    }
    let over = registry
        .stage(
            &fx.base_url,
            dir.path(),
            "one-too-many",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await;
    match over {
        Err(ServeError::Validation(detail)) => assert!(
            detail.contains("already serving"),
            "must be refused BY THE CAP, not incidentally: {detail}"
        ),
        other => panic!("beyond the cap must be refused, got {other:?}"),
    }
    // Freeing one lets the next in.
    registry.evict("key-0").await;
    registry
        .stage(
            &fx.base_url,
            dir.path(),
            "one-too-many",
            BASE_MODEL,
            BASE_MODEL,
            &request(&fx, "adapter.gguf"),
        )
        .await
        .expect("an eviction must free a slot");
}

/// T5.3 round-5 F-R5-7: pin BOTH arms of the round-4 same-job rule.
///
/// Round 4 stopped a successful re-init that omits `lora` from silently moving
/// a paying customer onto the base model. Neither arm of that rule was
/// executed by any test — reverting it to an unconditional clear left all rows
/// green, which is how the defect it fixes reached round 4 in the first place.
///
/// Arm one: same job, `lora` gone. That is a key refresh, not a new session,
/// so the connection must keep refusing. Arm two: a different job. That IS a
/// new session, so it must be allowed to proceed to the base model.
#[tokio::test]
async fn a_lora_less_reinit_refuses_on_the_same_job_and_proceeds_on_a_different_one() {
    use fabstir_llm_node::api::server::ApiServer;
    use fabstir_llm_node::crypto::{derive_shared_key, encrypt_with_aead};
    use futures_util::{SinkExt, StreamExt};
    use k256::ecdsa::{signature::Signer, SigningKey};
    use k256::SecretKey;
    use rand::rngs::OsRng;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::Message;

    let mut chain_model = [0u8; 32];
    chain_model[31] = 0xba;
    let fx = super::support::fixture(None).await;
    let afx = adapter_fixture(true, None).await;
    let mut harness = super::support::make_deps(
        &fx,
        super::support::MockSessions {
            snapshot: Ok(super::support::passing_snapshot()),
            model: chain_model,
            dispute: 30,
        },
        super::support::ScanBehaviour::Cleared,
        super::support::CountBehaviour::Tokens(9),
    );
    harness.deps.s5_base = afx.base_url.clone();
    let deps = Arc::new(harness.deps);

    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv: [u8; 32] = node_secret.to_bytes().into();
    let mut server = ApiServer::new_for_test();
    server.set_node_private_key_for_test(node_priv);
    let server = Arc::new(server);
    server.set_training_deps(deps.clone()).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let init = |inner: serde_json::Value| {
        let client_secret = SecretKey::random(&mut OsRng);
        let shared = derive_shared_key(
            node_secret.public_key().to_sec1_bytes().as_ref(),
            &client_secret.to_bytes(),
        )
        .unwrap();
        let nonce = [3u8; 24];
        let ciphertext =
            encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
        let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
        let mut sig = [0u8; 65];
        sig[..64].copy_from_slice(&signature.to_bytes());
        serde_json::json!({
            "type": "encrypted_session_init",
            "session_id": "s",
            "payload": {
                "ephPubHex": format!("0x{}", hex::encode(client_secret.public_key().to_sec1_bytes())),
                "ciphertextHex": format!("0x{}", hex::encode(&ciphertext)),
                "nonceHex": format!("0x{}", hex::encode(nonce)),
                "signatureHex": format!("0x{}", hex::encode(sig)),
                "aadHex": "",
            },
        })
        .to_string()
    };
    let payload = |job: &str, with_lora: bool| {
        let mut v = serde_json::json!({
            "jobId": job,
            "modelName": "qwen3.8-27b",
            "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "pricePerToken": 904,
        });
        if with_lora {
            v["lora"] = serde_json::json!({
                "manifestCID": afx.manifest_cid,
                "manifestSha256": afx.manifest_sha256,
                "file": "adapter.gguf",
            });
        }
        v
    };
    let prompt = serde_json::json!({
        "type": "inference",
        "id": "p1",
        "request": { "model": "qwen3.8-27b", "prompt": "hi", "max_tokens": 4, "session_id": "s" },
    })
    .to_string();

    async fn error_code(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let Ok(Some(Ok(Message::Text(text)))) =
                tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await
            else {
                break;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if v["type"] == "error" {
                return v["code"].as_str().unwrap_or_default().to_string();
            }
        }
        String::new()
    }

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;

    // Stage for job 777.
    ws.send(Message::Text(init(payload("777", true)))).await.unwrap();
    let mut staged = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if !deps.adapters.is_empty_for_test() {
            staged = true;
            break;
        }
    }
    assert!(staged, "the fixture must stage for real, or neither arm is exercised");

    // Arm zero: a PLAINTEXT init naming the same job. It evicts the adapter,
    // so if it also cleared the refusal every later prompt would be answered
    // from the base model and billed to job 777 (round-5 F-R5-2 — the same
    // defect as arm one, one branch away, and it survived a mutation until
    // this arm existed).
    ws.send(Message::Text(
        // Names the SAME job. Round-7 F-R7-6: when this omitted `job_id` the
        // predicate's `before != now` comparison had NO coverage at all —
        // inverting it to `before == now` left the whole suite green, which
        // would have silently reintroduced F-R5-2. The stale-capture case that
        // omission was covering now has its own row below, sharper.
        serde_json::json!({ "type": "session_init", "session_id": "s", "job_id": 777 })
            .to_string(),
    ))
    .await
    .unwrap();
    ws.send(Message::Text(prompt.clone())).await.unwrap();
    assert_eq!(
        error_code(&mut ws).await,
        "LORA_NOT_STAGED",
        "a plaintext re-init on the SAME job must not clear the refusal"
    );

    // Arm one: SAME job, no lora. A refresh — must keep refusing.
    ws.send(Message::Text(init(payload("777", false)))).await.unwrap();
    ws.send(Message::Text(prompt.clone())).await.unwrap();
    assert_eq!(
        error_code(&mut ws).await,
        "LORA_NOT_STAGED",
        "a re-init of the SAME job that drops `lora` is a key refresh, not a new session; \
         answering it from the base model is the silent downgrade this rule exists to stop"
    );

    // Arm two: a DIFFERENT job, no lora. Genuinely a new session — must proceed
    // (and then fail on the absent engine, which is not our concern here).
    ws.send(Message::Text(init(payload("888", false)))).await.unwrap();
    ws.send(Message::Text(prompt)).await.unwrap();
    // `assert_eq`, not `assert_ne` (round-6 F-R6-5): `error_code` returns an
    // empty string on timeout, so a mutation that merely SILENCES the socket —
    // a panic in the connection task, a wrong `break` — turned an `assert_ne`
    // green. `INFERENCE_FAILED` is what the current wiring produces here, from
    // the absent engine, so the arm now proves a frame actually arrived AND
    // that it was not a refusal.
    assert_eq!(
        error_code(&mut ws).await,
        "INFERENCE_FAILED",
        "a genuinely new session on the same socket must NOT inherit the previous \
         session's refusal — over-refusing is a failure mode too"
    );
}

/// T5.3 round-7 F-R7-7: the fix for the round-6 strand had no test of its own.
///
/// Round 6 found that the same-job predicate read the connection's MUTABLE
/// `job_id`, which every init overwrites including ones that go on to be
/// refused — so it degraded to `None` and no later init could prove "different
/// job", stranding the socket in permanent refusal. The fix compares against
/// `adapter_job_id` instead. But no row pinned it: the existing row's arms run
/// a SUCCESSFUL same-job init first, which repairs the stale capture before the
/// different-job arm is reached, so the pre-fix shape survived the suite.
///
/// This row puts a parameterless init DIRECTLY between the staging init and the
/// different-job init, with no successful same-job init in between, which is
/// the only ordering in which the strand materialises.
#[tokio::test]
async fn a_different_job_still_clears_after_an_init_that_carried_no_job_id() {
    use fabstir_llm_node::api::server::ApiServer;
    use fabstir_llm_node::crypto::{derive_shared_key, encrypt_with_aead};
    use futures_util::{SinkExt, StreamExt};
    use k256::ecdsa::{signature::Signer, SigningKey};
    use k256::SecretKey;
    use rand::rngs::OsRng;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::Message;

    let mut chain_model = [0u8; 32];
    chain_model[31] = 0xba;
    let fx = super::support::fixture(None).await;
    let afx = adapter_fixture(true, None).await;
    let mut harness = super::support::make_deps(
        &fx,
        super::support::MockSessions {
            snapshot: Ok(super::support::passing_snapshot()),
            model: chain_model,
            dispute: 30,
        },
        super::support::ScanBehaviour::Cleared,
        super::support::CountBehaviour::Tokens(9),
    );
    harness.deps.s5_base = afx.base_url.clone();
    let deps = Arc::new(harness.deps);

    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv: [u8; 32] = node_secret.to_bytes().into();
    let mut server = ApiServer::new_for_test();
    server.set_node_private_key_for_test(node_priv);
    let server = Arc::new(server);
    server.set_training_deps(deps.clone()).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let init = |inner: serde_json::Value| {
        let client_secret = SecretKey::random(&mut OsRng);
        let shared = derive_shared_key(
            node_secret.public_key().to_sec1_bytes().as_ref(),
            &client_secret.to_bytes(),
        )
        .unwrap();
        let nonce = [4u8; 24];
        let ciphertext =
            encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
        let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
        let mut sig = [0u8; 65];
        sig[..64].copy_from_slice(&signature.to_bytes());
        serde_json::json!({
            "type": "encrypted_session_init",
            "session_id": "s",
            "payload": {
                "ephPubHex": format!("0x{}", hex::encode(client_secret.public_key().to_sec1_bytes())),
                "ciphertextHex": format!("0x{}", hex::encode(&ciphertext)),
                "nonceHex": format!("0x{}", hex::encode(nonce)),
                "signatureHex": format!("0x{}", hex::encode(sig)),
                "aadHex": "",
            },
        })
        .to_string()
    };
    let payload = |job: &str, with_lora: bool| {
        let mut v = serde_json::json!({
            "jobId": job,
            "modelName": "qwen3.8-27b",
            "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "pricePerToken": 904,
        });
        if with_lora {
            v["lora"] = serde_json::json!({
                "manifestCID": afx.manifest_cid,
                "manifestSha256": afx.manifest_sha256,
                "file": "adapter.gguf",
            });
        }
        v
    };

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;

    // 1. Stage for job 777.
    ws.send(Message::Text(init(payload("777", true)))).await.unwrap();
    let mut staged = false;
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if !deps.adapters.is_empty_for_test() {
            staged = true;
            break;
        }
    }
    assert!(staged, "the fixture must stage for real, or this row proves nothing");

    // 2. An init carrying NO job_id, which clobbers the connection's job_id.
    //    This is the step that used to poison the comparison.
    ws.send(Message::Text(
        serde_json::json!({ "type": "session_init", "session_id": "s" }).to_string(),
    ))
    .await
    .unwrap();

    // 3. A genuinely different job, immediately, with no successful same-job
    //    init in between to repair the stale capture.
    ws.send(Message::Text(init(payload("888", false)))).await.unwrap();
    ws.send(Message::Text(
        serde_json::json!({
            "type": "inference",
            "id": "p9",
            "request": { "model": "qwen3.8-27b", "prompt": "hi", "max_tokens": 4, "session_id": "s" },
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut code = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await
        else {
            break;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if v["type"] == "error" {
            code = v["code"].as_str().unwrap_or_default().to_string();
            break;
        }
    }
    assert_eq!(
        code, "INFERENCE_FAILED",
        "job 888 is a new session and must proceed. LORA_NOT_STAGED here means the \
         predicate compared against the connection's mutable job_id, which step 2 \
         clobbered, and the socket is stranded refusing for its lifetime"
    );
}

/// T5.3 round-8 F-R8-5: the round-7 fix had no test that reached its own arm.
///
/// Round 7 added `(None, Some(_)) => true` so that a connection holding a
/// `Required` adapter which never bound to a job could still be cleared by a
/// later real job. Deleting that arm from both branches left all 147 rows
/// green, because no row ever produced a `Required` with no job bound.
///
/// The only shape that does is a `lora` whose `jobId` does not parse: the key
/// is minted, so the connection refuses, but staging fails with "serve-back
/// needs a jobId" and nothing is ever bound. Without the arm, no later init on
/// any job can clear it and the socket refuses for its lifetime.
#[tokio::test]
async fn an_adapter_that_never_bound_to_a_job_does_not_strand_the_socket() {
    use fabstir_llm_node::api::server::ApiServer;
    use fabstir_llm_node::crypto::{derive_shared_key, encrypt_with_aead};
    use futures_util::{SinkExt, StreamExt};
    use k256::ecdsa::{signature::Signer, SigningKey};
    use k256::SecretKey;
    use rand::rngs::OsRng;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite::Message;

    let fx = super::support::fixture(None).await;
    let afx = adapter_fixture(true, None).await;
    let harness = super::support::make_deps(
        &fx,
        super::support::MockSessions {
            snapshot: Ok(super::support::passing_snapshot()),
            model: super::support::model_id(0xAA),
            dispute: 30,
        },
        super::support::ScanBehaviour::Cleared,
        super::support::CountBehaviour::Tokens(9),
    );
    let deps = Arc::new(harness.deps);

    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv: [u8; 32] = node_secret.to_bytes().into();
    let mut server = ApiServer::new_for_test();
    server.set_node_private_key_for_test(node_priv);
    let server = Arc::new(server);
    server.set_training_deps(deps.clone()).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let init = |inner: serde_json::Value| {
        let client_secret = SecretKey::random(&mut OsRng);
        let shared = derive_shared_key(
            node_secret.public_key().to_sec1_bytes().as_ref(),
            &client_secret.to_bytes(),
        )
        .unwrap();
        let nonce = [8u8; 24];
        let ciphertext =
            encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
        let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
        let mut sig = [0u8; 65];
        sig[..64].copy_from_slice(&signature.to_bytes());
        serde_json::json!({
            "type": "encrypted_session_init",
            "session_id": "s",
            "payload": {
                "ephPubHex": format!("0x{}", hex::encode(client_secret.public_key().to_sec1_bytes())),
                "ciphertextHex": format!("0x{}", hex::encode(&ciphertext)),
                "nonceHex": format!("0x{}", hex::encode(nonce)),
                "signatureHex": format!("0x{}", hex::encode(sig)),
                "aadHex": "",
            },
        })
        .to_string()
    };

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let _welcome = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await;

    // 1. A lora whose jobId does not parse to u64. The key is minted and the
    //    connection refuses, but staging cannot bind it to a job.
    ws.send(Message::Text(init(serde_json::json!({
        "jobId": "",
        "modelName": "qwen3.8-27b",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 904,
        "lora": {
            "manifestCID": afx.manifest_cid,
            "manifestSha256": afx.manifest_sha256,
            "file": "adapter.gguf",
        },
    }))))
    .await
    .unwrap();

    // 2. A real, parseable job with no lora. This must clear the refusal.
    ws.send(Message::Text(init(serde_json::json!({
        "jobId": "777",
        "modelName": "qwen3.8-27b",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 904,
    }))))
    .await
    .unwrap();

    ws.send(Message::Text(
        serde_json::json!({
            "type": "inference",
            "id": "p7",
            "request": { "model": "qwen3.8-27b", "prompt": "hi", "max_tokens": 4, "session_id": "s" },
        })
        .to_string(),
    ))
    .await
    .unwrap();

    let mut code = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await
        else {
            break;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if v["type"] == "error" {
            let c = v["code"].as_str().unwrap_or_default().to_string();
            // The staging failure for step 1 is expected and is not the answer.
            if c == "LORA_STAGING_FAILED" {
                continue;
            }
            code = c;
            break;
        }
    }
    assert_eq!(
        code, "INFERENCE_FAILED",
        "job 777 is a real, bindable session and must clear a refusal that was never \
         bound to any job. LORA_NOT_STAGED here means the socket is stranded for its \
         lifetime with no init able to recover it"
    );
}
