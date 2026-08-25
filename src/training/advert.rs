// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The pre-escrow training advert, and the tokenizer it pins.
//!
//! **Why this module exists.** The LTX `AllowListBundle` cannot carry the three
//! fields a training client needs before escrow (`tokenizerSha256`,
//! `baseServingModelId`, `alphas`): its shape is fixed, and it is built from
//! `templates/allowlist.json`, which lists ComfyUI workflow graphs. The
//! training template is a metadata descriptor of a different shape, so putting
//! it in that bundle would pull a non-graph into the LTX `bundleHash`. Training
//! therefore gets a small advert of its own.
//!
//! **The ordering that makes the tokenizer safe.** A client counts tokens
//! before escrow, and a count that disagrees with the host's is an
//! ESTIMATE_MISMATCH on a funded session. So the client must count with exactly
//! the tokenizer the host bills with. That is guaranteed by the PIN, not by the
//! source: the client fetches the bytes, hashes them, and compares against
//! `tokenizerSha256` from this advert. A fetch that skips the comparison
//! defeats the whole mechanism, which is why the advert publishes the hash
//! alongside the URL rather than only the URL.
//!
//! **Fail-closed at boot.** The node verifies the file it will serve against
//! the template pin at startup. A host holding the wrong tokenizer serves NO
//! training rather than serving a tokenizer it does not bill with.

use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};

/// The route the tokenizer is served from. Published in the advert rather than
/// hard-coded client-side so the path can move without a client release.
pub const TOKENIZER_ROUTE: &str = "/v1/training/tokenizer";

/// Refuse a tokenizer file larger than this. `tokenizer.json` for the pinned
/// base is ~12 MB; the ceiling is a boot-time sanity bound on a file the node
/// holds resident and serves to unauthenticated callers, not a format rule.
pub const TOKENIZER_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The tokenizer, verified against the template pin and held in memory.
///
/// Resident rather than read per request on purpose: the bytes are verified
/// ONCE at boot, so nothing can swap the file underneath a later request. Read
/// per request and the hash we published would stop describing what we serve.
#[derive(Debug)]
pub struct TokenizerAsset {
    bytes: Vec<u8>,
    /// "0x" + lowercase hex, equal to the template's `tokenizerSha256`.
    sha256_hex: String,
    /// Strong ETag. The content is immutable for a given pin, so this is the
    /// hash itself rather than an mtime.
    etag: String,
}

impl TokenizerAsset {
    /// Load `path` and verify it against `pin` (the template's
    /// `tokenizerSha256`). Returns `Err` on anything that would leave the node
    /// serving bytes it does not bill with.
    pub fn load(path: &Path, pin: &str) -> Result<Self, String> {
        let meta = std::fs::metadata(path)
            .map_err(|e| format!("TRAINING_TOKENIZER_PATH {}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!(
                "TRAINING_TOKENIZER_PATH {} is not a regular file",
                path.display()
            ));
        }
        if meta.len() > TOKENIZER_MAX_BYTES {
            return Err(format!(
                "TRAINING_TOKENIZER_PATH {} is {} bytes, over the {TOKENIZER_MAX_BYTES} ceiling",
                path.display(),
                meta.len()
            ));
        }
        let bytes = std::fs::read(path)
            .map_err(|e| format!("TRAINING_TOKENIZER_PATH {}: {e}", path.display()))?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let sha256_hex = format!("0x{}", hex::encode(digest));

