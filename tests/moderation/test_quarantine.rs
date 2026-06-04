// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 6.1 — quarantine (preserve, never-delete, retention, audit, access). 🚨

use chrono::{DateTime, Duration, Utc};

use fabstir_llm_node::moderation::csam::quarantine::{Quarantine, Role};
use fabstir_llm_node::moderation::types::{Category, ModerationError};

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

#[test]
fn match_is_preserved_encrypted_not_deleted() {
    let mut q = Quarantine::new(b"qkey".to_vec(), 90);
    let content = b"matched evidence bytes";
    let case = q.preserve(content, Category::Csam, at()).unwrap();
    assert!(q.contains(&case), "preserved item must persist");
    // Stored at-rest as ciphertext, never the raw plaintext.
    assert!(q.sealed_len(&case).unwrap() > content.len());
    // An authorised role can retrieve (decrypt) the original.
    let got = q.retrieve(&case, Role::Reviewer, "alice", at()).unwrap();
    assert_eq!(got, content);
    // Retrieval is NOT deletion — the item is still preserved.
    assert!(q.contains(&case));
}

#[test]
fn never_auto_deletes() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let c1 = q.preserve(b"a", Category::Csam, at()).unwrap();
    let c2 = q.preserve(b"b", Category::Csam, at()).unwrap();
    // No delete API exists; preserving more never removes earlier items.
    assert!(q.contains(&c1) && q.contains(&c2));
    assert_eq!(q.len(), 2);
}

#[test]
fn retention_at_least_90_days() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let c = q.preserve(b"x", Category::Csam, at()).unwrap();
    assert!(q.retain_until(&c).unwrap() >= at() + Duration::days(90));
}

#[test]
fn retention_below_90_is_clamped_up() {
    // Misconfigured below the floor ⇒ clamped UP to 90 (fail-closed, never less).
    let mut q = Quarantine::new(b"k".to_vec(), 10);
    let c = q.preserve(b"x", Category::Csam, at()).unwrap();
    assert!(q.retain_until(&c).unwrap() >= at() + Duration::days(90));
}

#[test]
fn audit_log_is_append_only() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let c = q.preserve(b"x", Category::Csam, at()).unwrap();
    let n = q.audit_log().len();
    assert!(n >= 1, "preserve is audited");
    let first = q.audit_log()[0].action.clone();
    q.retrieve(&c, Role::Reviewer, "alice", at()).unwrap();
    assert!(q.audit_log().len() > n, "each action appends a new entry");
    assert_eq!(
        q.audit_log()[0].action,
        first,
        "earlier entries are never mutated"
    );
}

#[test]
fn access_requires_authorised_role() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let c = q.preserve(b"x", Category::Csam, at()).unwrap();
    let denied = q.retrieve(&c, Role::Unauthorised, "mallory", at());
    assert!(matches!(denied, Err(ModerationError::Unauthorised(_))));
    // The denied attempt is itself audited.
    assert!(q.audit_log().iter().any(|e| e.action.contains("denied")));
}
