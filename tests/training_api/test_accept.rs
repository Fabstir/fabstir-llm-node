// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Wave A of the T3.1 matrix: the pure accept core against interface A.3
//! (six gates + the drift-proof snapshot decode), C.6 (plausibility,
//! one-per-address, cooldown), and A.1's numeric wire rule. Every reject row
//! mutates EXACTLY ONE field from the passing M0 baseline, so a row can only
//! go green when its specific gate exists.

use ethers::types::{Address, U256};
use fabstir_llm_node::training::accept::{
    decode_session_snapshot, plausibility_gate, validate_session, AcceptConfig, AcceptReject,
    AttemptClaim, AttemptOutcome, AttemptRegistry, SessionSnapshot, SessionStatus,
    SESSION_JOBS_HEAD_WORDS,
};
use fabstir_llm_node::training::types::TrainingJob;

// --- the M0 baseline (vectors' price 904; declared 4,339,200 × 2 epochs) ---

const PRICE: u64 = 904;
const DECLARED: u64 = 4_339_200;
const EPOCHS: u64 = 2;
const TRAINING_TOKENS: u64 = DECLARED * EPOCHS; // 8,678,400
const NOW: u64 = 1_756_000_000;

fn addr(byte: u8) -> Address {
    Address::from_slice(&[byte; 20])
}

fn model_id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn passing_snapshot() -> SessionSnapshot {
    // deposit must satisfy deposit × 1000 / price − tokensUsed ≥ trainingTokens:
    // 8,678,400 × 904 / 1000 = 7,845,273.6 → 8,000,000 covers with headroom.
    SessionSnapshot {
        depositor: addr(0xD1),
        host: addr(0xB0),
        payment_token: addr(0xEC),
        deposit: U256::from(8_000_000u64),
        price_per_token: U256::from(PRICE),
        tokens_used: U256::zero(),
        max_duration: U256::from(14_400u64),
        start_time: U256::from(NOW - 100), // 100 s into the 1,200 s latitude
        proof_timeout_window: U256::from(3_600u64),
        status: SessionStatus::Active,
    }
}

fn run(snap: &SessionSnapshot) -> Result<(), AcceptReject> {
    validate_session(
        snap,
        model_id(0xAA),
        NOW,
        U256::from(TRAINING_TOKENS),
        U256::from(PRICE),
        &[addr(0xEC)],
        model_id(0xAA),
        addr(0xB0),
        &AcceptConfig::default(),
    )
}

fn expect_session_params(result: Result<(), AcceptReject>, needle: &str) {
    match result {
        Err(AcceptReject::SessionParams { detail }) => assert!(
            detail.to_lowercase().contains(&needle.to_lowercase()),
            "detail {detail:?} must name {needle:?}"
        ),
        other => panic!("expected SessionParams({needle}), got {other:?}"),
    }
}

// --- A.3: the POSITIVE control first (the always-reject regression row) ---

#[test]
fn a3_positive_control_fresh_m0_session_passes() {
    // A fresh session with the SDK's own M0 parameters passes with the
    // 1,200 s accept latitude to spare (the v0.2.1 constants defect made
    // EVERY honest job reject — a reject-only matrix can never catch that).
    assert_eq!(run(&passing_snapshot()), Ok(()));
    // And still passes 1,000 s into the session (inside the latitude).
    let mut late = passing_snapshot();
    late.start_time = U256::from(NOW - 1_000);
    assert_eq!(run(&late), Ok(()));
}

// --- A.3 reject rows: one mutated field each ---

#[test]
fn a3_rejects_insufficient_remaining_headroom_net_of_tokens_used() {
    // The static deposit covers, but tokensUsed has eaten the headroom:
    // 8,000,000 × 1000 / 904 = 8,849,557 capacity; minus 200,000 used
    // leaves 8,649,557 < 8,678,400 required.
    let mut snap = passing_snapshot();
    snap.tokens_used = U256::from(200_000u64);
    expect_session_params(run(&snap), "headroom");
}

