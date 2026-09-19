//! Design §3: every keyring rule; the mode × class matrix (four refusals, two accepts).

use fabstir_llm_node::kbs::keyring::{Keyring, KeyringClass, TEST_ID_PREFIX};
use std::os::unix::fs::PermissionsExt;

fn test_id(tail: u8) -> String {
    let mut id = [0u8; 32];
    id[..4].copy_from_slice(TEST_ID_PREFIX);
    id[31] = tail;
    hex::encode(id)
}

fn real_id(tail: u8) -> String {
    let mut id = [0u8; 32];
    id[0] = 0xab;
    id[31] = tail;
    hex::encode(id)
}

const PROVIDER: &str = "0x00112233445566778899aabbccddeeff00112233";
const DEK: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";

fn entry(model_id: &str, test: bool, extra: &str) -> String {
    format!(
        r#"{{"model_id":"{model_id}","provider":"{PROVIDER}","dek":"{DEK}","test":{test}{extra}}}"#
    )
}

fn file(entries: &[String]) -> Vec<u8> {
    format!(r#"{{"schema":1,"keys":[{}]}}"#, entries.join(",")).into_bytes()
}

#[test]
fn parses_a_valid_test_and_real_keyring() {
    let t = Keyring::parse(&file(&[entry(
        &test_id(1),
        true,
        r#","min_policy_version":3,"note":"x""#,
    )]))
    .unwrap();
    assert_eq!(t.class(), Some(KeyringClass::Test));
    let e = t
        .get(&hex::decode(test_id(1)).unwrap().try_into().unwrap())
        .unwrap();
    assert_eq!(e.min_policy_version, 3);
    assert_eq!(e.note, "x");
    assert_eq!(e.provider, PROVIDER);
    assert_eq!(*e.dek, hex::decode(DEK).unwrap().as_slice());
    let r = Keyring::parse(&file(&[entry(&real_id(1), false, "")])).unwrap();
    assert_eq!(r.class(), Some(KeyringClass::Real));
    assert_eq!(
        r.get(&hex::decode(real_id(1)).unwrap().try_into().unwrap())
            .unwrap()
            .min_policy_version,
        1
    );
    assert!(
        format!("{:?}", r).contains("<redacted>"),
        "the DEK never prints"
    );
    let file: fabstir_llm_node::kbs::keyring::KeyringFile =
        serde_json::from_slice(&file(&[entry(&real_id(1), false, "")])).unwrap();
    let dbg = format!("{file:?}");
    assert!(
        dbg.contains("<redacted>") && !dbg.contains(DEK),
        "the on-disk struct's Debug redacts too"
    );
}

#[test]
fn prefix_and_flag_must_agree_both_ways() {
    let e = Keyring::parse(&file(&[entry(&test_id(1), false, "")])).unwrap_err();
    assert!(e.0.contains("t5t: prefix but test is false"), "{e}");
    let e = Keyring::parse(&file(&[entry(&real_id(1), true, "")])).unwrap_err();
    assert!(e.0.contains("does not carry the t5t: prefix"), "{e}");
}

#[test]
fn hex_forms_are_strict() {
    let upper = test_id(1).to_uppercase();
    assert!(Keyring::parse(&file(&[entry(&upper, true, "")]))
        .unwrap_err()
        .0
        .contains("model_id"));
    let short = &test_id(1)[..62];
    assert!(Keyring::parse(&file(&[entry(short, true, "")]))
        .unwrap_err()
        .0
        .contains("model_id"));
    let bad_prov =
        entry(&test_id(1), true, "").replace(PROVIDER, "00112233445566778899aabbccddeeff00112233");
    assert!(Keyring::parse(&file(&[bad_prov]))
        .unwrap_err()
        .0
        .contains("provider"));
    let bad_prov = entry(&test_id(1), true, "")
        .replace(PROVIDER, "0x00112233445566778899AABBCCDDEEFF00112233");
    assert!(Keyring::parse(&file(&[bad_prov]))
        .unwrap_err()
        .0
        .contains("provider"));
    let bad_dek = entry(&test_id(1), true, "").replace(DEK, "0x0f");
    assert!(Keyring::parse(&file(&[bad_dek]))
        .unwrap_err()
        .0
        .contains("dek"));
}

#[test]
fn duplicates_empty_schema_and_unknown_fields_refuse() {
    let e = Keyring::parse(&file(&[
        entry(&test_id(1), true, ""),
        entry(&test_id(1), true, ""),
    ]))
    .unwrap_err();
    assert!(e.0.contains("duplicate"), "{e}");
    assert!(Keyring::parse(&file(&[]))
        .unwrap_err()
        .0
        .contains("no entries"));
    assert!(Keyring::parse(br#"{"schema":2,"keys":[]}"#)
        .unwrap_err()
        .0
        .contains("schema"));
    let e = Keyring::parse(&file(&[entry(&test_id(1), true, r#","extra":1"#)])).unwrap_err();
    assert!(e.0.contains("parse"), "{e}");
    let e = Keyring::parse(&file(&[entry(
        &test_id(1),
        true,
        r#","min_policy_version":0"#,
    )]))
    .unwrap_err();
    assert!(e.0.contains("min_policy_version"), "{e}");
}

#[test]
fn mode_class_matrix() {
    let test = Keyring::parse(&file(&[entry(&test_id(1), true, "")])).unwrap();
    let real = Keyring::parse(&file(&[entry(&real_id(1), false, "")])).unwrap();
    let mixed = Keyring::parse(&file(&[
        entry(&test_id(1), true, ""),
        entry(&real_id(2), false, ""),
    ]))
    .unwrap();
    assert_eq!(mixed.class(), None);
    // two accepts
    assert!(test.check_class(true).is_ok());
    assert!(real.check_class(false).is_ok());
    // four refusals
    assert!(
        test.check_class(false).is_err(),
        "test keyring under real modes"
    );
    assert!(
        real.check_class(true).is_err(),
        "real keyring under a test mode"
    );
    assert!(mixed.check_class(true).is_err(), "mixed under a test mode");
    assert!(mixed.check_class(false).is_err(), "mixed under real modes");
}

#[test]
fn load_enforces_mode_0600_and_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keyring.json");
    std::fs::write(&path, file(&[entry(&test_id(1), true, "")])).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let e = Keyring::load(&path, true).unwrap_err();
    assert!(e.0.contains("0600"), "{e}");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let k = Keyring::load(&path, true).unwrap();
    assert_eq!(k.len(), 1);
    // a read-only 0400 keyring is accepted too
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(Keyring::load(&path, true).unwrap().len(), 1);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    // the mode coupling runs at load too
    let e = Keyring::load(&path, false).unwrap_err();
    assert!(e.0.contains("test: false"), "{e}");
    let e = Keyring::load(&dir.path().join("absent.json"), true).unwrap_err();
    assert!(e.0.contains("absent.json"), "{e}");
}

#[test]
fn provider_registry_binds_every_entry() {
    let k = Keyring::parse(&file(&[
        entry(&test_id(1), true, ""),
        entry(&test_id(2), true, ""),
    ]))
    .unwrap();
    let reg = k.provider_registry();
    let id: [u8; 32] = hex::decode(test_id(2)).unwrap().try_into().unwrap();
    assert_eq!(reg.expected_provider(&id).unwrap(), PROVIDER);
    let other: [u8; 32] = hex::decode(real_id(9)).unwrap().try_into().unwrap();
    assert!(reg.expected_provider(&other).is_err());
}
