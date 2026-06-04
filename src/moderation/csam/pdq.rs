// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! PDQ perceptual image hash — from-scratch port of Meta's algorithm (D4: no FFT
//! crate; DCT via matrix multiply). 🚨
//!
//! Pipeline (faithful to `facebook/ThreatExchange/pdq`, see tests/fixtures/pdq):
//!   RGB → BT.601 luma (f64) → 64×64 via 2-pass separable box blur (Jarosz tent)
//!   then decimate → 64×64 DCT-II → extract frequency 1..16 × 1..16 (DC excluded)
//!   → median (Torben rank (n+1)/2, strict `>`) → 256-bit hash; quality from the
//!   integer-truncated gradient of the 64×64 buffer.
//!
//! Everything is computed in `f64` (an `f32` pipeline drifts a few bits). Exact
//! bit-for-bit Meta parity on natural JPEGs (decoder-dependent) is a go-live
//! cross-check, not a build gate (R1, tests/fixtures/pdq/README.md).

use std::f64::consts::PI;

use crate::moderation::types::{ModerationError, Pdq256, Result};

const N: usize = 64; // downsample target (NxN)
const K: usize = 16; // low-frequency block (KxK), frequencies 1..=16

/// A PDQ hash plus its quality score (0..=100; Meta discards `quality <= 49`).
pub struct PdqResult {
    pub hash: Pdq256,
    pub quality: u32,
}

/// Compute the PDQ hash + quality of a row-major RGB8 buffer. Fail-closed: a
/// zero dimension or a buffer whose length != `width*height*3` is an error.
pub fn compute_pdq_rgb(rgb: &[u8], width: u32, height: u32) -> Result<PdqResult> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgb.len() != w * h * 3 {
        return Err(ModerationError::DecodeFailed(format!(
            "bad rgb buffer: {} bytes for {width}x{height}",
            rgb.len()
        )));
    }
    let luma = luminance(rgb, w, h);
    let buf64 = decimate_to_64(&luma, w, h);
    let coeffs = dct_16x16(&buf64);
    let hash = bits_from_coeffs(&coeffs);
    let quality = quality_score(&buf64);
    Ok(PdqResult { hash, quality })
}

/// Hamming distance between two PDQ hashes (0..=256).
pub fn hamming(a: &Pdq256, b: &Pdq256) -> u32 {
    a.0.iter()
        .zip(b.0.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

/// BT.601 luma on raw RGB (no gamma), as `f64` in 0..=255.
fn luminance(rgb: &[u8], w: usize, h: usize) -> Vec<f64> {
    let mut out = vec![0.0f64; w * h];
    for (px, o) in rgb.chunks_exact(3).zip(out.iter_mut()) {
        *o = 0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64;
    }
    out
}

/// Jarosz filter window size for one axis (Meta `computeJaroszFilterWindowSize`,
/// i.e. `ceil(old / (2*64))`, floored at 1).
fn jarosz_window(old: usize) -> usize {
    old.div_ceil(2 * N).max(1)
}

/// Separable box blur of one row span `[base, base+len)` stride 1, radius `r`.
fn box_blur_axis(src: &[f64], dst: &mut [f64], len: usize, stride: usize, base: usize, r: usize) {
    // prefix sums along the axis
    let mut prefix = vec![0.0f64; len + 1];
    for i in 0..len {
        prefix[i + 1] = prefix[i] + src[base + i * stride];
    }
    for i in 0..len {
        let lo = i.saturating_sub(r);
        let hi = (i + r + 1).min(len);
        dst[base + i * stride] = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
    }
}

/// 2-pass separable box blur (Jarosz tent) then decimate to 64×64.
fn decimate_to_64(luma: &[f64], w: usize, h: usize) -> Vec<f64> {
    let (rx, ry) = (jarosz_window(w) / 2, jarosz_window(h) / 2);
    let mut buf = luma.to_vec();
    let mut tmp = vec![0.0f64; w * h];
    for _ in 0..2 {
        if rx > 0 {
            for y in 0..h {
                box_blur_axis(&buf, &mut tmp, w, 1, y * w, rx);
            }
            buf.copy_from_slice(&tmp);
        }
        if ry > 0 {
            for x in 0..w {
                box_blur_axis(&buf, &mut tmp, h, w, x, ry);
            }
            buf.copy_from_slice(&tmp);
        }
    }
    let mut out = vec![0.0f64; N * N];
    for (oy, row) in out.chunks_exact_mut(N).enumerate() {
        let sy = (((oy as f64 + 0.5) * h as f64 / N as f64) as usize).min(h - 1);
        for (ox, cell) in row.iter_mut().enumerate() {
            let sx = (((ox as f64 + 0.5) * w as f64 / N as f64) as usize).min(w - 1);
            *cell = buf[sy * w + sx];
        }
    }
    out
}

/// 64×64 DCT-II, returning the frequency 1..=16 × 1..=16 block (DC excluded), 256 coeffs.
fn dct_16x16(a: &[f64]) -> [f64; K * K] {
    // D16[f][x] = sqrt(2/64) * cos(pi*(f+1)*(2x+1)/128), f in 0..16, x in 0..64.
    let scale = (2.0 / N as f64).sqrt();
    let mut d16 = [[0.0f64; N]; K];
    for (f, row) in d16.iter_mut().enumerate() {
        for (x, c) in row.iter_mut().enumerate() {
            *c = scale * (PI * (f as f64 + 1.0) * (2.0 * x as f64 + 1.0) / (2.0 * N as f64)).cos();
        }
    }
    // P[f][col] = sum_row D16[f][row] * A[row][col]
    let mut p = [[0.0f64; N]; K];
    for f in 0..K {
        for col in 0..N {
            let mut s = 0.0;
            for row in 0..N {
                s += d16[f][row] * a[row * N + col];
            }
            p[f][col] = s;
        }
    }
    // C[fr][fc] = sum_col P[fr][col] * D16[fc][col]
    let mut coeffs = [0.0f64; K * K];
    for fr in 0..K {
        for fc in 0..K {
            let mut s = 0.0;
            for col in 0..N {
                s += p[fr][col] * d16[fc][col];
            }
            coeffs[fr * K + fc] = s;
        }
    }
    coeffs
}

/// 256-bit hash: bit k set iff `coeffs[k] > median`, median = Torben rank
/// `(n+1)/2 = 128` (= ascending `sorted[127]`), strict `>` (ties → 0). Row-major,
/// MSB-first within each byte.
fn bits_from_coeffs(coeffs: &[f64; K * K]) -> Pdq256 {
    let mut sorted = *coeffs;
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[(K * K).div_ceil(2) - 1]; // Torben rank (n+1)/2 = 128 ⇒ index 127
    let mut bytes = [0u8; 32];
    for (k, &c) in coeffs.iter().enumerate() {
        if c > median {
            bytes[k / 8] |= 1 << (7 - (k % 8));
        }
    }
    Pdq256(bytes)
}

/// Quality: integer-truncated gradient sum over the 64×64 buffer, scaled / clamped.
fn quality_score(buf: &[f64]) -> u32 {
    let mut sum: i64 = 0;
    for i in 0..N - 1 {
        for j in 0..N {
            let d = ((buf[i * N + j] - buf[(i + 1) * N + j]) * 100.0 / 255.0) as i64;
            sum += d.abs();
        }
    }
    for i in 0..N {
        for j in 0..N - 1 {
            let d = ((buf[i * N + j] - buf[i * N + j + 1]) * 100.0 / 255.0) as i64;
            sum += d.abs();
        }
    }
    (sum / 90).min(100) as u32
}
