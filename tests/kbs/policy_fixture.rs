//! A Policy v3 trusting a fixed throw-away authority root, the recording's host
//! profile (registers off its quote, compose/os-image/app id/key provider off
//! its event log) in a signed bundle, the matching evidence pieces, and a
//! provider signer.

use super::fixtures::fixture;
use fabstir_llm_node::kbs::authority::Authority;
use fabstir_llm_node::kbs::config::GpuEvidenceMode;
use fabstir_llm_node::kbs::cpu::decode_unverified;
use fabstir_llm_node::kbs::cpu::TdxEvidence;
use fabstir_llm_node::kbs::error::KbsError;
use fabstir_llm_node::kbs::eventlog::replay_and_extract;
use fabstir_llm_node::kbs::eventlog::Replayed;
use fabstir_llm_node::kbs::nras_claims::GpuOutcome;
use fabstir_llm_node::kbs::tools::sign_policy;
use fabstir_llm_node::kbs::verify::{self, Expected, Prefiltered, Verified};
/// Re-exported for the migrated tests' profile cases.
pub use fabstir_llm_node::tee::host_profile::HostProfile as Profile;
use fabstir_llm_node::tee::host_profile::{
    AuthorityBundle, AuthorityManifest, HostProfile, HostProfileSet, ProfileCvm, ProfileGpu,
    SignedAuthorityManifest, SignedHostProfileSet, MANIFEST_DOC, MAX_PROFILE_LIFETIME, SET_DOC,
};
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::{
    sha256_32, CcMode, CvmPolicy, GpuPolicy, HostAuthority, Policy, POLICY_DOC,
};
use k256::ecdsa::SigningKey;
use serde_json::json;

pub const REPORT_DATA_OFFSET: usize = 568;

pub fn test_model_id(tail: u8) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[..4].copy_from_slice(b"t5t:");
    id[31] = tail;
    id
}

#[allow(dead_code)]
pub fn real_model_id(tail: u8) -> [u8; 32] {
    let mut id = [0xab; 32];
    id[31] = tail;
    id
}

pub fn quote() -> Vec<u8> {
    fixture("dstack-0.5.9-simulator-quote.bin")
}

pub fn event_log() -> Vec<u8> {
    fixture("dstack-0.5.9-simulator-eventlog.json")
}

/// A schema-3 policy for `model_id` trusting [`root_key`]'s authority: the
/// recording GPU, [`CLASS`], no pins required, no policy floors (the old
/// 550.0.0 driver floor moved to the recording profile, D15). Its window is
/// around the REAL clock (the node's `fetch_validated_policy` reads it).
pub fn recording_policy(model_id: [u8; 32], version: u32) -> Policy {
    let real_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    Policy {
        doc: POLICY_DOC.into(),
        schema_version: 3,
        policy_version: version,
        model_id,
        not_before: real_now - 3600,
        expiry: real_now + 30 * 86_400,
        cvm: CvmPolicy {
            require_td_debug_off: true,
            allowed_tcb_status: vec!["Simulator".into(), "UpToDate".into()],
            allowed_advisory_ids: vec![],
            require_app_id_pinned: false,
            require_key_provider_pinned: false,
        },
        gpu: GpuPolicy {
            allowed_hwmodels: vec![HWMODEL.into()],
            require_cc_mode: Some(CcMode::On),
            require_secure_boot: true,
            require_debug_disabled: true,
            min_driver_version: None,
            min_vbios_version: None,
        },
        host_authority: HostAuthority {
            root: address_of(&root_key()),
            min_manifest_version: 1,
            min_threshold: 1,
            min_profile_epoch: 1,
            allowed_classes: vec![CLASS.into()],
        },
    }
}

/// The recording's quote with `report_data = sha256(pk_att) ‖ nonce`, as the
/// simulator patches it at serve time.
pub fn patched_quote(pk_att: &[u8; 33], nonce: &[u8; 32]) -> Vec<u8> {
    let mut q = quote();
    q[REPORT_DATA_OFFSET..REPORT_DATA_OFFSET + 32].copy_from_slice(&sha256_32(pk_att));
    q[REPORT_DATA_OFFSET + 32..REPORT_DATA_OFFSET + 64].copy_from_slice(nonce);
    q
}

