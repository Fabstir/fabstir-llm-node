// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! What session 1480 showed live: nothing proved for 405 s on a 300 s window,
//! because the sidecar reported duration 0.0 until the end, and no proof possible
//! while progress held at 99. Plus the poll grace and token rounding.

use anyhow::{anyhow, Result};
use ethers::types::H256;
use fabstir_llm_node::api::websocket::handlers::transcode::{
    known_duration, poll_gives_up, poll_grace_secs, tokens_for_units, transcode_tokens,
    POLL_GRACE_SECS_DEFAULT,
};
use fabstir_llm_node::transcoder::biller::{FinishOutcome, TranscodeBiller, TranscodeChain};
use fabstir_llm_node::transcoder::proof_steps::catch_up_due;
use fabstir_llm_node::transcoder::types::VideoFormat;
use std::sync::Mutex;
use std::time::Duration;

const JOB: u64 = 1480;
const START: u64 = 1_000_000;
const RATE: u64 = 2_000;
const WINDOW: u64 = 300;
const TOKENS: u64 = 1_980_000; // 30 minutes of 1080p, encrypted

struct C {
    now: u64,
    tokens_used: u64,
    last_proof: u64,
    proofs: Vec<(u64, u64)>,
    clock_runs: bool,
}
struct Fake(Mutex<C>);

