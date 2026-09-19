//! A Policy v2 built from the vendored dstack recording (registers off its quote,
//! compose/os-image off its event log), the matching evidence pieces, and a
//! provider signer.

use super::fixtures::fixture;
use fabstir_llm_node::kbs::cpu::decode_unverified;
use fabstir_llm_node::kbs::eventlog::replay_and_extract;
use fabstir_llm_node::kbs::tools::sign_policy;
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::{sha256_32, CcMode, CvmPolicy, GpuPolicy, Policy};
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

/// The recording's registers, read through the broker's own decoder.
pub fn recording_policy(model_id: [u8; 32], version: u32) -> Policy {
    let td = decode_unverified(&quote()).unwrap();
    let r = replay_and_extract(&event_log()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    Policy {
        schema_version: 2,
        policy_version: version,
        model_id,
        not_before: now - 3600,
        expiry: now + 30 * 86_400,
        cvm: CvmPolicy {
            mrtd: hex::encode(td.mr_td),
            rtmr0: hex::encode(td.rt_mr0),
            rtmr1: hex::encode(td.rt_mr1),
            rtmr2: hex::encode(td.rt_mr2),
            os_image_hash: hex::encode(r.os_image_hash),
            compose_hash: hex::encode(r.compose_hash),
            app_id: Some(hex::encode(r.app_id.unwrap())),
            key_provider: Some(hex::encode(r.key_provider.unwrap())),
            require_td_debug_off: true,
            allowed_tcb_status: vec!["Simulator".into(), "UpToDate".into()],
            allowed_advisory_ids: vec![],
        },
        gpu: GpuPolicy {
            allowed_hwmodels: vec!["GH100 A01 GSP BROM".into()],
            require_cc_mode: Some(CcMode::On),
            require_secure_boot: true,
            require_debug_disabled: true,
            min_driver_version: Some("550.0.0".into()),
            min_vbios_version: None,
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
