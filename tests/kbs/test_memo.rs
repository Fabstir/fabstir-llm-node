//! Design §12 / A-15 / A-16 through `cpu::verify_real` against a fake PCCS serving
//! the vendored collateral: commit only after `verify` Ok, never delete, replace on
//! verified success, stage dropped at the mode switch, memo-only and network-only
//! retries, allow-list before any connection.

use super::fixtures::fixture;
use super::harness::{egress_for, pccs_handler, pki, spawn_fake, Fake, HOST};
use super::test_cpu::{good_pair, GOOD_NOW};
use dcap_qvl::QuoteCollateralV3;
use fabstir_llm_node::kbs::cpu::verify_real;
use fabstir_llm_node::kbs::egress::{EgressClient, EgressError, EgressOptions};
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::kbs::memo::{memo_path, MemoEntry, MemoHttp, Mode};
use std::path::{Path, PathBuf};
use std::time::Duration;

const FRESH: Duration = Duration::from_secs(86_400);
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX: usize = 4 * 1_048_576;

fn collateral() -> QuoteCollateralV3 {
    good_pair().1
}

async fn fake_pccs(
    overrides: Vec<(&'static str, (u16, Vec<(String, String)>, Vec<u8>))>,
) -> (super::harness::TestPki, Fake) {
    let p = pki(HOST);
    let f = spawn_fake(&p, pccs_handler(collateral(), overrides), Duration::ZERO).await;
    (p, f)
}

fn memo(egress: &EgressClient, dir: &Path) -> MemoHttp {
    MemoHttp::collateral(egress.clone(), dir.to_path_buf(), FRESH, TIMEOUT, MAX)
}

fn memo_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn read_entry(path: &Path) -> MemoEntry {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn tcb_url(f: &Fake) -> String {
    // dcap-qvl's URL for this quote's FMSPC; recovered from the fake's hit list.
    let hits = f.hits.lock().unwrap();
    let pq = hits
        .iter()
        .find(|(_, p)| p.contains("/tcb?"))
        .expect("a tcb hit")
        .1
        .clone();
    format!("{}{}", f.base(), pq)
}

#[tokio::test]
async fn success_commits_after_verify_and_a_fresh_memo_serves_without_network() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();

    let m = memo(&eg, dir.path());
    let (ev, trace) = verify_real(&q, &m, &f.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(
        (trace.passes, trace.second_pass_mode, trace.committed),
        (1, None, 4)
    );
    assert_eq!(f.hit_count(), 4, "pckcrl, tcb, qe/identity, rootcacrl");
    assert_eq!(memo_files(dir.path()).len(), 4);
    let before: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();

    // second request: every URL served from the fresh memo, nothing staged, no hits
    let m2 = memo(&eg, dir.path());
    let (_, trace2) = verify_real(&q, &m2, &f.base(), GOOD_NOW).await.unwrap();
    assert_eq!(f.hit_count(), 4, "no network traffic on a fresh memo");
    assert_eq!(trace2.committed, 0);
    let after: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();
    assert_eq!(before, after, "fetched_at unchanged");
}

#[tokio::test]
async fn a_garbage_pck_crl_passes_fetch_but_is_never_committed() {
    // dcap-qvl stores the PCK CRL raw: `fetch` returns Ok, `verify` fails on DER.
    let (p, f) = fake_pccs(vec![(
        "/pckcrl",
        (
            200,
            vec![("SGX-PCK-CRL-Issuer-Chain".into(), "x".into())],
            b"garbage".to_vec(),
        ),
    )])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    let m = memo(&eg, dir.path());
    let e = verify_real(&q, &m, &f.base(), GOOD_NOW).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification, "{e}");
    assert!(
        memo_files(dir.path()).is_empty(),
        "commit-after-fetch would have memoed the garbage"
    );
}

#[tokio::test]
async fn a_503_with_a_committed_memo_is_served_from_memo() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();

    // now the PCCS answers 503 on tcb; the memo is stale enough to be re-asked
    let (p2, f2) = fake_pccs(vec![("/tcb?", (503, vec![], b"down".to_vec()))]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    // rewrite the memo URLs to the new fake's port (same host, new port)
    repoint_memo(dir.path(), &f.base(), &f2.base());
    let m = MemoHttp::collateral(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let (ev, trace) = verify_real(&q, &m, &f2.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(trace.passes, 1);
    assert_eq!(f2.hits_matching("/tcb?"), 1, "the network was tried first");
}

#[tokio::test]
async fn a_transport_error_with_a_memo_is_served_and_without_one_is_unavailable() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();

    // an unbound port: connection refused
    let dead: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let eg_dead = egress_for(&p, dead, &[]);
    let base_dead = format!("https://{HOST}:1");
    repoint_memo(dir.path(), &f.base(), &base_dead);
    let m = MemoHttp::collateral(
        eg_dead.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let (ev, _) = verify_real(&q, &m, &base_dead, GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");

    let empty = tempfile::tempdir().unwrap();
    let m = memo(&eg_dead, empty.path());
    let e = verify_real(&q, &m, &base_dead, GOOD_NOW).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    assert!(memo_files(empty.path()).is_empty());
}

#[tokio::test]
async fn a_non_2xx_is_never_memoed() {
    let (p, f) = fake_pccs(vec![("/tcb?", (503, vec![], b"down".to_vec()))]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    let e = verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap_err();
    assert_eq!(e.kind, Kind::Unavailable);
    assert!(memo_files(dir.path()).is_empty());
}

#[tokio::test]
async fn an_expired_memo_is_replaced_by_a_verified_network_only_pass() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let url = tcb_url(&f);
    let path = memo_path(dir.path(), &url);
    let mut entry = read_entry(&path);
    // Edit the memoed TCB info's nextUpdate into the past (dates are checked before
    // the signature, so this fails dated, not on the signature).
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD
        .decode(&entry.body_b64)
        .unwrap();
    let edited = String::from_utf8(body)
        .unwrap()
        .replace("2025-07-19T10:16:03Z", "2025-06-30T00:00:00Z");
    assert!(
        edited.contains("2025-06-30"),
        "the fixture's TCB nextUpdate was found"
    );
    entry.body_b64 = base64::engine::general_purpose::STANDARD.encode(edited);
    entry.fetched_at -= 10; // make the replacement visible
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    let old_fetched_at = entry.fetched_at;
    let hits_before = f.hit_count();

    let m = memo(&eg, dir.path());
    let (ev, trace) = verify_real(&q, &m, &f.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(
        (trace.passes, trace.second_pass_mode),
        (2, Some(Mode::NetworkOnly))
    );
    assert_eq!(f.hit_count() - hits_before, 4, "one network-only round");
    let replaced = read_entry(&path);
    assert!(
        replaced.fetched_at > old_fetched_at,
        "entry replaced, fetched_at advanced"
    );
    assert!(!String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(&replaced.body_b64)
            .unwrap()
    )
    .unwrap()
    .contains("2025-06-30"));
}

#[tokio::test]
async fn a_forged_quote_cannot_empty_the_memo() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (mut q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let before: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();
    let hits_before = f.hit_count();

    q[100] ^= 1; // the forged quote: same registers, bad signature
    let m = memo(&eg, dir.path());
    let e = verify_real(&q, &m, &f.base(), GOOD_NOW).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert_eq!(
        f.hit_count() - hits_before,
        4,
        "exactly one network-only round"
    );
    let after: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();
    assert_eq!(
        before, after,
        "every memo entry intact with its old fetched_at (evict-on-error would have emptied it)"
    );
}

#[tokio::test]
async fn a_maintenance_page_with_committed_memos_takes_the_memo_only_retry() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let before: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();

    // every path now answers 200 with an HTML page; the memo is past its fresh window
    let page = (
        200,
        vec![("content-type".to_string(), "text/html".to_string())],
        b"<html>maintenance</html>".to_vec(),
    );
    let (p2, f2) = fake_pccs(vec![("/", page)]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    repoint_memo(dir.path(), &f.base(), &f2.base());
    let m = MemoHttp::collateral(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let (ev, trace) = verify_real(&q, &m, &f2.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(
        (trace.passes, trace.second_pass_mode, trace.committed),
        (2, Some(Mode::MemoOnly), 0)
    );
    let after: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();
    let before_repointed: Vec<MemoEntry> = before
        .into_iter()
        .map(|mut e| {
            e.url = e.url.replace(&f.base(), &f2.base());
            e
        })
        .collect();
    assert_eq!(after.len(), 4);
    for e in &after {
        assert!(
            !e.body_b64.contains("PGh0bWw"),
            "the maintenance page never entered the memo"
        );
        let orig = before_repointed
            .iter()
            .find(|b| b.url == e.url)
            .expect("same url set");
        assert_eq!(orig.fetched_at, e.fetched_at, "old entries untouched");
    }
}

#[tokio::test]
async fn a_verify_refusal_on_fresh_network_collateral_never_falls_back_to_the_memo() {
    // Commit the good collateral, then let the PCCS serve a DIFFERENT valid set (the
    // outdated pair's, for another FMSPC): the fresh bodies refuse this quote. An older
    // memo must not out-vote Intel's current answer (a revocation looks exactly like this).
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let before: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();

    let other: QuoteCollateralV3 =
        serde_json::from_slice(&fixture("dcap-tdx_quote_outdated_collateral.json")).unwrap();
    let p2 = pki(HOST);
    let f2 = spawn_fake(&p2, pccs_handler(other, vec![]), Duration::ZERO).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    repoint_memo(dir.path(), &f.base(), &f2.base());
    // past the fresh window: network first. `now` inside the other set's own dates so the
    // refusal is NOT a dated one (a dated refusal of fresh bodies is a PCCS lag = outage):
    // the fresh set refuses this quote on its content, as a revocation would.
    const INSIDE_OTHER_WINDOW: u64 = 1_772_323_200; // 2026-03-01T00:00:00Z
    let m = MemoHttp::collateral(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let e = verify_real(&q, &m, &f2.base(), INSIDE_OTHER_WINDOW)
        .await
        .unwrap_err();
    assert_eq!(e.kind, Kind::Verification, "{e}");
    assert!(
        !fabstir_llm_node::kbs::cpu::is_dated_refusal(&e.detail),
        "a content refusal, not a dated one: {e}"
    );
    assert_eq!(
        f2.hit_count(),
        4,
        "one network pass; no memo-only retry (mutation: retry on any error → released)"
    );
    let after: Vec<MemoEntry> = memo_files(dir.path())
        .iter()
        .map(|p| read_entry(p))
        .collect();
    let before_repointed: Vec<MemoEntry> = before
        .into_iter()
        .map(|mut e| {
            e.url = e.url.replace(&f.base(), &f2.base());
            e
        })
        .collect();
    let mut a = after.clone();
    let mut b = before_repointed.clone();
    a.sort_by(|x, y| x.url.cmp(&y.url));
    b.sort_by(|x, y| x.url.cmp(&y.url));
    assert_eq!(a, b, "the refused fresh set was not committed either");
}

#[tokio::test]
async fn a_quote_that_fails_before_any_http_call_is_a_verification_refusal() {
    // A truncated quote fails inside `fetch` at PCK-chain extraction, before dcap-qvl
    // asks the PCCS for anything: bad evidence, not an outage.
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    let m = memo(&eg, dir.path());
    let e = verify_real(&q[..600], &m, &f.base(), GOOD_NOW)
        .await
        .unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Verification, 403), "{e}");
    assert_eq!(f.hit_count(), 0, "no HTTP call was made");
}

#[tokio::test]
async fn a_garbage_2xx_on_one_url_takes_the_memo_even_when_others_were_memo_served() {
    // Three memo entries fresh, the TCB entry older than the window: pass 1 serves three
    // from memo and fetches tcb, which answers a 200 maintenance page → fetch fails.
    // The committed (older, still valid) tcb memo must win: memo-only, not a doomed
    // network-only refetch (mutation: order the from_memo rule first → 502).
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let tcb = tcb_url(&f);
    let page = (
        200,
        vec![("content-type".to_string(), "text/html".to_string())],
        b"<html>maintenance</html>".to_vec(),
    );
    let (p2, f2) = fake_pccs(vec![("/tcb?", page)]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    repoint_memo(dir.path(), &f.base(), &f2.base());
    let tcb2 = tcb.replace(&f.base(), &f2.base());
    let path = memo_path(dir.path(), &tcb2);
    let mut entry = read_entry(&path);
    entry.fetched_at -= 2 * 86_400;
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    // 24 h fresh window: the other three are memo-served, tcb goes to the network
    let m = MemoHttp::collateral(eg2.clone(), dir.path().to_path_buf(), FRESH, TIMEOUT, MAX);
    let (ev, trace) = verify_real(&q, &m, &f2.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(
        (trace.passes, trace.second_pass_mode, trace.committed),
        (2, Some(Mode::MemoOnly), 0)
    );
    assert_eq!(
        f2.hits_matching("/tcb?"),
        1,
        "the network was tried once for tcb only"
    );
    assert_eq!(f2.hit_count(), 1);
}

#[tokio::test]
async fn a_url_with_neither_body_nor_memo_reports_the_transport_cause_not_a_memo_only_miss() {
    // Three fresh memo entries, none for tcb, PCCS unreachable: pass 1 serves three from
    // memo and fails tcb on transport. A memo-only pass could not succeed; the error the
    // node logs must be the connection failure (mutation: retry anyway → "memo-only: no
    // committed memo").
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let tcb = tcb_url(&f);
    let dead: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let base_dead = format!("https://{HOST}:1");
    repoint_memo(dir.path(), &f.base(), &base_dead);
    std::fs::remove_file(memo_path(dir.path(), &tcb.replace(&f.base(), &base_dead))).unwrap();
    let eg_dead = egress_for(&p, dead, &[]);
    let m = MemoHttp::collateral(
        eg_dead.clone(),
        dir.path().to_path_buf(),
        FRESH,
        TIMEOUT,
        MAX,
    );
    let e = verify_real(&q, &m, &base_dead, GOOD_NOW).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    assert!(
        e.detail.contains("connect"),
        "the transport cause, not a memo-only miss: {e}"
    );
    assert!(!e.detail.contains("memo-only"), "{e}");
}

#[tokio::test]
async fn a_fetch_outage_beside_memo_inputs_is_not_refetched_network_only() {
    // Three fresh memo entries, none for tcb, the PCCS answering 503 for tcb: pass 1
    // serves three from memo and fails tcb. A network-only pass would refetch all four
    // into the same outage, a PCCS timeout per URL under the wall, for nothing; the
    // outage is reported after one pass (mutation: drop the kind check on the
    // from_memo rule → pckcrl and tcb are both hit again, 3 hits).
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let tcb = tcb_url(&f);
    let (p2, f2) = fake_pccs(vec![("/tcb?", (503, vec![], b"down".to_vec()))]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    repoint_memo(dir.path(), &f.base(), &f2.base());
    std::fs::remove_file(memo_path(dir.path(), &tcb.replace(&f.base(), &f2.base()))).unwrap();
    let m = MemoHttp::collateral(eg2.clone(), dir.path().to_path_buf(), FRESH, TIMEOUT, MAX);
    let e = verify_real(&q, &m, &f2.base(), GOOD_NOW).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    assert_eq!(f2.hits_matching("/tcb?"), 1, "one pass only");
    assert_eq!(
        f2.hit_count(),
        1,
        "the memo-served URLs were never refetched"
    );
}

#[tokio::test]
async fn a_decodable_but_unusable_fresh_memo_entry_is_replaced_by_a_network_only_pass() {
    // The TCB entry decodes (valid JSON, valid base64) but its body is not collateral:
    // the fresh path serves it, dcap-qvl cannot parse it, and a memo-only retry would
    // serve the same bytes, pinning a 502 for the whole fresh window. A pass that ran
    // on the memo alone earns the network-only refetch, whose verified commit replaces
    // the entry (mutation: drop the memo-alone rule → MemoOnly → Err for 24 h).
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let path = memo_path(dir.path(), &tcb_url(&f));
    let mut entry = read_entry(&path);
    entry.body_b64 = "bm90IGNvbGxhdGVyYWw=".to_string(); // "not collateral"
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    let hits_before = f.hit_count();
    let m = memo(&eg, dir.path());
    let (ev, trace) = verify_real(&q, &m, &f.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(
        (trace.passes, trace.second_pass_mode),
        (2, Some(Mode::NetworkOnly))
    );
    assert!(
        trace.committed >= 4,
        "the verified network pass replaced the memo: {}",
        trace.committed
    );
    assert!(f.hit_count() > hits_before, "the network was asked");
    assert_ne!(
        read_entry(&path).body_b64,
        "bm90IGNvbGxhdGVyYWw=",
        "the unusable entry is gone"
    );
    // and the next request is served from the (repaired) memo again
    let (_, t3) = verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    assert_eq!(t3.passes, 1);
}

#[tokio::test]
async fn a_quote_without_an_embedded_pck_chain_never_reaches_the_pccs() {
    // Refused as the quote's own fault with zero HTTP calls (mutation: drop the
    // decode guard → dcap-qvl asks the PCCS for pckcert and a 404/timeout there
    // becomes an outage the node retries for ever).
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let e = verify_real(
        &super::test_cpu::type3_quote(),
        &memo(&eg, dir.path()),
        &f.base(),
        GOOD_NOW,
    )
    .await
    .unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Verification, 403), "{e}");
    assert!(e.detail.contains("certification data type 3"), "{e}");
    assert_eq!(f.hit_count(), 0);
}

#[tokio::test]
async fn a_corrupted_memo_entry_is_treated_as_absent_and_replaced() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let path = memo_path(dir.path(), &tcb_url(&f));
    let mut entry = read_entry(&path);
    entry.body_b64 = "!!!not base64!!!".into();
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    let hits = f.hit_count();
    // fresh window, yet the corrupted entry must not be served: the network fills it in
    let (ev, trace) = verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(
        f.hit_count() - hits,
        1,
        "only the corrupted URL went to the network (mutation: serve it → 502 forever)"
    );
    assert_eq!(trace.committed, 1);
    assert_ne!(
        read_entry(&path).body_b64,
        "!!!not base64!!!",
        "replaced by the verified pass"
    );
}

#[tokio::test]
async fn a_stale_memo_behind_a_garbage_outage_is_unavailable_not_verification() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    // make the memoed TCB info dated
    let path = memo_path(dir.path(), &tcb_url(&f));
    let mut entry = read_entry(&path);
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD
        .decode(&entry.body_b64)
        .unwrap();
    let edited = String::from_utf8(body)
        .unwrap()
        .replace("2025-07-19T10:16:03Z", "2025-06-30T00:00:00Z");
    entry.body_b64 = base64::engine::general_purpose::STANDARD.encode(edited);
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    // every path now answers a 200 maintenance page; the memo is past its window
    let page = (
        200,
        vec![("content-type".to_string(), "text/html".to_string())],
        b"<html>maintenance</html>".to_vec(),
    );
    let (p2, f2) = fake_pccs(vec![("/", page)]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    repoint_memo(dir.path(), &f.base(), &f2.base());
    let m = MemoHttp::collateral(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let e = verify_real(&q, &m, &f2.base(), GOOD_NOW).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "an outage plus a stale memo is not a refusal of the evidence (mutation: return e2 → 403): {e}");
    assert!(e.detail.contains("TCBInfo expired"), "{e}");
}

#[tokio::test]
async fn a_forged_quote_during_an_outage_stays_a_403_on_the_memo_only_pass() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    // every path now answers a 200 maintenance page (memo-only retry territory)
    let page = (
        200,
        vec![("content-type".to_string(), "text/html".to_string())],
        b"<html>maintenance</html>".to_vec(),
    );
    let (p2, f2) = fake_pccs(vec![("/", page)]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    repoint_memo(dir.path(), &f.base(), &f2.base());
    let mut forged = q.clone();
    forged[100] ^= 1;
    let m = MemoHttp::collateral(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let e = verify_real(&forged, &m, &f2.base(), GOOD_NOW)
        .await
        .unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Verification, 403), "the quote's own fault is never relabelled an outage (mutation: relabel any memo-only refusal → 502): {e}");
    assert!(fabstir_llm_node::kbs::cpu::is_dated_refusal(
        "tdx: TCBInfo expired"
    ));
    assert!(fabstir_llm_node::kbs::cpu::is_dated_refusal(
        "tdx: QE Identity issue date is in the future"
    ));
    assert!(!fabstir_llm_node::kbs::cpu::is_dated_refusal(
        "tdx: ISV enclave report signature is invalid"
    ));
    assert!(
        !fabstir_llm_node::kbs::cpu::is_dated_refusal("tdx: PCK certificate chain: CertExpired"),
        "the quote's own chain expiring is the quote's fault"
    );
}

#[tokio::test]
async fn a_signature_refusal_on_memo_inputs_survives_a_failed_network_retry() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    // the memo is fresh; the network is now dead; the quote is forged
    let dead: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let eg_dead = egress_for(&p, dead, &[]);
    let base_dead = format!("https://{HOST}:1");
    repoint_memo(dir.path(), &f.base(), &base_dead);
    let mut forged = q.clone();
    forged[100] ^= 1;
    let m = MemoHttp::collateral(
        eg_dead.clone(),
        dir.path().to_path_buf(),
        FRESH,
        TIMEOUT,
        MAX,
    );
    let e = verify_real(&forged, &m, &base_dead, GOOD_NOW)
        .await
        .unwrap_err();
    assert_eq!(
        (e.kind, e.status),
        (Kind::Verification, 403),
        "the quote's fault, not the outage (mutation: return e2 → 502): {e}"
    );
}

#[tokio::test]
async fn a_pccs_serving_dated_collateral_is_an_outage_not_a_refused_quote() {
    // The PCCS lags Intel: fresh bodies whose nextUpdate has passed. Pass 1 (network)
    // refuses dated; no memo is involved, so there is no retry; the node must see
    // `unavailable` and retry later, not a permanent 403 (mutation: return e1 → 403).
    let q = fixture("dcap-tdx_quote_outdated.bin");
    let stale: QuoteCollateralV3 =
        serde_json::from_slice(&fixture("dcap-tdx_quote_outdated_collateral.json")).unwrap();
    let p = pki(HOST);
    let f = spawn_fake(&p, pccs_handler(stale, vec![]), Duration::ZERO).await;
    let eg = egress_for(&p, f.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let m = memo(&eg, dir.path());
    let e = verify_real(&q, &m, &f.base(), super::test_cpu::OUTDATED_WINDOW_NOW)
        .await
        .unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    assert!(e.detail.contains("TCBInfo expired"), "{e}");
    assert!(
        memo_files(dir.path()).is_empty(),
        "dated bodies are never committed"
    );
    // and via the network-only retry after a stale memo: still an outage
    let (p2, f2) = fake_pccs(vec![]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    let (good_q, _) = good_pair();
    verify_real(&good_q, &memo(&eg2, dir.path()), &f2.base(), GOOD_NOW)
        .await
        .unwrap();
    let path = memo_path(dir.path(), &tcb_url(&f2));
    let mut entry = read_entry(&path);
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD
        .decode(&entry.body_b64)
        .unwrap();
    let edited = String::from_utf8(body)
        .unwrap()
        .replace("2025-07-19T10:16:03Z", "2025-06-30T00:00:00Z");
    entry.body_b64 = base64::engine::general_purpose::STANDARD.encode(edited.clone());
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    // the network now serves the same dated TCB info
    let mut dated = good_pair().1;
    dated.tcb_info = dated
        .tcb_info
        .replace("2025-07-19T10:16:03Z", "2025-06-30T00:00:00Z");
    let p3 = pki(HOST);
    let f3 = spawn_fake(&p3, pccs_handler(dated, vec![]), Duration::ZERO).await;
    let eg3 = egress_for(&p3, f3.addr, &[]);
    repoint_memo(dir.path(), &f2.base(), &f3.base());
    let m3 = memo(&eg3, dir.path());
    let e = verify_real(&good_q, &m3, &f3.base(), GOOD_NOW)
        .await
        .unwrap_err();
    assert_eq!(
        (e.kind, e.status),
        (Kind::Unavailable, 502),
        "network-only retry met dated bodies too: {e}"
    );
}

#[tokio::test]
async fn a_future_dated_memo_entry_is_not_fresh() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    let path = memo_path(dir.path(), &tcb_url(&f));
    let mut entry = read_entry(&path);
    entry.fetched_at += 10 * 86_400; // the clock stepped back after the commit
    std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
    let hits = f.hit_count();
    verify_real(&q, &memo(&eg, dir.path()), &f.base(), GOOD_NOW)
        .await
        .unwrap();
    assert_eq!(
        f.hit_count() - hits,
        1,
        "the future-dated URL went to the network (mutation: saturating_sub → 0 hits)"
    );
    assert!(
        read_entry(&path).fetched_at <= entry.fetched_at - 10 * 86_400 + 60,
        "replaced with a sane fetched_at"
    );
}

#[tokio::test]
async fn an_always_failing_optional_rootcacrl_probe_does_not_block_the_memo_only_pass() {
    // dcap-qvl probes /rootcacrl with `.ok()` and falls back to the root cert's CRL
    // distribution point, so a PCCS that never serves it is a normal PCCS. The
    // session rule must not count that probe as "a URL failed without a memo"
    // (mutation: drop the optional-probe filter → `failed_without_memo` is true and
    // the memo-only pass is never chosen).
    use fabstir_llm_node::kbs::memo::is_optional_probe;
    let (p, f) = fake_pccs(vec![("/rootcacrl", (404, vec![], b"no".to_vec()))]).await;
    let eg = egress_for(&p, f.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let base = f.base();
    let rootcacrl = format!("{base}/sgx/certification/v4/rootcacrl");
    let tcb = format!("{base}/tdx/certification/v4/tcb?fmspc=00806f050000&update=standard");
    assert!(is_optional_probe(&rootcacrl));
    assert!(!is_optional_probe(&tcb));
    // a committed memo for tcb, then a pass where tcb is served from the network and
    // rootcacrl fails with neither body nor memo
    let m0 = memo(&eg, dir.path());
    m0.get_with_rules(&tcb).await.unwrap();
    assert_eq!(m0.commit(), 1);
    let m = MemoHttp::collateral(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    m.get_with_rules(&tcb).await.unwrap();
    assert!(m.get_with_rules(&rootcacrl).await.is_err());
    let s = m.summary();
    assert!(
        s.all_touched_have_memo,
        "tcb returned a body and has a memo"
    );
    assert!(!s.failed_without_memo, "the optional probe must not count");
    // nor does it count as "the network was tried": a memo-alone pass on a PCCS that
    // 404s rootcacrl on every request must still earn the network-only refetch
    // (mutation: count every attempt → the round-34 rule never fires on such a PCCS)
    let m3 = MemoHttp::collateral(eg.clone(), dir.path().to_path_buf(), FRESH, TIMEOUT, MAX);
    m3.get_with_rules(&tcb).await.unwrap();
    assert!(m3.get_with_rules(&rootcacrl).await.is_err());
    let s3 = m3.summary();
    assert!(s3.from_memo && !s3.attempted_network, "{s3:?}");
    // a NON-optional URL failing the same way does count
    let qe = format!("{base}/tdx/certification/v4/qe/identity?update=standard");
    let (p2, f2) = fake_pccs(vec![("/qe/identity", (404, vec![], b"no".to_vec()))]).await;
    let eg2 = egress_for(&p2, f2.addr, &[]);
    let m2 = MemoHttp::collateral(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::ZERO,
        TIMEOUT,
        MAX,
    );
    let qe2 = qe.replace(&base, &f2.base());
    assert!(m2.get_with_rules(&qe2).await.is_err());
    assert!(m2.summary().failed_without_memo);
}

#[test]
fn a_fetch_failure_before_any_request_is_bad_evidence_and_after_one_is_an_outage() {
    use fabstir_llm_node::kbs::cpu::classify_fetch_error;
    use fabstir_llm_node::kbs::memo::SessionSummary;
    let base = SessionSummary {
        from_memo: false,
        from_network: false,
        all_touched_have_memo: false,
        staged: 0,
        any_request: true,
        attempted_network: true,
        failed_without_memo: true,
        urls: vec!["https://pccs.test/sgx/certification/v4/pckcrl?ca=platform".into()],
    };
    let e = anyhow::anyhow!("connect timed out");
    // a transport failure after a request is the PCCS's, whatever the URL
    // (mutation: the old pckcert-only rule → 403 on a PCCS blip)
    assert_eq!(classify_fetch_error(&base, &e).kind, Kind::Unavailable);
    let pckcert = SessionSummary {
        urls: vec![
            "https://pccs.test/sgx/certification/v4/pckcert?encrypted_ppid=..&cpusvn=..".into(),
        ],
        ..base.clone()
    };
    assert_eq!(classify_fetch_error(&pckcert, &e).kind, Kind::Unavailable);
    let no_request = SessionSummary {
        any_request: false,
        urls: vec![],
        ..base.clone()
    };
    assert_eq!(
        classify_fetch_error(&no_request, &e).kind,
        Kind::Verification
    );
    let real_outage = SessionSummary {
        urls: vec!["https://pccs.test/sgx/certification/v4/pckcrl?ca=platform&encoding=der".into()],
        ..base
    };
    assert_eq!(
        (
            classify_fetch_error(&real_outage, &e).kind,
            classify_fetch_error(&real_outage, &e).status
        ),
        (Kind::Unavailable, 502)
    );
}

#[tokio::test]
async fn two_sessions_commit_independently() {
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    let mut forged = q.clone();
    forged[100] ^= 1;
    let a = memo(&eg, dir.path());
    let b = memo(&eg, dir.path());
    let base = f.base();
    let (ra, rb) = tokio::join!(
        verify_real(&q, &a, &base, GOOD_NOW),
        verify_real(&forged, &b, &base, GOOD_NOW)
    );
    assert!(ra.is_ok());
    assert!(rb.is_err());
    assert_eq!(
        memo_files(dir.path()).len(),
        4,
        "only the Ok session's bodies are committed"
    );
}

#[tokio::test]
async fn a_memo_only_collateral_pass_does_not_touch_the_jwks_instance() {
    let (p, f) = fake_pccs(vec![("/jwks", (200, vec![], b"{\"keys\":[]}".to_vec()))]).await;
    let dir = tempfile::tempdir().unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let coll = memo(&eg, dir.path());
    coll.set_mode(Mode::MemoOnly);
    assert_eq!(coll.mode(), Mode::MemoOnly);
    let jwks = MemoHttp::network_first(eg.clone(), dir.path().to_path_buf(), TIMEOUT, MAX);
    let url = format!("{}/jwks", f.base());
    let r = jwks.get_with_rules(&url).await.unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(
        f.hits_matching("/jwks"),
        1,
        "the JWKS instance is network-first regardless"
    );
    assert!(
        coll.get_with_rules(&url).await.is_err(),
        "memo-only with no memo fails without a hit"
    );
    assert_eq!(f.hits_matching("/jwks"), 1);
}

#[tokio::test]
async fn allow_list_refuses_before_any_connection() {
    let (p, f) = fake_pccs(vec![]).await;
    let eg = egress_for(&p, f.addr, &[]);
    let port = f.addr.port();
    for url in [
        format!("https://evil.test:{port}/x"),
        format!("https://u:p@{HOST}:{port}/x"),
        format!("https://{HOST}:{}/x", port + 1),
        format!("http://{HOST}:{port}/x"),
        format!("https://{HOST}/x"), // port 443 is not the configured port
    ] {
        let e = eg.get(&url, TIMEOUT, MAX).await.unwrap_err();
        assert!(matches!(e, EgressError::NotAllowed(_)), "{url}: {e}");
    }
    assert_eq!(f.hit_count(), 0, "no connection was attempted");
    // the memo layer checks the allow-list too, memo reads included
    let dir = tempfile::tempdir().unwrap();
    let m = memo(&eg, dir.path());
    assert!(m
        .get_with_rules(&format!("https://evil.test:{port}/x"))
        .await
        .is_err());
    assert_eq!(f.hit_count(), 0);
}

#[tokio::test]
async fn body_cap_is_enforced() {
    let p = pki(HOST);
    let f = spawn_fake(
        &p,
        std::sync::Arc::new(|_, _, _| (200, vec![], vec![b'x'; 10_000])),
        Duration::ZERO,
    )
    .await;
    let eg = egress_for(&p, f.addr, &[]);
    let e = eg
        .get(&format!("{}/big", f.base()), TIMEOUT, 1000)
        .await
        .unwrap_err();
    assert!(matches!(e, EgressError::TooLarge(1000)), "{e}");
}

#[tokio::test]
async fn a_memo_write_failure_is_non_fatal() {
    use std::os::unix::fs::PermissionsExt;
    let (p, f) = fake_pccs(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    let ro = dir.path().join("memo");
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let eg = egress_for(&p, f.addr, &[]);
    let (q, _) = good_pair();
    let m = memo(&eg, &ro);
    let (ev, trace) = verify_real(&q, &m, &f.base(), GOOD_NOW).await.unwrap();
    assert_eq!(ev.tcb_status, "UpToDate");
    assert_eq!(trace.committed, 0, "nothing could be written");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn egress_client_requires_valid_extra_root() {
    let e = EgressClient::new(
        vec![(HOST.to_string(), 443)],
        EgressOptions {
            extra_root_pem: Some(b"not a pem".to_vec()),
            resolve: None,
            resolve_extra: Vec::new(),
        },
    )
    .err()
    .expect("bad root refused");
    assert!(matches!(e, EgressError::Transport(_)));
}

/// The memo is keyed by URL; a test that moves to a second fake (new port)
/// rewrites the committed entries' URLs so they are found under the new base.
fn repoint_memo(dir: &Path, from: &str, to: &str) {
    for path in memo_files(dir) {
        let mut e = read_entry(&path);
        e.url = e.url.replace(from, to);
        let new_path = memo_path(dir, &e.url);
        std::fs::write(&new_path, serde_json::to_vec(&e).unwrap()).unwrap();
        if new_path != path {
            std::fs::remove_file(&path).unwrap();
        }
    }
}

#[test]
fn fixture_collateral_deserialises() {
    let c: QuoteCollateralV3 =
        serde_json::from_slice(&fixture("dcap-tdx_quote_collateral.json")).unwrap();
    assert!(c.tcb_info.contains("nextUpdate"));
}
