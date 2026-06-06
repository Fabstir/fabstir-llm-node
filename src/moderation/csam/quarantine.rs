// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Quarantine — preserve matched material encrypted-at-rest, access-restricted,
//! retention ≥ 90 days, with an append-only audit log. 🚨 SECURITY-CRITICAL.
//!
//! NON-NEGOTIABLE: there is **no delete API**. Suspected CSAM is preserved as
//! evidence (B6 / never auto-delete); only authorised roles may retrieve it, and
//! every access — including a denied one — is audited.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

use crate::moderation::config::MIN_RETENTION_DAYS;
use crate::moderation::csam::atrest;
use crate::moderation::types::{AssetKind, Category, ModerationError, Result, Verdict};

/// Access role for restricted quarantine operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Authorised CSAM reviewer / reporter.
    Reviewer,
    /// Any other caller — denied.
    Unauthorised,
}

impl Role {
    fn authorised(&self) -> bool {
        matches!(self, Role::Reviewer)
    }
}

/// An append-only audit record: when, who, what.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub when: DateTime<Utc>,
    pub who: String,
    pub action: String,
}

struct Item {
    sealed: Vec<u8>,
    category: Category,
    #[allow(dead_code)]
    created_at: DateTime<Utc>,
    retain_until: DateTime<Utc>,
}

/// Encrypted, access-restricted, append-only-audited evidence store. No delete API.
pub struct Quarantine {
    key_material: Vec<u8>,
    items: HashMap<String, Item>,
    /// content-hash → case_id, for idempotent (retry-safe) preserve in this
    /// no-delete store (R3-D1). Keyed on the PLAINTEXT (before `atrest::seal`,
    /// which uses a random nonce ⇒ non-deterministic ciphertext — R4-D3).
    by_content: HashMap<[u8; 32], String>,
    audit: Vec<AuditEntry>,
    retention_days: i64,
    next_id: u64,
    /// 🧪 Test-only fault-injection seam (R5-C1): forces the next `preserve` to fail.
    fail_next_preserve: bool,
}

impl Quarantine {
    pub fn new(key_material: Vec<u8>, retention_days: u32) -> Self {
        Self {
            key_material,
            items: HashMap::new(),
            by_content: HashMap::new(),
            audit: Vec::new(),
            // Clamp UP to the legal floor — never below 90 days (fail-closed).
            retention_days: retention_days.max(MIN_RETENTION_DAYS) as i64,
            next_id: 0,
            fail_next_preserve: false,
        }
    }

    /// Preserve matched content (encrypted-at-rest). Returns the opaque case id.
    /// Never deletes anything; the content plaintext does not leave this store.
    ///
    /// **Idempotent by content (R3-D1):** re-preserving identical content no-ops and
    /// returns the existing case id, so a transcoder retry after a partial-preserve
    /// failure cannot duplicate evidence in this no-delete store. (This is internal
    /// case-id minting — the 3-arg signature is unchanged, so committed callers stay
    /// green; per-job audit provenance is added by `preserve_if_blocked`, R5-A2.)
    pub fn preserve(
        &mut self,
        content: &[u8],
        category: Category,
        now: DateTime<Utc>,
    ) -> Result<String> {
        // 🧪 Test-only fault injection (R5-C1): exercise the fail-closed paths;
        // `atrest::seal` is otherwise infallible in practice.
        if self.fail_next_preserve {
            self.fail_next_preserve = false;
            return Err(ModerationError::StoreError(
                "forced preserve failure (test seam)".into(),
            ));
        }
        let key = content_key(content, category);
        if let Some(existing) = self.by_content.get(&key) {
            return Ok(existing.clone());
        }
        let sealed = atrest::seal(&self.key_material, content)?;
        let case_id = format!("case-{}", self.next_id);
        self.next_id += 1;
        let retain_until = now + Duration::days(self.retention_days);
        self.items.insert(
            case_id.clone(),
            Item {
                sealed,
                category,
                created_at: now,
                retain_until,
            },
        );
        self.by_content.insert(key, case_id.clone());
        self.audit.push(AuditEntry {
            when: now,
            who: "system".into(),
            action: format!("preserve:{case_id}"),
        });
        Ok(case_id)
    }

    /// 🧪 Test-only fault-injection seam (R5-C1): make the **next** `preserve` call
    /// return a `StoreError`, so the fail-closed (preserve-failure ⇒ HOLD) paths can
    /// be exercised — `atrest::seal` is infallible in practice and `Quarantine` is
    /// concrete (no mock). NOT `#[cfg(test)]`: the integration-test crate compiles
    /// the library WITHOUT the `test` cfg (mirrors `ApiServer::new_for_test`), so a
    /// cfg-gated seam would be invisible to `tests/moderation/`. `#[doc(hidden)]`
    /// keeps it off the public API surface.
    #[doc(hidden)]
    pub fn fail_next_preserve(&mut self) {
        self.fail_next_preserve = true;
    }