#[test]
fn a3_rejects_wrong_price() {
    let mut snap = passing_snapshot();
    snap.price_per_token = U256::from(PRICE - 1);
    expect_session_params(run(&snap), "price");
}

#[test]
fn a3_rejects_unpriced_payment_token() {
    let mut snap = passing_snapshot();
    snap.payment_token = addr(0x99);
    expect_session_params(run(&snap), "token");
}

#[test]
fn a3_rejects_insufficient_remaining_lifetime() {
    // 1,300 s into the session: 14,400 − 1,300 = 13,100 < 12,600 + 600.
    let mut snap = passing_snapshot();
    snap.start_time = U256::from(NOW - 1_300);
    expect_session_params(run(&snap), "lifetime");
}

#[test]
fn a3_rejects_proof_window_below_floor() {
    let mut snap = passing_snapshot();
    snap.proof_timeout_window = U256::from(3_599u64);
    expect_session_params(run(&snap), "proof");
}

#[test]
fn a3_rejects_wrong_session_model() {
    let snap = passing_snapshot();
    let result = validate_session(
        &snap,
        model_id(0xBB), // sessionModel ≠ the training model id
        NOW,
        U256::from(TRAINING_TOKENS),
        U256::from(PRICE),
        &[addr(0xEC)],
        model_id(0xAA),
        addr(0xB0),
        &AcceptConfig::default(),
    );
    expect_session_params(result, "model");
}

#[test]
fn a3_rejects_foreign_host_session() {
    // submitProofOfWork auth is msg.sender == session.host: accepting a
    // session bound to another host would train for free.
    let mut snap = passing_snapshot();
    snap.host = addr(0x77);
    expect_session_params(run(&snap), "host");
}

#[test]
fn a3_rejects_non_active_status() {
    for status in [SessionStatus::Completed, SessionStatus::TimedOut] {
        let mut snap = passing_snapshot();
        snap.status = status;
        expect_session_params(run(&snap), "active");
    }
}

// --- the drift-proof decode (the 17-field trap made structural) ---

fn encode_snapshot_blob(snap: &SessionSnapshot, status_byte: u8) -> Vec<u8> {
    let mut blob = vec![0u8; SESSION_JOBS_HEAD_WORDS * 32];
    let put_addr = |blob: &mut [u8], w: usize, a: Address| {
        blob[w * 32 + 12..w * 32 + 32].copy_from_slice(a.as_bytes())
    };
    let put_uint =
        |blob: &mut [u8], w: usize, v: U256| v.to_big_endian(&mut blob[w * 32..(w + 1) * 32]);
    put_uint(&mut blob, 0, U256::from(42u64)); // id (ignored)
    put_addr(&mut blob, 1, snap.depositor);
    put_addr(&mut blob, 2, snap.host);
    put_addr(&mut blob, 3, snap.payment_token);
    put_uint(&mut blob, 4, snap.deposit);
    put_uint(&mut blob, 5, snap.price_per_token);
    put_uint(&mut blob, 6, snap.tokens_used);
    put_uint(&mut blob, 7, snap.max_duration);
    put_uint(&mut blob, 8, snap.start_time);
    put_uint(&mut blob, 9, U256::from(NOW - 100)); // lastProofTime (ignored)
    put_uint(&mut blob, 10, U256::from(1000u64)); // proofInterval TOKENS (ignored)
    put_uint(&mut blob, 11, snap.proof_timeout_window);
    blob[12 * 32 + 31] = status_byte;
    // w13/w14 zero; w15/w17 are dynamic offsets — head-only decode ignores
    // them, so leaving them zero mirrors "we never chase the tails".
    blob
}

#[test]
fn decode_round_trips_every_a3_field() {
    let snap = passing_snapshot();
    let decoded = decode_session_snapshot(&encode_snapshot_blob(&snap, 0)).expect("decodes");
    assert_eq!(decoded, snap);
}

