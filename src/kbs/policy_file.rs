// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! `FilePolicySource` (design §7, D8): the same signed files nginx serves to
//! nodes, read from `KBS_POLICY_DIR`, validated with the node's own
//! `fetch_validated_policy` PLUS the two checks it does not do: `policy.model_id ==
//! requested` (the check `HttpPolicySource` does, `src/tee/http_sources.rs`) and
//! `policy_version >= keyring.min_policy_version`.

use crate::kbs::error::KbsError;
use crate::tee::http_sources::MAX_POLICY_BYTES;
use crate::tee::policy::SignedModelPolicy;
use crate::tee::policy_source::{fetch_validated_policy, PolicySource, ProviderRegistry};
use crate::tee::types::{TeeError, TeeResult};
use async_trait::async_trait;
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct FilePolicySource {
    dir: PathBuf,
}

impl FilePolicySource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `{dir}/{hex(model_id) lowercase}.json`.
    pub fn path_for(&self, model_id: &[u8; 32]) -> PathBuf {
        self.dir.join(format!("{}.json", hex::encode(model_id)))
    }

    fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
        let f = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        f.take(MAX_POLICY_BYTES as u64 + 1).read_to_end(&mut buf)?;
        if buf.len() > MAX_POLICY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("policy file over {MAX_POLICY_BYTES} bytes"),
            ));
        }
        Ok(buf)
    }
}

#[async_trait]
impl PolicySource for FilePolicySource {
    async fn fetch_policy(&self, model_id: [u8; 32]) -> TeeResult<SignedModelPolicy> {
        let path = self.path_for(&model_id);
        let bytes = match Self::read_bounded(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(TeeError::NoProviderBound(model_id));
            }
            Err(e) => return Err(TeeError::Io(e)),
        };
        let signed: SignedModelPolicy = serde_json::from_slice(&bytes)
            .map_err(|e| TeeError::Fetch(format!("policy {}: {e}", path.display())))?;
        if signed.policy.model_id != model_id {
            return Err(TeeError::VerificationFailed(format!(
                "model id: policy file {} is for {}, requested {}",
                path.display(),
                hex::encode(signed.policy.model_id),
                hex::encode(model_id)
            )));
        }
        Ok(signed)
    }
}

/// Step 3 of the pipeline: load, validate (schema, signer, window), the model-id
/// check, then the keyring floor. Maps to the wire kinds.
pub async fn load_policy(
    source: &FilePolicySource,
    registry: &ProviderRegistry,
    model_id: [u8; 32],
    min_policy_version: u32,
) -> Result<SignedModelPolicy, KbsError> {
    let signed = fetch_validated_policy(source, registry, model_id)
        .await
        .map_err(|e| match e {
            TeeError::NoProviderBound(_) => KbsError::no_provider("no policy for this model"),
            TeeError::Io(e) => KbsError::fault(format!("policy dir: {e}")),
            // A file that exists but does not parse (a half-copied swap, a stray byte) is
            // broker-side and transient: never a permanent `verification` for the node.
            TeeError::Fetch(m) => KbsError::fault(m),
            other => KbsError::verification(format!("policy: {other}")),
        })?;
    if signed.policy.policy_version < min_policy_version {
        return Err(KbsError::verification(format!(
            "policy version {} below keyring floor {min_policy_version}",
            signed.policy.policy_version
        )));
    }
    Ok(signed)
}
