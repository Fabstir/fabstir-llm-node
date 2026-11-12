// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Signer policy (D10, D11(A), D12) through `Service`: reservation, the mp4 sign, the sequence sign.
//! Two clean review rounds required. Each test is named by the defect it catches.
mod common;
use base64::Engine;
use common::*;
use fabstir_provenance::contract::ErrorCode;
use fabstir_provenance::{flow, frames_index, Class};
use provenance_signer::repo::{Kind, Meta, Record};

fn ok_sign(
    e: &Env,
    template: &str,
    n_inputs: usize,
) -> (
    fabstir_provenance::contract::ReserveResponse,
    Prepared,
    provenance_signer::service::Signed,
) {
    let r = e.reserve(template, false);
    let fp = r.params.soft_binding_alg.as_ref().map(|_| fingerprint());
    let p = node_sign_request(&r.reservation_id, &r.params, fp.as_deref(), n_inputs);
    let s = e
        .svc
        .sign_mp4(&e.host(), &p.body, &p.req)
        .unwrap_or_else(|f| panic!("{template}: {f:?}"));
    (r, p, s)
}

#[test]
fn delivered_mp4_validates_and_is_timestamped() {
    let e = env();
    for (t, n) in [
        ("ltx-t2v-hdr", 0),
        ("ltx-i2v-hdr", 1),
        ("ltx-edit-hdr", 0),
        ("cosmos-passes-key", 0),
    ] {
        let (_, p, s) = ok_sign(&e, t, n);
        assert_eq!(s.manifest.len(), p.req.placeholder_len, "{t}");
        let out = flow::patch(&p.prep, &s.manifest).unwrap();
        let (state, ok, bad, _) = validate(&out, "video/mp4", &e.root_pem);
        assert!(bad.is_empty(), "{t}: {bad:?}");
        assert!(ok.iter().any(|c| c == "timeStamp.validated"), "{t}: {ok:?}");
        assert!(
            ok.iter().any(|c| c == "signingCredential.trusted"),
            "{t}: {ok:?}"
        );
        assert!(state == "Trusted" || state == "Valid", "{t}: {state}");
        assert_eq!(s.tsa.as_deref(), Some(e.tsa.url.as_str()), "{t}");
    }
}

#[test]
fn refuses_non_matching_definition() {
    // reservation says modified (edit); the node archives a generated (t2v-shaped) definition
    let e = env();
    let r = e.reserve("ltx-edit-hdr", false);
    let mut lie = r.params.clone();
    lie.class = Class::Generated;
    lie.visual_input = false;
    let p = node_sign_request(&r.reservation_id, &lie, Some(&fingerprint()), 0);
    let err = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err();
    assert!(
        matches!(
            err.0,
            ErrorCode::AssertionMismatch | ErrorCode::ForbiddenStructure
        ),
        "{err:?}"
    );
    assert!(
        e.svc
            .store
            .slot(&r.reservation_id, "mp4")
            .unwrap()
            .is_none(),
        "a refusal must not consume the slot"
    );
}

#[test]
fn fingerprint_digest_mismatch_refused() {
    // the archive's soft binding commits to one set; the request carries another
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    p.req.fingerprint_b64 =
        Some(base64::engine::general_purpose::STANDARD.encode(other_fingerprint()));
    p.body = serde_json::to_vec(&p.req).unwrap();
    let err = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err();
    assert_eq!(err.0, ErrorCode::FingerprintDigestMismatch);
    assert_eq!(err.1, "c2pa.soft-binding");
}

#[test]
fn picture_output_without_fingerprint_refused() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    p.req.fingerprint_b64 = None;
    p.body = serde_json::to_vec(&p.req).unwrap();
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err().0,
        ErrorCode::MalformedRequest
    );
}

#[test]
fn data_output_with_fingerprint_refused() {
    let e = env();
    let r = e.reserve("cosmos-passes-std", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, None, 0);
    p.req.fingerprint_b64 = Some(base64::engine::general_purpose::STANDARD.encode(fingerprint()));
    p.body = serde_json::to_vec(&p.req).unwrap();
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err().0,
        ErrorCode::ForbiddenStructure
    );
}