#[test]
fn decode_fails_closed_on_short_return() {
    // A 17-word head (the trap shape) must ERROR, never mis-decode.
    let snap = passing_snapshot();
    let blob = encode_snapshot_blob(&snap, 0);
    let err = decode_session_snapshot(&blob[..17 * 32]).unwrap_err();
    assert!(err.contains("short"), "{err}");
    assert!(decode_session_snapshot(&[]).is_err());
}

#[test]
fn decode_fails_closed_on_unknown_status() {
    let snap = passing_snapshot();
    let err = decode_session_snapshot(&encode_snapshot_blob(&snap, 7)).unwrap_err();
    assert!(err.contains("unknown session status"), "{err}");
}

// --- the attempt registry: A.3 one-train-ever + C.6 address rules ---

const COOLDOWN: u64 = 60;

#[test]
fn registry_one_train_per_session_ever_after_terminal_reject() {
    // THE row only the node-local record can catch: a terminal reject leaves
    // the chain session Active; a second `train` on it must still reject.
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(7, addr(1), NOW, COOLDOWN), AttemptClaim::Ok);
    reg.finish(7, addr(1), NOW + 10, AttemptOutcome::Rejected);
    assert_eq!(
        reg.try_begin(7, addr(1), NOW + COOLDOWN + 100, COOLDOWN),
        AttemptClaim::SessionReused
    );
}

#[test]
fn registry_one_train_per_session_ever_after_complete() {
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(8, addr(1), NOW, COOLDOWN), AttemptClaim::Ok);
    reg.finish(8, addr(1), NOW + 10, AttemptOutcome::Completed);
    assert_eq!(
        reg.try_begin(8, addr(1), NOW + 20, COOLDOWN),
        AttemptClaim::SessionReused
    );
}

#[test]
fn registry_duplicate_train_mid_run_is_train_active() {
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(9, addr(1), NOW, COOLDOWN), AttemptClaim::Ok);
    assert_eq!(
        reg.try_begin(9, addr(1), NOW + 5, COOLDOWN),
        AttemptClaim::TrainActive
    );
}

#[test]
fn registry_one_job_per_address_at_a_time() {
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(10, addr(1), NOW, COOLDOWN), AttemptClaim::Ok);
    // A DIFFERENT session, same client address, while the first runs.
    assert_eq!(
        reg.try_begin(11, addr(1), NOW + 5, COOLDOWN),
        AttemptClaim::AddressBusy
    );
    // Another address is fine.
    assert_eq!(
        reg.try_begin(12, addr(2), NOW + 5, COOLDOWN),
        AttemptClaim::Ok
    );
}

#[test]
fn registry_cooldown_after_terminal_reject_only() {
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(13, addr(3), NOW, COOLDOWN), AttemptClaim::Ok);
    reg.finish(13, addr(3), NOW + 10, AttemptOutcome::Rejected);
    // Inside the cooldown, a FRESH session for the same address waits.
    assert_eq!(
        reg.try_begin(14, addr(3), NOW + 30, COOLDOWN),
        AttemptClaim::Cooldown
    );
    // After it, fine.
    assert_eq!(
        reg.try_begin(14, addr(3), NOW + 10 + COOLDOWN + 1, COOLDOWN),
        AttemptClaim::Ok
    );
    // A COMPLETED run starts no cooldown.
    let reg2 = AttemptRegistry::new();
    assert_eq!(reg2.try_begin(15, addr(4), NOW, COOLDOWN), AttemptClaim::Ok);
    reg2.finish(15, addr(4), NOW + 10, AttemptOutcome::Completed);
    assert_eq!(
        reg2.try_begin(16, addr(4), NOW + 20, COOLDOWN),
        AttemptClaim::Ok
    );
}

// --- C.6 plausibility ---

#[test]
fn plausibility_rejects_implausible_byte_total() {
    // totalBytes ≤ declaredTokens × 8 is the manifest gate.
    assert_eq!(plausibility_gate(DECLARED * 8, DECLARED), Ok(()));
    match plausibility_gate(DECLARED * 8 + 1, DECLARED) {
        Err(AcceptReject::Plausibility { detail }) => {
            assert!(detail.contains("totalBytes"), "{detail}")
        }
        other => panic!("expected Plausibility, got {other:?}"),
    }
}

