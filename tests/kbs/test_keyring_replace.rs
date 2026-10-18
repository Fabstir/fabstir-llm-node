//! `keyring add --replace`: a fresh seal of a model already in the keyring
//! swaps that model's entry for a new DEK and keeps every other entry. The
//! tool works on any valid file; a running broker additionally refuses a
//! keyring that mixes test and real entries (`check_class`), so on the box the
//! other entries are other real models.

use super::policy_fixture::test_model_id;
use fabstir_llm_node::kbs::tools::{keyring_add, keyring_check};

const PROVIDER: &str = "0x00112233445566778899aabbccddeeff00112233";
const OLD: [u8; 32] = [0x42; 32];
const NEW: [u8; 32] = [0x43; 32];

fn entries(path: &std::path::Path) -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    v["keys"].as_array().unwrap().clone()
}

fn entry(path: &std::path::Path, id: &str) -> serde_json::Value {
    entries(path)
        .into_iter()
        .find(|e| e["model_id"] == id)
        .unwrap()
}

#[test]
fn replace_swaps_only_the_named_entry_and_refuses_when_there_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keyring.json");
    let (t1, real) = (hex::encode(test_model_id(1)), hex::encode([0xab; 32]));
    let add = |id: &str, dek: &[u8; 32], test, mpv, note: &str, replace| {
        keyring_add(&path, id, PROVIDER, dek, test, mpv, note, replace)
    };
    add(&t1, &OLD, true, 1, "cpu round", false).unwrap();
    add(&real, &OLD, false, 1, "run 2", false).unwrap();
    let t1_before = entry(&path, &t1);
    // Replace the real entry: two entries still, the new DEK and fields, t1 untouched.
    assert_eq!(add(&real, &NEW, false, 2, "mainnet cut", true).unwrap(), 2);
    assert_eq!(keyring_check(&path).unwrap(), 2);
    let r = entry(&path, &real);
    assert_eq!(r["dek"], hex::encode(NEW), "mutation: keep the old entry");
    assert_eq!(
        (r["min_policy_version"].as_u64(), r["note"].as_str()),
        (Some(2), Some("mainnet cut"))
    );
    assert_eq!(
        entry(&path, &t1),
        t1_before,
        "mutation: remove the wrong entry"
    );
    // Nothing to replace: refused, the file untouched (a mistyped id never adds).
    let before = std::fs::read(&path).unwrap();
    let other = hex::encode([0xcd; 32]);
    let e = add(&other, &NEW, false, 1, "", true).unwrap_err();
    assert!(
        e.0.contains("no entry for"),
        "mutation: add when absent: {e}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Without --replace a duplicate is still refused, the file untouched.
    let e = add(&real, &OLD, false, 1, "", false).unwrap_err();
    assert!(e.0.contains("duplicate"), "{e}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // A replacement keeps the provider: a different one is refused, untouched.
    let e = keyring_add(
        &path,
        &real,
        "0x99112233445566778899aabbccddeeff00112233",
        &NEW,
        false,
        1,
        "",
        true,
    )
    .unwrap_err();
    assert!(
        e.0.contains("keeps the provider"),
        "mutation: let --replace change it: {e}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // The test-prefix rule still applies to a replacement.
    let e = add(&t1, &NEW, false, 1, "", true).unwrap_err();
    assert!(e.0.contains("carries the t5t: prefix"), "{e}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn replace_through_the_binary_writes_a_fresh_dek_and_leaves_no_orphan_on_refusal() {
    let bin = env!("CARGO_BIN_EXE_fabstir-kbs");
    let dir = tempfile::tempdir().unwrap();
    let keyring = dir.path().join("keyring.json");
    let real = hex::encode([0xab; 32]);
    keyring_add(&keyring, &real, PROVIDER, &OLD, false, 1, "run 2", false).unwrap();
    let run = |id: &str, dek_out: &std::path::Path| {
        std::process::Command::new(bin)
            .args([
                "keyring",
                "add",
                "--model-id",
                id,
                "--provider",
                PROVIDER,
                "--generate",
            ])
            .arg("--dek-out")
            .arg(dek_out)
            .args([
                "--min-policy-version",
                "1",
                "--note",
                "fresh",
                "--replace",
                "--file",
            ])
            .arg(&keyring)
            .output()
            .unwrap()
    };
    let dek = dir.path().join("dek.hex");
    let out = run(&real, &dek);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("replaced the entry"), "{stderr}");
    let fresh = std::fs::read_to_string(&dek).unwrap();
    assert_eq!(
        entry(&keyring, &real)["dek"],
        fresh.trim(),
        "the keyring holds the DEK written out"
    );
    assert_ne!(fresh.trim(), hex::encode(OLD));
    // Refused (no entry to replace): the generated DEK file is removed.
    let orphan = dir.path().join("orphan.hex");
    assert!(!run(&hex::encode([0xcd; 32]), &orphan).status.success());
    assert!(!orphan.exists(), "a refused add leaves no DEK on disk");
    assert_eq!(entries(&keyring).len(), 1);
}