    /// Retrieve (decrypt) preserved content. Restricted: an unauthorised role is
    /// denied (and the denial audited). Retrieval is NOT deletion.
    pub fn retrieve(
        &mut self,
        case_id: &str,
        role: Role,
        who: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<u8>> {
        if !role.authorised() {
            self.audit.push(AuditEntry {
                when: now,
                who: who.into(),
                action: format!("access-denied:{case_id}"),
            });
            return Err(ModerationError::Unauthorised(format!(
                "role not authorised for {case_id}"
            )));
        }
        let content = {
            let item = self
                .items
                .get(case_id)
                .ok_or_else(|| ModerationError::StoreError(format!("no such case {case_id}")))?;
            atrest::open(&self.key_material, &item.sealed)?
        };
        self.audit.push(AuditEntry {
            when: now,
            who: who.into(),
            action: format!("retrieve:{case_id}"),
        });
        Ok(content)
    }

    pub fn contains(&self, case_id: &str) -> bool {
        self.items.contains_key(case_id)
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn retain_until(&self, case_id: &str) -> Option<DateTime<Utc>> {
        self.items.get(case_id).map(|i| i.retain_until)
    }
    pub fn category(&self, case_id: &str) -> Option<Category> {
        self.items.get(case_id).map(|i| i.category)
    }
    pub fn sealed_len(&self, case_id: &str) -> Option<usize> {
        self.items.get(case_id).map(|i| i.sealed.len())
    }
    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.audit
    }

    /// Append an audit entry for an external action (e.g. a review/report). The log
    /// is append-only — this only adds, never mutates or removes prior entries.
    pub fn audit_action(&mut self, who: &str, action: &str, now: DateTime<Utc>) {
        self.audit.push(AuditEntry {
            when: now,
            who: who.to_string(),
            action: action.to_string(),
        });
    }
}

/// Stable content-addressing key for idempotent preserve: `SHA-256(tag(category) ||
/// content)` over the **plaintext** (the sealed bytes are non-deterministic — random
/// nonce, R4-D3). Category is folded in so identical bytes under two categories never
/// collapse to one mis-categorised case.
fn content_key(content: &[u8], category: Category) -> [u8; 32] {
    let tag: u8 = match category {
        Category::Csam => 0,
        Category::Nsfw => 1,
        Category::IllegalSpeech => 2,
        Category::Unknown => 3,
    };
    let mut h = Sha256::new();
    h.update([tag]);
    h.update(content);
    h.finalize().into()
}

/// Map the content kind to its evidence [`Category`] for preservation. The category
/// is the structured, caller-known signal — NOT a reason-string parse and NOT a
/// (nonexistent) `ModerationResult` field (R2-F1). Kept inside `csam` so the
/// CSAM-policy mapping stays isolated.
pub fn evidence_category(kind: AssetKind) -> Category {
    match kind {
        AssetKind::Image | AssetKind::VideoKeyframe => Category::Csam,
        AssetKind::Subtitle => Category::IllegalSpeech,
    }
}

/// Preserve-on-block — the B6 detect→preserve bridge. Preserves **every** blob (one
/// case id each) ONLY when `verdict` is not `Cleared`; returns the case ids (empty
/// when Cleared).
///
/// 🚨 **Fail-closed (R2-F2):** a preserve failure returns `Err`, and the caller MUST
/// hard-hold (503) — never clear, never record a releasing verdict. This is the whole
/// point of the wiring: a `blocked` verdict can never be returned with no evidence.
///
/// Idempotent by content (retry-safe, R3-D1). Records a per-job audit entry on
/// **every** hit — including a dedup no-op — via the append-only `audit_action`, so
/// one shared case id carries every job that matched (R4-C1) without changing
/// `preserve`'s signature (R5-A2).
pub fn preserve_if_blocked(
    quarantine: &mut Quarantine,
    verdict: Verdict,
    blobs: &[&[u8]],
    category: Category,
    job: Option<u64>,
    now: DateTime<Utc>,
) -> Result<Vec<String>> {
    if verdict.releases() {
        return Ok(Vec::new());
    }
    let job_label = job.map_or_else(|| "none".to_string(), |j| j.to_string());
    let mut case_ids = Vec::with_capacity(blobs.len());
    for blob in blobs {
        let case_id = quarantine.preserve(blob, category, now)?;
        quarantine.audit_action(
            &format!("job:{job_label}"),
            &format!("preserve-hit:{case_id}:job={job_label}"),
            now,
        );
        case_ids.push(case_id);
    }
    Ok(case_ids)
}