// --- A.1 numeric wire rule (null/missing/non-number must FAIL serde) ---

fn valid_job_json() -> serde_json::Value {
    serde_json::json!({
        "templateId": "train-qlora-qwen38-27b-v1",
        "templateHash": "0xabc",
        "dataset": {
            "manifestCID": "uAAA",
            "manifestSha256": "0xdef",
            "declaredTokens": DECLARED,
            "samples": 5000
        },
        "epochs": 2,
        "hyper": {
            "rank": 16, "alpha": 32, "lr": "0.000200",
            "seed": "18446744073709551629", "seqLen": 2048
        },
        "output": "adapter-v1"
    })
}

#[test]
fn wire_rule_valid_job_deserialises() {
    let job: TrainingJob = serde_json::from_value(valid_job_json()).expect("valid job");
    assert_eq!(job.dataset.declared_tokens, DECLARED);
}

#[test]
fn wire_rule_null_and_missing_and_wrong_type_numerics_fail() {
    let mutations: Vec<Box<dyn Fn(&mut serde_json::Value)>> = vec![
        Box::new(|v| v["dataset"]["declaredTokens"] = serde_json::Value::Null),
        Box::new(|v| {
            v["epochs"].take();
            v.as_object_mut().unwrap().remove("epochs");
        }),
        Box::new(|v| v["hyper"]["seqLen"] = serde_json::json!("2048")), // string, not number
        Box::new(|v| v["hyper"]["rank"] = serde_json::Value::Null),
        Box::new(|v| v["dataset"]["samples"] = serde_json::json!(-1)),
    ];
    for (i, mutate) in mutations.iter().enumerate() {
        let mut json = valid_job_json();
        mutate(&mut json);
        assert!(
            serde_json::from_value::<TrainingJob>(json).is_err(),
            "mutation {i} must fail deserialisation (A.1 numeric wire rule)"
        );
    }
}

// --- the LIVE-chain byte fixture (the stale-ABI regression tripwire) ---

#[test]
fn decode_matches_the_pinned_live_chain_return() {
    // Captured from the DEPLOYED contract: sessionJobs(931) on 0xD067…adA4,
    // 2026-08-23. The T3 converge round proved the client-ABI file is STALE
    // (phantom `requester` at w2) — this fixture pins the real layout so the
    // decode and any future "verification" can never drift to it again.
    let hex_text = include_str!("fixtures/sessionjobs_931.hex");
    let raw = hex::decode(hex_text.trim()).expect("fixture is hex");
    let snap = decode_session_snapshot(&raw).expect("live return decodes");
    assert_eq!(
        format!("{:?}", snap.depositor),
        "0x1a84ef2650c4299659f522c1961c6be4bc22cb14"
    );
    assert_eq!(
        format!("{:?}", snap.host),
        "0x4594f755f593b517bb3194f4dec20c48a3f04504"
    );
    // The USDC token address — the semantic anchor that exposed the drift.
    assert_eq!(
        format!("{:?}", snap.payment_token),
        "0x036cbd53842c5426634e7929541ec2318f3dcf7e"
    );
    assert_eq!(snap.deposit, U256::from(695_977u64));
    assert_eq!(snap.price_per_token, U256::from(904u64));
    assert_eq!(snap.tokens_used, U256::from(733_225u64));
    assert_eq!(snap.max_duration, U256::from(3_600u64));
    assert_eq!(snap.start_time, U256::from(1_784_417_870u64));
    assert_eq!(snap.proof_timeout_window, U256::from(300u64));
    assert_eq!(snap.status, SessionStatus::Completed);
}

// --- A.3 boundary pins (round-1: the <↔<= mutations survived without these) ---

