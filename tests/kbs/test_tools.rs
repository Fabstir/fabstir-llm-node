//! Design §13.1: the container trio (byte-equality against `encrypt_model`, round
//! trips through the node's `decrypt_model`), `reseal` under a new policy hash,
//! `policy sign`, and the keyring tooling.

use super::policy_fixture::{recording_policy, sign, signer, test_model_id};
use fabstir_llm_node::kbs::tools::{
    keyring_add, keyring_check, reseal, seal, sign_policy, DEFAULT_CHUNK_SIZE,
};
use fabstir_llm_node::tee::container::{
    decrypt_model, decrypt_model_to_writer, encrypt_model, encrypt_model_to_writer,
    encrypt_model_with_nonce_base, ContainerHeader,
};
use fabstir_llm_node::tee::types::TeeError;

const DEK: [u8; 32] = [0x42; 32];
const MODEL: [u8; 32] = [0x11; 32];
const HASH: [u8; 32] = [0x22; 32];

fn plaintext(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 251) as u8).collect()
}

#[test]
fn streaming_sealer_is_byte_equal_to_encrypt_model_and_round_trips() {
    for len in [0usize, 1, 1000, 4096, 4097, 3 * 4096 + 5] {
        let pt = plaintext(len);
        let in_mem = encrypt_model(&pt, &DEK, MODEL, HASH, 4096).unwrap();
        let nonce_base = ContainerHeader::decode(&in_mem).unwrap().nonce_base;
        let via_base =
            encrypt_model_with_nonce_base(&pt, &DEK, MODEL, HASH, 4096, nonce_base).unwrap();
        assert_eq!(in_mem, via_base, "len {len}");
        let mut streamed = Vec::new();
        encrypt_model_to_writer(
            std::io::Cursor::new(&pt),
            pt.len() as u64,
            &mut streamed,
            &DEK,
            MODEL,
            HASH,
            4096,
            nonce_base,
        )
        .unwrap();
        assert_eq!(in_mem, streamed, "len {len}");
        // both decrypt paths agree with the node's
        let mut a = Vec::new();
        decrypt_model(&streamed, &DEK, &MODEL, &HASH, &mut a).unwrap();
        let mut b = Vec::new();
        decrypt_model_to_writer(std::io::Cursor::new(&streamed), &mut b, &DEK, &MODEL, &HASH)
            .unwrap();
        assert_eq!(a, pt);
        assert_eq!(b, pt);
    }
}

