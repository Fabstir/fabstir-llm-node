// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared policy-schema-3 builders for the node test suites
//! (`docs/development/IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md`, V3.1).
//!
//! Every test that needs a schema-3 [`Policy`], a [`HostProfile`] or a signed
//! authority bundle builds it here, so the shape lives in one place. The
//! separate crates `tee_e2e.rs` and `phase5_peak_heap.rs` include this file by
//! `#[path]`, hence `allow(dead_code)`: each crate uses a subset.
#![allow(dead_code)]

use fabstir_llm_node::tee::host_profile::{
    canonical_bytes, eip191_digest, AuthorityBundle, AuthorityManifest, HostProfile,
    HostProfileSet, ProfileCvm, ProfileGpu, SignedAuthorityManifest, SignedHostProfileSet,
    MANIFEST_DOC, MANIFEST_SCHEMA_VERSION, MAX_PROFILE_LIFETIME, SET_DOC, SET_SCHEMA_VERSION,
};
use fabstir_llm_node::tee::types::{
    CcMode, CvmPolicy, GpuPolicy, HostAuthority, Policy, POLICY_DOC, POLICY_SCHEMA_VERSION,
};
use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_keccak::{Hasher, Keccak};

/// The host class every test profile carries and every test policy allows.
pub const CLASS: &str = "test-class";
/// An authority root for policies whose bundle is never signed in the test.
pub const ROOT: &str = "0x00000000000000000000000000000000000000a1";
/// The id of [`a_profile`].
pub const PROFILE_ID: &str = "test/profile-1";

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The provider's authority section: `root`, minimums 1, [`CLASS`] allowed.
pub fn host_authority(root: &str) -> HostAuthority {
    HostAuthority {
        root: root.to_string(),
        min_manifest_version: 1,
        min_threshold: 1,
        min_profile_epoch: 1,
        allowed_classes: vec![CLASS.to_string()],
    }
}

/// The provider's CVM requirements: TD debug off, `UpToDate`, no advisories,
/// no pins required.
pub fn cvm_policy() -> CvmPolicy {
    CvmPolicy {
        require_td_debug_off: true,
        allowed_tcb_status: vec!["UpToDate".to_string()],
        allowed_advisory_ids: vec![],
        require_app_id_pinned: false,
        require_key_provider_pinned: false,
    }
}

/// The provider's GPU requirements: CC On, secure boot, debug disabled, no
/// floors, `hwmodels` allowed.
pub fn gpu_policy(hwmodels: &[&str]) -> GpuPolicy {
    GpuPolicy {
        allowed_hwmodels: hwmodels.iter().map(|s| s.to_string()).collect(),
        require_cc_mode: Some(CcMode::On),
        require_secure_boot: true,
        require_debug_disabled: true,
        min_driver_version: None,
        min_vbios_version: None,
    }
}

/// A valid schema-3 policy trusting [`ROOT`].
pub fn a_policy(model_id: [u8; 32], not_before: u64, expiry: u64, hwmodels: &[&str]) -> Policy {
    Policy {
        doc: POLICY_DOC.to_string(),
        schema_version: POLICY_SCHEMA_VERSION,
        policy_version: 1,
        model_id,
        not_before,
        expiry,
        cvm: cvm_policy(),
        gpu: gpu_policy(hwmodels),
        host_authority: host_authority(ROOT),
    }
}