#[test]
fn a3_lifetime_boundary_exactly_required_passes_one_less_rejects() {
    // Boundary: remaining lifetime == TRAIN_JOB_TIMEOUT + margin (13,200 s)
    // → start_time == NOW − 1,200 must PASS; one second later must REJECT.
    let mut snap = passing_snapshot();
    snap.start_time = U256::from(NOW - 1_200);
    assert_eq!(
        run(&snap),
        Ok(()),
        "exactly the required lifetime must pass"
    );
    snap.start_time = U256::from(NOW - 1_201);
    expect_session_params(run(&snap), "lifetime");
}

#[test]
fn a3_headroom_boundary_exactly_required_passes_one_more_rejects() {
    // capacity = floor(8,000,000 × 1000 / 904) = 8,849,557; required
    // 8,678,400 → boundary tokensUsed = 171,157 passes; 171,158 rejects.
    let mut snap = passing_snapshot();
    snap.tokens_used = U256::from(171_157u64);
    assert_eq!(run(&snap), Ok(()), "exactly-sufficient headroom must pass");
    snap.tokens_used = U256::from(171_158u64);
    expect_session_params(run(&snap), "headroom");
}

// --- round-2 cooldown/exactly-once rules ---

#[test]
fn premature_retry_does_not_extend_the_cooldown() {
    // Round-2 catch: the unconditional stamp let a premature retry push its
    // own window forward forever. A Cooldown-refused attempt consumes its
    // session but must NOT re-arm the clock.
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(200, addr(7), NOW, COOLDOWN), AttemptClaim::Ok);
    reg.finish(200, addr(7), NOW, AttemptOutcome::Rejected); // arms → NOW+60
                                                             // Premature retry at NOW+30: refused; recorded WITHOUT re-arming.
    assert_eq!(
        reg.try_begin(201, addr(7), NOW + 30, COOLDOWN),
        AttemptClaim::Cooldown
    );
    assert!(reg.record_terminal_reject(201, addr(7), NOW + 30, false));
    // At NOW+61 the ORIGINAL window has elapsed — a fresh session proceeds
    // (a re-armed clock would block until NOW+90).
    assert_eq!(
        reg.try_begin(202, addr(7), NOW + 61, COOLDOWN),
        AttemptClaim::Ok
    );
}

#[test]
fn address_busy_refusals_do_not_poison_the_post_completion_window() {
    // Round-2 catch: AddressBusy stamps during a legit run blocked the
    // depositor's next honest session after a SUCCESSFUL completion.
    let reg = AttemptRegistry::new();
    assert_eq!(reg.try_begin(210, addr(8), NOW, COOLDOWN), AttemptClaim::Ok);
    // Same-depositor attempts while the run is active: busy, recorded
    // WITHOUT arming (capacity back-off, not an offence).
    assert_eq!(
        reg.try_begin(211, addr(8), NOW + 10, COOLDOWN),
        AttemptClaim::AddressBusy
    );
    assert!(reg.record_terminal_reject(211, addr(8), NOW + 10, false));
    // The run completes SUCCESSFULLY — no cooldown may block the next one.
    reg.finish(210, addr(8), NOW + 50, AttemptOutcome::Completed);
    assert_eq!(
        reg.try_begin(212, addr(8), NOW + 51, COOLDOWN),
        AttemptClaim::Ok
    );
}

#[test]
fn record_terminal_reject_is_the_exactly_once_key() {
    // Round-2 catch: concurrent consult failures double-scheduled the settle.
    // Only the FIRST terminal record answers true.
    let reg = AttemptRegistry::new();
    assert!(reg.record_terminal_reject(220, addr(9), NOW, true));
    assert!(!reg.record_terminal_reject(220, addr(9), NOW + 1, true));
    // And an Active claim closed by it also counts as the first record…
    assert_eq!(
        reg.try_begin(221, addr(9), NOW + 100, COOLDOWN),
        AttemptClaim::Ok
    );
    assert!(reg.record_terminal_reject(221, addr(9), NOW + 101, true));
    assert!(!reg.record_terminal_reject(221, addr(9), NOW + 102, true));
}
