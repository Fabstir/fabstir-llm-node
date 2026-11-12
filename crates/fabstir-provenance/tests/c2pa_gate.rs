// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The D11(A) signer gate (`defs::check_builder`) on real c2pa Builders made by the node flow.
//! Signer policy: two clean review rounds required. Each refusal test is named by the defect it catches.
mod common;
use c2pa::{Builder, Context};
use common::*;
use fabstir_provenance::class::lookup;
use fabstir_provenance::defs::{self, check_builder, Params, Refusal, SequenceRef};
use fabstir_provenance::flow::{self, ReserveOnly};
use serde_json::{json, Value};
use std::io::Cursor;

const DIGEST: [u8; 32] = [0x11; 32];

fn ctx() -> Context {
    Context::new()
        .with_settings(flow::settings(None, false))
        .unwrap()
        .with_signer(Box::new(ReserveOnly(12_000)))
}

fn params(template: &str) -> Params {
    let seq = (template == "ltx-sdr2hdr-hdr").then(|| SequenceRef {
        sequence_id: SEQ_ID.into(),
        manifest_uuid: UUID_B.into(),
    });
    Params::for_mp4(lookup(template).unwrap(), NODE_VERSION, UUID_A, URL, seq)
}

fn digest_for(p: &Params) -> Option<[u8; 32]> {
    p.soft_binding_alg.as_ref().map(|_| DIGEST)
}

struct Media {
    src: Vec<u8>,
    parent: Vec<u8>,
    still: Vec<u8>,
}
fn media() -> Media {
    Media {
        src: fixture("tiny.mp4"),
        parent: fixture("tiny-parent.mp4"),
        still: fixture("still.png"),
    }
}

/// The honest node: the shared flow, with the inputs the template takes.
fn honest(p: &Params, m: &Media, n_inputs: usize) -> flow::Handoff {
    let mut formats: Vec<&str> = vec![];
    if p.class == fabstir_provenance::Class::Modified {
        formats.push("video/mp4");
    }
    formats.extend(std::iter::repeat_n("image/png", n_inputs));
    let d = digest_for(p);
    flow::prepare_mp4(ctx(), p, d.as_ref(), &formats, &m.src, 12_000)
        .unwrap()
        .0
}

/// A lying node: any definition JSON and any ingredients, through the same c2pa calls.
fn crafted(def: Value, ingredients: &[(Value, &str, &[u8])]) -> flow::Handoff {
    let mut b = Builder::from_context(ctx()).with_definition(def).unwrap();
    for (j, f, bytes) in ingredients {
        b.add_ingredient_from_stream(j.to_string(), f, &mut Cursor::new(bytes.to_vec()))
            .unwrap();
    }
    let mut ar = Cursor::new(Vec::new());
    b.to_archive(&mut ar).unwrap();
    flow::Handoff {
        archive: ar.into_inner(),
        bmff_hash: vec![7u8; 32],
        placeholder_len: 0,
        reserve: 12_000,
    }
}

/// The signer side: reload the archive, add ITS hard binding, run the gate against ITS params.
fn gate(
    h: &flow::Handoff,
    expected: &Params,
    digest: Option<[u8; 32]>,
) -> Result<defs::CheckReport, Refusal> {
    // what the signer does: Reader-based load (no legacy ZIP path)
    let mut b = flow::gate_load(ctx(), &h.archive).unwrap();
    b.set_format(&expected.format);
    b.add_assertion(
        flow::BMFF_LABEL,
        &flow::bmff_hash(Some(h.bmff_hash.clone())).unwrap(),
    )
    .unwrap();
    check_builder(&b, expected, digest.as_ref())
}

fn honest_def(p: &Params, n_inputs: usize) -> Value {
    defs::definition_json(p, digest_for(p).as_ref(), n_inputs).unwrap()
}

// ------------------------------------------------------------------------------------------ accepts

#[test]
fn accepts_every_class_from_the_honest_flow() {
    let m = media();
    for (t, n) in [
        ("ltx-t2v-hdr", 0),
        ("ltx-i2v-hdr", 1),
        ("ltx-flf2v-hdr", 2),
        ("ltx-edit-hdr", 0),
        ("ltx-restore25-ref-hdr", 1),
        ("ltx-sdr2hdr-hdr", 0),
        ("cosmos-passes-key", 0),
    ] {
        let p = params(t);
        let r = gate(&honest(&p, &m, n), &p, digest_for(&p)).unwrap_or_else(|e| panic!("{t}: {e}"));
        assert_eq!(r.n_inputs, n, "{t}");
        assert_eq!(r.has_visual_input, p.visual_input, "{t}");
    }
}

// ------------------------------------------------------------------------------------------ refusals

