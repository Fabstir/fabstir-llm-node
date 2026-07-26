// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! BL3/U7 — the control-video frame-count gate's mp4 parser. The graph derives
//! the RENDERED frame count from the control clip while billing uses the job's
//! `frames`, so the node must read the clip's own sample count (stsz) without
//! decoding — and must fail CLOSED (an Err, never a panic) on anything it can't
//! read. Boxes here are synthesized byte-exact; the parser was cross-checked
//! against ffprobe on the two real fixture clips (126 @25fps 5s, 121 @24fps 5s).

use fabstir_llm_node::ltx::mp4::video_sample_count;

/// A box: 4-byte BE size + 4-byte type + payload.
fn boxed(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(payload);
    out
}

/// hdlr payload: version/flags + pre_defined + handler_type + reserved[3] + name.
fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 8];
    p.extend_from_slice(handler);
    p.extend_from_slice(&[0u8; 12]);
    p.push(0); // empty name
    boxed(b"hdlr", &p)
}

/// stsz payload: version/flags + sample_size(0 ⇒ per-sample table) + sample_count.
fn stsz(count: u32) -> Vec<u8> {
    let mut p = vec![0u8; 4];
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&count.to_be_bytes());
    boxed(b"stsz", &p)
}

fn trak(handler: &[u8; 4], sample_count: Option<u32>) -> Vec<u8> {
    let mut stbl_payload = Vec::new();
    if let Some(n) = sample_count {
        stbl_payload.extend_from_slice(&stsz(n));
    }
    let stbl = boxed(b"stbl", &stbl_payload);
    let minf = boxed(b"minf", &stbl);
    let mut mdia_payload = hdlr(handler);
    mdia_payload.extend_from_slice(&minf);
    let mdia = boxed(b"mdia", &mdia_payload);
    boxed(b"trak", &mdia)
}

