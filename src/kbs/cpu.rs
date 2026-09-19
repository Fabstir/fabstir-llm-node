// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The CPU half: the TDX quote through dcap-qvl 0.6.3 (design §8, gate A-9).
//!
//! - [`decode_unverified`] parses the quote for the pre-filter and the simulator
//!   mode (`Quote::parse`, not the un-re-exported SCALE `decode`); an SGX quote
//!   PARSES fine and is refused on the "not a TDX quote" row (`as_td10() == None`).
//! - [`verify_real`] runs `CollateralClient::fetch` through a [`MemoHttp`] instance
//!   and `dcap_qvl::verify::verify`, with the §12 two-pass memo rule: a committed
//!   memo is never deleted; the network-only pass replaces it only on verified
//!   success; the stage is dropped at the mode switch.
//!
//! What `verify` enforces (no need to re-check): `new_prod()` with
//! `allow_debug=false`, `allow_service_td=false`, `SEPT_VE_DISABLE` required,
//! TD1.5 `mr_service_td == 0`, root CRL first, TCB info / QE identity
//! `issueDate`/`nextUpdate` hard-bailed, PCK CRL `nextUpdate`, cert validity. A
//! debug TD is refused INSIDE dcap-qvl in real mode; the broker's `td_debug` row is
//! decisive only in simulator mode.

use crate::kbs::error::{KbsError, Kind};
use crate::kbs::memo::{MemoHttp, Mode};
use dcap_qvl::collateral::CollateralClient;
use dcap_qvl::configs::DefaultConfig;
use dcap_qvl::quote::Quote;
use dcap_qvl::QuoteCollateralV3;

/// The status string the simulator mode reports (the test policy allow-lists it).
pub const SIMULATOR_STATUS: &str = "Simulator";

/// What the broker decides on (design D16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TdxEvidence {
    pub mr_td: [u8; 48],
    pub rt_mr0: [u8; 48],
    pub rt_mr1: [u8; 48],
    pub rt_mr2: [u8; 48],
    pub rt_mr3: [u8; 48],
    pub report_data: [u8; 64],
    /// TUD.DEBUG (`td_attributes[0] & 1`).
    pub td_debug: bool,
    pub tcb_status: String,
    pub advisory_ids: Vec<String>,
}

/// The pre-filter's view: registers and `report_data` off the UNVERIFIED quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnverifiedTd {
    pub mr_td: [u8; 48],
    pub rt_mr0: [u8; 48],
    pub rt_mr1: [u8; 48],
    pub rt_mr2: [u8; 48],
    pub rt_mr3: [u8; 48],
    pub report_data: [u8; 64],
    pub td_debug: bool,
}

/// Which pass produced the outcome, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyTrace {
    pub passes: u8,
    pub second_pass_mode: Option<Mode>,
    pub committed: usize,
}

/// dcap-qvl's `PCK_ID_PCK_CERT_CHAIN`: the quote embeds its PCK certificate chain.
const CERT_TYPE_PCK_CHAIN: u16 = 5;

/// Parse the quote; refuse a non-TDX report and a certification data type other
/// than an embedded PCK chain (dstack quotes always embed it; types 2/3 would send
/// dcap-qvl to the PCCS `pckcert` lookup, whose failures cannot be told from the
/// quote's own, so the class is refused here before any egress).
pub fn decode_unverified(raw: &[u8]) -> Result<UnverifiedTd, KbsError> {
    let q = Quote::parse(raw)
        .map_err(|e| KbsError::verification(format!("quote does not parse: {e}")))?;
    let td = q
        .report
        .as_td10()
        .ok_or_else(|| KbsError::verification("not a TDX quote"))?;
    let cert_type = q.inner_cert_type();
    if cert_type != CERT_TYPE_PCK_CHAIN {
        return Err(KbsError::verification(format!(
            "quote certification data type {cert_type}: only an embedded PCK chain (type 5) is served"
        )));
    }
    Ok(UnverifiedTd {
        mr_td: td.mr_td,
        rt_mr0: td.rt_mr0,
        rt_mr1: td.rt_mr1,
        rt_mr2: td.rt_mr2,
        rt_mr3: td.rt_mr3,
        report_data: td.report_data,
        td_debug: td.td_attributes[0] & 1 == 1,
    })
}