fn word(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

#[async_trait::async_trait]
impl TranscodeChain for Fake {
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>> {
        let c = self.0.lock().unwrap();
        let words = [
            job_id,
            1,
            2,
            3,
            11_000_000,
            5_000,
            c.tokens_used,
            3_600,
            START,
            c.last_proof,
            100,
            WINDOW,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        Ok(words.iter().flat_map(|w| word(*w)).collect())
    }
    async fn chain_now(&self) -> Result<u64> {
        // During a final claim the fake's clock moves, as the chain's would.
        let mut c = self.0.lock().unwrap();
        if c.clock_runs {
            c.now += 60;
        }
        Ok(c.now)
    }
    async fn rate_limit(&self, _job: u64) -> Result<u64> {
        Ok(RATE)
    }
    async fn put_attestation(&self, path: &str, _b: Vec<u8>) -> Result<String> {
        Ok(format!("cid:{path}"))
    }
    async fn submit_proof(&self, _j: u64, tokens: u64, _h: [u8; 32], _c: String) -> Result<H256> {
        let mut c = self.0.lock().unwrap();
        if tokens > (c.now - c.last_proof) * RATE {
            return Err(anyhow!("Too many"));
        }
        c.tokens_used += tokens;
        c.last_proof = c.now;
        let at = c.now;
        c.proofs.push((at, tokens));
        Ok(H256::zero())
    }
}

fn fake() -> Fake {
    Fake(Mutex::new(C {
        now: START,
        tokens_used: 0,
        last_proof: START,
        proofs: Vec::new(),
        clock_runs: false,
    }))
}

/// One poll, as the task makes it, with the sidecar's raw duration.
async fn poll(b: &mut TranscodeBiller, f: &Fake, t: u64, pct: i32, duration: Option<f64>) {
    f.0.lock().unwrap().now = START + t;
    let estimate = known_duration(duration).map(|_| TOKENS);
    b.on_progress(f, pct, estimate, START + t).await;
}

/// The longest gap with no proof, from the session's start to the last proof.
fn longest_silence(f: &Fake) -> u64 {
    let c = f.0.lock().unwrap();
    let mut last = START;
    let mut worst = 0;
    for (at, _) in &c.proofs {
        worst = worst.max(at - last);
        last = *at;
    }
    worst
}

#[test]
fn a_zero_duration_is_unknown_not_a_zero_length_job() {
    assert_eq!(known_duration(None), None);
    assert_eq!(known_duration(Some(0.0)), None);
    assert_eq!(known_duration(Some(1800.0)), Some(1800.0));
}

#[tokio::test(start_paused = true)]
async fn session_1480_replayed_with_an_early_duration_stays_inside_its_window() {
    let f = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    // Queued and downloading: 0.0. It must not pin the estimate at zero.
    poll(&mut b, &f, 2, 0, Some(0.0)).await;
    poll(&mut b, &f, 10, 0, Some(0.0)).await;
    // Encoding (221 s), polled every 20 s, then 184 s held at 99.
    for t in (20..=220).step_by(20) {
        poll(&mut b, &f, t, ((t * 99) / 221) as i32, Some(1800.0)).await;
    }
    for t in (240..=400).step_by(20) {
        poll(&mut b, &f, t, 99, Some(1800.0)).await;
    }
    assert!(!f.0.lock().unwrap().proofs.is_empty(), "proved in steps");
    let (first, _) = f.0.lock().unwrap().proofs[0];
    assert!(
        first - START < WINDOW / 2 + 40,
        "first proof at {}",
        first - START
    );
    assert!(
        longest_silence(&f) <= WINDOW / 2 + 40,
        "silence {} on a {} window",
        longest_silence(&f),
        WINDOW
    );
    // Still proving during the hold at 99, and every proof inside the rate cap.
    let during_hold =
        f.0.lock()
            .unwrap()
            .proofs
            .iter()
            .filter(|(at, _)| *at > START + 221)
            .count();
    assert!(during_hold >= 1, "a proof landed while progress held at 99");
    // The final claim settles it to the unit.
    f.0.lock().unwrap().now = START + 405;
    f.0.lock().unwrap().clock_runs = true;
    b.finish(&f, TOKENS).await;
    let used = f.0.lock().unwrap().tokens_used;
    assert!(used <= TOKENS, "never over-claims: {used}");
}

#[tokio::test]
async fn progress_going_back_down_never_over_claims() {
    let f = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    for t in (20..=200).step_by(20) {
        poll(&mut b, &f, t, (t / 3) as i32, Some(1800.0)).await;
    }
    let before = f.0.lock().unwrap().tokens_used;
    assert!(before > 0);
    // A restarted job: progress drops to 2 and climbs slowly. It stays below
    // what is already proved, so there is nothing to claim.
    for t in (220..=800).step_by(20) {
        poll(&mut b, &f, t, 2, Some(1800.0)).await;
    }
    assert_eq!(f.0.lock().unwrap().tokens_used, before);
    assert_eq!(b.proven(), before);
}

#[test]
fn catch_up_needs_unproven_work_and_half_the_window() {
    assert!(catch_up_due(100, 500, 1_000 + 150, 1_000, 300));
    assert!(!catch_up_due(100, 500, 1_000 + 149, 1_000, 300), "too soon");
    assert!(
        !catch_up_due(500, 500, 2_000, 1_000, 300),
        "nothing unproven"
    );
    assert!(
        !catch_up_due(600, 500, 2_000, 1_000, 300),
        "ahead of target"
    );
}

#[tokio::test(start_paused = true)]
async fn the_final_claim_still_pays_in_full_after_catch_up() {
    let f = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    for t in (20..=400).step_by(20) {
        poll(&mut b, &f, t, 99.min((t / 2) as i32), Some(1800.0)).await;
    }
    // The rest is claimed at the end, to the unit.
    f.0.lock().unwrap().now = START + 1_500;
    f.0.lock().unwrap().clock_runs = true;
    let used_before = f.0.lock().unwrap().tokens_used;
    assert!(used_before <= TOKENS);
    assert_eq!(
        b.finish(&f, TOKENS).await,
        FinishOutcome::Paid { proven: TOKENS }
    );
    assert_eq!(f.0.lock().unwrap().tokens_used, TOKENS);
}

#[test]
fn the_poll_grace_covers_a_recreate_and_then_gives_up() {
    let grace = Duration::from_secs(POLL_GRACE_SECS_DEFAULT);
    assert_eq!(POLL_GRACE_SECS_DEFAULT, 120);
    assert!(!poll_gives_up(Duration::from_secs(10), grace), "a recreate");
    assert!(!poll_gives_up(Duration::from_secs(119), grace));
    assert!(poll_gives_up(Duration::from_secs(120), grace));
    assert_eq!(poll_grace_secs(None), 120);
    assert_eq!(poll_grace_secs(Some("30")), 30);
    assert_eq!(poll_grace_secs(Some("junk")), 120);
}

fn format(vf: &str) -> VideoFormat {
    serde_json::from_value(serde_json::json!({
        "id": 1, "ext": "mp4", "vcodec": "h264_nvenc", "vf": vf
    }))
    .unwrap()
}

#[test]
fn tokens_carry_no_floating_point_noise() {
    let fmts = [format("scale=1920x1080")];
    // Session 1480 billed 1,980,001 for this.
    let (units, tokens) = transcode_tokens(1800.0, &fmts, true);
    assert_eq!(tokens, 1_980_000);
    assert_eq!(units, 1980.0);
    // Whole seconds at factor 1.1 are always exact, across the range.
    for secs in 1u64..=20_000 {
        assert_eq!(
            transcode_tokens(secs as f64, &fmts, true).1,
            secs * 1100,
            "{secs} s"
        );
    }
    // A real fraction of a token still rounds up.
    assert_eq!(tokens_for_units(0.0015), 2);
    assert_eq!(tokens_for_units(132.0), 132_000);
}

#[tokio::test]
async fn a_long_hold_at_99_keeps_proving_until_the_work_is_covered() {
    // A two-hour source: 12 minutes at 99 for upload and moderation, longer than
    // the 300 s window. The rate cap leaves plenty unproven, so proofs carry on.
    let f = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    for t in (20..=200).step_by(20) {
        poll(&mut b, &f, t, ((t * 99) / 200) as i32, Some(1800.0)).await;
    }
    for t in (220..=920).step_by(20) {
        poll(&mut b, &f, t, 99, Some(1800.0)).await;
    }
    let silence = longest_silence(&f);
    assert!(silence <= WINDOW / 2 + 40, "silence {silence}");
    let c = f.0.lock().unwrap();
    assert!(
        c.proofs.iter().filter(|(at, _)| *at > START + 400).count() >= 3,
        "{:?}",
        c.proofs
    );
    assert!(c.tokens_used <= TOKENS * 99 / 100);
}

struct RevertingChain(Fake, Mutex<u32>);

#[async_trait::async_trait]
impl TranscodeChain for RevertingChain {
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>> {
        self.0.session_raw(job_id).await
    }
    async fn chain_now(&self) -> Result<u64> {
        self.0.chain_now().await
    }
    async fn rate_limit(&self, job_id: u64) -> Result<u64> {
        self.0.rate_limit(job_id).await
    }
    async fn put_attestation(&self, path: &str, b: Vec<u8>) -> Result<String> {
        self.0.put_attestation(path, b).await
    }
    async fn submit_proof(&self, _j: u64, _t: u64, _h: [u8; 32], _c: String) -> Result<H256> {
        *self.1.lock().unwrap() += 1;
        Err(anyhow!("reverted"))
    }
}

#[tokio::test]
async fn a_proof_that_keeps_failing_is_not_retried_on_every_poll() {
    let f = RevertingChain(fake(), Mutex::new(0));
    let mut b = TranscodeBiller::new(JOB, "task".into());
    // Two minutes at the sidecar's 2 s poll: 60 polls past the first quarter.
    for t in (2..=120).step_by(2) {
        f.0 .0.lock().unwrap().now = START + t;
        b.on_progress(&f, 30, Some(TOKENS), START + t).await;
    }
    let attempts = *f.1.lock().unwrap();
    assert!(attempts >= 2, "still retries: {attempts}");
    assert!(attempts <= 8, "{attempts} attempts in 60 polls");
}

fn preview(vf: &str, percent: u32) -> VideoFormat {
    serde_json::from_value(serde_json::json!({
        "id": 2, "ext": "mp4", "vcodec": "h264_nvenc", "vf": vf, "trim_percent": percent
    }))
    .unwrap()
}

#[test]
fn a_preview_counts_only_its_share_in_the_estimate() {
    use fabstir_llm_node::api::websocket::handlers::transcode::transcode_estimate_tokens;
    // A preview-only job: 20 % of a 120 s source. From transcoder 6cc694e the
    // result reports the 24 s really encoded, and the bill is on that.
    let only = [preview("scale=1920x1080", 20)];
    let estimate = transcode_estimate_tokens(120.0, &only, true);
    let final_bill = transcode_tokens(24.0, &only, true).1;
    assert_eq!(estimate, 26_400);
    assert!(estimate <= final_bill, "{estimate} > {final_bill}");
    // A full rendition with a preview: the result reports the full length, so
    // the estimate stays below the bill.
    let mixed = [format("scale=1920x1080"), preview("scale=1280x720", 20)];
    let estimate = transcode_estimate_tokens(120.0, &mixed, true);
    assert_eq!(estimate, 132_000 + 13_200);
    assert!(estimate <= transcode_tokens(120.0, &mixed, true).1);
    // No trim, or a meaningless one, renders the whole source.
    let full = [format("scale=1920x1080")];
    assert_eq!(transcode_estimate_tokens(120.0, &full, true), 132_000);
    assert_eq!(
        transcode_estimate_tokens(120.0, &[preview("scale=1920x1080", 0)], true),
        132_000
    );
    assert_eq!(
        transcode_estimate_tokens(120.0, &[preview("scale=1920x1080", 100)], true),
        132_000
    );
}

#[test]
fn the_final_wait_follows_the_job_with_no_hour_ceiling() {
    use fabstir_llm_node::transcoder::biller::{final_wait_cap_secs, FINAL_RATE_WAIT_CAP_SECS};
    // The SDK developer's example: 45 min on the four-rung ladder, 8.1M owed at
    // the end at 2,000 tokens/s. The 8.62.2 ceiling gave up at 3,600 s.
    assert_eq!(
        final_wait_cap_secs(8_100_000, 2_000),
        4_050 + FINAL_RATE_WAIT_CAP_SECS
    );
    assert_eq!(
        final_wait_cap_secs(132_000, 2_000),
        66 + FINAL_RATE_WAIT_CAP_SECS
    );
    assert_eq!(final_wait_cap_secs(1_000, 0), FINAL_RATE_WAIT_CAP_SECS);
}

#[test]
fn software_encoders_bill_at_their_codec_family() {
    use fabstir_llm_node::transcoder::billing::codec_factor;
    for (codec, factor) in [
        ("h264_nvenc", 1.0),
        ("libx264", 1.0),
        ("hevc_nvenc", 1.2),
        ("libx265", 1.2),
        ("h265", 1.2),
        ("av1_nvenc", 1.5),
        ("libsvtav1", 1.5),
        ("libaom-av1", 1.5),
        ("librav1e", 1.5),
        ("", 1.0),
    ] {
        assert_eq!(codec_factor(codec), factor, "{codec}");
    }
}
