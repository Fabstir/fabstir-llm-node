//! Design §5 / gate A-8: issue/take/expiry/replay/binding/caps with an injected `now`.

use fabstir_llm_node::kbs::nonce::{CapScope, NonceError, NonceStore};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(300);
const M1: [u8; 32] = [1u8; 32];
const M2: [u8; 32] = [2u8; 32];
const K1: [u8; 33] = [3u8; 33];
const K2: [u8; 33] = [4u8; 33];

#[test]
fn issue_take_bind_happy_path_and_replay() {
    let s = NonceStore::new(TTL, 100, 10);
    let t0 = Instant::now();
    let n = s.issue(M1, K1, "10.0.0.1", t0).unwrap();
    assert_eq!(s.outstanding(), 1);
    let issued = s.take(&n, t0 + Duration::from_secs(1)).unwrap();
    assert!(issued.bind(&M1, &K1).is_ok());
    assert_eq!(issued.source, "10.0.0.1");
    assert_eq!(s.outstanding(), 0);
    // one-time use: a second take is Unknown, whatever the binding
    assert_eq!(
        s.take(&n, t0 + Duration::from_secs(2)).unwrap_err(),
        NonceError::Unknown
    );
}

#[test]
fn unissued_is_unknown() {
    let s = NonceStore::new(TTL, 100, 10);
    assert_eq!(
        s.take(&[9u8; 32], Instant::now()).unwrap_err(),
        NonceError::Unknown
    );
}

#[test]
fn expiry_is_decided_by_the_injected_clock_not_the_wall_clock() {
    let s = NonceStore::new(TTL, 100, 10);
    let t0 = Instant::now();
    let a = s.issue(M1, K1, "a", t0).unwrap();
    let b = s.issue(M1, K1, "a", t0).unwrap();
    // mutation guard: real time has advanced microseconds, so comparing against
    // Instant::now() instead of the argument would return Some here.
    assert_eq!(
        s.take(&a, t0 + TTL + Duration::from_secs(1)).unwrap_err(),
        NonceError::Expired
    );
    assert!(s.take(&b, t0 + TTL - Duration::from_secs(1)).is_ok());
}

#[test]
fn binding_refuses_other_key_and_other_model() {
    let s = NonceStore::new(TTL, 100, 10);
    let t0 = Instant::now();
    let n = s.issue(M1, K1, "a", t0).unwrap();
    let issued = s.take(&n, t0).unwrap();
    assert_eq!(
        issued.bind(&M1, &K2).unwrap_err(),
        NonceError::BoundToAnotherKey
    );
    assert_eq!(
        issued.bind(&M2, &K1).unwrap_err(),
        NonceError::BoundToAnotherModel
    );
    // model is checked first when both differ
    assert_eq!(
        issued.bind(&M2, &K2).unwrap_err(),
        NonceError::BoundToAnotherModel
    );
}

#[test]
fn global_cap_refuses_and_never_evicts() {
    let s = NonceStore::new(TTL, 3, 3);
    let t0 = Instant::now();
    let first = s.issue(M1, K1, "a", t0).unwrap();
    s.issue(M1, K1, "a", t0).unwrap();
    s.issue(M1, K1, "a", t0).unwrap();
    assert_eq!(
        s.issue(M1, K1, "b", t0).unwrap_err(),
        NonceError::TooManyOutstanding(CapScope::Global)
    );
    // the first nonce is still there (no eviction)
    assert!(s.take(&first, t0).is_ok());
    // and its slot is free again
    assert!(s.issue(M1, K1, "b", t0).is_ok());
}

#[test]
fn per_source_cap_is_independent_of_other_sources() {
    let s = NonceStore::new(TTL, 100, 2);
    let t0 = Instant::now();
    s.issue(M1, K1, "a", t0).unwrap();
    let n = s.issue(M1, K1, "a", t0).unwrap();
    assert_eq!(
        s.issue(M1, K1, "a", t0).unwrap_err(),
        NonceError::TooManyOutstanding(CapScope::Source)
    );
    assert!(s.issue(M1, K1, "b", t0).is_ok(), "another source proceeds");
    // burning one of a's frees a's slot
    s.take(&n, t0).unwrap();
    assert!(s.issue(M1, K1, "a", t0).is_ok());
}

#[test]
fn a_cap_that_would_refuse_sweeps_first_and_periodic_sweeps_keep_the_store_bounded() {
    let s = NonceStore::new(TTL, 1000, 1000);
    let t0 = Instant::now();
    for _ in 0..600 {
        s.issue(M1, K1, "a", t0).unwrap();
    }
    // all expired now; the next 300 issues (under the periodic threshold) must not
    // be refused by stale entries once a cap would refuse
    let later = t0 + TTL + Duration::from_secs(1);
    for _ in 0..600 {
        s.issue(M1, K1, "a", later).unwrap();
    }
    assert!(s.outstanding() <= 1000);
    for _ in 0..600 {
        s.issue(M1, K1, "a", later + TTL + Duration::from_secs(1))
            .unwrap();
    }
    assert!(
        s.outstanding() < 1000,
        "periodic sweeps keep the store bounded: {}",
        s.outstanding()
    );
}

#[test]
fn a_source_parked_at_its_cap_cannot_force_a_sweep_per_challenge() {
    // Refusals inside one second of the last sweep do not sweep again; after a second
    // they do (and free the expired budget).
    let s = NonceStore::new(TTL, 100, 2);
    let t0 = Instant::now();
    s.issue(M1, K1, "a", t0).unwrap();
    s.issue(M1, K1, "a", t0).unwrap();
    let t1 = t0 + TTL + Duration::from_millis(500); // entries expired, but < 1 s since the last sweep? no sweep ran yet at t0 (first issues)
                                                    // the first refusal at t1 sweeps (last_sweep None) and frees the budget
    assert!(s.issue(M1, K1, "a", t1).is_ok());
    // fill again, then refusals within the same second do not re-sweep
    s.issue(M1, K1, "a", t1).unwrap();
    let t2 = t1 + TTL + Duration::from_millis(100);
    // entries expired again, and the last sweep was at t1: t2 - t1 > 1 s → sweeps → ok
    assert!(s.issue(M1, K1, "a", t2).is_ok());
    s.issue(M1, K1, "a", t2).unwrap();
    // within 100 ms of the sweep at t2 a per-source refusal does not walk the map
    assert!(s
        .issue(M1, K1, "a", t2 + Duration::from_millis(50))
        .is_err());
    // after 100 ms it does, and the expired budget is freed for the crash-looping node
    assert!(s
        .issue(M1, K1, "a", t2 + TTL + Duration::from_millis(200))
        .is_ok());
}

#[test]
fn sweep_frees_expired_entries_and_their_source_budget() {
    let s = NonceStore::new(TTL, 2, 2);
    let t0 = Instant::now();
    s.issue(M1, K1, "a", t0).unwrap();
    s.issue(M1, K1, "a", t0).unwrap();
    assert!(s.issue(M1, K1, "a", t0).is_err());
    // after the TTL, an insert sweeps both and both caps are free again
    let later = t0 + TTL + Duration::from_secs(1);
    assert!(s.issue(M1, K1, "a", later).is_ok());
    assert_eq!(s.outstanding(), 1);
}
