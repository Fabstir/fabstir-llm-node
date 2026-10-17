//! Shared builders for the policy schema 3 tool tests (V3.3): a throw-away
//! authority (the policy_fixture root and signer keys) over the recording
//! profile, and `authority check` runners.

use super::policy_fixture::{address_of, recording_profile, root_key, sign_doc, signer_key};
use fabstir_llm_node::kbs::tools_authority::{authority_check, CheckInputs, CheckReport, Prior};
use fabstir_llm_node::tee::host_profile::{
    AuthorityBundle, AuthorityManifest, HostProfileSet, SignedAuthorityManifest,
    SignedHostProfileSet, MANIFEST_DOC, SET_DOC,
};

pub fn manifest(version: u32, expiry: u64) -> AuthorityManifest {
    AuthorityManifest {
        doc: MANIFEST_DOC.into(),
        schema_version: 1,
        root: address_of(&root_key()),
        manifest_version: version,
        signers: vec![address_of(&signer_key(0))],
        threshold: 1,
        not_before: 0,
        expiry,
    }
}

pub fn signed_manifest(m: AuthorityManifest) -> SignedAuthorityManifest {
    SignedAuthorityManifest {
        signature: sign_doc(&m, &root_key()),
        manifest: m,
    }
}

/// Set `epoch` holding the recording profile with `evidence` (the knob that
/// changes the body without changing the identity).
pub fn set(epoch: u64, evidence: &str) -> HostProfileSet {
    let mut p = recording_profile();
    p.evidence = evidence.into();
    HostProfileSet {
        doc: SET_DOC.into(),
        schema_version: 1,
        root: address_of(&root_key()),
        epoch,
        issued_at: 0,
        profiles: vec![p],
    }
}

pub fn signed_set(s: HostProfileSet) -> SignedHostProfileSet {
    SignedHostProfileSet {
        signatures: vec![sign_doc(&s, &signer_key(0))],
        set: s,
    }
}

pub fn bundle(m: AuthorityManifest, s: HostProfileSet) -> AuthorityBundle {
    AuthorityBundle {
        manifest: signed_manifest(m),
        set: signed_set(s),
    }
}

pub fn prev(b: &AuthorityBundle) -> Prior {
    Prior::Previous(Box::new(b.clone()))
}

pub const FAR: u64 = u64::MAX - 1;

/// `authority check` of `b` at `now`, with `--previous` and the rest set by `f`.
pub fn check_prev(
    b: &AuthorityBundle,
    now: u64,
    previous: Option<&AuthorityBundle>,
    f: impl FnOnce(&mut CheckInputs<'_>),
) -> CheckReport {
    let bytes = serde_json::to_vec(b).unwrap();
    let prev = previous.map(|p| serde_json::to_vec(p).unwrap());
    let mut i = CheckInputs {
        bundle: &bytes,
        bundle_name: "bundle.json",
        policies: vec![],
        previous: prev.as_deref(),
        warn_days: None,
        replays: vec![],
        hwm_dir: None,
        now,
    };
    f(&mut i);
    authority_check(&i)
}

pub fn check(b: &AuthorityBundle, now: u64, f: impl FnOnce(&mut CheckInputs<'_>)) -> CheckReport {
    check_prev(b, now, None, f)
}

/// Every path under `dir` with its bytes (directories empty), for read-only checks.
pub fn walkdir(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.push((p.display().to_string(), vec![]));
            out.extend(walkdir(&p));
        } else {
            out.push((p.display().to_string(), std::fs::read(&p).unwrap()));
        }
    }
    out
}
