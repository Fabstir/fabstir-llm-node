//! Policy schema 3 (V3.2), D23: the canonical-bytes golden for all three
//! documents, asserted under the `kbs` feature, where `serde_json/preserve_order`
//! (pulled in by `dcap-qvl`) makes struct declaration order visible to
//! `to_value`. Split from `test_authority.rs` (400-line cap).

use super::policy_fixture::{recording_policy, test_model_id};
use fabstir_llm_node::tee::host_profile::{
    canonical_bytes, AuthorityManifest, HostProfile, HostProfileSet, ProfileCvm, ProfileGpu,
};
use fabstir_llm_node::tee::types::Policy;

fn policy() -> Policy {
    recording_policy(test_model_id(1), 1)
}

#[test]
fn canonical_bytes_golden_for_all_three_documents() {
    // D23. Under the `kbs` feature `serde_json/preserve_order` is ON, so
    // `to_value` keeps DECLARATION order; the canonical form must still be
    // sorted. Mutation: identity instead of `sort_json_keys` → declaration
    // order → these goldens fail. (Each type's first fields are `doc`,
    // `schema_version`, ...: sorted order differs by construction.)
    let a40 = |c: char| format!("0x{}", c.to_string().repeat(40));
    let m = AuthorityManifest {
        doc: "fabstir-host-authority-manifest".into(),
        schema_version: 1,
        root: a40('1'),
        manifest_version: 2,
        signers: vec![a40('2')],
        threshold: 1,
        not_before: 3,
        expiry: 4,
    };
    assert_eq!(
        String::from_utf8(canonical_bytes(&m).unwrap()).unwrap(),
        format!(
            r#"{{"doc":"fabstir-host-authority-manifest","expiry":4,"manifest_version":2,"not_before":3,"root":"{}","schema_version":1,"signers":["{}"],"threshold":1}}"#,
            a40('1'),
            a40('2')
        )
    );
    let s = HostProfileSet {
        doc: "fabstir-host-profile-set".into(),
        schema_version: 1,
        root: a40('1'),
        epoch: 5,
        issued_at: 6,
        profiles: vec![HostProfile {
            id: "p".into(),
            class: "c".into(),
            cvm: ProfileCvm {
                mrtd: "a".into(),
                rtmr0: "b".into(),
                rtmr1: "c".into(),
                rtmr2: "d".into(),
                os_image_hash: "e".into(),
                compose_hashes: vec!["f".into()],
                app_id: None,
                key_provider: Some("ab".into()),
            },
            gpu: ProfileGpu {
                hwmodels: vec!["GH100".into()],
                min_driver_version: Some("1".into()),
                min_vbios_version: None,
            },
            not_before: 7,
            expiry: 8,
            evidence: "x".into(),
        }],
    };
    assert_eq!(
        String::from_utf8(canonical_bytes(&s).unwrap()).unwrap(),
        format!(
            r#"{{"doc":"fabstir-host-profile-set","epoch":5,"issued_at":6,"profiles":[{{"class":"c","cvm":{{"app_id":null,"compose_hashes":["f"],"key_provider":"ab","mrtd":"a","os_image_hash":"e","rtmr0":"b","rtmr1":"c","rtmr2":"d"}},"evidence":"x","expiry":8,"gpu":{{"hwmodels":["GH100"],"min_driver_version":"1","min_vbios_version":null}},"id":"p","not_before":7}}],"root":"{}","schema_version":1}}"#,
            a40('1')
        )
    );
    let mut p = policy();
    p.model_id = [1u8; 32];
    p.not_before = 9;
    p.expiry = 10;
    p.cvm.allowed_tcb_status = vec!["UpToDate".into()];
    p.cvm.allowed_advisory_ids = vec![];
    p.gpu.allowed_hwmodels = vec!["GH100".into()];
    p.gpu.min_driver_version = None;
    p.gpu.min_vbios_version = None;
    p.host_authority.root = a40('1');
    p.host_authority.allowed_classes = vec!["c".into()];
    let ones = vec!["1"; 32].join(",");
    assert_eq!(
        String::from_utf8(canonical_bytes(&p).unwrap()).unwrap(),
        format!(
            r#"{{"cvm":{{"allowed_advisory_ids":[],"allowed_tcb_status":["UpToDate"],"require_app_id_pinned":false,"require_key_provider_pinned":false,"require_td_debug_off":true}},"doc":"fabstir-model-release-policy","expiry":10,"gpu":{{"allowed_hwmodels":["GH100"],"min_driver_version":null,"min_vbios_version":null,"require_cc_mode":"on","require_debug_disabled":true,"require_secure_boot":true}},"host_authority":{{"allowed_classes":["c"],"min_manifest_version":1,"min_profile_epoch":1,"min_threshold":1,"root":"{}"}},"model_id":[{ones}],"not_before":9,"policy_version":1,"schema_version":3}}"#,
            a40('1')
        )
    );
}