/// A valid, unpinned profile of [`CLASS`] whose `mrtd` is `mrtd` (the mock's
/// `image_measurement`), accepting `hwmodels`, inside its window NOW (100 days
/// back, 300 ahead: exactly the 400-day maximum).
pub fn a_profile(mrtd: [u8; 48], hwmodels: &[&str]) -> HostProfile {
    let now = now_unix();
    HostProfile {
        id: PROFILE_ID.to_string(),
        class: CLASS.to_string(),
        cvm: ProfileCvm {
            mrtd: hex::encode(mrtd),
            rtmr0: "00".repeat(48),
            rtmr1: "00".repeat(48),
            rtmr2: "00".repeat(48),
            os_image_hash: "00".repeat(32),
            compose_hashes: vec!["00".repeat(32)],
            app_id: None,
            key_provider: None,
        },
        gpu: ProfileGpu {
            hwmodels: hwmodels.iter().map(|s| s.to_string()).collect(),
            min_driver_version: None,
            min_vbios_version: None,
        },
        not_before: now - 100 * 86_400,
        expiry: now - 100 * 86_400 + MAX_PROFILE_LIFETIME,
        evidence: "test fixture".to_string(),
    }
}

/// A fresh random key.
pub fn key() -> SigningKey {
    SigningKey::random(&mut rand::rngs::OsRng)
}

/// The lowercase `0x` Ethereum address of `sk`.
pub fn address_of(sk: &SigningKey) -> String {
    let point = sk.verifying_key().to_encoded_point(false);
    let mut k = Keccak::v256();
    k.update(&point.as_bytes()[1..]);
    let mut h = [0u8; 32];
    k.finalize(&mut h);
    format!("0x{}", hex::encode(&h[12..]))
}

/// `sk`'s 65-byte recoverable signature (Ethereum `v` = 27/28) over the
/// EIP-191 digest of `doc`'s canonical bytes: what `fabstir-kbs … sign` emits.
pub fn sign_doc<T: Serialize>(doc: &T, sk: &SigningKey) -> Vec<u8> {
    let digest = eip191_digest(&canonical_bytes(doc).expect("canonical"));
    let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash_recoverable(&digest).expect("sign");
    let mut out = sig.to_bytes().to_vec();
    out.push(recid.to_byte() + 27);
    out
}

/// A valid manifest v1 of `root` naming `signers`, `threshold`, window 0..MAX-1.
pub fn a_manifest(root: &str, signers: &[String], threshold: u32) -> AuthorityManifest {
    AuthorityManifest {
        doc: MANIFEST_DOC.to_string(),
        schema_version: MANIFEST_SCHEMA_VERSION,
        root: root.to_string(),
        manifest_version: 1,
        signers: signers.to_vec(),
        threshold,
        not_before: 0,
        expiry: u64::MAX - 1,
    }
}

/// A valid set of `root` at `epoch` holding `profiles`, issued at 0.
pub fn a_set(root: &str, epoch: u64, profiles: Vec<HostProfile>) -> HostProfileSet {
    HostProfileSet {
        doc: SET_DOC.to_string(),
        schema_version: SET_SCHEMA_VERSION,
        root: root.to_string(),
        epoch,
        issued_at: 0,
        profiles,
    }
}

/// Throw-away keys behind a bundle.
pub struct Keys {
    pub root: SigningKey,
    pub signers: Vec<SigningKey>,
}

impl Keys {
    pub fn root_address(&self) -> String {
        address_of(&self.root)
    }
    pub fn signer_addresses(&self) -> Vec<String> {
        self.signers.iter().map(address_of).collect()
    }
}

/// A fully signed bundle: manifest v1 (one fresh signer, threshold 1) signed by
/// a fresh root, and a set at epoch 1 holding `profiles` signed by that signer.
pub fn a_bundle(profiles: Vec<HostProfile>) -> (AuthorityBundle, Keys) {
    let keys = Keys {
        root: key(),
        signers: vec![key()],
    };
    let root = keys.root_address();
    let manifest = a_manifest(&root, &keys.signer_addresses(), 1);
    let set = a_set(&root, 1, profiles);
    let bundle = AuthorityBundle {
        manifest: SignedAuthorityManifest {
            signature: sign_doc(&manifest, &keys.root),
            manifest,
        },
        set: SignedHostProfileSet {
            signatures: vec![sign_doc(&set, &keys.signers[0])],
            set,
        },
    };
    (bundle, keys)
}
