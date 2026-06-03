// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TEE / Confidential-Inference module (model-weight protection via NVIDIA CC).
//!
//! A model provider can ship proprietary weights to an untrusted GPU host such
//! that the operator — even as root — cannot extract the plaintext: the node
//! runs inside a CPU-TEE confidential VM with the GPU in CC mode, remotely
//! attests, and the weight-decryption key is released only on a passing
//! attestation. See `docs/development/IMPLEMENTATION-NVIDIA-TEE.md`.
//!
//! Phase 1.1 introduces the core [`types`]; later sub-phases add the attestation
//! provider/verifier traits, ECIES key-wrap, the key broker, the encrypted-model
//! container, and model-source orchestration — all behind a mock backend so the
//! pipeline is fully testable on any Linux without CC hardware.
pub mod container;
pub mod key_broker;
pub mod keywrap;
pub mod mock;
pub mod model_source;
pub mod orchestration;
pub mod policy;
pub mod policy_source;
pub mod provider;
pub mod types;
pub mod verifier;

pub use container::{
    chunk_count, decrypt_model, encrypt_model, ContainerHeader, AEAD_TAG_LEN, CONTAINER_MAGIC,
    CONTAINER_VERSION, HEADER_LEN,
};
pub use key_broker::{KeyBrokerClient, NodeAttestationClient};
pub use keywrap::{generate_ephemeral_keypair, unwrap_key, wrap_key, KEY_WRAP_HKDF_INFO};
pub use mock::{MockAttestationProvider, MockKeyBroker};
pub use model_source::{
    host_tee_enabled, is_tmpfs, secure_delete, BlobSource, EncryptedModelLoader, EncryptedModelSpec,
};
pub use orchestration::{prepare_attested_model, PreparedModel};
pub use policy::{
    canonical_policy_bytes, check_policy_validity, policy_signature_digest, SignedModelPolicy,
};
pub use policy_source::{fetch_validated_policy, PolicySource, ProviderRegistry};
pub use provider::AttestationProvider;
pub use types::{
    cross_bind_report_data, sha256_32, Claims, Evidence, GpuReportFields, Policy, TeeError,
    TeeResult, WrappedKey,
};
pub use verifier::{AttestationVerifier, DefaultVerifier};
