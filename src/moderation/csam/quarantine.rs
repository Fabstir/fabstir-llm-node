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

use crate::moderation::config::MIN_RETENTION_DAYS;
use crate::moderation::csam::atrest;
use crate::moderation::types::{Category, ModerationError, Result};

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
    audit: Vec<AuditEntry>,
    retention_days: i64,
    next_id: u64,
}

impl Quarantine {
    pub fn new(key_material: Vec<u8>, retention_days: u32) -> Self {
        Self {
            key_material,
            items: HashMap::new(),
            audit: Vec::new(),
            // Clamp UP to the legal floor — never below 90 days (fail-closed).
            retention_days: retention_days.max(MIN_RETENTION_DAYS) as i64,
            next_id: 0,
        }
    }

    /// Preserve matched content (encrypted-at-rest). Returns the opaque case id.
    /// Never deletes anything; the content plaintext does not leave this store.
    pub fn preserve(
        &mut self,
        content: &[u8],
        category: Category,
        now: DateTime<Utc>,
    ) -> Result<String> {
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
        self.audit.push(AuditEntry {
            when: now,
            who: "system".into(),
            action: format!("preserve:{case_id}"),
        });
        Ok(case_id)
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