#[test]
fn streaming_decrypt_refuses_the_same_things_as_the_node() {
    let pt = plaintext(10_000);
    let c = encrypt_model(&pt, &DEK, MODEL, HASH, 4096).unwrap();
    let mut out = Vec::new();
    let e = decrypt_model_to_writer(std::io::Cursor::new(&c), &mut out, &DEK, &MODEL, &[0; 32])
        .unwrap_err();
    assert!(matches!(e, TeeError::VerificationFailed(m) if m.contains("policy_hash")));
    let e = decrypt_model_to_writer(std::io::Cursor::new(&c), &mut out, &DEK, &[0; 32], &HASH)
        .unwrap_err();
    assert!(matches!(e, TeeError::VerificationFailed(m) if m.contains("model_id")));
    let e = decrypt_model_to_writer(std::io::Cursor::new(&c), &mut out, &[0; 32], &MODEL, &HASH)
        .unwrap_err();
    assert!(matches!(e, TeeError::Crypto(_)));
    // a trailing byte lands in the last chunk and fails its tag (as the node's path does)
    let mut longer = c.clone();
    longer.push(0);
    let e = decrypt_model_to_writer(std::io::Cursor::new(&longer), &mut out, &DEK, &MODEL, &HASH)
        .unwrap_err();
    assert!(matches!(e, TeeError::Crypto(_)));
    let e = decrypt_model(&longer, &DEK, &MODEL, &HASH, &mut Vec::new()).unwrap_err();
    assert!(matches!(e, TeeError::Crypto(_)));
    // more than a full chunk beyond the header's count is refused as "longer"
    let mut much_longer = c.clone();
    much_longer.extend_from_slice(&[0u8; 4096 + 16]);
    let e = decrypt_model_to_writer(
        std::io::Cursor::new(&much_longer),
        &mut out,
        &DEK,
        &MODEL,
        &HASH,
    )
    .unwrap_err();
    assert!(matches!(e, TeeError::Crypto(m) if m.contains("longer")));
    // a container cut inside a non-last chunk is the in-memory path's refusal, not an
    // I/O error of the stream (mutation: plain `?` → Io("failed to fill whole buffer"))
    let two = encrypt_model(&vec![7u8; 4096 + 100], &DEK, MODEL, HASH, 4096).unwrap();
    let cut = &two[..two.len() - 100 - 16 - 1000];
    let e = decrypt_model_to_writer(std::io::Cursor::new(cut), &mut out, &DEK, &MODEL, &HASH)
        .unwrap_err();
    assert!(
        matches!(e, TeeError::Crypto(ref m) if m.contains("truncated at chunk 0")),
        "{e:?}"
    );
    let e = decrypt_model(cut, &DEK, &MODEL, &HASH, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(e, TeeError::Crypto(ref m) if m.contains("truncated at chunk 0")),
        "{e:?}"
    );
    // a stream shorter than the header is the same "header truncated" refusal as the in-memory path
    let e = decrypt_model_to_writer(
        std::io::Cursor::new(&c[..50]),
        &mut out,
        &DEK,
        &MODEL,
        &HASH,
    )
    .unwrap_err();
    assert!(matches!(e, TeeError::Crypto(m) if m.contains("header truncated")));
    // a zero-chunk container is exactly its header: trailing bytes are refused by both paths
    let empty = encrypt_model(&[], &DEK, MODEL, HASH, 4096).unwrap();
    assert_eq!(empty.len(), 98);
    let mut o = Vec::new();
    decrypt_model_to_writer(std::io::Cursor::new(&empty), &mut o, &DEK, &MODEL, &HASH).unwrap();
    assert!(o.is_empty());
    // an empty container with a huge header chunk_size is accepted by both paths (no buffer needed)
    let mut big_empty = empty.clone();
    big_empty[42..46].copy_from_slice(&u32::MAX.to_be_bytes());
    // (the header is unauthenticated; decode still succeeds and there is nothing to verify)
    let mut o2 = Vec::new();
    decrypt_model_to_writer(
        std::io::Cursor::new(&big_empty),
        &mut o2,
        &DEK,
        &MODEL,
        &HASH,
    )
    .unwrap();
    assert!(decrypt_model(&big_empty, &DEK, &MODEL, &HASH, &mut Vec::new()).is_ok());
    let mut trailing = empty.clone();
    trailing.extend_from_slice(b"garbage");
    assert!(
        matches!(decrypt_model_to_writer(std::io::Cursor::new(&trailing), &mut o, &DEK, &MODEL, &HASH).unwrap_err(), TeeError::Crypto(m) if m.contains("longer"))
    );
    assert!(
        matches!(decrypt_model(&trailing, &DEK, &MODEL, &HASH, &mut Vec::new()).unwrap_err(), TeeError::Crypto(m) if m.contains("longer"))
    );
    // a hostile header chunk_size must not cost the allocation before any tag verifies
    let mut hostile = c.clone();
    hostile[42..46].copy_from_slice(&u32::MAX.to_be_bytes());
    let e = decrypt_model_to_writer(
        std::io::Cursor::new(&hostile),
        &mut out,
        &DEK,
        &MODEL,
        &HASH,
    )
    .unwrap_err();
    assert!(matches!(e, TeeError::Crypto(m) if m.contains("streaming cap")));
    // a source LONGER than the declared length is refused too (a model still being copied)
    let mut sink = Vec::new();
    let e = encrypt_model_to_writer(
        std::io::Cursor::new(&pt),
        100,
        &mut sink,
        &DEK,
        MODEL,
        HASH,
        4096,
        [0; 16],
    )
    .unwrap_err();
    assert!(matches!(e, TeeError::Crypto(m) if m.contains("longer than the declared length")));
    // a short read on the encrypt side is an error, never a short container
    let mut sink = Vec::new();
    let e = encrypt_model_to_writer(
        std::io::Cursor::new(&pt[..100]),
        200,
        &mut sink,
        &DEK,
        MODEL,
        HASH,
        4096,
        [0; 16],
    )
    .unwrap_err();
    assert!(matches!(e, TeeError::Io(_)));
}

