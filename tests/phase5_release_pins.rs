// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P4.5 (design P10): the composes are pinned to the LAST release that
//! was cut into the Phala image, and never to an earlier one.
//!
//! `RELEASE_PINS` is the history of `(node version, image digest)` re-cuts. The
//! standing rule for every future re-cut: a release that WILL be re-cut adds a
//! row `(version, "PENDING")` in the same edit as its version bump (so rules
//! (ii) and (iii) are red until the new digest is pasted); the paste at
//! `deployment/phala/build.sh` time fills the row. Adding the row supersedes the
//! previous digest by construction, so there is no separate "append" step to
//! forget. A host2-only release (never re-cut) adds nothing and stays green.
//!
//! Rules: (i) both composes' `ai.platformless.node_version` label == the last
//! row's version; (ii) both composes' image digest == the last row's digest;
//! (iii) the composes' digest is in no earlier row.
//!
//! Own file: `tests/phase5_compose_guard.rs` is already over the 400-line cap.

use std::path::PathBuf;

/// `(node version, image digest)` per re-cut, oldest first.
const RELEASE_PINS: &[(&str, &str)] = &[
    (
        "8.55.0",
        "715f28be882da2796d411691d8c87542d1b4b22ea6d4da326a73a62dd2bd4c83",
    ),
    // P4.5 bundle: /info preflight, t5t: witness rule, collector DevTools refusal
    // (built 2026-09-20 from 6f40af7; pushed by build.sh on 3XS-Z).
    (
        "8.56.0",
        "0d7466556c41059a6ea4a3c977c53337194092fb3d07e328aff9d2ae737e9469",
    ),
    // P5.5 streaming load + the plaintext's home: the composes gain two named
    // volumes, TEE_CONTAINER_DIR / TEE_PLAINTEXT_VOLUME literals and the
    // TEE_DECRYPT_ON_DISK / TEE_BLOB_MAX_BYTES placeholders (built 2026-09-20).
    (
        "8.57.0",
        "26ea9818478ee34c8dfae673b7d6f26667f75912f282b3cb5a49e17de42faa73",
    ),
];

const COMPOSES: &[&str] = &["compose.gpu.yml", "compose.cpu.yml"];

fn compose(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("deployment/phala")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The value of a `key: "value"` line (comments stripped), if present once.
fn value_of(text: &str, key: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = code.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix(':') {
                assert!(found.is_none(), "`{key}` appears more than once");
                found = Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    found
}

fn image_digest(text: &str) -> String {
    let image = value_of(text, "image").expect("an `image:` line");
    image
        .split_once("@sha256:")
        .map(|(_, d)| d.to_string())
        .unwrap_or_else(|| panic!("image `{image}` is not digest-pinned"))
}

fn last_pin() -> (&'static str, &'static str) {
    *RELEASE_PINS.last().expect("at least one re-cut")
}

#[test]
fn composes_carry_the_last_pinned_version() {
    let (version, _) = last_pin();
    for name in COMPOSES {
        let label = value_of(&compose(name), "ai.platformless.node_version")
            .unwrap_or_else(|| panic!("{name}: missing `ai.platformless.node_version` label"));
        assert_eq!(
            label, version,
            "{name}: the node_version label must equal the last RELEASE_PINS row (a re-cut adds a row; a host2-only release adds nothing)"
        );
    }
}

#[test]
fn composes_pin_the_last_pinned_digest() {
    let (version, digest) = last_pin();
    assert_ne!(
        digest, "PENDING",
        "RELEASE_PINS row {version} is still PENDING: run deployment/phala/build.sh, paste the digest into BOTH composes and into this row (design P10 / §6.5)"
    );
    for name in COMPOSES {
        assert_eq!(
            image_digest(&compose(name)),
            digest,
            "{name}: the image digest must equal the last RELEASE_PINS row ({version})"
        );
    }
}

#[test]
fn composes_never_pin_a_superseded_digest() {
    let earlier = &RELEASE_PINS[..RELEASE_PINS.len() - 1];
    for name in COMPOSES {
        let d = image_digest(&compose(name));
        if let Some((v, _)) = earlier.iter().find(|(_, old)| *old == d) {
            panic!("{name}: pinned to the {v} image, superseded by the {} re-cut (design P10: paste the new digest)", last_pin().0);
        }
    }
}

#[test]
fn release_pins_are_well_formed() {
    // Versions ascend, digests are 64 lowercase hex or the one PENDING marker at the end.
    let mut prev: Option<(u32, u32, u32)> = None;
    for (i, (v, d)) in RELEASE_PINS.iter().enumerate() {
        let parts: Vec<u32> = v
            .split('.')
            .map(|p| p.parse().expect("numeric version"))
            .collect();
        assert_eq!(parts.len(), 3, "{v}");
        let cur = (parts[0], parts[1], parts[2]);
        assert!(prev.map(|p| p < cur).unwrap_or(true), "{v} does not ascend");
        prev = Some(cur);
        let last = i + 1 == RELEASE_PINS.len();
        assert!(
            (*d == "PENDING" && last)
                || (d.len() == 64
                    && d.chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())),
            "{v}: digest `{d}` must be 64 lowercase hex (PENDING only on the last row)"
        );
    }
}
