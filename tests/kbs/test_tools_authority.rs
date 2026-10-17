//! Policy schema 3 tooling (D10, D14): `authority sign`, `authority
//! assemble` (step rule + highest-signed ledger) and `profile sign` (a
//! manifest signer's key, `--append`). `authority check` is in
//! `test_tools_check.rs`, the profile tools in `test_tools_profile.rs`, the
//! migration in `test_tools_migrate.rs`, the whole ceremony through the
//! binary in `test_tools_cli.rs`.

use super::fixtures::fixture;
use super::policy_fixture::{
    address_of, kbs_in, recording_profile, root_key, sign_doc, signer_key,
};
use super::tools_fixture::{bundle, check, check_prev, manifest, prev, set, signed_manifest, FAR};
use fabstir_llm_node::kbs::tools_authority::{
    assemble, assemble_set, assemble_to, bundle_hwm, read_ledger, read_previous,
    read_signature_file, sign_manifest, sign_set, step_rule, ManifestSigner, Prior,
};
use fabstir_llm_node::tee::host_profile::{
    parse_bundle, verify_set, AuthorityBundle, AuthorityManifest, HostProfileSet,
    SignedAuthorityManifest, SignedHostProfileSet, MANIFEST_DOC, MAX_SET_SIGNATURES,
};