#[test]
fn seal_then_reseal_moves_the_container_to_the_new_policy_hash() {
    let dir = tempfile::tempdir().unwrap();
    let id = test_model_id(7);
    let sk = signer();
    let (p1, _) = sign(&recording_policy(id, 1), &sk);
    let (p2, _) = sign(&recording_policy(id, 2), &sk);
    let h1 = p1.policy_hash().unwrap();
    let h2 = p2.policy_hash().unwrap();
    assert_ne!(h1, h2);
    let model = dir.path().join("model.bin");
    let pt = plaintext(DEFAULT_CHUNK_SIZE as usize * 2 + 12345);
    std::fs::write(&model, &pt).unwrap();
    let c1 = dir.path().join("c1.enc");
    let c2 = dir.path().join("c2.enc");
    assert_eq!(
        seal(&model, &c1, &DEK, id, &p1, None).unwrap(),
        pt.len() as u64
    );
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&c1).unwrap().permissions().mode() & 0o777,
            0o644,
            "explicit 0644, not umask-derived"
        );
    }
    assert_eq!(reseal(&c1, &c2, &DEK, &p2, None).unwrap(), pt.len() as u64);
    let bytes2 = std::fs::read(&c2).unwrap();
    let mut out = Vec::new();
    decrypt_model(&bytes2, &DEK, &id, &h2, &mut out).unwrap();
    assert_eq!(out, pt);
    let e = decrypt_model(&bytes2, &DEK, &id, &h1, &mut Vec::new()).unwrap_err();
    assert!(
        matches!(e, TeeError::VerificationFailed(_)),
        "refused under the old policy hash"
    );
    // a wrong DEK fails the reseal (the decrypt thread's error wins)
    let e = reseal(&c1, &dir.path().join("c3.enc"), &[0; 32], &p2, None).unwrap_err();
    assert!(e.0.contains("decryption failed"), "{e}");
    // a signed policy edited after signing (expiry bumped) never gets sealed under
    let mut tampered = p1.clone();
    tampered.policy.expiry += 1;
    let e = seal(
        &model,
        &dir.path().join("c5.enc"),
        &DEK,
        id,
        &tampered,
        None,
    )
    .unwrap_err();
    assert!(e.0.contains("signer") || e.0.contains("signature"), "{e}");
    assert!(!dir.path().join("c5.enc").exists());
    assert!(reseal(&c1, &dir.path().join("c6.enc"), &DEK, &tampered, None).is_err());
    // an expired policy, or one signed by another key than the named provider, never gets sealed under
    let mut expired = recording_policy(id, 3);
    expired.expiry = expired.not_before + 1;
    let (expired_signed, addr) = sign(&expired, &sk);
    let e = seal(
        &model,
        &dir.path().join("c7.enc"),
        &DEK,
        id,
        &expired_signed,
        None,
    )
    .unwrap_err();
    assert!(
        e.0.contains("expir") || e.0.contains("window") || e.0.contains("valid"),
        "{e}"
    );
    // a policy whose not_before is still ahead (the T−1 target seal) IS accepted
    let mut future = recording_policy(id, 4);
    future.not_before += 2 * 86_400;
    future.expiry = future.not_before + 30 * 86_400;
    let (future_signed, _) = sign(&future, &sk);
    assert!(
        seal(
            &model,
            &dir.path().join("c9.enc"),
            &DEK,
            id,
            &future_signed,
            None
        )
        .is_ok(),
        "not-yet-valid seals fine"
    );
    let e = reseal(
        &c1,
        &dir.path().join("c8.enc"),
        &DEK,
        &p2,
        Some("0x0000000000000000000000000000000000000001"),
    )
    .unwrap_err();
    assert!(e.0.contains("not the expected provider"), "{e}");
    assert!(
        reseal(
            &c1,
            &dir.path().join("c8.enc"),
            &DEK,
            &p2,
            Some(&addr.to_uppercase())
        )
        .is_ok(),
        "the named provider, case-insensitive"
    );
    // the keyring is replaced atomically: no temp file is left behind
    let kdir = tempfile::tempdir().unwrap();
    let kpath = kdir.path().join("keyring.json");
    keyring_add(
        &kpath,
        &hex::encode(test_model_id(9)),
        "0x00112233445566778899aabbccddeeff00112233",
        &DEK,
        true,
        1,
        "",
    )
    .unwrap();
    keyring_add(
        &kpath,
        &hex::encode(test_model_id(10)),
        "0x00112233445566778899aabbccddeeff00112233",
        &DEK,
        true,
        1,
        "",
    )
    .unwrap();
    let names: Vec<String> = std::fs::read_dir(kdir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["keyring.json"], "{names:?}");
    // seal refuses a policy for another model
    let (other, _) = sign(&recording_policy(test_model_id(8), 1), &sk);
    assert!(seal(&model, &dir.path().join("c4.enc"), &DEK, id, &other, None).is_err());
    // out == input is refused BEFORE the source is touched (File::create would truncate it)
    let before = std::fs::read(&model).unwrap();
    let e = seal(&model, &model, &DEK, id, &p1, None).unwrap_err();
    assert!(e.0.contains("exists"), "{e}");
    assert_eq!(
        std::fs::read(&model).unwrap(),
        before,
        "the plaintext model is intact"
    );
    let c1_before = std::fs::read(&c1).unwrap();
    let e = reseal(&c1, &c1, &DEK, &p2, None).unwrap_err();
    assert!(e.0.contains("exists"), "{e}");
    assert_eq!(
        std::fs::read(&c1).unwrap(),
        c1_before,
        "the container is intact"
    );
    // a symlink to the input as --out: refused, the input untouched (File::create would follow it)
    let link = dir.path().join("current.enc");
    std::os::unix::fs::symlink(&c1, &link).unwrap();
    let e = reseal(&c1, &link, &DEK, &p2, None).unwrap_err();
    assert!(e.0.contains("exists"), "{e}");
    assert_eq!(
        std::fs::read(&c1).unwrap(),
        c1_before,
        "the container behind the symlink is intact"
    );
    assert!(
        link.symlink_metadata().is_ok(),
        "the link itself was not removed"
    );
    // a dangling spelling of the input's own path (absent out that resolves to the input) is refused
    let spelled = dir.path().join(".").join("model.bin");
    let e = seal(
        &spelled,
        &dir.path().join("./model.bin"),
        &DEK,
        id,
        &p1,
        None,
    )
    .unwrap_err();
    assert!(e.0.contains("exists") || e.0.contains("must differ"), "{e}");
    // a pre-existing, unrelated --out is never overwritten or deleted, even when the
    // failure happens before any output is written (a typo'd --model)
    let live = dir.path().join("live.enc");
    std::fs::write(&live, b"precious").unwrap();
    let e = seal(&dir.path().join("typo.bin"), &live, &DEK, id, &p1, None).unwrap_err();
    assert!(e.0.contains("exists"), "{e}");
    assert_eq!(std::fs::read(&live).unwrap(), b"precious");
    // a failed reseal leaves no partial output behind
    assert!(
        !dir.path().join("c3.enc").exists(),
        "partial output removed on error"
    );
    // the output is created exclusively: a file planted between the check and the
    // create (here: simply already there) is never truncated, and the loser does not
    // delete it
    let planted = dir.path().join("planted.enc");
    std::fs::write(&planted, b"winner").unwrap();
    assert!(seal(&model, &planted, &DEK, id, &p1, None).is_err());
    assert_eq!(std::fs::read(&planted).unwrap(), b"winner");
}

