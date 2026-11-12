// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP0.3 fingerprint value format. The fixture XML is ffmpeg 7.0.2's `signature` output for the synthetic
//! `tiny.mp4` (FFMPEG_FILTER). Mutations: packing order, trit weights, header fields, validation.
mod common;
use fabstir_provenance::fingerprint::{
    self, parse_ffmpeg_xml, FingerprintError, SignatureSet, FRAME_RECORD_BYTES, HEADER_BYTES,
};

fn fixture_set() -> SignatureSet {
    parse_ffmpeg_xml(&String::from_utf8(common::fixture("tiny-signature.xml")).unwrap()).unwrap()
}

#[test]
fn parses_ffmpeg_signature_xml() {
    let s = fixture_set();
    assert_eq!(s.media_time_unit, 12288);
    assert_eq!(s.frames.len(), 48);
    assert_eq!(s.frames[1].media_time, 512);
    assert!(s.frames.iter().all(|f| f.trits.iter().all(|t| *t <= 2)));
}

#[test]
fn packed_set_digest_is_golden() {
    let packed = fixture_set().pack().unwrap();
    assert_eq!(packed.len(), HEADER_BYTES + 48 * FRAME_RECORD_BYTES);
    assert_eq!(&packed[..6], b"PAIFP\x01");
    common::golden(
        "fingerprint-tiny",
        &serde_json::json!({
            "alg": fingerprint::ALG,
            "filter": fingerprint::FFMPEG_FILTER,
            "packed_len": packed.len(),
            "packed_head_hex": hex::encode(&packed[..HEADER_BYTES + FRAME_RECORD_BYTES]),
            "sha256": hex::encode(fingerprint::digest(&packed)),
        }),
    );
}

#[test]
fn pack_unpack_round_trip() {
    let s = fixture_set();
    assert_eq!(SignatureSet::unpack(&s.pack().unwrap()).unwrap(), s);
}

#[test]
fn trit_weights_are_little_endian_base3() {
    let mut s = fixture_set();
    s.frames.truncate(1);
    s.frames[0].trits = [0; 380];
    s.frames[0].trits[0] = 2; // weight 1
    s.frames[0].trits[4] = 1; // weight 81
    s.frames[0].trits[379] = 2; // last byte, weight 81
    let p = s.pack().unwrap();
    assert_eq!(p[HEADER_BYTES + 9], 2 + 81);
    assert_eq!(p[HEADER_BYTES + 9 + 75], 2 * 81);
}

fn xml(frames: &str) -> String {
    format!("<Mpeg7><MediaTimeUnit>12288</MediaTimeUnit>{frames}</Mpeg7>")
}
fn frame(t: u64, vals: &str) -> String {
    format!("<VideoFrame><MediaTimeOfFrame>{t}</MediaTimeOfFrame><FrameConfidence>80</FrameConfidence><Word>1 2 3 4 5</Word><FrameSignature>{vals}</FrameSignature></VideoFrame>")
}
fn vals(n: usize, v: &str) -> String {
    vec![v; n].join("  ")
}

#[test]
fn empty_signature_is_refused() {
    // the AP0.1b trap: an output-side duration limit leaves a file with no frames
    assert_eq!(parse_ffmpeg_xml(&xml("")), Err(FingerprintError::Empty));
    assert_eq!(parse_ffmpeg_xml(""), Err(FingerprintError::NoTimeUnit));
}

#[test]
fn malformed_frames_are_refused() {
    assert!(matches!(
        parse_ffmpeg_xml(&xml(&frame(0, &vals(379, "1")))),
        Err(FingerprintError::BadFrame { .. })
    ));
    assert!(matches!(
        parse_ffmpeg_xml(&xml(&frame(0, &vals(381, "1")))),
        Err(FingerprintError::BadFrame { .. })
    ));
    assert!(matches!(
        parse_ffmpeg_xml(&xml(&frame(0, &(vals(379, "1") + " 3")))),
        Err(FingerprintError::BadFrame { .. })
    ));
    let two = frame(512, &vals(380, "0")) + &frame(0, &vals(380, "0"));
    assert!(matches!(
        parse_ffmpeg_xml(&xml(&two)),
        Err(FingerprintError::BadFrame { .. })
    ));
    assert!(parse_ffmpeg_xml(&xml(&frame(0, &vals(380, "2")))).is_ok());
}

#[test]
fn malformed_packed_sets_are_refused() {
    let good = fixture_set().pack().unwrap();
    let mut b = good.clone();
    b[0] = b'X';
    assert!(SignatureSet::unpack(&b).is_err(), "magic");
    let mut b = good.clone();
    b[5] = 2;
    assert!(SignatureSet::unpack(&b).is_err(), "version");
    assert!(
        SignatureSet::unpack(&good[..good.len() - 1]).is_err(),
        "truncated"
    );
    let mut b = good.clone();
    b.push(0);
    assert!(SignatureSet::unpack(&b).is_err(), "trailing byte");
    let mut b = good.clone();
    b[HEADER_BYTES + 9] = 243;
    assert!(SignatureSet::unpack(&b).is_err(), "packed byte > 242");
    let mut b = good.clone();
    b[10..14].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(
        SignatureSet::unpack(&b[..HEADER_BYTES]),
        Err(FingerprintError::Empty)
    );
    let mut b = good;
    b[6..10].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(SignatureSet::unpack(&b), Err(FingerprintError::NoTimeUnit));
}

#[test]
fn frame_times_beyond_a_day_are_refused() {
    // a hostile time would overflow the matcher's time-line bins (and is never a real clip)
    let day = 86_401u64 * 12288;
    assert!(matches!(
        parse_ffmpeg_xml(&xml(&frame(day, &vals(380, "1")))),
        Err(FingerprintError::BadFrame { .. })
    ));
    let mut s = fixture_set();
    s.frames.truncate(1);
    s.frames[0].media_time = day;
    assert!(SignatureSet::unpack(&s.pack().unwrap()).is_err());
}
