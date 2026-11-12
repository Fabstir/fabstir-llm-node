// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The fingerprint matching rule of AP0.1b (`AP0-WATERMARK-RESULTS.md`), ported from the probe's numpy code.
//!
//! Per query frame, the nearest registered frame by L1 distance over the 380 values. A clip query matches a
//! registered clip when at least `min_aligned` query frames whose nearest distance is below `theta` lie on
//! one time line t_r = s * t_q + b, s in {0.8, 1, 1.25}, b in 0.08 s bins (a bin counts its two neighbours).
//! A one-frame query (a screenshot) matches when its best distance is below `theta_single`.
//! Bins round half to even, as numpy's `np.round` does in the probe (an offset of exactly 0.04 s is a tie).
//! Parameters frozen from AP0.1b's exploratory validation; AP4 re-measures them on fresh clips.

use crate::fingerprint::{SignatureSet, TRITS_PER_FRAME};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchParams {
    pub theta: u32,
    pub min_aligned: usize,
    pub theta_single: u32,
}

pub const AP01B: MatchParams = MatchParams {
    theta: 180,
    min_aligned: 16,
    theta_single: 60,
};
const SLOPES: [f64; 3] = [0.8, 1.0, 1.25];
const BIN_SECS: f64 = 0.08;

pub fn l1(a: &[u8; TRITS_PER_FRAME], b: &[u8; TRITS_PER_FRAME]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs())
        .sum()
}

fn times(s: &SignatureSet) -> Vec<f64> {
    s.frames
        .iter()
        .map(|f| f.media_time as f64 / s.media_time_unit as f64)
        .collect()
}

/// (index of the nearest registered frame, its distance) for each query frame.
fn nearest(q: &SignatureSet, r: &SignatureSet) -> Vec<(usize, u32)> {
    q.frames
        .iter()
        .map(|qf| {
            r.frames
                .iter()
                .enumerate()
                .map(|(j, rf)| (j, l1(&qf.trits, &rf.trits)))
                .min_by_key(|(_, d)| *d)
                .expect("registered sets are never empty")
        })
        .collect()
}

/// The time-line bin of a (query time, registered time) pair for slope `s`: numpy's `np.round` semantics
/// (half to even), computed in the probe's operation order.
pub fn bin(t_q: f64, t_r: f64, s: f64) -> i64 {
    ((t_r - s * t_q) / BIN_SECS).round_ties_even() as i64
}

/// The largest number of query frames below `theta` that lie on one time line.
pub fn aligned_votes(q: &SignatureSet, r: &SignatureSet, theta: u32) -> usize {
    let tq = times(q);
    let tr = times(r);
    let nn = nearest(q, r);
    let ok: Vec<(f64, f64)> = nn
        .iter()
        .enumerate()
        .filter(|(_, (_, d))| *d < theta)
        .map(|(i, (j, _))| (tq[i], tr[*j]))
        .collect();
    if ok.is_empty() {
        return 0;
    }
    let mut best = 0;
    for s in SLOPES {
        let mut counts: BTreeMap<i64, usize> = BTreeMap::new();
        for (a, b) in &ok {
            *counts.entry(bin(*a, *b, s)).or_default() += 1;
        }
        for v in counts.keys() {
            let c: usize = counts
                .range(v.saturating_sub(1)..=v.saturating_add(1))
                .map(|(_, n)| *n)
                .sum();
            best = best.max(c);
        }
    }
    best
}

/// The smallest distance between the first query frame and any registered frame.
pub fn single_distance(q: &SignatureSet, r: &SignatureSet) -> u32 {
    r.frames
        .iter()
        .map(|rf| l1(&q.frames[0].trits, &rf.trits))
        .min()
        .expect("registered sets are never empty")
}

pub fn is_match(q: &SignatureSet, r: &SignatureSet, p: MatchParams) -> bool {
    if q.frames.len() == 1 {
        single_distance(q, r) < p.theta_single
    } else {
        aligned_votes(q, r, p.theta) >= p.min_aligned
    }
}
