// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! D7 class table. Mutations: drop or misclass a row; give a Data row a Picture role.
mod common;
use fabstir_provenance::class::{lookup, Class, OutputRole, CLASS_TABLE};
use serde_json::json;

#[test]
fn every_v48_template_is_classed() {
    let ids = String::from_utf8(common::fixture("v48-template-ids.txt")).unwrap();
    let ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(ids.len(), 37);
    for id in &ids {
        assert!(lookup(id).is_some(), "{id} is not classed");
    }
    assert_eq!(
        CLASS_TABLE.len(),
        ids.len(),
        "table has rows for templates outside the allow-list"
    );
}

#[test]
fn class_table_matches_golden() {
    common::golden("class-table", &json!(CLASS_TABLE));
}

#[test]
fn unclassed_template_has_no_row() {
    assert!(lookup("ltx-t2v-hdr-x").is_none());
    assert!(lookup("").is_none());
}

#[test]
fn only_prompt_templates_lack_visual_input() {
    for r in CLASS_TABLE {
        let prompt_only = r.template_id == "ltx-t2v-hdr";
        match r.class {
            Class::Data => assert!(!r.visual_input, "{}", r.template_id),
            _ => assert_eq!(r.visual_input, !prompt_only, "{}", r.template_id),
        }
    }
}

#[test]
fn data_class_never_has_a_picture_role() {
    for r in CLASS_TABLE {
        if r.class == Class::Data {
            assert_eq!(r.mp4_role, OutputRole::Data, "{}", r.template_id);
            assert_ne!(r.exr_role, Some(OutputRole::Picture), "{}", r.template_id);
        }
        if r.class != Class::Data {
            assert_eq!(r.mp4_role, OutputRole::Picture, "{}", r.template_id);
        }
    }
}

#[test]
fn class_counts_match_d7() {
    let n = |c| CLASS_TABLE.iter().filter(|r| r.class == c).count();
    assert_eq!(
        (n(Class::Generated), n(Class::Modified), n(Class::Data)),
        (7, 27, 3)
    );
    // the alpha matte is the only Data EXR outside the cosmos passes
    let data_exr: Vec<_> = CLASS_TABLE
        .iter()
        .filter(|r| r.class != Class::Data && r.exr_role == Some(OutputRole::Data))
        .map(|r| r.template_id)
        .collect();
    assert_eq!(data_exr, vec!["ltx-alpha-hdr", "ltx-alpha-chain-hdr"]);
}