/// Simulator mode (test keyring only): parse only, no signature or collateral check.
pub fn simulator(raw: &[u8]) -> Result<TdxEvidence, KbsError> {
    let u = decode_unverified(raw)?;
    Ok(TdxEvidence {
        mr_td: u.mr_td,
        rt_mr0: u.rt_mr0,
        rt_mr1: u.rt_mr1,
        rt_mr2: u.rt_mr2,
        rt_mr3: u.rt_mr3,
        report_data: u.report_data,
        td_debug: u.td_debug,
        tcb_status: SIMULATOR_STATUS.to_string(),
        advisory_ids: Vec::new(),
    })
}

/// One fetch + verify pass through `memo` in its current mode. `Err` is either a
/// fetch failure (`unavailable`, unless it was a 2xx that did not parse, which is
/// still the upstream's fault) or a verify failure (`verification`, carrying
/// dcap-qvl's message verbatim).
pub async fn one_pass(
    raw: &[u8],
    memo: &MemoHttp,
    pccs_url: &str,
    now_secs: u64,
) -> Result<TdxEvidence, KbsError> {
    let client = CollateralClient::<DefaultConfig, MemoHttp>::new(memo.clone(), pccs_url);
    let collateral: QuoteCollateralV3 = match client.fetch(raw).await {
        Ok(c) => c,
        Err(e) => return Err(classify_fetch_error(&memo.summary(), &e)),
    };
    verify_with(raw, &collateral, now_secs)
}

/// A DATED `verification` refusal is about the collateral, never the quote: the
/// node must retry through it (`unavailable`), not park on it.
fn relabel_dated_as_outage(e: KbsError, why: &str) -> KbsError {
    if e.kind == Kind::Verification && is_dated_refusal(&e.detail) {
        return KbsError::unavailable_egress(format!("{why}: {}", e.detail));
    }
    e
}

/// dcap-qvl 0.6.3's date refusals: TCB info / QE identity `nextUpdate` ("… expired"),
/// CRL expiry (webpki `CrlExpired`), certificate validity. Matched on the message
/// because `verify` returns one error type; an unknown message stays a refusal
/// (fail-closed).
pub fn is_dated_refusal(detail: &str) -> bool {
    let d = detail.to_ascii_lowercase();
    // Collateral-about messages only: TCB info / QE identity (dcap-qvl `bail!`s) and
    // CRL expiry (webpki `CrlExpired`). A generic "expired" would also match the
    // QUOTE's own embedded PCK certificate chain (webpki `CertExpired`), which is the
    // quote's fault and must stay a 403.
    d.contains("tcbinfo expired")
        || d.contains("tcbinfo issue date is in the future")
        || d.contains("qe identity expired")
        || d.contains("qe identity issue date is in the future")
        || d.contains("crlexpired")
        || d.contains("crl expired")
}

/// A `CollateralClient::fetch` failure is the quote's fault when it happened BEFORE
/// any request (PCK chain / FMSPC extraction from the embedded chain): bad evidence,
/// not an upstream outage. Anything after a request is `unavailable`. (The `pckcert`
/// lookup dcap-qvl makes for cert data types 2/3 is never reached: those types are
/// refused by [`decode_unverified`] before any egress.)
pub fn classify_fetch_error(s: &crate::kbs::memo::SessionSummary, e: &anyhow::Error) -> KbsError {
    if !s.any_request {
        return KbsError::verification(format!("quote: {e:#}"));
    }
    KbsError::unavailable_egress(format!("collateral: {e:#}"))
}

/// `dcap_qvl::verify::verify` over already-fetched collateral (also the offline test path).
pub fn verify_with(
    raw: &[u8],
    collateral: &QuoteCollateralV3,
    now_secs: u64,
) -> Result<TdxEvidence, KbsError> {
    let report = dcap_qvl::verify::verify(raw, collateral, now_secs)
        .map_err(|e| KbsError::verification(format!("tdx: {e:#}")))?;
    let td = report
        .report
        .as_td10()
        .ok_or_else(|| KbsError::verification("not a TDX quote"))?;
    Ok(TdxEvidence {
        mr_td: td.mr_td,
        rt_mr0: td.rt_mr0,
        rt_mr1: td.rt_mr1,
        rt_mr2: td.rt_mr2,
        rt_mr3: td.rt_mr3,
        report_data: td.report_data,
        td_debug: td.td_attributes[0] & 1 == 1,
        tcb_status: report.status,
        advisory_ids: report.advisory_ids,
    })
}

