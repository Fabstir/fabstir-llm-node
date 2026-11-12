// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP0.3 golden definitions: the deterministic ManifestDefinition JSON per class. Mutations: any change to
//! a field, label, action, source type, soft binding, sequence reference or claim generator turns one red.
mod common;
use common::*;
use fabstir_provenance::class::lookup;
use fabstir_provenance::defs::{self, DefError, Params, SequenceRef};
use fabstir_provenance::fingerprint;

const DIGEST: [u8; 32] = [0x11; 32];

fn mp4(template: &str, seq: bool) -> Params {
    let row = lookup(template).unwrap();
    let s = seq.then(|| SequenceRef {
        sequence_id: SEQ_ID.into(),
        manifest_uuid: UUID_B.into(),
    });
    Params::for_mp4(row, NODE_VERSION, UUID_A, URL, s)
}

fn def(p: &Params, n_inputs: usize) -> serde_json::Value {
    let d = p.soft_binding_alg.as_ref().map(|_| &DIGEST);
    defs::definition_json(p, d, n_inputs).unwrap()
}

#[test]
fn golden_generated_prompt_only() {
    golden(
        "definition-generated-t2v",
        &def(&mp4("ltx-t2v-hdr", false), 0),
    );
}

#[test]
fn golden_generated_from_a_still() {
    golden(
        "definition-generated-i2v",
        &def(&mp4("ltx-i2v-hdr", false), 1),
    );
}

#[test]
fn golden_generated_exr_job_with_sequence() {
    golden(
        "definition-generated-t2v-exr",
        &def(&mp4("ltx-t2v-hdr", true), 0),
    );
}

#[test]
fn golden_modified() {
    golden(
        "definition-modified-edit",
        &def(&mp4("ltx-edit-hdr", false), 0),
    );
}

#[test]
fn golden_modified_with_reference_input() {
    golden(
        "definition-modified-restore25-ref",
        &def(&mp4("ltx-restore25-ref-hdr", false), 1),
    );
}

#[test]
fn golden_data() {
    golden(
        "definition-data-passes",
        &def(&mp4("cosmos-passes-key", true), 0),
    );
}

#[test]
fn golden_sequence_manifest() {
    let p = Params::for_sequence(
        lookup("ltx-sdr2hdr-hdr").unwrap(),
        NODE_VERSION,
        UUID_B,
        URL,
    );
    golden(
        "definition-sequence-sdr2hdr",
        &defs::definition_json(&p, None, 0).unwrap(),
    );
}

#[test]
fn golden_params_issued_for_each_class() {
    let all: Vec<_> = [
        "ltx-t2v-hdr",
        "ltx-i2v-hdr",
        "ltx-edit-hdr",
        "ltx-alpha-hdr",
        "cosmos-passes-std",
    ]
    .iter()
    .map(|t| mp4(t, false))
    .collect();
    golden("params-mp4", &serde_json::json!(all));
}

#[test]
fn picture_output_requires_its_fingerprint_digest() {
    let p = mp4("ltx-t2v-hdr", false);
    assert_eq!(p.soft_binding_alg.as_deref(), Some(fingerprint::ALG));
    assert_eq!(
        defs::definition_json(&p, None, 0),
        Err(DefError::FingerprintPresence)
    );
}

#[test]
fn data_output_refuses_a_fingerprint_digest() {
    let p = mp4("cosmos-passes-full", false);
    assert!(p.soft_binding_alg.is_none());
    assert_eq!(
        defs::definition_json(&p, Some(&DIGEST), 0),
        Err(DefError::FingerprintPresence)
    );
}

#[test]
fn prompt_only_template_refuses_visual_inputs() {
    assert_eq!(
        defs::definition_json(&mp4("ltx-t2v-hdr", false), Some(&DIGEST), 1),
        Err(DefError::IngredientCount(1))
    );
}

#[test]
fn visual_input_template_requires_an_input() {
    assert_eq!(
        defs::definition_json(&mp4("ltx-i2v-hdr", false), Some(&DIGEST), 0),
        Err(DefError::IngredientCount(0))
    );
    assert_eq!(
        defs::definition_json(
            &mp4("ltx-i2v-hdr", false),
            Some(&DIGEST),
            defs::MAX_VISUAL_INPUTS + 1
        ),
        Err(DefError::IngredientCount(9))
    );
}

#[test]
fn definitions_carry_no_identity_or_prompt_fields() {
    // Belt and braces for D6: only these keys may appear anywhere in a definition.
    const ALLOWED: &[&str] = &[
        "claim_version",
        "claim_generator_info",
        "name",
        "version",
        "specVersion",
        "format",
        "label",
        "instance_id",
        "assertions",
        "data",
        "created",
        "actions",
        "action",
        "digitalSourceType",
        "parameters",
        "ingredientIds",
        "allActionsIncluded",
        "modelType",
        "modelName",
        "modelIdentifier",
        "contentProfile",
        "humanOversightLevel",
        "alg",
        "blocks",
        "scope",
        "value",
        "bindingMetadata",
        "informationalUrl",
        "sequenceId",
        "manifest",
    ];
    fn walk(v: &serde_json::Value, bad: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, x) in m {
                    if !ALLOWED.contains(&k.as_str()) {
                        bad.push(k.clone());
                    }
                    walk(x, bad);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, bad)),
            _ => {}
        }
    }
    for (t, n) in [
        ("ltx-t2v-hdr", 0),
        ("ltx-ingredients-hdr", 3),
        ("ltx-edit-chain-hdr", 1),
        ("cosmos-passes-key", 0),
    ] {
        let mut bad = vec![];
        walk(&def(&mp4(t, true), n), &mut bad);
        assert!(bad.is_empty(), "{t}: unexpected keys {bad:?}");
    }
}