#[test]
fn one_signature_per_reserved_manifest() {
    let e = env();
    let (r, p, s) = ok_sign(&e, "ltx-t2v-hdr", 0);
    // identical retry (fresh nonce at the HTTP layer): the stored manifest, nothing signed again
    let again = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    assert!(again.replay);
    assert_eq!(again.manifest, s.manifest);
    // a different body for the consumed slot
    let p2 = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    assert_ne!(p2.body, p.body, "a rebuilt archive differs (salts)");
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p2.body, &p2.req).unwrap_err().0,
        ErrorCode::SlotConsumed
    );
}

#[test]
fn sign_stores_fingerprint_atomically() {
    let e = env();
    let (r, _, s) = ok_sign(&e, "ltx-i2v-hdr", 1);
    let rec = e
        .svc
        .repo
        .get(&r.params.manifest_uuid)
        .expect("record stored before the manifest is returned");
    assert_eq!(rec.manifest, s.manifest);
    assert_eq!(rec.fingerprint.as_deref(), Some(&fingerprint()[..]));
    assert!(rec.meta.has_visual_input);
    assert_eq!(rec.meta.class, Class::Generated);
    // t2v: no visual input
    let (r2, _, _) = ok_sign(&e, "ltx-t2v-hdr", 0);
    assert!(
        !e.svc
            .repo
            .get(&r2.params.manifest_uuid)
            .unwrap()
            .meta
            .has_visual_input
    );
    // a Data output has no fingerprint record
    let (r3, _, _) = ok_sign(&e, "cosmos-passes-full", 0);
    assert!(e
        .svc
        .repo
        .get(&r3.params.manifest_uuid)
        .unwrap()
        .fingerprint
        .is_none());
}

#[test]
fn occupied_manifest_id_is_refused() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    // someone else's record already occupies the id (first write wins)
    let squat = Record {
        manifest: b"x".to_vec(),
        meta: Meta {
            kind: Kind::Mp4,
            class: Class::Generated,
            has_visual_input: false,
            time: 0,
            public: true,
            request_sha256: "00".into(),
        },
        fingerprint: None,
        frames_index: None,
    };
    e.svc.repo.put(&r.params.manifest_uuid, &squat).unwrap();
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    assert!(e.svc.sign_mp4(&e.host(), &p.body, &p.req).is_err());
    assert_eq!(
        e.svc.repo.get(&r.params.manifest_uuid).unwrap().manifest,
        b"x"
    );
}

#[test]
fn no_manifest_returned_when_the_repository_write_fails() {
    // the store-before-return rule: a disk failure after signing must not hand out an unregistered manifest
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let records = e.dir.path().join("repo").join("records");
    std::fs::set_permissions(&records, std::fs::Permissions::from_mode(0o500)).unwrap();
    let out = e.svc.sign_mp4(&e.host(), &p.body, &p.req);
    std::fs::set_permissions(&records, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        out.unwrap_err().0,
        fabstir_provenance::contract::ErrorCode::Internal
    );
    assert!(
        e.svc
            .store
            .slot(&r.reservation_id, "mp4")
            .unwrap()
            .is_none(),
        "slot stays free for the retry"
    );
    // and the retry succeeds once the disk is back
    assert!(e.svc.sign_mp4(&e.host(), &p.body, &p.req).is_ok());
}

#[test]
fn repository_is_first_write_wins() {
    let e = env();
    let uuid = "6b1f3c9e-2a4d-4f0b-9c1e-5a7d8e9f0a1b";
    let meta = Meta {
        kind: Kind::Mp4,
        class: Class::Generated,
        has_visual_input: false,
        time: 1,
        public: true,
        request_sha256: "aa".into(),
    };
    let a = Record {
        manifest: b"first".to_vec(),
        meta: meta.clone(),
        fingerprint: Some(fingerprint()),
        frames_index: None,
    };
    let b = Record {
        manifest: b"second".to_vec(),
        meta,
        fingerprint: None,
        frames_index: None,
    };
    e.svc.repo.put(uuid, &a).unwrap();
    assert!(e.svc.repo.put(uuid, &b).is_err());
    assert_eq!(e.svc.repo.get(uuid).unwrap().manifest, b"first");
    assert_eq!(e.svc.repo.with_index(|i| i.len()), 1);
    assert!(
        e.svc.repo.put("../escape", &a).is_err(),
        "ids are uuids only"
    );
}

