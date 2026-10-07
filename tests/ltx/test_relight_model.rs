// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// VFX Passes (VP1.1): relight model ids, the D20 model check per family, the relight-fhd rule, the store's `sidecar` field,
// colour encoding, the attestation model id and env meta, and the start check. Each test names the mutation that turns it red.

use fabstir_llm_node::ltx::relight::{
    attestation_model_id, check_relight_start, relight_env_meta, RelightEcho, RelightPins,
    NVIDIA_FAMILY,
};
use fabstir_llm_node::ltx::template::{
    check_session_model_for, check_template_rules, expected_session_model, ltx_model_id,
    template_model_id, TemplateEntry, TemplateStore,
};

const KEY: &str = "0x54ab12370f7426db759f6b0df01cbffbeea55753d4aceef23657406d7494fae0";
const STD: &str = "0x01d3a8502c21c9e92deb1218c47a4f983f28a2308e32049602d5ba78a8311fab";
const FULL: &str = "0xc003ede527a0d0bbd4ef7759c509890fc5b9a6eb379e3d4762b2e92518605580";

fn hex32(s: &str) -> [u8; 32] {
    let v = hex::decode(s.trim_start_matches("0x")).unwrap();
    v.try_into().unwrap()
}

fn entry(id: &str, sidecar: Option<&str>) -> TemplateEntry {
    TemplateEntry {
        template_id: id.to_string(),
        template_hash: "0x00".to_string(),
        image_inputs: 0,
        image_semantics: vec![],
        video_inputs: 1,
        video_semantics: vec!["sourceVideo".to_string()],
        fps: Some(vec![24, 25]),
        max_frames: Some(145),
        resolution_rule: Some("relight-fhd".to_string()),
        exact_control: Some(true),
        frame_grid: Some(true),
        sidecar: sidecar.map(str::to_string),
    }
}

#[test]
fn relight_model_ids_are_the_pinned_literals() {
    // swap the family prefix -> red
    assert_eq!(NVIDIA_FAMILY, "NVIDIA/Cosmos-DiffusionRenderer");
    for (id, lit) in [
        ("cosmos-passes-key", KEY),
        ("cosmos-passes-std", STD),
        ("cosmos-passes-full", FULL),
    ] {
        assert_eq!(
            template_model_id(&entry(id, Some("relight"))),
            hex32(lit),
            "{id}"
        );
    }
}

#[test]
fn ltx_entries_keep_the_lightricks_id() {
    let e = entry("ltx-alpha-hdr", None);
    assert_eq!(template_model_id(&e), ltx_model_id("ltx-alpha-hdr"));
}

#[test]
fn expected_session_model_follows_the_family() {
    let relight = entry("cosmos-passes-std", Some("relight"));
    assert_eq!(
        expected_session_model(Some(&relight), "cosmos-passes-std"),
        hex32(STD)
    );
    // always the Lightricks prefix -> red
    assert_ne!(
        expected_session_model(Some(&relight), "cosmos-passes-std"),
        ltx_model_id("cosmos-passes-std")
    );
    assert_eq!(
        expected_session_model(None, "ltx-t2v-hdr"),
        ltx_model_id("ltx-t2v-hdr")
    );
}

#[test]
fn check_session_model_for_accepts_own_id_and_refuses_others() {
    let std_id = hex32(STD);
    assert!(check_session_model_for(Some(std_id), std_id, "cosmos-passes-std").is_ok());
    assert!(check_session_model_for(
        Some(ltx_model_id("cosmos-passes-std")),
        std_id,
        "cosmos-passes-std"
    )
    .is_err());
    // a session opened at the Key price must not buy a Full job
    assert!(check_session_model_for(Some(hex32(KEY)), hex32(FULL), "cosmos-passes-full").is_err());
    assert!(check_session_model_for(None, std_id, "cosmos-passes-std").is_err());
    assert!(check_session_model_for(Some([0u8; 32]), std_id, "cosmos-passes-std").is_err());
}

#[test]
fn relight_fhd_accepts_only_1920x1088() {
    let e = entry("cosmos-passes-std", Some("relight"));
    assert!(check_template_rules(&e, 1920, 1088, 25, 145).is_ok());
    // 1536x1024 is on the bundle ladder, so only the rule can refuse it
    assert!(check_template_rules(&e, 1536, 1024, 25, 121).is_err());
    assert!(check_template_rules(&e, 1088, 1920, 25, 121).is_err());
    assert!(check_template_rules(&e, 1920, 1088, 48, 121).is_err());
}

