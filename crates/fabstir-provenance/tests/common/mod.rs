// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
#![allow(dead_code)]
use fabstir_provenance::defs::sort_keys;
use serde_json::Value;

pub const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
pub const NODE_VERSION: &str = "8.64.0";
pub const UUID_A: &str = "6b1f3c9e-2a4d-4f0b-9c1e-5a7d8e9f0a1b";
pub const UUID_B: &str = "0d6f2a71-9b3e-4c58-8e1f-3a2b4c5d6e7f";
pub const SEQ_ID: &str = "4e2b9c10-7d3a-4f61-a0b2-c3d4e5f60718";
pub const URL: &str = "https://platformlessai.org/verify";

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

/// Compare `v` with `tests/golden/<name>.json` (canonical, pretty). `UPDATE_GOLDEN=1` (re)writes it; a
/// missing golden is a failure otherwise. Every regeneration needs a changelog line naming the cause.
pub fn golden(name: &str, v: &Value) {
    let path = format!("{DIR}/golden/{name}.json");
    let got = serde_json::to_string_pretty(&sort_keys(v)).unwrap() + "\n";
    if std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1") {
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("missing golden {path} (run with UPDATE_GOLDEN=1 once, review, commit)")
    });
    assert_eq!(got, want, "golden {name} differs");
}