#[test]
fn reservation_belongs_to_its_host_and_expires() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let other = hex::encode(addr(&other_key()));
    assert_eq!(
        e.svc.sign_mp4(&other, &p.body, &p.req).unwrap_err().0,
        ErrorCode::ReservationWrongHost
    );
    e.advance(1800 + 900 + 1);
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err().0,
        ErrorCode::ReservationExpired
    );
    let mut unknown = p.req.clone();
    unknown.reservation_id = "rsv_000000000000000000000000".into();
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &unknown).unwrap_err().0,
        ErrorCode::ReservationUnknown
    );
}

#[test]
fn reserve_refuses_unclassed_old_or_malformed() {
    let e = env();
    let h = e.host();
    let mut q = reserve_req("ltx-unknown-hdr", false);
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::ClassMismatch
    );
    q = reserve_req("ltx-t2v-hdr", false);
    q.bundle_version = 48;
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::VersionMismatch,
        "v48 graphs leak the prompt"
    );
    q = reserve_req("ltx-restore25-hdr", true);
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::ClassMismatch,
        "no EXR from this template"
    );
    q = reserve_req("ltx-t2v-hdr", false);
    q.node_version = "8.64.0 prompt: a cat".into();
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::VersionMismatch
    );
    q.node_version = "8.64.0-session4711".into();
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::VersionMismatch,
        "only configured versions, never free text"
    );
    q = reserve_req("ltx-t2v-hdr", false);
    q.deadline_secs = 0;
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::MalformedRequest
    );
    q = reserve_req("ltx-t2v-hdr", false);
    q.slots.mp4 = 2;
    assert_eq!(
        e.svc.reserve(&h, &q).unwrap_err().0,
        ErrorCode::MalformedRequest
    );
}

#[test]
fn reserve_issues_class_from_its_own_table() {
    let e = env();
    let r = e.reserve("ltx-alpha-hdr", true);
    assert_eq!(r.params.class, Class::Modified);
    assert_eq!(r.params.model_name, "LTX-2.5");
    assert!(r.params.soft_binding_alg.is_some());
    let g = r.sequence.expect("sequence granted");
    assert_eq!(r.params.sequence.as_ref().unwrap().sequence_id, g.id);
    assert_ne!(g.manifest_uuid, r.params.manifest_uuid);
    assert_eq!(r.expires_at, T0 + 1800 + 900);
}

#[test]
fn reserve_out_of_range_refused() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    p.req.reserve_size = 4_000;
    p.body = serde_json::to_vec(&p.req).unwrap();
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err().0,
        ErrorCode::ReserveOutOfRange
    );
}

#[test]
fn placeholder_length_must_match() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    p.req.placeholder_len += 8;
    p.body = serde_json::to_vec(&p.req).unwrap();
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err().0,
        ErrorCode::PlaceholderLengthMismatch
    );
}

#[test]
fn timestamp_fails_over_to_second_tsa() {
    let dead = dead_url();
    let d2 = dead.clone();
    let e = env_with(move |good| vec![d2, good.to_string()]);
    let (_, _, s) = ok_sign(&e, "ltx-t2v-hdr", 0);
    assert_eq!(s.tsa.as_deref(), Some(e.tsa.url.as_str()));
}

#[test]
fn all_tsas_down_is_retryable_and_consumes_nothing() {
    let (a, b) = (dead_url(), dead_url());
    let e = env_with(move |_| vec![a, b]);
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let err = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err();
    assert_eq!(err.0, ErrorCode::TsaUnavailable);
    assert!(err.0.retryable());
    assert!(e
        .svc
        .store
        .slot(&r.reservation_id, "mp4")
        .unwrap()
        .is_none());
    assert!(e.svc.repo.get(&r.params.manifest_uuid).is_none());
}

