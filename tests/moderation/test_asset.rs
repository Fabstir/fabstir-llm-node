// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 5.1 — asset intake moderation (image hash+match; `.vtt` text scan). 🚨

use fabstir_llm_node::moderation::asset::{AssetModerator, TextScanList};
use fabstir_llm_node::moderation::csam::hashlist::HashListSource;
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;
use fabstir_llm_node::moderation::types::{AssetKind, Verdict};

/// Encode a solid-colour PNG so we have real, decodable image bytes.
fn png(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(color));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .expect("encode png");
    buf.into_inner()
}

fn clean_list() -> MockHashListSource {
    MockHashListSource::loaded(vec![], vec![]) // legitimately-empty Loaded list
}

#[test]
fn image_match_blocks() {
    let bytes = png(8, 8, [100, 150, 200]);
    let sha = Matcher::sha256(&bytes); // exact match on the file bytes
    let snap = MockHashListSource::loaded(vec![sha], vec![])
        .refresh()
        .unwrap();
    let am = AssetModerator::new(snap, OwnHashList::new(), 31, TextScanList::launch_mock());
    assert_eq!(
        am.moderate(AssetKind::Image, &bytes).verdict,
        Verdict::Blocked
    );
}

#[test]
fn image_clean_clears() {
    let bytes = png(8, 8, [10, 20, 30]);
    let snap = clean_list().refresh().unwrap();
    let am = AssetModerator::new(snap, OwnHashList::new(), 31, TextScanList::launch_mock());
    assert_eq!(
        am.moderate(AssetKind::Image, &bytes).verdict,
        Verdict::Cleared
    );
}

#[test]
fn decode_failure_holds() {
    // An undecodable image must fail-closed (hold), never clear.
    let snap = clean_list().refresh().unwrap();
    let am = AssetModerator::new(snap, OwnHashList::new(), 31, TextScanList::launch_mock());
    let v = am
        .moderate(AssetKind::Image, b"this is definitely not an image")
        .verdict;
    assert_ne!(v, Verdict::Cleared, "an undecodable image must hold");
}

#[test]
fn vtt_bad_url_flags() {
    let snap = clean_list().refresh().unwrap();
    let am = AssetModerator::new(snap, OwnHashList::new(), 31, TextScanList::launch_mock());
    let bad = TextScanList::MOCK_BAD_URLS[0];
    let vtt = format!("WEBVTT\n\n00:00.000 --> 00:01.000\nsee {bad} for more\n");
    assert_eq!(
        am.moderate(AssetKind::Subtitle, vtt.as_bytes()).verdict,
        Verdict::Flagged
    );
}

#[test]
fn vtt_clean_clears() {
    let snap = clean_list().refresh().unwrap();
    let am = AssetModerator::new(snap, OwnHashList::new(), 31, TextScanList::launch_mock());
    let vtt = "WEBVTT\n\n00:00.000 --> 00:01.000\nA perfectly normal subtitle line.\n";
    assert_eq!(
        am.moderate(AssetKind::Subtitle, vtt.as_bytes()).verdict,
        Verdict::Cleared
    );
}

#[test]
fn subtitle_invalid_utf8_holds() {
    // Non-UTF-8 subtitle bytes can't be scanned ⇒ fail-closed hold.
    let snap = clean_list().refresh().unwrap();
    let am = AssetModerator::new(snap, OwnHashList::new(), 31, TextScanList::launch_mock());
    let v = am
        .moderate(AssetKind::Subtitle, &[0xff, 0xfe, 0x00])
        .verdict;
    assert_ne!(v, Verdict::Cleared, "unscannable subtitle must hold");
}