/// Real mode with the §12 two-pass rule. `memo` must be a fresh collateral
/// instance for this request (its stage is committed here on success).
pub async fn verify_real(
    raw: &[u8],
    memo: &MemoHttp,
    pccs_url: &str,
    now_secs: u64,
) -> Result<(TdxEvidence, VerifyTrace), KbsError> {
    // The prefilter ran this already; repeated here so no caller can send a quote
    // without an embedded PCK chain to the PCCS `pckcert` lookup.
    decode_unverified(raw)?;
    memo.set_mode(Mode::Normal);
    match one_pass(raw, memo, pccs_url, now_secs).await {
        Ok(ev) => {
            let committed = memo.commit();
            return Ok((
                ev,
                VerifyTrace {
                    passes: 1,
                    second_pass_mode: None,
                    committed,
                },
            ));
        }
        Err(e1) => {
            let s = memo.summary();
            // Memo inputs involved → the memo may be stale: refetch. Network inputs
            // unusable (a fetch failure: garbage 2xx, missing header) with a committed
            // memo for every URL → serve the memo. NEVER after `verify` refused a set
            // of freshly fetched bodies: that is Intel revoking or downgrading this
            // platform, and an older memo must not out-vote it.
            // Order matters: a FETCH failure with a committed memo for every URL
            // takes the memo even when some other URL was memo-served (a network-only
            // retry would meet the same garbage); only then does a memo input on a
            // VERIFY refusal earn the network-only refetch (a fetch outage beside memo
            // inputs is refetched by nobody: the network-only pass would meet the same
            // outage and cost another timeout per URL under the wall). A forged quote
            // on a fresh memo still costs one refetch per request (accepted, design
            // §5/D6).
            // A pass that ran on the memo ALONE (every URL fresh) and still failed at
            // fetch had nothing but memo bodies to choke on: a decodable entry the
            // consumer cannot parse would otherwise pin a 502 for the whole fresh
            // window, since a memo-only retry serves the same bytes. Only a verified
            // network pass can replace it.
            let memo_alone_unusable =
                e1.kind == Kind::Unavailable && s.from_memo && !s.attempted_network;
            let second = if memo_alone_unusable {
                Some(Mode::NetworkOnly)
            } else if e1.kind == Kind::Unavailable
                && s.all_touched_have_memo
                && !s.failed_without_memo
            {
                Some(Mode::MemoOnly)
            } else if s.from_memo && e1.kind == Kind::Verification {
                Some(Mode::NetworkOnly)
            } else {
                None
            };
            let Some(mode) = second else {
                // No retry: a DATED refusal of freshly fetched bodies is the PCCS
                // lagging Intel (an outage the node retries through), not the quote.
                return Err(relabel_dated_as_outage(e1, "fresh collateral out of date"));
            };
            tracing::warn!(first = %e1, ?mode, "collateral pass failed; retrying once in the other mode");
            memo.set_mode(mode); // drops pass 1's stage
            match one_pass(raw, memo, pccs_url, now_secs).await {
                Ok(ev) => {
                    // The network-only pass replaces the memo only now, on verified
                    // success; a memo-only pass has nothing staged.
                    let committed = memo.commit();
                    Ok((
                        ev,
                        VerifyTrace {
                            passes: 2,
                            second_pass_mode: Some(mode),
                            committed,
                        },
                    ))
                }
                Err(e2) => {
                    tracing::warn!(first = %e1, second = %e2, "both collateral passes failed");
                    if mode == Mode::NetworkOnly
                        && e1.kind == Kind::Verification
                        && !is_dated_refusal(&e1.detail)
                        && e2.kind == Kind::Unavailable
                    {
                        // The memo refused the quote for the quote's own fault; the
                        // network being down on the refetch does not turn that into
                        // an outage the node should retry.
                        return Err(e1);
                    }
                    if mode == Mode::NetworkOnly {
                        // The refetched bodies are dated too: the PCCS lags Intel.
                        return Err(relabel_dated_as_outage(
                            e2,
                            "refetched collateral out of date",
                        ));
                    }
                    if mode == Mode::MemoOnly
                        && e2.kind == Kind::Verification
                        && is_dated_refusal(&e2.detail)
                    {
                        // The network gave nothing usable and the memo is past its
                        // dates: an outage, not a refusal of the evidence. Any OTHER
                        // verify refusal on the memo (signature, debug TD, registers)
                        // is the quote's fault and stays a 403, outage or not.
                        return Err(KbsError::unavailable_egress(format!(
                            "collateral unusable ({}); memo refused ({})",
                            e1.detail, e2.detail
                        )));
                    }
                    Err(e2)
                }
            }
        }
    }
}