#[test]
fn sequence_manifest_binds_the_frames_index() {
    let e = env();
    let r = e.reserve("ltx-sdr2hdr-hdr", true);
    let g = r.sequence.clone().unwrap();
    let frames = vec![
        ("frame_000001.exr".to_string(), b"one".to_vec()),
        ("frame_000002.exr".to_string(), b"two".to_vec()),
    ];
    let idx = frames_index::build(Class::Modified, &frames).unwrap();
    let req = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r.reservation_id.clone(),
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&idx),
    };
    let body = serde_json::to_vec(&req).unwrap();
    let s = e.svc.sign_sequence(&e.host(), &body, &req).unwrap();
    assert_eq!(s.manifest_uuid, g.manifest_uuid);
    let rec = e.svc.repo.get(&g.manifest_uuid).unwrap();
    assert_eq!(rec.frames_index.as_deref(), Some(&idx[..]));
    // validates against the index; one flipped byte does not
    let ctx = c2pa::Context::new()
        .with_settings(flow::settings(Some(&e.root_pem), false))
        .unwrap();
    let ok = c2pa::Reader::from_context(ctx)
        .with_manifest_data_and_stream(
            &s.manifest,
            "application/json",
            std::io::Cursor::new(idx.clone()),
        )
        .unwrap();
    assert_ne!(ok.validation_state(), c2pa::ValidationState::Invalid);
    let mut bad = idx.clone();
    let n = bad.len() - 5;
    bad[n] ^= 1;
    let ctx = c2pa::Context::new()
        .with_settings(flow::settings(Some(&e.root_pem), false))
        .unwrap();
    let tampered = c2pa::Reader::from_context(ctx)
        .with_manifest_data_and_stream(&s.manifest, "application/json", std::io::Cursor::new(bad))
        .unwrap();
    assert_eq!(tampered.validation_state(), c2pa::ValidationState::Invalid);
    // wrong class in the index refused
    let wrong = frames_index::build(Class::Generated, &frames).unwrap();
    let r2 = e.reserve("ltx-sdr2hdr-hdr", true);
    let req2 = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r2.reservation_id,
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(wrong),
    };
    assert_eq!(
        e.svc
            .sign_sequence(&e.host(), &serde_json::to_vec(&req2).unwrap(), &req2)
            .unwrap_err()
            .0,
        ErrorCode::MalformedRequest
    );
}

#[test]
fn sequence_slot_requires_a_sequence_reservation() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let idx = frames_index::build(
        Class::Generated,
        &[("frame_000001.exr".into(), b"x".to_vec())],
    )
    .unwrap();
    let req = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r.reservation_id,
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(idx),
    };
    assert_eq!(
        e.svc
            .sign_sequence(&e.host(), &serde_json::to_vec(&req).unwrap(), &req)
            .unwrap_err()
            .0,
        ErrorCode::MalformedRequest
    );
}

#[test]
fn index_match_finds_registered_outputs_only() {
    let e = env();
    let (r, _, _) = ok_sign(&e, "ltx-i2v-hdr", 1);
    let m = e.svc.index_match(&fingerprint()).unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(
        m[0].manifest,
        format!("urn:c2pa:{}", r.params.manifest_uuid)
    );
    assert!(m[0].has_visual_input);
    assert!(e.svc.index_match(&other_fingerprint()).unwrap().is_empty());
    let json = serde_json::to_string(&m).unwrap();
    assert!(
        !json.contains("trits") && !json.contains("fingerprint"),
        "never raw fingerprints: {json}"
    );
}

#[test]
fn rate_limit_per_host() {
    let e = env();
    for _ in 0..600 {
        e.reserve("ltx-t2v-hdr", false);
    }
    assert_eq!(
        e.svc
            .reserve(&e.host(), &reserve_req("ltx-t2v-hdr", false))
            .unwrap_err()
            .0,
        ErrorCode::RateLimited
    );
    e.advance(61);
    assert!(e
        .svc
        .reserve(&e.host(), &reserve_req("ltx-t2v-hdr", false))
        .is_ok());
}

// ------------------------------------------------------------------------------------------ review round 1

