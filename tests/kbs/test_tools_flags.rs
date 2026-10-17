//! The provider tooling's CLI spellings (design §13.1, policy schema 3 D14):
//! the runbook types these flags, so a spelling drift is a test failure. Moved
//! here from `test_tools.rs` (V3.3) and extended to the schema-3 commands.

use super::policy_fixture::test_model_id;
use fabstir_llm_node::kbs::tools::keyring_add;

const DEK: [u8; 32] = [0x42; 32];

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
    // Policy schema 3 (D14): every flag the runbook types, by its design
    // spelling; `profile new` takes NO capture (a capture never sources a
    // profile, H2).
    let design: [(&[&str], &[&str], &[&str]); 11] = [
        (
            &["authority", "sign"],
            &[
                "--manifest <MANIFEST>",
                "--key-file",
                "--signature-file",
                "--previous",
                "--first",
                "--out",
            ],
            &[],
        ),
        (
            &["authority", "assemble"],
            &[
                "--manifest-signed",
                "--set-signed",
                "--previous",
                "--first",
                "--out",
            ],
            &[],
        ),
        (
            &["authority", "check"],
            &[
                "--bundle",
                "--policy",
                "--previous",
                "--warn-days",
                "--replay",
                "--hwm-dir",
            ],
            &[],
        ),
        (
            &["profile", "new"],
            &[
                "--id",
                "--class",
                "--not-before",
                "--expiry",
                "--evidence",
                "--hwmodel",
                "--min-driver-version",
                "--min-vbios-version",
                "--measurements",
                "--os-image-digest",
                "--compose-file",
                "--compose-reference",
                "--env-names",
                "--out",
            ],
            &["--capture", "--app-id", "--key-provider"],
        ),
        (
            &["profile", "pin"],
            &[
                "--profile",
                "--capture",
                "--app-id",
                "--key-provider-override",
                "--out",
            ],
            &[],
        ),
        (
            &["profile", "check"],
            &[
                "--profile",
                "--measurements",
                "--os-image-digest",
                "--compose-file",
                "--compose-reference",
                "--env-names",
            ],
            &[],
        ),
        (
            &["profile", "assemble"],
            &[
                "--root",
                "--epoch",
                "--issued-at",
                "--previous",
                "--first",
                "--profile",
                "--out",
            ],
            &[],
        ),
        (
            &["profile", "sign"],
            &["--set", "--key-file", "--manifest", "--out", "--append"],
            &[],
        ),
        (
            &["policy", "migrate-v2"],
            &[
                "--in <IN>",
                "--expected-provider",
                "--root",
                "--class",
                "--id",
                "--policy-not-before",
                "--policy-expiry",
                "--profile-not-before",
                "--profile-expiry",
                "--policy-version",
                "--min-manifest-version",
                "--min-threshold",
                "--min-profile-epoch",
                "--require-pins <REQUIRE_PINS>",
                "--evidence",
                "--policy-out",
                "--profile-out",
            ],
            &["--input"],
        ),
        (&["policy", "sign"], &["--authority"], &[]),
        (&["seal"], &["--authority"], &[]),
    ];
    for (cmd, want, never) in design {
        let mut args = cmd.to_vec();
        args.push("--help");
        let h = help(&args);
        for flag in want {
            assert!(has_flag(&h, flag), "{cmd:?} lacks {flag}: {h}");
        }
        for flag in never {
            assert!(!h.contains(flag), "{cmd:?} must not take {flag}: {h}");
        }
    }
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
fn authority_check_exit_codes_are_1_refused_3_warn_0_ok() {
    // 0 = clean, 3 = warnings only, 1 = a refusal (2 stays clap's usage error);
    // the daily timer alerts on any non-zero exit (D17).
    use super::policy_fixture::{address_of, root_key, sign_doc, signer_key};
    use super::policy_fixture::{kbs_in, recording_policy, recording_profile, sign, signer};
    use fabstir_llm_node::tee::host_profile::{
        AuthorityBundle, AuthorityManifest, HostProfileSet, SignedAuthorityManifest,
        SignedHostProfileSet, MANIFEST_DOC, SET_DOC,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let bundle = |expiry: u64| {
        let manifest = AuthorityManifest {
            doc: MANIFEST_DOC.into(),
            schema_version: 1,
            root: address_of(&root_key()),
            manifest_version: 1,
            signers: vec![address_of(&signer_key(0))],
            threshold: 1,
            not_before: 0,
            expiry,
        };
        let set = HostProfileSet {
            doc: SET_DOC.into(),
            schema_version: 1,
            root: manifest.root.clone(),
            epoch: 1,
            issued_at: 0,
            profiles: vec![recording_profile()],
        };
        AuthorityBundle {
            manifest: SignedAuthorityManifest {
                signature: sign_doc(&manifest, &root_key()),
                manifest,
            },
            set: SignedHostProfileSet {
                signatures: vec![sign_doc(&set, &signer_key(0))],
                set,
            },
        }
    };
    let d = tempfile::tempdir().unwrap();
    let put = |n: &str, json: Vec<u8>| std::fs::write(d.path().join(n), json).unwrap();
    put(
        "far.json",
        serde_json::to_vec(&bundle(now + 400 * 86_400)).unwrap(),
    );
    put(
        "soon.json",
        serde_json::to_vec(&bundle(now + 10 * 86_400)).unwrap(),
    );
    let mut other = recording_policy(test_model_id(3), 1);
    other.host_authority.root = "0x00000000000000000000000000000000000000aa".into();
    put(
        "other-root.json",
        serde_json::to_vec(&sign(&other, &signer()).0).unwrap(),
    );
    for (args, code) in [
        ("authority check --bundle @far.json", 0),
        ("authority check --bundle @soon.json --warn-days 30", 3),
        (
            "authority check --bundle @far.json --policy @other-root.json",
            1,
        ),
        // A refusal with warnings is still a refusal.
        (
            "authority check --bundle @soon.json --warn-days 30 --policy @other-root.json",
            1,
        ),
    ] {
        let out = kbs_in(d.path(), args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.status.code(), Some(code), "{args}: {stdout}");
        let result = ["result=ok", "result=refused", "", "result=warn"][code as usize];
        assert!(stdout.contains(result), "{args}: {stdout}");
    }
}

/// `flag` appears in `help` as a whole flag: followed by a space, a newline or
/// a comma, so `--policy` is not satisfied by `--policy-out`.
fn has_flag(help: &str, flag: &str) -> bool {
    help.match_indices(flag).any(|(i, _)| {
        matches!(
            help[i + flag.len()..].chars().next(),
            None | Some(' ' | '\n' | ',' | ']')
        )
    })
}