        // Compare on the NORMALISED hex: the template may or may not carry the
        // 0x prefix and may differ in case, and neither difference means the
        // bytes differ. Anything else here is a genuine mismatch.
        if !norm_hex(&sha256_hex).eq(&norm_hex(pin)) {
            return Err(format!(
                "tokenizer at {} hashes to {sha256_hex}, but the template pins {pin}; \
                 this host would count with bytes it does not bill with",
                path.display()
            ));
        }
        Ok(TokenizerAsset {
            etag: format!("\"{}\"", hex::encode(digest)),
            bytes,
            sha256_hex,
        })
    }

    /// Build from bytes WITHOUT checking them against a pin.
    ///
    /// Test scaffolding only, and public solely because integration tests in
    /// `tests/` compile against this crate externally, where `#[cfg(test)]`
    /// does not reach. Production must go through [`TokenizerAsset::load`],
    /// which is the only path that verifies the template pin.
    #[doc(hidden)]
    pub fn from_bytes_for_tests(bytes: Vec<u8>) -> Self {
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        TokenizerAsset {
            etag: format!("\"{}\"", hex::encode(digest)),
            bytes,
            sha256_hex: format!("0x{}", hex::encode(digest)),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256_hex(&self) -> &str {
        &self.sha256_hex
    }
    pub fn etag(&self) -> &str {
        &self.etag
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Strip `0x` and lowercase, so a prefix or case difference is never read as a
/// hash difference.
fn norm_hex(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .to_ascii_lowercase()
}

/// The advert a client reads before escrow.
///
/// `tokenizer.available` is false when this host serves no tokenizer (none
/// configured, or the configured one failed its pin). Counting is impossible
/// against such a host; serve-back is unaffected.
///
/// `pricePerToken` is a decimal STRING, matching `train_accepted.billing`;
/// every token COUNT is a JSON number, because counts are maths inputs and are
/// bounded well inside the JS safe-integer range. That split is deliberate and
/// is the same one pinned in `tests/training/vectors/billing.json`.
pub fn advert_json(
    template: &crate::training::core::TrainingTemplate,
    tokenizer: Option<&Arc<TokenizerAsset>>,
    model_id: &[u8; 32],
    price_per_token: &ethers::types::U256,
    allow_list_version: u64,
    rate_limit_tokens_per_sec: u64,
) -> serde_json::Value {
    let mut tmpl = serde_json::json!({
        "templateId": template.template_id,
        "templateHash": template.template_hash,
        "tokenizerSha256": template.tokenizer_sha256,
        "baseServingModelId": template.base_serving_model_id,
        "ranks": template.ranks,
        "alphas": template.alphas,
        "seqLens": template.seq_lens,
        "countingRecipe": template.counting_recipe,
        "specialsPerSample": template.specials_per_sample,
        "maxEpochs": template.max_epochs,
        "maxTotalTokens": template.max_total_tokens,
        "sliceTokens": template.slice_tokens,
    });
    // `lrs` is optional in the template; omit rather than publish null, so a
    // client can distinguish "unconstrained" from "constrained to nothing".
    if let Some(lrs) = &template.lrs {
        tmpl["lrs"] = serde_json::json!(lrs);
    }
    serde_json::json!({
        "allowListVersion": allow_list_version,
        "modelId": format!("0x{}", hex::encode(model_id)),
        "pricePerToken": price_per_token.to_string(),
        "rateLimitTokensPerSec": rate_limit_tokens_per_sec,
        "template": tmpl,
        // Node-ENFORCED dataset limits, distinct from the template's training
        // bounds above. Published so a client refuses before an upload rather
        // than after: maxDatasetBytes is the cheap one, and without it a
        // client discovers the refusal only after posting the whole dataset.
        "bounds": {
            "maxDatasetBytes":
                crate::training::staging::SHARD_PLAINTEXT_MAX_BYTES
                    * crate::training::staging::MAX_SHARDS as u64,
            "maxShardBytes": crate::training::staging::SHARD_PLAINTEXT_MAX_BYTES,
            "maxShards": crate::training::staging::MAX_SHARDS,
            "maxManifestBytes": crate::training::staging::MANIFEST_MAX_BYTES,
            // C.6 plausibility: totalBytes must not exceed declaredTokens * 8.
            "maxBytesPerToken": 8,
        },
        "tokenizer": match tokenizer {
            Some(t) => serde_json::json!({
                "available": true,
                "url": TOKENIZER_ROUTE,
                "sha256": t.sha256_hex(),
                "bytes": t.len(),
            }),
            // Closed vocabulary, like CAPACITY's `detail.reason`. The client
            // cannot count against this host and should say so BEFORE escrow
            // rather than discovering it at estimate time. `template
            // .tokenizerSha256` is still published above, because the pin is a
            // property of the template and stays true wherever the bytes come
            // from.
            None => serde_json::json!({
                "available": false,
                "reason": "notServed",
            }),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::core::TrainingTemplate;

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn sha_of(bytes: &[u8]) -> String {
        format!("0x{}", hex::encode(<[u8; 32]>::from(Sha256::digest(bytes))))
    }

    fn template() -> TrainingTemplate {
        TrainingTemplate {
            template_id: "train-qlora-qwen38-27b-v1".into(),
            base_serving_model_id: "0x8923".to_string() + &"0".repeat(60),
            template_hash: "0x43e1".to_string() + &"0".repeat(60),
            tokenizer_sha256: "0x0997".to_string() + &"0".repeat(60),
            counting_recipe: "count-v1".into(),
            specials_per_sample: 1,
            ranks: vec![8, 16],
            alphas: vec![16, 32],
            seq_lens: vec![2048],
            lrs: None,
            max_epochs: 3,
            max_total_tokens: 15_000_000,
            slice_tokens: 1_000_000,
        }
    }

    #[test]
    fn load_refuses_bytes_that_do_not_match_the_pin() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "tokenizer.json", b"{\"model\":\"a\"}");
        // The pin belongs to DIFFERENT bytes. Serving these would mean the
        // client counts with one tokenizer and the host bills with another.
        let err = TokenizerAsset::load(&p, &sha_of(b"{\"model\":\"b\"}")).unwrap_err();
        assert!(err.contains("but the template pins"), "{err}");
        assert!(err.contains("does not bill with"), "{err}");
    }

    #[test]
    fn load_accepts_a_match_and_ignores_prefix_and_case() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"{\"model\":\"pinned\"}";
        let p = write(&dir, "tokenizer.json", body);
        let pin = sha_of(body);

        let a = TokenizerAsset::load(&p, &pin).unwrap();
        assert_eq!(a.bytes(), body);
        assert_eq!(a.len(), body.len());
        assert_eq!(a.sha256_hex(), pin);
        // Strong ETag, quoted, and NOT carrying the 0x prefix.
        assert!(
            a.etag().starts_with('"') && a.etag().ends_with('"'),
            "{}",
            a.etag()
        );
        assert!(!a.etag().contains("0x"), "{}", a.etag());

        // A prefix-less, upper-case pin is the SAME hash, not a mismatch.
        let shouty = pin.trim_start_matches("0x").to_ascii_uppercase();
        assert!(TokenizerAsset::load(&p, &shouty).is_ok());
    }

    #[test]
    fn load_refuses_a_missing_file_and_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(TokenizerAsset::load(&dir.path().join("nope.json"), "0x00").is_err());
        // A directory at the path must not be read as an empty tokenizer.
        assert!(TokenizerAsset::load(dir.path(), &sha_of(b"")).is_err());
    }

    #[test]
    fn advert_publishes_the_three_fields_the_ltx_bundle_cannot_carry() {
        let t = template();
        let tok = Arc::new(TokenizerAsset::from_bytes_for_tests(b"tok".to_vec()));
        let v = advert_json(
            &t,
            Some(&tok),
            &[0xABu8; 32],
            &ethers::types::U256::from(904u64),
            1,
            10_000,
        );

        // The whole reason this module exists.
        assert_eq!(
            v["template"]["tokenizerSha256"],
            serde_json::json!(t.tokenizer_sha256)
        );
        assert_eq!(
            v["template"]["baseServingModelId"],
            serde_json::json!(t.base_serving_model_id)
        );
        assert_eq!(v["template"]["alphas"], serde_json::json!([16, 32]));

        // The UI dev refused to default this rather than guess 0, correctly:
        // guessing mis-counts EVERY sample and lands as DECLARED_TOKENS_MISMATCH
        // on a funded job. Publishing it is the fix.
        assert_eq!(v["template"]["specialsPerSample"], serde_json::json!(1));
        assert_eq!(
            v["template"]["countingRecipe"],
            serde_json::json!("count-v1")
        );

        // Node-enforced dataset bounds: refuse BEFORE the upload, not after.
        assert_eq!(v["bounds"]["maxShards"], serde_json::json!(64));
        assert_eq!(
            v["bounds"]["maxShardBytes"],
            serde_json::json!(25_161_728u64)
        );
        assert_eq!(
            v["bounds"]["maxDatasetBytes"],
            serde_json::json!(25_161_728u64 * 64),
            "the cheap pre-upload check"
        );
        assert_eq!(v["bounds"]["maxBytesPerToken"], serde_json::json!(8));

        // pricePerToken is a decimal STRING (matches train_accepted.billing)...
        assert_eq!(v["pricePerToken"], serde_json::json!("904"));
        assert!(v["pricePerToken"].is_string());
        // ...while token COUNTS stay numbers, because they are maths inputs.
        assert!(v["template"]["sliceTokens"].is_number());
        assert!(v["template"]["maxTotalTokens"].is_number());
        assert_eq!(
            v["template"]["maxTotalTokens"],
            serde_json::json!(15_000_000)
        );

        // The client must be able to verify what it fetches.
        assert_eq!(v["tokenizer"]["url"], serde_json::json!(TOKENIZER_ROUTE));
        assert_eq!(
            v["tokenizer"]["sha256"],
            serde_json::json!(tok.sha256_hex())
        );
        assert_eq!(v["tokenizer"]["bytes"], serde_json::json!(3));

        assert_eq!(
            v["modelId"],
            serde_json::json!(format!("0x{}", "ab".repeat(32)))
        );
        assert_eq!(v["allowListVersion"], serde_json::json!(1));
    }

    #[test]
    fn advert_says_so_when_this_host_serves_no_tokenizer() {
        // Serve-back needs no tokenizer, so a host without one still adverts.
        let v = advert_json(
            &template(),
            None,
            &[0u8; 32],
            &ethers::types::U256::from(1u64),
            1,
            10,
        );
        assert_eq!(v["tokenizer"]["available"], serde_json::json!(false));
        assert_eq!(v["tokenizer"]["reason"], serde_json::json!("notServed"));
        assert!(
            v["tokenizer"].get("url").is_none(),
            "a URL that serves nothing invites an unverifiable fetch: {v}"
        );
        // The PIN is a property of the template, not of this host, so it stays
        // published: a client may source the bytes elsewhere and still verify.
        assert_eq!(
            v["template"]["tokenizerSha256"],
            serde_json::json!(template().tokenizer_sha256)
        );
    }

    #[test]
    fn optional_lrs_is_omitted_rather_than_published_as_null() {
        let tok = Arc::new(TokenizerAsset::from_bytes_for_tests(b"tok".to_vec()));
        let price = ethers::types::U256::from(1u64);

        let absent = advert_json(&template(), Some(&tok), &[0u8; 32], &price, 1, 10);
        assert!(
            absent["template"].get("lrs").is_none(),
            "null would read as 'constrained to nothing'"
        );

        let mut t = template();
        t.lrs = Some(vec!["1e-4".into()]);
        let present = advert_json(&t, Some(&tok), &[0u8; 32], &price, 1, 10);
        assert_eq!(present["template"]["lrs"], serde_json::json!(["1e-4"]));
    }
}