#[test]
fn concurrent_requests_for_one_slot_sign_once() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let a = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let b = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    assert_ne!(a.body, b.body);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let outs: Vec<_> = [a, b]
        .into_iter()
        .map(|p| {
            let (svc, host, bar) = (e.svc.clone(), e.host(), barrier.clone());
            std::thread::spawn(move || {
                bar.wait();
                svc.sign_mp4(&host, &p.body, &p.req).map(|s| s.manifest)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    assert_eq!(outs.iter().filter(|o| o.is_ok()).count(), 1, "{outs:?}");
    assert_eq!(
        outs.iter()
            .filter(|o| matches!(o, Err(f) if f.0 == ErrorCode::SlotConsumed))
            .count(),
        1
    );
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xedb8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A real (stored, uncompressed) ZIP holding `manifest.json`: the legacy archive form c2pa would load.
fn legacy_zip(manifest_json: &[u8]) -> Vec<u8> {
    let name = b"manifest.json";
    let crc = crc32(manifest_json);
    let n = manifest_json.len() as u32;
    let mut z = vec![];
    let le16 = |v: u16| v.to_le_bytes().to_vec();
    let le32 = |v: u32| v.to_le_bytes().to_vec();
    z.extend(le32(0x0403_4b50));
    for v in [20u16, 0, 0, 0, 0] {
        z.extend(le16(v));
    }
    z.extend(le32(crc));
    z.extend(le32(n));
    z.extend(le32(n));
    z.extend(le16(name.len() as u16));
    z.extend(le16(0));
    z.extend_from_slice(name);
    z.extend_from_slice(manifest_json);
    let cd = z.len() as u32;
    z.extend(le32(0x0201_4b50));
    for v in [20u16, 20, 0, 0, 0, 0] {
        z.extend(le16(v));
    }
    z.extend(le32(crc));
    z.extend(le32(n));
    z.extend(le32(n));
    for v in [name.len() as u16, 0, 0, 0, 0] {
        z.extend(le16(v));
    }
    z.extend(le32(0));
    z.extend(le32(0));
    z.extend_from_slice(name);
    let cd_len = z.len() as u32 - cd;
    z.extend(le32(0x0605_4b50));
    for v in [0u16, 0, 1, 1] {
        z.extend(le16(v));
    }
    z.extend(le32(cd_len));
    z.extend(le32(cd));
    z.extend(le16(0));
    z
}

#[test]
fn legacy_zip_archive_refused() {
    // the ZIP form can set builder fields (intent, vendor, remote_url) and decompresses without bound
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    // a loadable legacy archive: the honest builder's own JSON plus a smuggled builder field
    let archive = base64::engine::general_purpose::STANDARD
        .decode(&p.req.builder_archive_b64)
        .unwrap();
    let honest = c2pa::Builder::from_context(c2pa::Context::new())
        .with_archive(std::io::Cursor::new(archive))
        .unwrap();
    let mut j = serde_json::to_value(&honest).unwrap();
    j["vendor"] = serde_json::json!("prompt-a-cat");
    let zip = legacy_zip(&serde_json::to_vec(&j).unwrap());
    let loaded = c2pa::Builder::from_context(c2pa::Context::new())
        .with_archive(std::io::Cursor::new(zip.clone()))
        .unwrap();
    assert_eq!(
        loaded.definition.vendor.as_deref(),
        Some("prompt-a-cat"),
        "c2pa really loads this form"
    );
    p.req.builder_archive_b64 = base64::engine::general_purpose::STANDARD.encode(zip);
    p.body = serde_json::to_vec(&p.req).unwrap();
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap_err().0,
        ErrorCode::ArchiveUnreadable
    );
}

#[test]
fn input_carrying_a_manifest_is_refused_in_v1() {
    // A lying node feeds part 1 (signed, carrying our manifest) to c2pa as a stream ingredient, so the
    // ingredient carries a manifest and node-written validation results: refused. (The honest node builds
    // ingredients from their formats alone and never parses an input's manifest.)
    let e = env();
    let r1 = e.reserve("ltx-edit-chain-hdr", false);
    let p1 = node_sign_request(&r1.reservation_id, &r1.params, Some(&fingerprint()), 0);
    let s1 = e.svc.sign_mp4(&e.host(), &p1.body, &p1.req).unwrap();
    let part1 = flow::patch(&p1.prep, &s1.manifest).unwrap();
    let r2 = e.reserve("ltx-edit-chain-hdr", false);
    let ctx = c2pa::Context::new()
        .with_settings(flow::settings(Some(&e.root_pem), false))
        .unwrap()
        .with_signer(Box::new(fabstir_provenance::flow::ReserveOnly(12_000)));
    let digest = fabstir_provenance::fingerprint::digest(&fingerprint());
    let def = fabstir_provenance::defs::definition_json(&r2.params, Some(&digest), 0).unwrap();
    let mut b = c2pa::Builder::from_context(ctx)
        .with_definition(def)
        .unwrap();
    b.add_ingredient_from_stream(
        serde_json::json!({"relationship": "parentOf", "label": "parent"}).to_string(),
        "video/mp4",
        &mut std::io::Cursor::new(part1),
    )
    .unwrap();
    let mut ar = std::io::Cursor::new(Vec::new());
    b.to_archive(&mut ar).unwrap();
    let h = flow::Handoff {
        archive: ar.into_inner(),
        bmff_hash: vec![7u8; 32],
        placeholder_len: 14_357,
        reserve: 12_000,
    };
    let b64 = base64::engine::general_purpose::STANDARD;
    let req = fabstir_provenance::contract::SignRequest {
        reservation_id: r2.reservation_id.clone(),
        slot: "mp4".into(),
        reserve_size: h.reserve,
        placeholder_len: h.placeholder_len,
        bmff_hash_sha256_hex: hex::encode(&h.bmff_hash),
        builder_archive_b64: b64.encode(&h.archive),
        fingerprint_b64: Some(b64.encode(fingerprint())),
    };
    let body = serde_json::to_vec(&req).unwrap();
    let err = e.svc.sign_mp4(&e.host(), &body, &req).unwrap_err();
    assert_eq!(err.0, ErrorCode::ForbiddenStructure, "{err:?}");
}

#[test]
fn public_manifest_is_a_bare_store_that_validates() {
    let e = env();
    let (r, p, s) = ok_sign(&e, "ltx-t2v-hdr", 0);
    let (bytes, _) = e
        .svc
        .public_manifest(&format!("urn:c2pa:{}", r.params.manifest_uuid))
        .unwrap();
    assert_eq!(&bytes[..], &s.manifest[flow::C2PA_BOX_HDR..]);
    // a remote-manifest consumer: the store plus the delivered file
    let out = flow::patch(&p.prep, &s.manifest).unwrap();
    let ctx = c2pa::Context::new()
        .with_settings(flow::settings(Some(&e.root_pem), false))
        .unwrap();
    let rd = c2pa::Reader::from_context(ctx)
        .with_manifest_data_and_stream(&bytes, "video/mp4", std::io::Cursor::new(out))
        .unwrap();
    assert_ne!(rd.validation_state(), c2pa::ValidationState::Invalid);
}

#[test]
fn frames_index_is_public_for_sequences_only() {
    let e = env();
    let r = e.reserve("ltx-sdr2hdr-hdr", true);
    let g = r.sequence.clone().unwrap();
    let idx = frames_index::build(
        Class::Modified,
        &[("frame_000001.exr".into(), b"one".to_vec())],
    )
    .unwrap();
    let req = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r.reservation_id.clone(),
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&idx),
    };
    e.svc
        .sign_sequence(&e.host(), &serde_json::to_vec(&req).unwrap(), &req)
        .unwrap();
    assert_eq!(
        e.svc.public_frames_index(&g.manifest_uuid).as_deref(),
        Some(&idx[..])
    );
    let (r2, _, _) = ok_sign(&e, "ltx-t2v-hdr", 0);
    assert!(e
        .svc
        .public_frames_index(&r2.params.manifest_uuid)
        .is_none());
}

#[test]
fn alpha_matte_sequence_is_signed_as_data() {
    let e = env();
    let r = e.reserve("ltx-alpha-hdr", true);
    let idx =
        frames_index::build(Class::Data, &[("frame_000001.exr".into(), b"m".to_vec())]).unwrap();
    let req = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r.reservation_id,
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&idx),
    };
    assert!(e
        .svc
        .sign_sequence(&e.host(), &serde_json::to_vec(&req).unwrap(), &req)
        .is_ok());
}