pub fn gpu_payload(nonce: &[u8; 32], canned: Option<serde_json::Value>) -> Vec<u8> {
    let mut v = json!({
        "nonce": hex::encode(nonce),
        "evidence_list": [{"certificate": "c", "evidence": "e", "arch": "HOPPER"}],
        "arch": "HOPPER",
    });
    if let Some(c) = canned {
        v["canned"] = c;
    }
    serde_json::to_vec(&v).unwrap()
}

pub fn signer() -> SigningKey {
    SigningKey::random(&mut rand::rngs::OsRng)
}

pub fn sign(policy: &Policy, sk: &SigningKey) -> (SignedModelPolicy, String) {
    sign_policy(policy, "models/test.enc", sk).unwrap()
}

// ---- policy schema 3: the recording's host profile and a throw-away authority ----

/// The host class every recording profile carries and every recording policy allows.
pub const CLASS: &str = "test-class";
/// The recording profile's id.
pub const PROFILE_ID: &str = "recording/simulator";
/// The recording's GPU model string (the canned NRAS answer's `hwmodel`).
pub const HWMODEL: &str = "GH100 A01 GSP BROM";
/// The fixed `now` every authority decision in these tests is taken at (the
/// broker's injected clock): profile windows are around it, so the tests do
/// not age. Run 1 boot 2's capture time.
pub const NOW: u64 = 1_790_176_148;

/// Fixed throw-away authority keys, so a policy can name its root without
/// carrying keys around. Test-only; never used outside `tests/`.
pub fn root_key() -> SigningKey {
    SigningKey::from_bytes(&[0x11u8; 32].into()).unwrap()
}
pub fn signer_key(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[0x20u8 + n; 32].into()).unwrap()
}

/// The lowercase `0x` address of `sk`.
pub fn address_of(sk: &SigningKey) -> String {
    use tiny_keccak::{Hasher, Keccak};
    let point = sk.verifying_key().to_encoded_point(false);
    let mut k = Keccak::v256();
    k.update(&point.as_bytes()[1..]);
    let mut h = [0u8; 32];
    k.finalize(&mut h);
    format!("0x{}", hex::encode(&h[12..]))
}

/// `sk`'s 65-byte EIP-191 signature over `doc`'s canonical bytes.
pub fn sign_doc<T: serde::Serialize>(doc: &T, sk: &SigningKey) -> Vec<u8> {
    use fabstir_llm_node::tee::host_profile::{canonical_bytes, eip191_digest};
    let digest = eip191_digest(&canonical_bytes(doc).unwrap());
    let (sig, recid) = sk.sign_prehash_recoverable(&digest).unwrap();
    let mut out = sig.to_bytes().to_vec();
    out.push(recid.to_byte() + 27);
    out
}

/// The recording's identity as an approved host: registers off its quote,
/// compose/os image/app id/key provider off its event log, both pinned; the
/// recording GPU; a 550.0.0 driver floor (the old policy floor moved here,
/// D15); a window around now of exactly the 400-day maximum.
pub fn recording_profile() -> HostProfile {
    let td = decode_unverified(&quote()).unwrap();
    let r = replay_and_extract(&event_log()).unwrap();
    let nb = NOW - 100 * 86_400;
    HostProfile {
        id: PROFILE_ID.into(),
        class: CLASS.into(),
        cvm: ProfileCvm {
            mrtd: hex::encode(td.mr_td),
            rtmr0: hex::encode(td.rt_mr0),
            rtmr1: hex::encode(td.rt_mr1),
            rtmr2: hex::encode(td.rt_mr2),
            os_image_hash: hex::encode(r.os_image_hash),
            compose_hashes: vec![hex::encode(r.compose_hash)],
            app_id: Some(hex::encode(r.app_id.unwrap())),
            key_provider: Some(hex::encode(r.key_provider.unwrap())),
        },
        gpu: ProfileGpu {
            hwmodels: vec![HWMODEL.into()],
            min_driver_version: Some("550.0.0".into()),
            min_vbios_version: None,
        },
        not_before: nb,
        expiry: nb + MAX_PROFILE_LIFETIME,
        evidence: "dstack 0.5.9 simulator recording".into(),
    }
}

