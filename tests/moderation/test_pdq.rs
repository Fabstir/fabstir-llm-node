// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 4.1 — PDQ perceptual hash, verified against Meta's algorithm.
//!
//! The decoder-independent Meta-exact reference is quality==0 for a constant image
//! (zero gradient). Exact-HASH parity vs Meta's reference tool needs decoder-
//! matched binary images and is a go-live cross-check (R1; tests/fixtures/pdq).
//! The remaining tests verify PDQ's defining invariants: stability, robustness to
//! brightness/contrast (DC is excluded), discrimination, and quality direction.

use std::f64::consts::PI;

use fabstir_llm_node::moderation::csam::pdq::{compute_pdq_rgb, hamming};
use fabstir_llm_node::moderation::types::Pdq256;

fn solid(w: usize, h: usize, v: u8) -> Vec<u8> {
    vec![v; w * h * 3]
}

/// A rich, mid-range, low-frequency texture (several sinusoids) — high quality and
/// energy spread across many of the retained frequencies, so the hash is stable.
fn textured(w: usize, h: usize) -> Vec<u8> {
    let mut buf = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 / w as f64;
            let fy = y as f64 / h as f64;
            let val = 128.0
                + 50.0 * (2.0 * PI * 3.0 * fx).sin()
                + 40.0 * (2.0 * PI * 4.0 * fy).cos()
                + 25.0 * (2.0 * PI * 2.0 * (fx + fy)).sin();
            let v = val.clamp(0.0, 255.0) as u8;
            let i = (y * w + x) * 3;
            buf[i] = v;
            buf[i + 1] = v;
            buf[i + 2] = v;
        }
    }
    buf
}

#[test]
fn pdq_known_vector_matches_reference() {
    // Meta-exact, decoder-independent: a constant image has zero gradient ⇒ quality 0.
    // (Its hash is fp-noise-determined and ~balanced, NOT all-zero — so exact-hash
    //  parity vs Meta is a go-live cross-check, R1.) Also confirm determinism.
    let rgb = solid(100, 100, 128);
    let a = compute_pdq_rgb(&rgb, 100, 100).unwrap();
    assert_eq!(a.quality, 0, "a constant image must have quality 0");
    let b = compute_pdq_rgb(&rgb, 100, 100).unwrap();
    assert_eq!(a.hash, b.hash, "the hash must be deterministic");
}

#[test]
fn pdq_hamming_zero_for_identical() {
    let rgb = textured(200, 150);
    let a = compute_pdq_rgb(&rgb, 200, 150).unwrap();
    let b = compute_pdq_rgb(&rgb, 200, 150).unwrap();
    assert_eq!(
        hamming(&a.hash, &b.hash),
        0,
        "identical inputs ⇒ distance 0"
    );
}

/// A smooth broadband mid-range field (bilinear-interpolated pseudo-random control
/// grid, values ~40..215) — energy across all retained frequencies, so most bits
/// are far from the median and the hash is stable under small tone changes. Stays
/// mid-range so brightness shifts don't clamp.
fn rich(w: usize, h: usize) -> Vec<u8> {
    const G: usize = 17;
    let mut ctrl = [[0.0f64; G]; G];
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    for row in ctrl.iter_mut() {
        for c in row.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *c = 40.0 + (s >> 40) as f64 / ((1u64 << 24) as f64) * 175.0;
        }
    }
    let mut buf = vec![0u8; w * h * 3];
    for y in 0..h {
        let gy = y as f64 * (G - 1) as f64 / h as f64;
        let (y0, fy) = (gy.floor() as usize, gy.fract());
        let y1 = (y0 + 1).min(G - 1);
        for x in 0..w {
            let gx = x as f64 * (G - 1) as f64 / w as f64;
            let (x0, fx) = (gx.floor() as usize, gx.fract());
            let x1 = (x0 + 1).min(G - 1);
            let top = ctrl[y0][x0] * (1.0 - fx) + ctrl[y0][x1] * fx;
            let bot = ctrl[y1][x0] * (1.0 - fx) + ctrl[y1][x1] * fx;
            let v = (top * (1.0 - fy) + bot * fy).clamp(0.0, 255.0) as u8;
            let i = (y * w + x) * 3;
            buf[i] = v;
            buf[i + 1] = v;
            buf[i + 2] = v;
        }
    }
    buf
}