#[test]
fn index_query_size_is_capped() {
    let e = env();
    let mut big = fabstir_provenance::fingerprint::SignatureSet::unpack(&fingerprint()).unwrap();
    let f0 = big.frames[0].clone();
    big.frames = (0..provenance_signer::service::MAX_QUERY_FRAMES as u64 + 1)
        .map(|i| {
            let mut f = f0.clone();
            f.media_time = i;
            f
        })
        .collect();
    assert_eq!(
        e.svc.index_match(&big.pack().unwrap()).unwrap_err().0,
        ErrorCode::BodyTooLarge
    );
}

#[test]
fn reservation_ids_are_format_checked_before_locking() {
    let e = env();
    let (_, p, _) = ok_sign(&e, "ltx-t2v-hdr", 0);
    assert_eq!(
        e.svc.lock_count(),
        0,
        "a finished request releases its lock entry"
    );
    let mut req = p.req.clone();
    req.reservation_id = "x".repeat(10_000);
    assert_eq!(
        e.svc.sign_mp4(&e.host(), &p.body, &req).unwrap_err().0,
        ErrorCode::ReservationUnknown
    );
    assert_eq!(e.svc.lock_count(), 0, "junk ids never enter the lock map");
}

#[test]
fn request_rate_is_limited_per_host_on_every_route() {
    let e = env_full(|good| vec![good.to_string()], |s| s.requests_per_minute = 3);
    let k = host_key();
    let mut codes = vec![];
    for i in 0..5u8 {
        let h =
            fabstir_provenance::contract::sign_request(&k, "POST", "/v1/sign", T0, [i; 16], b"{}");
        codes.push(
            e.svc
                .authenticate(
                    "POST",
                    "/v1/sign",
                    &h.timestamp,
                    &h.nonce,
                    &h.signature,
                    b"{}",
                )
                .map(|_| ())
                .map_err(|f| f.0),
        );
    }
    assert_eq!(codes.iter().filter(|c| c.is_ok()).count(), 3, "{codes:?}");
    assert!(codes[3..].iter().all(|c| *c == Err(ErrorCode::RateLimited)));
}

