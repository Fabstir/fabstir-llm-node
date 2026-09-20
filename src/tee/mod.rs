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
pub mod container_cache;
pub mod dstack;
pub mod dstack_provider;
pub mod gpu_evidence;
pub mod http_sources;
pub mod kbs_http;
pub mod key_broker;
pub mod keywrap;
pub mod live;
pub mod mock;
pub mod model_source;
pub mod orchestration;
pub mod plaintext_home;
pub mod policy;
pub mod policy_source;
pub mod provider;
pub mod types;
pub mod verifier;

pub use container::{
    chunk_count, decrypt_model, encrypt_model, ContainerHeader, AEAD_TAG_LEN, CONTAINER_MAGIC,
    CONTAINER_VERSION, HEADER_LEN,
};
pub use container_cache::{ContainerOutcome, HeaderCheck, Sha256Tee, SpaceCheck};
pub use dstack::{
    DstackClient, Endpoint as DstackEndpoint, InfoResponse as DstackInfo, QuoteResponse,
};
pub use dstack_provider::DstackAttestationProvider;
pub use gpu_evidence::{GpuEvidenceCollector, GpuEvidenceMode};
pub use http_sources::{HttpBlobSource, HttpPolicySource};
pub use kbs_http::HttpKeyBrokerClient;
pub use key_broker::{KeyBrokerClient, NodeAttestationClient};
pub use keywrap::{generate_ephemeral_keypair, unwrap_key, wrap_key, KEY_WRAP_HKDF_INFO};
pub use mock::{MockAttestationProvider, MockKeyBroker};
pub use model_source::{
    advertise_tee_attested, advertises_tee_attested, attested_model_id, host_tee_enabled, is_tmpfs,
    mark_attested_model_id, mark_test_release_loaded, raw_stderr, secure_delete,
    test_release_loaded, BlobSource, EncryptedModelLoader, EncryptedModelSpec,
};
pub use orchestration::{prepare_attested_model, PreparedModel};
pub use plaintext_home::{home_rule, mount_info, MountInfo};
pub use policy::{
    canonical_policy_bytes, check_policy_validity, policy_signature_digest, SignedModelPolicy,
};
pub use policy_source::{fetch_validated_policy, PolicySource, ProviderRegistry};
pub use provider::AttestationProvider;
pub use types::{
    report_data, report_data_identity, sha256_32, version_at_least, CcMode, Claims, CvmPolicy,
    Evidence, GpuPolicy, GpuReportFields, Policy, TeeError, TeeResult, WrappedKey,
    POLICY_SCHEMA_VERSION, REPORT_DATA_LEN,
};
pub use verifier::{AttestationVerifier, DefaultVerifier};
