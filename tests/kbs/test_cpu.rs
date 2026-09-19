//! Design §8 / A-9 prep on REAL bytes, offline: dcap-qvl's own sample quote + collateral.
//! `now` values per the fixture README (the good pair is `UpToDate` only before the
//! PCK CRL's `nextUpdate` 2025-07-19T10:00:35Z; the outdated pair never yields a
//! status, only error arms).

use super::fixtures::fixture;
use dcap_qvl::QuoteCollateralV3;
use fabstir_llm_node::kbs::cpu::{decode_unverified, simulator, verify_with, SIMULATOR_STATUS};
use fabstir_llm_node::kbs::error::Kind;

pub const GOOD_NOW: u64 = 1_751_328_000; // 2025-07-01T00:00:00Z
pub const OUTDATED_WINDOW_NOW: u64 = 1_774_396_800; // 2026-03-25T00:00:00Z

pub fn good_pair() -> (Vec<u8>, QuoteCollateralV3) {
    (
        fixture("dcap-tdx_quote.bin"),
        serde_json::from_slice(&fixture("dcap-tdx_quote_collateral.json")).unwrap(),
    )
}

#[test]
fn good_pair_verifies_up_to_date_and_decodes_fields() {
    let (q, c) = good_pair();
    let ev = verify_with(&q, &c, GOOD_NOW).unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert!(ev.advisory_ids.is_empty());
    assert!(!ev.td_debug);
    // the verified registers equal the unverified decode's
    let u = decode_unverified(&q).unwrap();
    assert_eq!(u.mr_td, ev.mr_td);
    assert_eq!(u.rt_mr3, ev.rt_mr3);
    assert_eq!(u.report_data, ev.report_data);
}

#[test]
fn one_flipped_quote_byte_fails_the_signature() {
    let (mut q, c) = good_pair();
    // inside the TD report body (past the 48-byte header), not the cert chain
    q[100] ^= 1;
    let e = verify_with(&q, &c, GOOD_NOW).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(e.detail.contains("signature"), "{e}");
}

#[test]
fn good_pair_past_its_pck_crl_next_update_is_refused_dated() {
    let (q, c) = good_pair();
    // 2025-07-19T10:00:35Z + 1 day
    let e = verify_with(&q, &c, 1_753_005_635).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(
        e.detail.to_lowercase().contains("expire") || e.detail.contains("Crl"),
        "{e}"
    );
}

#[test]
fn outdated_pair_propagates_tcbinfo_expired_verbatim() {
    let q = fixture("dcap-tdx_quote_outdated.bin");
    let c: QuoteCollateralV3 =
        serde_json::from_slice(&fixture("dcap-tdx_quote_outdated_collateral.json")).unwrap();
    let e = verify_with(&q, &c, OUTDATED_WINDOW_NOW).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(
        e.detail.contains("TCBInfo expired"),
        "the broker must not swallow dcap-qvl's message: {e}"
    );
}

#[test]
fn a_quote_without_an_embedded_pck_chain_is_refused_before_any_egress() {
    // Cert data types 2/3 send dcap-qvl to the PCCS `pckcert` lookup, whose failures
    // cannot be told from the quote's own; the class is refused at decode. The type
    // sits 6 bytes before the embedded chain (u16 LE type, u32 LE size, PEM).
    use fabstir_llm_node::kbs::cpu::decode_unverified;
    let (q, _) = good_pair();
    assert!(decode_unverified(&q).is_ok());
    let pem = q
        .windows(27)
        .position(|w| w == b"-----BEGIN CERTIFICATE-----")
        .expect("embedded chain");
    assert_eq!(q[pem - 6..pem - 4], [5, 0], "type 5 precedes the chain");
    let mut t3 = q.clone();
    t3[pem - 6] = 3;
    let e = decode_unverified(&t3).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(e.detail.contains("certification data type 3"), "{e}");
}

/// The good quote with its certification data type rewritten to 3 (encrypted PPID).
pub fn type3_quote() -> Vec<u8> {
    let (q, _) = good_pair();
    let pem = q
        .windows(27)
        .position(|w| w == b"-----BEGIN CERTIFICATE-----")
        .expect("embedded chain");
    let mut t3 = q.clone();
    t3[pem - 6] = 3;
    t3
}

#[test]
fn sgx_quote_parses_but_is_not_a_tdx_quote() {
    let q = fixture("dcap-sgx_quote.bin");
    let e = decode_unverified(&q).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert_eq!(e.detail, "not a TDX quote");
    let e = simulator(&q).unwrap_err();
    assert_eq!(e.detail, "not a TDX quote");
}

#[test]
fn garbage_does_not_parse() {
    let e = decode_unverified(b"not a quote").unwrap_err();
    assert!(e.detail.contains("does not parse"), "{e}");
}

#[test]
fn simulator_mode_decodes_the_recording() {
    let q = fixture("dstack-0.5.9-simulator-quote.bin");
    let ev = simulator(&q).unwrap();
    assert_eq!(ev.tcb_status, SIMULATOR_STATUS);
    assert!(ev.advisory_ids.is_empty());
    assert!(!ev.td_debug);
    assert_eq!(
        ev.report_data, [0u8; 64],
        "the recording's report_data is zero; the simulator patches it"
    );
    assert_eq!(&hex::encode(ev.rt_mr3)[..8], "f28a1490");
}
