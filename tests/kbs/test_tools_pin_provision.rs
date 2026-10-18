//! `profile pin --provision` (OQ-V3-2): the one-boot variant of D18. Phala's
//! provision response, saved before the CVM starts, supplies `app_id` and
//! confirms the compose hash and the OS image; the key provider stays the
//! committed constant unless overridden (STOP), exactly as the capture path.

use super::fixtures::fixture;
use super::policy_fixture::kbs_in;
use fabstir_llm_node::kbs::tools_authority::{
    pin_profile_provision, provision_identity, ProvisionIdentity, PHALA_KMS_KEY_PROVIDER,
};
use fabstir_llm_node::tee::host_profile::{parse_bundle, HostProfile};

const APP_ID: &str = "81acd506565cc9c0da4ed5acbb47ce9e3a35654f";

/// The committed bundle's one profile (run 1 boot 2, both pins) and its
/// unpinned form, the one `profile new` writes.
fn profiles() -> (HostProfile, HostProfile) {
    let b = parse_bundle(&fixture("authority-bundle.json"), "fixture").unwrap();
    let pinned = b.set.set.profiles[0].clone();
    let mut unpinned = pinned.clone();
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    (pinned, unpinned)
}

/// A provision response as Phala could save it: run 1's values, `0x` and
/// upper case on the hex, extra fields, a null.
fn response(p: &HostProfile) -> String {
    serde_json::json!({
        "compose_hash": format!("0x{}", p.cvm.compose_hashes[0].to_uppercase()),
        "app_id": APP_ID.to_uppercase(),
        "os_image_hash": format!("0x{}", p.cvm.os_image_hash),
        "kms_id": "kms-base-prod5",
        "kms_info": {"chain": null},
        "app_env_encrypt_pubkey": "00",
        "node_id": 12,
        "fmspc": null
    })
    .to_string()
}

fn flip(s: &str) -> String {
    let mut b = hex::decode(s).unwrap();
    b[0] ^= 1;
    hex::encode(b)
}