#[test]
fn authority_sign_assemble_check_round_trip_step_rule_and_first_only_for_one() {
    let root = root_key();
    // authority sign: --first only for manifest_version 1; the key must be the root.
    let m1 = sign_manifest(manifest(1, FAR), ManifestSigner::Key(&root), &Prior::First).unwrap();
    assert_eq!(m1, signed_manifest(manifest(1, FAR)));
    let e = sign_manifest(manifest(2, FAR), ManifestSigner::Key(&root), &Prior::First);
    assert!(
        e.unwrap_err().0.contains("--first"),
        "mutation: --first with v2"
    );
    let e = sign_manifest(
        manifest(1, FAR),
        ManifestSigner::Key(&signer_key(0)),
        &Prior::First,
    );
    assert!(e.unwrap_err().0.contains("!= manifest root"));
    // profile assemble: --first only for epoch 1.
    let p = recording_profile();
    assert!(assemble_set(&m1.manifest.root, 1, 0, &Prior::First, vec![p.clone()]).is_ok());
    let e = assemble_set(&m1.manifest.root, 2, 0, &Prior::First, vec![p]).unwrap_err();
    assert!(
        e.0.contains("--first is only for epoch 1"),
        "mutation: --first with epoch 2"
    );
    // authority assemble --first: both counters 1.
    let b11 = bundle(manifest(1, FAR), set(1, "a"));
    let (got, _) = assemble(b11.manifest.clone(), b11.set.clone(), &Prior::First, None).unwrap();
    assert_eq!(got, b11);
    for (mv, ep) in [(1, 2), (2, 1)] {
        let b = bundle(manifest(mv, FAR), set(ep, "a"));
        let e = assemble(b.manifest, b.set, &Prior::First, None).unwrap_err();
        assert!(e.0.contains("--first"), "({mv},{ep})");
    }
    // The step rule, legal: +1/+1, +1/0 with an equal set, 0/+1 with an equal manifest.
    let legal = [
        ("+1/+1", bundle(manifest(2, FAR - 1), set(2, "b"))),
        ("+1/0", bundle(manifest(2, FAR - 1), set(1, "a"))),
        ("0/+1", bundle(manifest(1, FAR), set(2, "b"))),
    ];
    for (case, b) in &legal {
        step_rule(&b11, b).unwrap_or_else(|e| panic!("{case}: {e}"));
        assemble(b.manifest.clone(), b.set.clone(), &prev(&b11), None)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        let r = check_prev(b, 1, Some(&b11), |_| {});
        assert!(
            r.failures.is_empty() && r.lines.contains(&"step_rule=ok".to_string()),
            "{case}: {r:?}"
        );
    }
    // Illegal: a 0 step with a different body, 0/0, +2 on one counter.
    let b = |mv, expiry, ep, ev| bundle(manifest(mv, expiry), set(ep, ev));
    let illegal = [
        (
            "0 step, manifest differs",
            b(1, FAR - 1, 2, "b"),
            "body differs",
        ),
        ("0 step, set differs", b(2, FAR - 1, 1, "b"), "body differs"),
        ("0/0", b(1, FAR, 1, "a"), "neither"),
        ("+2 manifest", b(3, FAR - 1, 1, "a"), "0 or +1"),
        ("+2 epoch", b(1, FAR, 3, "b"), "0 or +1"),
    ];
    for (case, b, why) in &illegal {
        let e = step_rule(&b11, b).unwrap_err();
        assert!(e.0.contains(why), "{case}: {e}");
        assert!(
            assemble(b.manifest.clone(), b.set.clone(), &prev(&b11), None).is_err(),
            "{case}"
        );
        let r = check_prev(b, 1, Some(&b11), |_| {});
        assert!(
            r.failures
                .iter()
                .any(|f| f.contains("--previous") && f.contains(why)),
            "{case}: {r:?}"
        );
    }

    // The ledger (D10), through the real writer.
    let home = tempfile::tempdir().unwrap();
    let ledger = home.path().join(".fabstir-authority");
    let out = |n: &str| home.path().join(n);
    let asm = |n: &str, b: &AuthorityBundle, p: &Prior| {
        assemble_to(&out(n), b.manifest.clone(), b.set.clone(), p, &ledger)
    };
    let highest = || {
        read_ledger(&ledger, &b11.manifest.manifest.root)
            .unwrap()
            .unwrap()
    };
    asm("b11.json", &b11, &Prior::First).unwrap();
    let b12a = bundle(manifest(1, FAR), set(2, "staged a"));
    asm("b12a.json", &b12a, &prev(&b11)).unwrap();
    let recorded = highest();
    assert_eq!(
        (recorded.epoch, recorded.set_digest.clone()),
        (2, bundle_hwm(&b12a).unwrap().set_digest)
    );
    // A second body at epoch 2 (an abandoned staged epoch re-used): the step
    // rule alone admits it, the ledger refuses it and writes nothing.
    let b12b = bundle(manifest(1, FAR), set(2, "staged b"));
    assert!(assemble(b12b.manifest.clone(), b12b.set.clone(), &prev(&b11), None).is_ok());
    let e = asm("b12b.json", &b12b, &prev(&b11)).unwrap_err();
    assert!(
        e.0.contains("already assembled with another body"),
        "mutation: drop the ledger: {e}"
    );
    assert!(!out("b12b.json").exists());
    // Below the highest: refused even for the very bundle assembled first.
    let e = asm("again.json", &b11, &Prior::First).unwrap_err();
    assert!(e.0.contains("below 2"), "{e}");
    // At the highest, from that very bundle: a manifest-only bump passes.
    let b22 = bundle(manifest(2, FAR - 1), set(2, "staged a"));
    asm("b22.json", &b22, &prev(&b12a))
        .unwrap_or_else(|e| panic!("mutation: drop the --previous exemption: {e}"));
    assert_eq!((highest().manifest_version, highest().epoch), (2, 2));
    // Round trip: the assembled file is what `authority check` reads.
    let written: AuthorityBundle =
        serde_json::from_slice(&std::fs::read(out("b22.json")).unwrap()).unwrap();
    assert_eq!(written, b22);
    let r = check(&written, 1, |_| {});
    assert!(
        r.failures.is_empty() && r.lines[0].contains("manifest_version=2 epoch=2"),
        "{r:?}"
    );
}

