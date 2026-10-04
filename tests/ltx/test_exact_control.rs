// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NM1 D3 — exact control length where LTX can deliver it. LTX keeps
//! `8·floor((in−1)/8)+1` frames, so a template carrying `exactControl` can
//! deliver every billed frame only when `(billed − 1) % 8 == 0`; there the
//! control clip must carry AT LEAST the billed count (not billed − 1).

use fabstir_llm_node::api::websocket::handlers::ltx::{
    check_control_video, check_control_video_exact, exact_applies,
};

// The mp4 synthesisers, copied from tests/ltx/test_mp4.rs:12-58 (a minimal ISO
// BMFF file whose one video track's stsz carries the sample count).
fn boxed(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(payload);
    out
}

fn hdlr(handler: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 8];
    p.extend_from_slice(handler);
    p.extend_from_slice(&[0u8; 12]);
    p.push(0);
    boxed(b"hdlr", &p)
}

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

fn clip(samples: u32) -> Vec<u8> {
    mp4_with(&[trak(b"vide", Some(samples))])
}

#[test]
fn test_exact_control_refuses_short_clip() {
    // 120 samples for 121 billed: exact → refused, with the count in the message.
    let err = check_control_video_exact(&clip(120), 121, true).unwrap_err();
    assert!(err.contains("control video has 120 frame(s)"), "{err}");
    // The same clip when exactness does not apply keeps today's ±1 floor.
    assert!(check_control_video_exact(&clip(120), 121, false).is_ok());
    // Exact and long enough → accepted; longer clips are cropped by the graph.
    assert!(check_control_video_exact(&clip(121), 121, true).is_ok());
    assert!(check_control_video_exact(&clip(300), 121, true).is_ok());
    // The two-argument form keeps its behaviour (delegates with exact = false).
    assert!(check_control_video(&clip(120), 121).is_ok());
    assert!(check_control_video(&clip(119), 121).is_err());
}

#[test]
fn test_exact_applies_grid() {
    assert!(exact_applies(true, 121)); // 24 fps 5 s, 25 fps 4.8 s
    assert!(!exact_applies(true, 126)); // 25 fps 5 s: off LTX's grid
    assert!(exact_applies(true, 201)); // 25 fps 8 s
    assert!(!exact_applies(false, 121)); // no exactControl on the template
}