#[test]
fn provision_identity_canonicalises_hex_and_refuses_what_is_not_hex() {
    let (_, u) = profiles();
    let r = provision_identity(response(&u).as_bytes()).unwrap();
    assert_eq!(
        r,
        ProvisionIdentity {
            app_id: Some(APP_ID.into()),
            compose_hash: u.cvm.compose_hashes[0].clone(),
            os_image_hash: Some(u.cvm.os_image_hash.clone()),
            kms_id: Some("kms-base-prod5".into()),
        },
        "mutation: keep 0x or upper case"
    );
    for (body, why) in [
        (r#"{"app_id":"81"}"#, "no compose_hash"),
        (r#"{"compose_hash":"0x"}"#, "compose_hash is not hex"),
        (r#"{"compose_hash":"zz"}"#, "compose_hash is not hex"),
        (
            r#"{"compose_hash":"ab","app_id":"0xg1"}"#,
            "app_id is not hex",
        ),
        (
            r#"{"compose_hash":"ab","os_image_hash":7}"#,
            "os_image_hash is not a string",
        ),
        ("[]", "no compose_hash"),
        ("not json", "provision response:"),
    ] {
        let e = provision_identity(body.as_bytes()).unwrap_err();
        assert!(e.0.contains(why), "{body}: {e}");
    }
    let r = provision_identity(br#"{"compose_hash":"AB","app_id":null}"#).unwrap();
    assert_eq!(
        (r.app_id, r.os_image_hash, r.compose_hash),
        (None, None, "ab".into())
    );
}

#[test]
fn provision_pin_needs_the_compose_and_image_to_match_and_pins_exactly_what_the_capture_path_does()
{
    let (pinned, u) = profiles();
    let r = provision_identity(response(&u).as_bytes()).unwrap();
    let ok = pin_profile_provision(u.clone(), &r, None).unwrap();
    assert_eq!(
        ok.profile, pinned,
        "run 1's pins, re-derived from the response"
    );
    assert!(ok.notice.is_none());
    assert_eq!(
        ok.profile.cvm.key_provider.as_deref(),
        Some(PHALA_KMS_KEY_PROVIDER),
        "mutation: take the key provider from anywhere but the constant"
    );
    // A profile accepting two composes: the response may match either one.
    let mut two = u.clone();
    two.cvm.compose_hashes = vec![flip(&r.compose_hash), r.compose_hash.clone()];
    let p = pin_profile_provision(two.clone(), &r, None).unwrap();
    assert_eq!(
        p.profile.cvm.compose_hashes, two.cvm.compose_hashes,
        "mutation: compare with the first compose hash only"
    );
    // Each confirmable row, flipped in the response, refuses by name.
    let mut bad = r.clone();
    bad.compose_hash = flip(&r.compose_hash);
    let e = pin_profile_provision(u.clone(), &bad, None).unwrap_err();
    assert!(
        e.0.contains("compose hash: provision"),
        "mutation: skip the compose row: {e}"
    );
    let mut bad = r.clone();
    bad.os_image_hash = Some(flip(&u.cvm.os_image_hash));
    let e = pin_profile_provision(u.clone(), &bad, None).unwrap_err();
    assert!(
        e.0.contains("os image: provision"),
        "mutation: skip the image row: {e}"
    );
    // No OS image in the response: still pinned (boot 1 confirms it).
    let mut no_os = r.clone();
    no_os.os_image_hash = None;
    assert_eq!(
        pin_profile_provision(u.clone(), &no_os, None)
            .unwrap()
            .profile,
        pinned
    );
    // No app id (not Phala's own KMS): refused, pointing at the two-boot path.
    let mut no_app = r.clone();
    no_app.app_id = None;
    let e = pin_profile_provision(u.clone(), &no_app, None).unwrap_err();
    assert!(
        e.0.contains("no app_id") && e.0.contains("--capture"),
        "{e}"
    );
    // An app id of the wrong length fails the profile's own validation.
    let mut short = r.clone();
    short.app_id = Some(APP_ID[..38].into());
    assert!(pin_profile_provision(u.clone(), &short, None).is_err());
    // Already pinned, wholly or by half: refused.
    assert!(pin_profile_provision(pinned, &r, None)
        .unwrap_err()
        .0
        .contains("already pinned"));
    let mut half = u.clone();
    half.cvm.app_id = Some(APP_ID.into());
    assert!(pin_profile_provision(half, &r, None)
        .unwrap_err()
        .0
        .contains("already pinned"));
    // The override is pinned and prints STOP.
    let over = hex::encode(b"{\"name\":\"other\"}");
    let p = pin_profile_provision(u, &r, Some(&over)).unwrap();
    assert_eq!(p.profile.cvm.key_provider.as_deref(), Some(over.as_str()));
    assert!(p.notice.unwrap().starts_with("STOP"));
}

#[test]
fn provision_pin_through_the_binary_and_the_flags_it_refuses() {
    let (pinned, u) = profiles();
    let d = tempfile::tempdir().unwrap();
    let put = |n: &str, b: &[u8]| std::fs::write(d.path().join(n), b).unwrap();
    put("u.json", &serde_json::to_vec(&u).unwrap());
    put("prov.json", response(&u).as_bytes());
    let out = kbs_in(
        d.path(),
        "profile pin --profile @u.json --provision @prov.json --out @p.json",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for line in [
        "pinned=true",
        "source=provision",
        &format!("app_id={APP_ID}"),
        "os_image=confirmed",
        "registers=unconfirmed",
        "kms_id=kms-base-prod5",
    ] {
        assert!(stdout.contains(line), "{line} missing: {stdout}");
    }
    let got: HostProfile =
        serde_json::from_slice(&std::fs::read(d.path().join("p.json")).unwrap()).unwrap();
    assert_eq!(got, pinned);
    // --provision takes the app id from the response, never from a flag; the
    // capture path still needs --app-id; one of the two sources is required.
    for cmd in [
        format!(
            "profile pin --profile @u.json --provision @prov.json --app-id {APP_ID} --out @x.json"
        ),
        "profile pin --profile @u.json --provision @prov.json --capture @. --out @x.json".into(),
        "profile pin --profile @u.json --capture @. --out @x.json".into(),
        "profile pin --profile @u.json --out @x.json".into(),
    ] {
        let out = kbs_in(d.path(), &cmd);
        assert!(!out.status.success(), "accepted: {cmd}");
        assert!(!d.path().join("x.json").exists(), "wrote output: {cmd}");
    }
}