#[test]
fn refuses_non_matching_definition() {
    // the node claims "generated" for a reservation the signer issued as "modified"
    let m = media();
    let lie = params("ltx-t2v-hdr");
    let expected = params("ltx-edit-hdr");
    assert!(gate(&honest(&lie, &m, 0), &expected, digest_for(&expected)).is_err());
}

#[test]
fn refuses_wrong_digital_source_type() {
    let p = params("ltx-t2v-hdr");
    let mut def = honest_def(&p, 0);
    def["assertions"][0]["data"]["actions"][0]["digitalSourceType"] = json!(defs::SOURCE_TYPE_DATA);
    let h = crafted(def, &[]);
    assert_eq!(
        gate(&h, &p, digest_for(&p)),
        Err(Refusal::AssertionMismatch("c2pa.actions.v2".into()))
    );
}

#[test]
fn fingerprint_digest_mismatch_refused() {
    let m = media();
    let p = params("ltx-t2v-hdr");
    let h = honest(&p, &m, 0);
    assert_eq!(
        gate(&h, &p, Some([0x22; 32])),
        Err(Refusal::AssertionMismatch("c2pa.soft-binding".into()))
    );
}

#[test]
fn refuses_missing_soft_binding_on_a_picture() {
    let p = params("ltx-t2v-hdr");
    let mut def = honest_def(&p, 0);
    def["assertions"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["label"] != "c2pa.soft-binding");
    assert!(matches!(
        gate(&crafted(def, &[]), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_soft_binding_on_a_data_output() {
    let data = params("cosmos-passes-key");
    let mut def = honest_def(&data, 0);
    let pic = params("ltx-t2v-hdr");
    let sb = honest_def(&pic, 0)["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["label"] == "c2pa.soft-binding")
        .unwrap()
        .clone();
    def["assertions"].as_array_mut().unwrap().push(sb);
    assert!(gate(&crafted(def, &[]), &data, None).is_err());
}

#[test]
fn refuses_dropped_ai_disclosure() {
    let m = media();
    let p = params("ltx-edit-hdr");
    let mut def = honest_def(&p, 0);
    def["assertions"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["label"] != "c2pa.ai-disclosure");
    let ing = [(
        json!({"relationship":"parentOf","label":"parent"}),
        "video/mp4",
        &m.parent[..],
    )];
    assert!(matches!(
        gate(&crafted(def, &ing), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_extra_assertion() {
    let p = params("ltx-t2v-hdr");
    let mut def = honest_def(&p, 0);
    def["assertions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"label":"org.example.note","data":{"prompt":"a cat"},"created":true}));
    assert!(matches!(
        gate(&crafted(def, &[]), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_second_hard_binding() {
    // c2pa will not archive a definition that already holds a hash binding, so a lying node would have to
    // hand-craft the archive; model that by adding the second binding on the reloaded builder.
    let m = media();
    let p = params("ltx-t2v-hdr");
    let h = honest(&p, &m, 0);
    let mut b = Builder::from_context(ctx())
        .with_archive(Cursor::new(h.archive.clone()))
        .unwrap();
    b.set_format(&p.format);
    let data_hash: c2pa::assertions::DataHash = serde_json::from_value(
        json!({"exclusions":[],"name":"x","alg":"sha256","hash":[0,1,2],"pad":[]}),
    )
    .unwrap();
    b.add_assertion("c2pa.hash.data", &data_hash).unwrap();
    b.add_assertion(
        flow::BMFF_LABEL,
        &flow::bmff_hash(Some(h.bmff_hash.clone())).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        check_builder(&b, &p, Some(&DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_hard_binding_of_another_kind() {
    let m = media();
    let p = params("ltx-t2v-hdr");
    let h = honest(&p, &m, 0);
    let mut b = Builder::from_context(ctx())
        .with_archive(Cursor::new(h.archive.clone()))
        .unwrap();
    b.set_format(&p.format);
    let data_hash: c2pa::assertions::DataHash = serde_json::from_value(
        json!({"exclusions":[],"name":"x","alg":"sha256","hash":[0,1,2],"pad":[]}),
    )
    .unwrap();
    b.add_assertion("c2pa.hash.data", &data_hash).unwrap();
    assert!(matches!(
        check_builder(&b, &p, Some(&DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_other_manifest_label() {
    let m = media();
    let p = params("ltx-t2v-hdr");
    let mut other = p.clone();
    other.manifest_uuid = UUID_B.into();
    assert_eq!(
        gate(&honest(&other, &m, 0), &p, Some(DIGEST)),
        Err(Refusal::AssertionMismatch("label".into()))
    );
}

#[test]
fn refuses_other_node_version() {
    let m = media();
    let p = params("ltx-t2v-hdr");
    let mut other = p.clone();
    other.node_version = "8.64.1".into();
    assert_eq!(
        gate(&honest(&other, &m, 0), &p, Some(DIGEST)),
        Err(Refusal::AssertionMismatch("claim_generator_info".into()))
    );
}

#[test]
fn refuses_other_model() {
    let m = media();
    let p = params("ltx-t2v-hdr");
    let mut other = p.clone();
    other.model_name = "LTX-2.5".into();
    assert_eq!(
        gate(&honest(&other, &m, 0), &p, Some(DIGEST)),
        Err(Refusal::AssertionMismatch("c2pa.ai-disclosure".into()))
    );
}

#[test]
fn refuses_dropped_sequence_reference() {
    let m = media();
    let p = params("ltx-sdr2hdr-hdr");
    let mut lie = p.clone();
    lie.sequence = None;
    assert!(gate(&honest(&lie, &m, 0), &p, Some(DIGEST)).is_err());
}

#[test]
fn refuses_visual_input_template_without_an_input() {
    // an i2v clip registered as prompt-only would let the detector call the user's still "AI-generated" (D14)
    let p = params("ltx-i2v-hdr");
    let t2v_shape = defs::definition_json(&params("ltx-t2v-hdr"), Some(&DIGEST), 0).unwrap();
    let mut def = t2v_shape;
    def["assertions"][1]["data"] = honest_def(&p, 1)["assertions"][1]["data"].clone();
    assert!(matches!(
        gate(&crafted(def, &[]), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_ingredient_on_prompt_only_template() {
    let m = media();
    let p = params("ltx-t2v-hdr");
    let ing = [(
        json!({"relationship":"inputTo","label":"input0"}),
        "image/png",
        &m.still[..],
    )];
    assert!(matches!(
        gate(&crafted(honest_def(&p, 0), &ing), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_ingredient_with_a_title() {
    let m = media();
    let p = params("ltx-edit-hdr");
    let ing = [(
        json!({"relationship":"parentOf","label":"parent","title":"Jane's wedding.mp4"}),
        "video/mp4",
        &m.parent[..],
    )];
    assert!(matches!(
        gate(&crafted(honest_def(&p, 0), &ing), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_modified_without_parent() {
    let m = media();
    let p = params("ltx-edit-hdr");
    let ing = [(
        json!({"relationship":"inputTo","label":"parent"}),
        "video/mp4",
        &m.parent[..],
    )];
    assert!(matches!(
        gate(&crafted(honest_def(&p, 0), &ing), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_parent_on_generated_class() {
    let m = media();
    let p = params("ltx-i2v-hdr");
    let ing = [(
        json!({"relationship":"parentOf","label":"input0"}),
        "image/png",
        &m.still[..],
    )];
    assert!(matches!(
        gate(&crafted(honest_def(&p, 1), &ing), &p, Some(DIGEST)),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

// ------------------------------------------------------------------------------------------ review round 1

#[test]
fn refuses_node_written_ingredient_fields() {
    // every field c2pa would copy into the signed ingredient assertion besides the allow-list
    let m = media();
    let p = params("ltx-edit-hdr");
    for (k, v) in [
        ("description", json!("prompt: a cat on the moon")),
        (
            "informational_URI",
            json!("https://example.com/session/879"),
        ),
        ("document_id", json!("xmp.did:wallet-0x1234")),
        ("provenance", json!("https://example.com/m")),
        ("hash", json!("deadbeef")),
        (
            "metadata",
            json!({"reviewRatings": [{"value": 1, "explanation": "seed 42"}]}),
        ),
    ] {
        let mut ing = json!({"relationship":"parentOf","label":"parent"});
        ing[k] = v;
        let h = crafted(honest_def(&p, 0), &[(ing, "video/mp4", &m.parent[..])]);
        match gate(&h, &p, Some(DIGEST)) {
            Err(Refusal::ForbiddenStructure(msg)) => assert!(msg.contains(k), "{k}: {msg}"),
            // accepted only if c2pa's own archive round trip dropped the field: then it cannot be signed
            Ok(_) => {
                let b = Builder::from_context(ctx())
                    .with_archive(Cursor::new(h.archive.clone()))
                    .unwrap();
                let ing = serde_json::to_value(&b.definition.ingredients[0]).unwrap();
                assert!(
                    ing.get(k).map(|v| v.is_null()).unwrap_or(true),
                    "{k} survived the archive and was accepted"
                );
            }
            Err(e) => panic!("{k}: wrong refusal {e:?}"),
        }
    }
}

#[test]
fn refuses_free_text_instance_id() {
    let m = media();
    let p = params("ltx-edit-hdr");
    let ing =
        json!({"relationship":"parentOf","label":"parent","instance_id":"job 1418 / session 879"});
    assert!(matches!(
        gate(
            &crafted(honest_def(&p, 0), &[(ing, "video/mp4", &m.parent[..])]),
            &p,
            Some(DIGEST)
        ),
        Err(Refusal::ForbiddenStructure(_))
    ));
}

#[test]
fn refuses_builder_level_fields() {
    // the legacy ZIP archive form can set these (the signer refuses non-JUMBF archives too); set them on the
    // reloaded builder directly
    let m = media();
    let p = params("ltx-t2v-hdr");
    let h = honest(&p, &m, 0);
    let load = || {
        let mut b = Builder::from_context(ctx())
            .with_archive(Cursor::new(h.archive.clone()))
            .unwrap();
        b.set_format(&p.format);
        b.add_assertion(
            flow::BMFF_LABEL,
            &flow::bmff_hash(Some(h.bmff_hash.clone())).unwrap(),
        )
        .unwrap();
        b
    };
    let mut b = load();
    b.definition.vendor = Some("prompt-a-cat".into());
    assert!(
        matches!(
            check_builder(&b, &p, Some(&DIGEST)),
            Err(Refusal::ForbiddenStructure(_))
        ),
        "vendor"
    );
    let mut b = load();
    b.definition.hash_alg = Some("sha1".into());
    assert!(
        matches!(
            check_builder(&b, &p, Some(&DIGEST)),
            Err(Refusal::ForbiddenStructure(_))
        ),
        "hash alg"
    );
    let mut b = load();
    b.remote_url = Some("https://example.com/m".into());
    assert!(
        matches!(
            check_builder(&b, &p, Some(&DIGEST)),
            Err(Refusal::ForbiddenStructure(_))
        ),
        "remote url"
    );
    let mut b = load();
    b.no_embed = true;
    assert!(
        matches!(
            check_builder(&b, &p, Some(&DIGEST)),
            Err(Refusal::ForbiddenStructure(_))
        ),
        "no_embed"
    );
    let mut b = load();
    b.timestamp_manifest_labels.insert("urn:c2pa:x".into());
    assert!(
        matches!(
            check_builder(&b, &p, Some(&DIGEST)),
            Err(Refusal::ForbiddenStructure(_))
        ),
        "timestamp labels"
    );
    assert!(check_builder(&load(), &p, Some(&DIGEST)).is_ok());
    // (a v1 claim with our urn:c2pa label is refused by c2pa itself: "invalid Claim GUID")
}

#[test]
fn matte_sequences_are_data() {
    let row = lookup("ltx-alpha-hdr").unwrap();
    assert_eq!(
        Params::for_sequence(row, NODE_VERSION, UUID_B, URL).class,
        fabstir_provenance::Class::Data
    );
    assert_eq!(
        Params::for_sequence(
            lookup("ltx-sdr2hdr-hdr").unwrap(),
            NODE_VERSION,
            UUID_B,
            URL
        )
        .class,
        fabstir_provenance::Class::Modified
    );
}

// ------------------------------------------------------------------------------------------ review round 2

#[test]
fn ingredients_never_read_the_input_bytes() {
    // ingredients are made from their format alone: an Adobe-style XMP (xmp.iid, DocumentID, title) in the
    // user's still cannot reach the claim, and an input's own manifest is never parsed (contract v1.0)
    let still = fixture("still-xmp.png");
    assert!(
        String::from_utf8_lossy(&still).contains("xmp.iid:"),
        "fixture carries XMP"
    );
    let p = params("ltx-i2v-hdr");
    let m = media();
    let (h, _) =
        flow::prepare_mp4(ctx(), &p, Some(&DIGEST), &["image/png"], &m.src, 12_000).unwrap();
    let r = gate(&h, &p, Some(DIGEST)).expect("accepted");
    assert_eq!(r.ingredient_formats, vec!["image/png".to_string()]);
    let b = Builder::from_context(ctx())
        .with_archive(Cursor::new(h.archive.clone()))
        .unwrap();
    let ing = serde_json::to_value(&b.definition.ingredients[0]).unwrap();
    assert_eq!(
        ing["instance_id"],
        defs::ingredient_instance_id(&p.manifest_uuid, 0)
    );
    let keys: Vec<_> = ing.as_object().unwrap().keys().cloned().collect();
    assert!(
        keys.iter()
            .all(|k| defs::INGREDIENT_KEYS.contains(&k.as_str())),
        "{keys:?}"
    );
}

#[test]
fn ingredient_instance_ids_are_derived_not_random() {
    assert_eq!(
        defs::ingredient_instance_id(UUID_A, 0),
        defs::ingredient_instance_id(UUID_A, 0)
    );
    assert_ne!(
        defs::ingredient_instance_id(UUID_A, 0),
        defs::ingredient_instance_id(UUID_A, 1)
    );
    assert_ne!(
        defs::ingredient_instance_id(UUID_A, 0),
        defs::ingredient_instance_id(UUID_B, 0)
    );
    assert!(defs::ingredient_instance_id(UUID_A, 3).starts_with("xmp:iid:"));
}