fn store_dir(sidecar_json: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("cosmos-passes-std")).unwrap();
    std::fs::copy(
        "templates/cosmos-passes-std/v1.json",
        d.path().join("cosmos-passes-std/v1.json"),
    )
    .unwrap();
    let allow = format!(
        r#"{{"allowListVersion":27,"templates":[{{"templateId":"cosmos-passes-std","version":"v1","videoInputs":1,
        "videoSemantics":["sourceVideo"],"fps":[24,25],"maxFrames":145,"resolutionRule":"relight-fhd","exactControl":true,
        "frameGrid":true{sidecar_json}}}],"loras":["cosmos-passes-std@v1"],"bounds":{{"frames":{{"min":121,"max":751}},
        "fps":[24,25],"resolutions":[{{"w":1920,"h":1088}}],"imageMaxBytes":1,"imageFormats":["png"],"videoMaxBytes":1,
        "videoFormats":["mp4"],"deepVideoMaxBytes":1}}}}"#
    );
    std::fs::write(d.path().join("allowlist.json"), allow).unwrap();
    d
}

#[test]
fn store_copies_the_sidecar_field() {
    let d = store_dir(r#","sidecar":"relight""#);
    let store = TemplateStore::new(d.path()).unwrap();
    // drop the copy at the store's entry literal -> None -> red
    assert_eq!(
        store.entry("cosmos-passes-std").unwrap().sidecar.as_deref(),
        Some("relight")
    );
}

#[test]
fn store_refuses_an_unknown_sidecar() {
    let d = store_dir(r#","sidecar":"foo""#);
    assert!(TemplateStore::new(d.path()).is_err());
}

#[test]
fn colour_encoding_for_relight_templates() {
    use fabstir_llm_node::ltx::exr::colour_encoding_for;
    use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
    for id in [
        "cosmos-passes-key",
        "cosmos-passes-std",
        "cosmos-passes-full",
    ] {
        let job = LtxJob {
            template_id: id.to_string(),
            template_hash: "0x00".into(),
            prompt: String::new(),
            seed: "1".into(),
            frames: 121,
            fps: 25,
            resolution: Resolution { w: 1920, h: 1088 },
            lora: format!("{id}@v1"),
            output: OutputKind::ExrFrames,
            images: None,
            videos: None,
            strength: None,
            azimuth: None,
            elevation: None,
            distance: None,
            input_wire: None,
        };
        assert_eq!(colour_encoding_for(&job), "vfx-passes-v1");
    }
}

#[test]
fn attestation_model_id_is_the_template_id_for_relight_only_until_oq_v5() {
    // relight: the session model; LTX: None = keep LTX_MODEL_ID from env (relight-only fallback, D7)
    assert_eq!(
        attestation_model_id(&entry("cosmos-passes-std", Some("relight"))),
        Some(format!("0x{}", &STD[2..]))
    );
    assert_eq!(attestation_model_id(&entry("ltx-alpha-hdr", None)), None);
}

#[test]
fn relight_env_meta_takes_each_field_from_its_source() {
    let pins = RelightPins {
        weights: [(
            "Diffusion_Renderer_Inverse_Cosmos_7B/model.pt".to_string(),
            "aa".repeat(32),
        )]
        .into_iter()
        .collect(),
        stack: "bb".repeat(32),
    };
    let echo = RelightEcho {
        gpu: "RTX PRO 6000".into(),
        cuda: "12.8".into(),
    };
    let m = relight_env_meta(&pins, &echo, "node-commit");
    // keccak of the canonical (key-sorted JSON) weights map — hashing `stack`, or another encoding -> red
    let canonical = serde_json::to_vec(&pins.weights).unwrap();
    assert_eq!(
        m.weights_hash,
        format!("0x{}", hex::encode(ethers::utils::keccak256(canonical)))
    );
    assert_eq!(
        String::from_utf8(serde_json::to_vec(&pins.weights).unwrap()).unwrap(),
        format!(
            r#"{{"Diffusion_Renderer_Inverse_Cosmos_7B/model.pt":"{}"}}"#,
            "aa".repeat(32)
        )
    );
    assert_eq!(m.lora_hash, "");
    assert_eq!(m.comfy_commit, "bb".repeat(32));
    assert_eq!(m.node_commit, "node-commit");
    assert_eq!(m.cuda_version, "12.8");
    assert_eq!(m.gpu_class, "RTX PRO 6000");
}

#[test]
fn relight_start_needs_one_generation_slot() {
    assert!(check_relight_start(Some("http://relight-sidecar:8190"), 1).is_ok());
    assert!(check_relight_start(Some("http://relight-sidecar:8190"), 2).is_err());
    assert!(check_relight_start(None, 2).is_ok());
}
