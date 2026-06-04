// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 0.2 — ModerationConfig fail-closed defaults + serde roundtrip.

use fabstir_llm_node::moderation::config::ModerationConfig;
use fabstir_llm_node::moderation::types::{Category, Disposition};

#[test]
fn config_defaults_are_fail_closed() {
    let cfg = ModerationConfig::default();

    // Retention is at least 90 days (B6 legal floor).
    assert!(
        cfg.retention_days >= 90,
        "retention must be >= 90 days, got {}",
        cfg.retention_days
    );

    // An unmapped / unknown category must default to a HOLD (Block), never Clear.
    assert_eq!(
        cfg.disposition_for(Category::Unknown),
        Disposition::Block,
        "unknown category must default to Block (fail-closed)"
    );

    // CSAM always blocks.
    assert_eq!(
        cfg.disposition_for(Category::Csam),
        Disposition::Block,
        "CSAM must always Block"
    );

    // The PDQ near-match threshold is bounded (0..=256 bits) and non-trivial.
    assert!(
        cfg.pdq_max_distance <= 256,
        "pdq_max_distance must be a valid Hamming distance (<=256)"
    );
}

#[test]
fn config_roundtrip_serde() {
    let cfg = ModerationConfig::default();
    let json = serde_json::to_string(&cfg).expect("serialize");
    let back: ModerationConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(cfg, back, "config must roundtrip through serde unchanged");
}

#[test]
fn config_rejects_short_retention() {
    // A config FILE that asks for less than the 90-day legal floor must be
    // rejected on load (fail-closed at the untrusted boundary), not accepted.
    let json = r#"{"pdq_max_distance":31,"retention_days":10,"refresh_interval_secs":3600,"disposition":{}}"#;
    let parsed: std::result::Result<ModerationConfig, _> = serde_json::from_str(json);
    assert!(
        parsed.is_err(),
        "deserializing retention_days < 90 must fail-closed, got {:?}",
        parsed.map(|c| c.retention_days)
    );
}
