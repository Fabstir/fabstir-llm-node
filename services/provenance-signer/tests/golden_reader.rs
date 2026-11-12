// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP0.3 masked Reader goldens: what a validator shows for each class, with the volatile fields (ids, salted
//! hashes, times, serials) masked. A change to any manifest content, structure or validation code shows here.
mod common;
use base64::Engine;
use common::*;
use fabstir_provenance::{flow, frames_index, Class};
use serde_json::Value;

fn mask(v: &mut Value) {
    match v {
        Value::Object(m) => {
            let old = std::mem::take(m);
            for (k, mut x) in old {
                if matches!(k.as_str(), "hash" | "time" | "cert_serial_number") {
                    x = Value::String("<masked>".into());
                } else {
                    mask(&mut x);
                }
                let mut key = Value::String(k);
                mask(&mut key);
                m.insert(key.as_str().unwrap().to_string(), x);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(mask),
        Value::String(s) => {
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = find_uuid(rest) {
                out.push_str(&rest[..i]);
                out.push_str("<uuid>");
                rest = &rest[i + 36..];
            }
            out.push_str(rest);
            *s = out;
        }
        _ => {}
    }
}

fn find_uuid(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(35)).find(|&i| {
        b[i..i + 36].iter().enumerate().all(|(k, c)| {
            if matches!(k, 8 | 13 | 18 | 23) {
                *c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
    })
}

fn golden(name: &str, mut v: Value) {
    mask(&mut v);
    let path = format!("{}/tests/golden/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let got =
        serde_json::to_string_pretty(&fabstir_provenance::defs::sort_keys(&v)).unwrap() + "\n";
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {path}"));
    assert_eq!(got, want, "golden {name} differs");
}

fn signed_reader_json(e: &Env, template: &str, n_inputs: usize, sequence: bool) -> Value {
    let r = e.reserve(template, sequence);
    let fp = r.params.soft_binding_alg.as_ref().map(|_| fingerprint());
    let p = node_sign_request(&r.reservation_id, &r.params, fp.as_deref(), n_inputs);
    let s = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    let out = flow::patch(&p.prep, &s.manifest).unwrap();
    let (state, _, bad, rd) = validate(&out, "video/mp4", &e.root_pem);
    assert!(bad.is_empty(), "{template}: {bad:?}");
    assert_eq!(state, "Trusted", "{template}");
    serde_json::from_str(&rd.json()).unwrap()
}

#[test]
fn reader_golden_generated_t2v() {
    let e = env();
    golden(
        "reader-generated-t2v",
        signed_reader_json(&e, "ltx-t2v-hdr", 0, false),
    );
}

#[test]
fn reader_golden_generated_i2v() {
    let e = env();
    golden(
        "reader-generated-i2v",
        signed_reader_json(&e, "ltx-i2v-hdr", 1, false),
    );
}

#[test]
fn reader_golden_modified_with_reference() {
    let e = env();
    golden(
        "reader-modified-restore25-ref",
        signed_reader_json(&e, "ltx-restore25-ref-hdr", 1, false),
    );
}

#[test]
fn reader_golden_modified_exr_job() {
    let e = env();
    golden(
        "reader-modified-sdr2hdr-exr",
        signed_reader_json(&e, "ltx-sdr2hdr-hdr", 0, true),
    );
}

#[test]
fn reader_golden_data_passes() {
    let e = env();
    golden(
        "reader-data-passes",
        signed_reader_json(&e, "cosmos-passes-key", 0, true),
    );
}

#[test]
fn reader_golden_sequence_manifest() {
    let e = env();
    let r = e.reserve("ltx-sdr2hdr-hdr", true);
    let idx = frames_index::build(
        Class::Modified,
        &[("frame_000001.exr".into(), b"one".to_vec())],
    )
    .unwrap();
    let req = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r.reservation_id,
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&idx),
    };
    let s = e
        .svc
        .sign_sequence(&e.host(), &serde_json::to_vec(&req).unwrap(), &req)
        .unwrap();
    let ctx = c2pa::Context::new()
        .with_settings(flow::settings(Some(&e.root_pem), false))
        .unwrap();
    let rd = c2pa::Reader::from_context(ctx)
        .with_manifest_data_and_stream(&s.manifest, "application/json", std::io::Cursor::new(idx))
        .unwrap();
    assert_eq!(format!("{:?}", rd.validation_state()), "Trusted");
    golden(
        "reader-sequence-sdr2hdr",
        serde_json::from_str(&rd.json()).unwrap(),
    );
}

/// Not a gate: writes one signed sample per class (+ the anchors) for cross-implementation checks
/// (c2pa-python, c2patool). `PROV_SAMPLES_DIR=<dir> cargo test --test golden_reader export_samples -- --ignored`
#[test]
#[ignore]
fn export_samples() {
    let dir =
        std::path::PathBuf::from(std::env::var("PROV_SAMPLES_DIR").expect("PROV_SAMPLES_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let e = env();
    std::fs::write(
        dir.join("anchors.pem"),
        format!("{}{}", e.root_pem, e.tsa.ca_pem),
    )
    .unwrap();
    for (t, n, seq) in [
        ("ltx-t2v-hdr", 0, false),
        ("ltx-i2v-hdr", 1, false),
        ("ltx-flf2v-hdr", 2, false),
        ("ltx-edit-hdr", 0, false),
        ("ltx-restore25-ref-hdr", 1, false),
        ("ltx-sdr2hdr-hdr", 0, true),
        ("cosmos-passes-key", 0, true),
    ] {
        let r = e.reserve(t, seq);
        let fp = r.params.soft_binding_alg.as_ref().map(|_| fingerprint());
        let p = node_sign_request(&r.reservation_id, &r.params, fp.as_deref(), n);
        let s = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
        std::fs::write(
            dir.join(format!("{t}.mp4")),
            flow::patch(&p.prep, &s.manifest).unwrap(),
        )
        .unwrap();
        if seq {
            let idx = frames_index::build(
                r.params.class,
                &[("frame_000001.exr".into(), b"one".to_vec())],
            )
            .unwrap();
            let req = fabstir_provenance::contract::SignSequenceRequest {
                reservation_id: r.reservation_id,
                slot: "sequence".into(),
                frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&idx),
            };
            let s = e
                .svc
                .sign_sequence(&e.host(), &serde_json::to_vec(&req).unwrap(), &req)
                .unwrap();
            std::fs::write(dir.join(format!("{t}.frames-index.json")), &idx).unwrap();
            std::fs::write(dir.join(format!("{t}.frames-index.json.c2pa")), &s.manifest).unwrap();
        }
    }
}