#[test]
fn authority_sign_accepts_an_external_signature_file() {
    let m = manifest(1, FAR);
    let sig = sign_doc(&m, &root_key());
    for file in [
        format!("0x{}\n", hex::encode(&sig)).into_bytes(),
        sig.clone(),
    ] {
        let got = read_signature_file(&file).unwrap();
        let signed =
            sign_manifest(m.clone(), ManifestSigner::External(got), &Prior::First).unwrap();
        assert_eq!(
            signed.signature, sig,
            "the file's signature is the one attached"
        );
    }
    let other = sign_doc(&m, &signer_key(0));
    let e = sign_manifest(m.clone(), ManifestSigner::External(other), &Prior::First).unwrap_err();
    assert!(e.0.contains("!= manifest root"), "{e}");
    assert!(read_signature_file(b"0x1234").is_err());
    // Through the binary.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.json"), serde_json::to_vec(&m).unwrap()).unwrap();
    std::fs::write(dir.path().join("sig.hex"), hex::encode(&sig)).unwrap();
    let cmd = "authority sign --first --manifest @m.json --signature-file @sig.hex --out @s.json";
    let out = kbs_in(dir.path(), cmd);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let signed: SignedAuthorityManifest =
        serde_json::from_slice(&std::fs::read(dir.path().join("s.json")).unwrap()).unwrap();
    assert_eq!(signed.signature, sig);
}

#[test]
fn ledger_guards_the_manifest_side_and_the_previous_bundle_is_verified() {
    let home = tempfile::tempdir().unwrap();
    let ledger = home.path().join("ledger");
    let asm = |n: &str, b: &AuthorityBundle, p: &Prior| {
        assemble_to(
            &home.path().join(n),
            b.manifest.clone(),
            b.set.clone(),
            p,
            &ledger,
        )
    };
    let b11 = bundle(manifest(1, FAR), set(1, "a"));
    let root = b11.manifest.manifest.root.clone();
    asm("b11", &b11, &Prior::First).unwrap();
    // A manifest v2 (the set unchanged) is recorded with ITS digest.
    let b21a = bundle(manifest(2, FAR - 1), set(1, "a"));
    asm("b21a", &b21a, &prev(&b11)).unwrap();
    let rec = read_ledger(&ledger, &root).unwrap().unwrap();
    let want = bundle_hwm(&b21a).unwrap();
    assert_eq!(
        (rec.manifest_digest, rec.set_digest),
        (want.manifest_digest, want.set_digest),
        "mutation: record the wrong digest"
    );
    // Another manifest body at v2 is refused; v1 is now below the highest.
    let b21b = bundle(manifest(2, FAR - 2), set(1, "a"));
    let e = asm("b21b", &b21b, &prev(&b11)).unwrap_err();
    assert!(
        e.0.contains("manifest_version 2 was already assembled"),
        "{e}"
    );
    let e = asm("b12", &bundle(manifest(1, FAR), set(2, "b")), &prev(&b11)).unwrap_err();
    assert!(e.0.contains("manifest_version 1 is below 2"), "{e}");
    // The identical body re-assembled (a lost output file) is admitted.
    asm("b21a-again", &b21a, &prev(&b11)).unwrap();
    // A corrupt ledger fails closed.
    std::fs::write(ledger.join(format!("{}.json", &root[2..])), b"{").unwrap();
    let b31 = bundle(manifest(3, FAR - 3), set(1, "a"));
    assert!(asm("b31", &b31, &prev(&b21a))
        .unwrap_err()
        .0
        .contains("corrupt"));
    // --previous is verified (signatures, own root) and must share the root.
    let mut forged = b11.clone();
    forged.set.set.epoch = 7;
    let bytes = serde_json::to_vec(&forged).unwrap();
    assert!(
        read_previous(&bytes, "--previous").is_err(),
        "a re-numbered set"
    );
    let mut forged = b11.clone();
    forged.manifest.manifest.manifest_version = 7;
    let bytes = serde_json::to_vec(&forged).unwrap();
    assert!(
        read_previous(&bytes, "--previous").is_err(),
        "a re-numbered manifest"
    );
    let other = signer_key(5);
    let mut m = manifest(1, FAR);
    m.root = address_of(&other);
    let mut s = set(1, "a");
    s.root = m.root.clone();
    let foreign = AuthorityBundle {
        manifest: SignedAuthorityManifest {
            signature: sign_doc(&m, &other),
            manifest: m,
        },
        set: super::tools_fixture::signed_set(s),
    };
    read_previous(&serde_json::to_vec(&foreign).unwrap(), "--previous").unwrap();
    let e = step_rule(&foreign, &bundle(manifest(2, FAR), set(1, "a"))).unwrap_err();
    assert!(e.0.contains("differs from the previous bundle"), "{e}");
}