fn mp4_with(traks: &[Vec<u8>]) -> Vec<u8> {
    let ftyp = boxed(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    let moov_payload: Vec<u8> = traks.concat();
    let moov = boxed(b"moov", &moov_payload);
    let mut out = ftyp;
    out.extend_from_slice(&moov);
    out
}

#[test]
fn test_video_track_sample_count_read_from_stsz() {
    let bytes = mp4_with(&[trak(b"vide", Some(121))]);
    assert_eq!(video_sample_count(&bytes).unwrap(), 121);
}

#[test]
fn test_non_video_traks_are_skipped() {
    // audio first, then video — the count must come from the vide track
    let bytes = mp4_with(&[trak(b"soun", Some(9999)), trak(b"vide", Some(126))]);
    assert_eq!(video_sample_count(&bytes).unwrap(), 126);
}

/// A.2 — CHARACTERISATION, not a requirement. `video_sample_count` returns the
/// FIRST `vide` trak's count, so a container carrying two video traks is read
/// from whichever the muxer wrote first, regardless of which one a decoder
/// would pick as the primary stream.
///
/// This was investigated as a suspected live false-rejection ("a cover-art
/// trak sorts first, reads as 1 sample, and the control-video gate rejects a
/// lawful clip"). **It is not reachable from real encoder output.** ISO BMFF
/// has no `attached_pic` concept: FFmpeg writes cover art to
/// `moov/udta/meta/ilst/covr`, a metadata box, not a trak.
///
/// Probe run 2026-07-25 against a purpose-built real fixture
/// (`fabstir-moderation/tests/fixtures/cover_art.mp4` — a sibling repo, NOT a
/// dependency of this crate, so the numbers here are a RECORDED RESULT rather
/// than something this test re-checks): one `vide` trak, `video_sample_count`
/// == 125 (its true frame count), and `check_control_video` ACCEPTS it at both
/// 121 and 126 billed frames. All 12 mp4/mov files then present across the
/// workspace — including the helper's own control clips and one 2-trak
/// `vide`+`soun` file — had exactly one `vide` trak.
///
/// The shape below therefore has to be synthesised. It is pinned so that the
/// boundary of that finding is reproducible, and so that a future decision on
/// multi-track containers (OQ-L11 stream selection, DL14(b)) has a test to
/// flip rather than a claim to re-derive.
#[test]
fn test_first_vide_trak_wins_when_a_container_carries_two() {
    let bytes = mp4_with(&[trak(b"vide", Some(1)), trak(b"vide", Some(126))]);
    assert_eq!(
        video_sample_count(&bytes).unwrap(),
        1,
        "the parser reads the FIRST vide trak, not the longest or the primary one"
    );
    let bytes = mp4_with(&[trak(b"vide", Some(126)), trak(b"vide", Some(1))]);
    assert_eq!(
        video_sample_count(&bytes).unwrap(),
        126,
        "trak order alone decides which count the gate sees"
    );
}

#[test]
fn test_no_video_track_is_an_error() {
    let bytes = mp4_with(&[trak(b"soun", Some(240))]);
    assert!(video_sample_count(&bytes).unwrap_err().contains("no video"));
}

#[test]
fn test_missing_stsz_fails_closed_as_fragmented() {
    // a vide trak with no stbl/stsz — the fragmented-mp4 shape
    let bytes = mp4_with(&[trak(b"vide", None)]);
    assert!(video_sample_count(&bytes)
        .unwrap_err()
        .contains("fragmented"));
}

#[test]
fn test_zero_sample_count_fails_closed() {
    let bytes = mp4_with(&[trak(b"vide", Some(0))]);
    assert!(video_sample_count(&bytes)
        .unwrap_err()
        .contains("0 samples"));
}

#[test]
fn test_no_moov_is_an_error_not_a_panic() {
    let ftyp_only = boxed(b"ftyp", b"isom");
    assert!(video_sample_count(&ftyp_only).unwrap_err().contains("moov"));
}

#[test]
fn test_truncated_and_hostile_inputs_never_panic() {
    let good = mp4_with(&[trak(b"vide", Some(121))]);
    // every prefix of a valid file: Err or the correct Ok, never a panic
    for cut in 0..good.len() {
        let _ = video_sample_count(&good[..cut]);
    }
    // a box whose declared size overruns the buffer
    let mut overrun = boxed(b"moov", &[]);
    overrun[3] = 0xFF; // size huge
    assert!(video_sample_count(&overrun).is_err());
    // size smaller than its own header
    let mut tiny = boxed(b"moov", &[]);
    tiny[0..4].copy_from_slice(&3u32.to_be_bytes());
    assert!(video_sample_count(&tiny).is_err());
}

// ── the handler's control-video gate (v8.36.1 semantics) ─────────────────────
// Billing safety only needs the LOWER bound: an under-length clip means fewer
// rendered frames than billed (overbilling). Over-length clips are SAFE — both
// pinned-graph families crop server-side (the trio's VHS_LoadVideo carries
// frame_load_cap patched to the billed count; iclora's Video Slice takes the
// first `duration` seconds) — so v8.36.1 accepts them, letting clients send
// e.g. a 14.76 s clip against a 14 s job without client-side re-encoding.

use fabstir_llm_node::api::websocket::handlers::ltx::check_control_video;

fn clip(samples: u32) -> Vec<u8> {
    mp4_with(&[trak(b"vide", Some(samples))])
}

#[test]
fn test_control_gate_accepts_billed_range_and_over_length() {
    for samples in [120, 121, 122, 150, 369] {
        assert!(
            check_control_video(&clip(samples), 121).is_ok(),
            "{samples} samples vs 121 billed must pass (over-length crops in-graph)"
        );
    }
}

#[test]
fn test_control_gate_rejects_under_length() {
    // fewer than billed-1 frames -> the render would come up short of billing
    for samples in [1, 60, 119] {
        assert!(
            check_control_video(&clip(samples), 121).is_err(),
            "{samples} samples vs 121 billed must fail closed"
        );
    }
}

#[test]
fn test_control_gate_still_rejects_non_mp4_and_unreadable() {
    assert!(check_control_video(b"RIFFxxxxWEBP", 121).is_err());
    let ftyp_only = boxed(b"ftyp", b"isom");
    assert!(check_control_video(&ftyp_only, 121).is_err());
}
