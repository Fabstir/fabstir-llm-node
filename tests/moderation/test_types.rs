// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 1.1 — Verdict / ModerationResult / MatchResult serde roundtrip.

use fabstir_llm_node::moderation::types::{MatchResult, ModerationResult, Verdict};

#[test]
fn types_roundtrip_serde() {
    let r = ModerationResult {
        verdict: Verdict::Blocked,
        reason: Some("csam".into()),
        report_id: None,
    };
    let back: ModerationResult = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(r, back);

    let m = MatchResult {
        is_match: true,
        distance: Some(12),
    };
    let mb: MatchResult = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
    assert_eq!(m, mb);

    for v in [Verdict::Cleared, Verdict::Blocked, Verdict::Flagged] {
        let vb: Verdict = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(v, vb);
    }
}