#[test]
fn a_small_plaintext_with_a_huge_chunk_size_seals_without_allocating_the_chunk() {
    // 1 GiB chunk_size, 100-byte plaintext: sized by the plaintext (a chunk-sized
    // buffer would be a 1 GiB allocation here).
    let pt = plaintext(100);
    let c = encrypt_model(&pt, &DEK, MODEL, HASH, 1 << 30).unwrap();
    let mut out = Vec::new();
    decrypt_model(&c, &DEK, &MODEL, &HASH, &mut out).unwrap();
    assert_eq!(out, pt);
}

#[test]
fn policy_sign_round_trips_through_the_nodes_verifier_and_validates_first() {
    let id = test_model_id(1);
    let sk = signer();
    let (signed, addr) = sign_policy(&recording_policy(id, 1), "models/x.enc", &sk).unwrap();
    assert!(signed.verify_signer(&addr).is_ok());
    assert!(signed
        .verify_signer("0x0000000000000000000000000000000000000001")
        .is_err());
    let mut bad = recording_policy(id, 1);
    bad.cvm.mrtd = "0x00".into();
    assert!(
        sign_policy(&bad, "models/x.enc", &sk).is_err(),
        "a malformed policy is never signed"
    );
}

#[test]
fn cli_flags_match_the_design() {
    // The runbook types these: `reseal --in`, `policy sign --encrypted-ref`, `keyring add --generate --dek-out`.
    let bin = env!("CARGO_BIN_EXE_fabstir-kbs");
    let help = |args: &[&str]| {
        String::from_utf8(
            std::process::Command::new(bin)
                .args(args)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
    };
    let reseal = help(&["reseal", "--help"]);
    assert!(reseal.contains("--in <IN>"), "{reseal}");
    assert!(!reseal.contains("--input"), "{reseal}");
    let sign = help(&["policy", "sign", "--help"]);
    assert!(
        sign.contains("--encrypted-ref") && sign.contains("--key-file"),
        "{sign}"
    );
    let seal_help = help(&["seal", "--help"]);
    assert!(seal_help.contains("--provider"), "{seal_help}");
    let add = help(&["keyring", "add", "--help"]);
    assert!(
        add.contains("--generate")
            && add.contains("--dek-out")
            && add.contains("--min-policy-version"),
        "{add}"
    );
    let ver = help(&["--version"]);
    assert!(ver.contains("fabstir-kbs v"), "{ver}");
    // `keyring check` without --file resolves $KBS_DATA_DIR/keyring.json like the broker
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keyring.json");
    keyring_add(
        &path,
        &hex::encode(test_model_id(5)),
        "0x00112233445566778899aabbccddeeff00112233",
        &DEK,
        true,
        1,
        "",
    )
    .unwrap();
    let out = std::process::Command::new(bin)
        .args(["keyring", "check"])
        .env("KBS_DATA_DIR", dir.path())
        .env_remove("KBS_KEYRING_FILE")
        .env_remove("KBS_ENV_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains(&path.to_string_lossy().to_string()));
    // and, with the process env stripped (sudo -u), from the unit's env file
    let env_file = dir.path().join("env");
    std::fs::write(
        &env_file,
        format!(
            "# unit env\nKBS_LISTEN=127.0.0.1:3030\nKBS_DATA_DIR=\"{}\"\n",
            dir.path().display()
        ),
    )
    .unwrap();
    let out = std::process::Command::new(bin)
        .args(["keyring", "check"])
        .env_remove("KBS_DATA_DIR")
        .env_remove("KBS_KEYRING_FILE")
        .env("KBS_ENV_FILE", &env_file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(&path.to_string_lossy().to_string()),
        "resolved from the env file (mutation: process env only → /var/lib default)"
    );
}

#[test]
fn keyring_add_generate_never_truncates_a_dek_file_and_leaves_no_orphan() {
    let bin = env!("CARGO_BIN_EXE_fabstir-kbs");
    let dir = tempfile::tempdir().unwrap();
    let keyring = dir.path().join("keyring.json");
    let dek_out = dir.path().join("dek.hex");
    let provider = "0x00112233445566778899aabbccddeeff00112233";
    let run = |args: &[&str]| std::process::Command::new(bin).args(args).output().unwrap();
    // a refused add (prefix/test mismatch) leaves no DEK file behind
    let out = run(&[
        "keyring",
        "add",
        "--model-id",
        &hex::encode([0xab; 32]),
        "--provider",
        provider,
        "--generate",
        "--dek-out",
        dek_out.to_str().unwrap(),
        "--test",
        "--file",
        keyring.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(!dek_out.exists(), "orphan DEK removed after a refused add");
    assert!(!keyring.exists());
    // a good add writes the DEK file (0600) and the keyring
    let out = run(&[
        "keyring",
        "add",
        "--model-id",
        &hex::encode(test_model_id(3)),
        "--provider",
        provider,
        "--generate",
        "--dek-out",
        dek_out.to_str().unwrap(),
        "--test",
        "--file",
        keyring.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dek1 = std::fs::read(&dek_out).unwrap();
    assert_eq!(dek1.len(), 64);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&dek_out).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains(std::str::from_utf8(&dek1).unwrap()),
        "the DEK is never printed"
    );
    // a second --generate to the same --dek-out is refused and the file is untouched
    let out = run(&[
        "keyring",
        "add",
        "--model-id",
        &hex::encode(test_model_id(4)),
        "--provider",
        provider,
        "--generate",
        "--dek-out",
        dek_out.to_str().unwrap(),
        "--test",
        "--file",
        keyring.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert_eq!(
        std::fs::read(&dek_out).unwrap(),
        dek1,
        "an existing DEK file is never truncated"
    );
    assert_eq!(
        keyring_check(&keyring).unwrap(),
        1,
        "and the refused add did not touch the keyring"
    );
}

#[test]
fn keyring_add_and_check_apply_the_broker_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keyring.json");
    let t1 = hex::encode(test_model_id(1));
    let provider = "0x00112233445566778899aabbccddeeff00112233";
    assert_eq!(
        keyring_add(&path, &t1, provider, &DEK, true, 1, "one").unwrap(),
        1
    );
    assert_eq!(keyring_check(&path).unwrap(), 1);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // the prefix rule
    let e = keyring_add(&path, &t1, provider, &DEK, false, 1, "").unwrap_err();
    assert!(e.0.contains("carries the t5t: prefix"), "{e}");
    let real = hex::encode([0xab; 32]);
    let e = keyring_add(&path, &real, provider, &DEK, true, 1, "").unwrap_err();
    assert!(e.0.contains("does not carry"), "{e}");
    // a duplicate is refused and the file is left as it was
    let before = std::fs::read(&path).unwrap();
    let e = keyring_add(&path, &t1, provider, &DEK, true, 1, "dup").unwrap_err();
    assert!(e.0.contains("duplicate"), "{e}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // a second, distinct entry
    assert_eq!(
        keyring_add(
            &path,
            &hex::encode(test_model_id(2)),
            provider,
            &DEK,
            true,
            2,
            ""
        )
        .unwrap(),
        2
    );
    assert_eq!(keyring_check(&path).unwrap(), 2);
    // a symlinked keyring path is refused (the rename would replace the link, not the file)
    let link = dir.path().join("keyring-link.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let e = keyring_add(
        &link,
        &hex::encode(test_model_id(3)),
        provider,
        &DEK,
        true,
        1,
        "",
    )
    .unwrap_err();
    assert!(e.0.contains("symlink"), "{e}");
    assert_eq!(keyring_check(&path).unwrap(), 2, "untouched");
}