// ------------------------------------------------------------------------------------------ review round 2

/// A crafted archive for reservation `r`: the honest definition in its resolved form with the given action
/// ingredient hash, plus one parent ingredient with the given instance id. Same lengths as the honest one.
fn crafted_archive(
    r: &fabstir_provenance::contract::ReserveResponse,
    ref_hash: &[u8; 32],
    instance_id: &str,
) -> Vec<u8> {
    let digest = fabstir_provenance::fingerprint::digest(&fingerprint());
    let mut def = fabstir_provenance::defs::definition_json(&r.params, Some(&digest), 0).unwrap();
    def["assertions"][0]["data"]["actions"][0]["parameters"] = serde_json::json!({
        "ingredients": [{"url": "self#jumbf=c2pa.assertions/c2pa.ingredient.v3", "hash": base64::engine::general_purpose::STANDARD.encode(ref_hash)}]
    });
    let ctx = c2pa::Context::new()
        .with_settings(flow::settings(None, false))
        .unwrap()
        .with_signer(Box::new(flow::ReserveOnly(12_000)));
    let mut b = c2pa::Builder::from_context(ctx)
        .with_definition(def)
        .unwrap();
    let ing: c2pa::Ingredient = serde_json::from_value(serde_json::json!({"relationship": "parentOf", "label": "parent", "format": "video/mp4", "instance_id": instance_id})).unwrap();
    b.add_ingredient(ing);
    let mut ar = std::io::Cursor::new(Vec::new());
    b.to_archive(&mut ar).unwrap();
    ar.into_inner()
}

#[test]
fn node_bytes_in_action_references_are_never_signed() {
    let e = env();
    let r = e.reserve("ltx-edit-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let junk = [0x41u8; 32];
    let archive = crafted_archive(
        &r,
        &junk,
        &fabstir_provenance::defs::ingredient_instance_id(&r.params.manifest_uuid, 0),
    );
    p.req.builder_archive_b64 = base64::engine::general_purpose::STANDARD.encode(archive);
    p.body = serde_json::to_vec(&p.req).unwrap();
    let s = e
        .svc
        .sign_mp4(&e.host(), &p.body, &p.req)
        .expect("the crafted archive passes the gate (hashes are not compared)");
    assert!(
        !s.manifest.windows(32).any(|w| w == junk),
        "the node's 32 bytes reached the signed claim"
    );
    // and the signed output still validates
    let out = flow::patch(&p.prep, &s.manifest).unwrap();
    let (_, _, bad, _) = validate(&out, "video/mp4", &e.root_pem);
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn node_chosen_ingredient_instance_ids_are_never_signed() {
    let e = env();
    let r = e.reserve("ltx-edit-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let covert = "xmp:iid:deadbeef-0000-1111-2222-cafef00dcafe";
    let archive = crafted_archive(&r, &[0u8; 32], covert);
    p.req.builder_archive_b64 = base64::engine::general_purpose::STANDARD.encode(archive);
    p.body = serde_json::to_vec(&p.req).unwrap();
    let s = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    assert!(
        !String::from_utf8_lossy(&s.manifest).contains("deadbeef"),
        "a node-chosen id reached the claim"
    );
    let want = fabstir_provenance::defs::ingredient_instance_id(&r.params.manifest_uuid, 0);
    assert!(String::from_utf8_lossy(&s.manifest).contains(want.trim_start_matches("xmp:iid:")));
}