/// A signed bundle of `root_key()` at (`manifest_version`, `epoch`) holding
/// `profiles`, the manifest naming `signer_key(0)` (threshold 1).
pub fn bundle(manifest_version: u32, epoch: u64, profiles: Vec<HostProfile>) -> AuthorityBundle {
    let root = address_of(&root_key());
    let manifest = AuthorityManifest {
        doc: MANIFEST_DOC.into(),
        schema_version: 1,
        root: root.clone(),
        manifest_version,
        signers: vec![address_of(&signer_key(0))],
        threshold: 1,
        not_before: 0,
        expiry: u64::MAX - 1,
    };
    let set = HostProfileSet {
        doc: SET_DOC.into(),
        schema_version: 1,
        root,
        epoch,
        issued_at: 0,
        profiles,
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
}

/// The recording bundle: manifest v1, epoch 1, the recording profile.
pub fn recording_bundle() -> AuthorityBundle {
    bundle(1, 1, vec![recording_profile()])
}

/// Write `b` where the broker reads it: `{dir}/{root without 0x}.json`.
pub fn write_bundle(dir: &std::path::Path, b: &AuthorityBundle) {
    std::fs::create_dir_all(dir).unwrap();
    let key = b
        .manifest
        .manifest
        .root
        .trim_start_matches("0x")
        .to_string();
    std::fs::write(
        dir.join(format!("{key}.json")),
        serde_json::to_vec(b).unwrap(),
    )
    .unwrap();
}

/// An [`Authority`] for the PURE checks, built the way the loader builds it
/// (minus the HWM and the archive): validated against `policy` at `now`.
pub fn authority_for(policy: &Policy, b: &AuthorityBundle, now: u64) -> Authority {
    Authority::from_bundle(b.clone(), policy, now).unwrap()
}

/// `verify::prefilter` over the recording authority (one profile, `prof`) at [`NOW`].
pub fn prefilter_with(
    policy: &Policy,
    prof: &HostProfile,
    exp: &Expected,
    quote: &[u8],
    log: &[u8],
    gpu: &[u8],
    mode: GpuEvidenceMode,
) -> Result<Prefiltered, KbsError> {
    let a = authority_for(policy, &bundle(1, 1, vec![prof.clone()]), NOW);
    verify::prefilter(policy, &a, exp, quote, log, gpu, mode, NOW)
}

/// [`prefilter_with`] with the recording profile: `verify::prefilter`'s old call
/// shape, so the migrated tests keep their call sites.
pub fn prefilter(
    policy: &Policy,
    exp: &Expected,
    quote: &[u8],
    log: &[u8],
    gpu: &[u8],
    mode: GpuEvidenceMode,
) -> Result<Prefiltered, KbsError> {
    prefilter_with(policy, &recording_profile(), exp, quote, log, gpu, mode)
}

/// `verify::check_policy` over the recording authority (one profile, `prof`) at [`NOW`].
pub fn check_with(
    policy: &Policy,
    prof: &HostProfile,
    exp: &Expected,
    cpu: &TdxEvidence,
    r: &Replayed,
    gpu: &GpuOutcome,
    test: bool,
) -> Result<Verified, KbsError> {
    let a = authority_for(policy, &bundle(1, 1, vec![prof.clone()]), NOW);
    verify::check_policy(policy, &a, exp, cpu, r, gpu, test, NOW)
}

/// [`check_with`] with the recording profile: `verify::check_policy`'s old call
/// shape, so the migrated tests keep their call sites.
pub fn check_policy(
    policy: &Policy,
    exp: &Expected,
    cpu: &TdxEvidence,
    r: &Replayed,
    gpu: &GpuOutcome,
    test: bool,
) -> Result<Verified, KbsError> {
    check_with(policy, &recording_profile(), exp, cpu, r, gpu, test)
}

/// Run the real `fabstir-kbs` with `args` split on whitespace; an argument
/// `@name` is `dir/name` (the V3.3 tool tests).
pub fn kbs_in(dir: &std::path::Path, args: &str) -> std::process::Output {
    kbs_in_env(dir, args, &[])
}

/// [`kbs_in`] with extra environment (e.g. `HOME` for the authority ledger).
pub fn kbs_in_env(
    dir: &std::path::Path,
    args: &str,
    env: &[(&str, &std::path::Path)],
) -> std::process::Output {
    let args: Vec<std::ffi::OsString> = args
        .split_whitespace()
        .map(|a| match a.strip_prefix('@') {
            Some(name) => dir.join(name).into_os_string(),
            None => a.into(),
        })
        .collect();
    std::process::Command::new(env!("CARGO_BIN_EXE_fabstir-kbs"))
        .args(args)
        .envs(env.iter().copied())
        .output()
        .unwrap()
}
