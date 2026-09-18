// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3.3 — the boot-time decision table for the attested load path,
//! over an explicit environment map (no `set_var`). Every "refused" row is what
//! makes `HOST_TEE_ENABLED=true` safe to put in the composes: a node that
//! advertises `tee-attested` either has an attested model or does not start.

use fabstir_llm_node::tee::live::{
    LiveConfig, EXPECTED_SHA256_ENV, MODEL_ID_ENV, PROVIDER_ENV, REQUIRE_VALIDATION_ENV,
};
use fabstir_llm_node::tee::types::TeeError;
use fabstir_llm_node::tee::{advertise_tee_attested, advertises_tee_attested, test_release_loaded};
use std::collections::HashMap;

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

const ID: &str = "abababababababababababababababababababababababababababababababab";
const PROVIDER: &str = "0x0123456789abcdef0123456789abcdef01234567";

#[test]
fn plain_node_when_tee_is_off_and_no_model_id() {
    assert_eq!(LiveConfig::resolve(&env(&[]), false).unwrap(), None);
    assert_eq!(
        LiveConfig::resolve(&env(&[("MODEL_PATH", "/models/x.gguf")]), false).unwrap(),
        None
    );
}

#[test]
fn model_id_without_the_flag_is_refused() {
    let e = env(&[(MODEL_ID_ENV, ID)]);
    match LiveConfig::resolve(&e, false) {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("HOST_TEE_ENABLED"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_flag_without_a_model_id_is_refused() {
    // The row that protects the advert: never tee-attested with nothing behind it.
    match LiveConfig::resolve(&env(&[]), true) {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("never advertise"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn attested_path_resolves_with_the_required_variables() {
    let e = env(&[(MODEL_ID_ENV, ID), (PROVIDER_ENV, PROVIDER)]);
    let cfg = LiveConfig::resolve(&e, true).unwrap().unwrap();
    assert_eq!(cfg.model_id, [0xABu8; 32]);
    assert_eq!(cfg.provider, PROVIDER);
    assert_eq!(cfg.expected_sha256, None);
    let e = env(&[
        (MODEL_ID_ENV, &format!("0x{ID}")),
        (PROVIDER_ENV, PROVIDER),
        (EXPECTED_SHA256_ENV, &format!("0x{}", "CD".repeat(32))),
    ]);
    let cfg = LiveConfig::resolve(&e, true).unwrap().unwrap();
    assert_eq!(
        cfg.expected_sha256.as_deref(),
        Some("cd".repeat(32).as_str())
    );
    // Validation required AND the hash present: the attested path resolves.
    let e = env(&[
        (MODEL_ID_ENV, ID),
        (PROVIDER_ENV, PROVIDER),
        (EXPECTED_SHA256_ENV, &"cd".repeat(32)),
        (REQUIRE_VALIDATION_ENV, "true"),
    ]);
    assert!(LiveConfig::resolve(&e, true).unwrap().is_some());
    // And on the plain node the variable is not this module's business.
    assert_eq!(
        LiveConfig::resolve(&env(&[(REQUIRE_VALIDATION_ENV, "true")]), false).unwrap(),
        None
    );
}

#[test]
fn attested_path_refuses_plain_model_path_disable_llm_and_bad_values() {
    let base = |extra: &[(&str, &str)]| {
        let mut v = vec![(MODEL_ID_ENV, ID), (PROVIDER_ENV, PROVIDER)];
        v.extend_from_slice(extra);
        env(&v)
    };
    for (extra, needle) in [
        (vec![("MODEL_PATH", "/models/x.gguf")], "MODEL_PATH"),
        (vec![("DISABLE_LLM", "true")], "DISABLE_LLM"),
        (vec![("DISABLE_LLM", "1")], "DISABLE_LLM"),
        (vec![(EXPECTED_SHA256_ENV, "abc")], "64 hex"),
        // Validation asked for but nothing to bind the plaintext to: refused,
        // never a silent no-op (P3 converge round 2).
        (vec![(REQUIRE_VALIDATION_ENV, "true")], EXPECTED_SHA256_ENV),
        (vec![(REQUIRE_VALIDATION_ENV, "1")], EXPECTED_SHA256_ENV),
    ] {
        match LiveConfig::resolve(&base(&extra), true) {
            Err(TeeError::VerificationFailed(m)) => assert!(m.contains(needle), "{m}"),
            other => panic!("{extra:?}: {other:?}"),
        }
    }
    // Missing or malformed provider / model id.
    assert!(matches!(
        LiveConfig::resolve(&env(&[(MODEL_ID_ENV, ID)]), true),
        Err(TeeError::VerificationFailed(_))
    ));
    assert!(matches!(
        LiveConfig::resolve(
            &env(&[(MODEL_ID_ENV, ID), (PROVIDER_ENV, "not-an-address")]),
            true
        ),
        Err(TeeError::VerificationFailed(_))
    ));
    assert!(matches!(
        LiveConfig::resolve(
            &env(&[(MODEL_ID_ENV, "abcd"), (PROVIDER_ENV, PROVIDER)]),
            true
        ),
        Err(TeeError::VerificationFailed(_))
    ));
}

#[test]
fn tee_attested_is_advertised_only_for_a_real_release() {
    // P3 converge round 11: a CPU gate node (test-keyring release) must not tell
    // the NodeRegistry or a client that its weights were keyed against real GPU
    // evidence. Pure rule, then the live wrapper in this (untouched) process.
    assert!(advertise_tee_attested(true, false));
    assert!(!advertise_tee_attested(true, true));
    assert!(!advertise_tee_attested(false, false));
    assert!(!advertise_tee_attested(false, true));
    assert!(
        !test_release_loaded(),
        "no attested load ran in this process"
    );
    // The live wrapper reads HOST_TEE_ENABLED from the real environment (a
    // OnceLock), so it is only asserted where that variable is known absent;
    // the pure rule above is the test, this is a consistency check.
    if std::env::var_os("HOST_TEE_ENABLED").is_none() {
        assert!(!advertises_tee_attested());
    }
}