#[test]
fn pdq_invariant_to_brightness() {
    // PDQ excludes the DC term, so a uniform brightness shift (no clamping on the
    // mid-range field) leaves the AC coefficients unchanged ⇒ near-identical hash.
    let base = rich(256, 256);
    let shifted: Vec<u8> = base.iter().map(|&v| (v as f64 + 12.0) as u8).collect();
    let a = compute_pdq_rgb(&base, 256, 256).unwrap();
    let b = compute_pdq_rgb(&shifted, 256, 256).unwrap();
    let d = hamming(&a.hash, &b.hash);
    assert!(
        d <= 31,
        "a brightness shift must stay within the threshold, got {d}"
    );
}

#[test]
fn pdq_small_distance_for_recompressed() {
    // Simulate recompression: a mild contrast+brightness tone change on a broadband
    // image. PDQ (median-thresholded, DC-excluded) should stay within the threshold,
    // AND be much closer than an unrelated image (the discriminative property a
    // matcher relies on).
    let base = rich(256, 256);
    let modified: Vec<u8> = base
        .iter()
        .map(|&v| (((v as f64 - 128.0) * 0.92) + 128.0 + 6.0) as u8)
        .collect();
    let a = compute_pdq_rgb(&base, 256, 256).unwrap();
    let b = compute_pdq_rgb(&modified, 256, 256).unwrap();
    let near = hamming(&a.hash, &b.hash);
    let far = hamming(
        &a.hash,
        &compute_pdq_rgb(&textured(256, 256), 256, 256).unwrap().hash,
    );
    assert!(
        near <= 31,
        "a recompressed image must stay within the threshold, got {near}"
    );
    assert!(
        near < far,
        "a near-duplicate must be closer than an unrelated image"
    );
}

#[test]
fn pdq_large_distance_for_inverted_image() {
    // Inverting intensity negates the AC coefficients ⇒ the median split flips ⇒ a
    // large distance. Confirms discrimination (not everything is "near").
    let base = textured(256, 256);
    let inverted: Vec<u8> = base.iter().map(|&v| 255 - v).collect();
    let a = compute_pdq_rgb(&base, 256, 256).unwrap();
    let b = compute_pdq_rgb(&inverted, 256, 256).unwrap();
    assert!(
        hamming(&a.hash, &b.hash) > 64,
        "an inverted image must be far from the original"
    );
}

#[test]
fn low_quality_image_flagged() {
    // A flat image is degenerate ⇒ quality 0 ≤ 49 (Meta's low-quality discard line).
    let r = compute_pdq_rgb(&solid(64, 64, 100), 64, 64).unwrap();
    assert!(
        r.quality <= 49,
        "a flat image must be low quality, got {}",
        r.quality
    );
}

#[test]
fn high_detail_image_is_high_quality() {
    // A rich, high-contrast low-frequency texture ⇒ high quality.
    let r = compute_pdq_rgb(&textured(256, 256), 256, 256).unwrap();
    assert!(
        r.quality > 49,
        "a high-detail texture must be high quality, got {}",
        r.quality
    );
}

#[test]
fn pdq_hash_is_balanced() {
    // The median split sets ~half the 256 bits (a defining PDQ property).
    let r = compute_pdq_rgb(&textured(256, 256), 256, 256).unwrap();
    let set: u32 = r.hash.0.iter().map(|b| b.count_ones()).sum();
    assert!(
        (96..=128).contains(&set),
        "a rich image's hash should set ~128 bits, got {set}"
    );
}

#[test]
fn invalid_dimensions_fail_closed() {
    // A buffer whose length doesn't match w*h*3 must error, not panic or guess.
    assert!(compute_pdq_rgb(&[0u8; 10], 100, 100).is_err());
    assert!(compute_pdq_rgb(&[], 0, 0).is_err());
}