#[test]
fn profile_sign_refuses_a_key_outside_the_manifest_and_append_keeps_the_body() {
    let m = AuthorityManifest {
        doc: MANIFEST_DOC.into(),
        schema_version: 1,
        root: address_of(&root_key()),
        manifest_version: 1,
        signers: vec![address_of(&signer_key(0)), address_of(&signer_key(1))],
        threshold: 2,
        not_before: 0,
        expiry: u64::MAX - 1,
    };
    let manifest = SignedAuthorityManifest {
        signature: sign_doc(&m, &root_key()),
        manifest: m.clone(),
    };
    let set = HostProfileSet {
        root: m.root.clone(),
        ..parse_bundle(&fixture("authority-bundle.json"), "f")
            .unwrap()
            .set
            .set
    };
    let first = sign_set(set.clone(), &signer_key(0), &manifest, None).unwrap();
    assert_eq!(first.signatures, vec![sign_doc(&set, &signer_key(0))]);
    let e = sign_set(set.clone(), &signer_key(2), &manifest, None).unwrap_err();
    assert!(
        e.0.contains("is not a signer"),
        "mutation: skip the manifest check: {e}"
    );
    let forged = SignedAuthorityManifest {
        signature: sign_doc(&m, &signer_key(0)),
        ..manifest.clone()
    };
    assert!(sign_set(set.clone(), &signer_key(0), &forged, None).is_err());
    // --append: the body and the first signature are kept, the new one added.
    let both = sign_set(set.clone(), &signer_key(1), &manifest, Some(first.clone())).unwrap();
    assert_eq!(both.set, first.set, "mutation: rewrite the body");
    assert_eq!(
        both.signatures,
        vec![first.signatures[0].clone(), sign_doc(&set, &signer_key(1))]
    );
    assert_eq!(
        verify_set(&both, &m).unwrap().signers.len(),
        2,
        "2-of-2 reached"
    );
    let mut other = set.clone();
    other.issued_at += 1;
    assert!(
        sign_set(other, &signer_key(1), &manifest, Some(first.clone()))
            .unwrap_err()
            .0
            .contains("differs")
    );
    assert!(
        sign_set(set.clone(), &signer_key(0), &manifest, Some(first.clone()))
            .unwrap_err()
            .0
            .contains("already signed")
    );
    let mut foreign = set.clone();
    foreign.root = address_of(&signer_key(7));
    assert!(sign_set(foreign, &signer_key(0), &manifest, None)
        .unwrap_err()
        .0
        .contains("!= manifest root"));
    let almost = SignedHostProfileSet {
        signatures: vec![first.signatures[0].clone(); MAX_SET_SIGNATURES - 1],
        ..first.clone()
    };
    let at_max = sign_set(set.clone(), &signer_key(1), &manifest, Some(almost)).unwrap();
    assert_eq!(
        at_max.signatures.len(),
        MAX_SET_SIGNATURES,
        "the 16th is allowed"
    );
    let full = SignedHostProfileSet {
        signatures: vec![first.signatures[0].clone(); MAX_SET_SIGNATURES],
        ..first
    };
    assert!(sign_set(set, &signer_key(1), &manifest, Some(full))
        .unwrap_err()
        .0
        .contains("maximum"));
}
