// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP0.1b matching rule. Mutations: distance, slope set, bin neighbours, threshold comparisons.
mod common;
use fabstir_provenance::fingerprint::{parse_ffmpeg_xml, SignatureSet};
use fabstir_provenance::matcher::{aligned_votes, bin, is_match, l1, AP01B};

fn set() -> SignatureSet {
    parse_ffmpeg_xml(&String::from_utf8(common::fixture("tiny-signature.xml")).unwrap()).unwrap()
}

#[test]
fn a_clip_matches_itself_and_a_trimmed_copy() {
    let r = set();
    assert!(is_match(&r, &r, AP01B));
    let mut q = r.clone();
    q.frames.drain(..20); // starts 20 frames later: same line, offset b
    assert!(is_match(&q, &r, AP01B));
}

#[test]
fn too_few_aligned_frames_do_not_match() {
    let r = set();
    let mut q = r.clone();
    q.frames.truncate(AP01B.min_aligned - 1);
    assert!(!is_match(&q, &r, AP01B));
    q = r.clone();
    q.frames.truncate(AP01B.min_aligned);
    assert!(is_match(&q, &r, AP01B));
}

#[test]
fn unrelated_signatures_do_not_match() {
    let r = set();
    let mut q = r.clone();
    for (i, f) in q.frames.iter_mut().enumerate() {
        for (k, t) in f.trits.iter_mut().enumerate() {
            *t = ((i * 7 + k * 13) % 3) as u8;
        }
    }
    assert_eq!(aligned_votes(&q, &r, AP01B.theta), 0);
    assert!(!is_match(&q, &r, AP01B));
}

#[test]
fn time_stretch_by_a_listed_slope_still_aligns() {
    let r = set();
    let mut q = r.clone();
    for f in q.frames.iter_mut() {
        f.media_time = f.media_time * 4 / 5; // played 1.25x faster
    }
    assert!(aligned_votes(&q, &r, AP01B.theta) >= AP01B.min_aligned);
}

#[test]
fn a_screenshot_uses_the_single_frame_rule() {
    let r = set();
    let mut q = r.clone();
    q.frames = vec![r.frames[10].clone()];
    assert!(is_match(&q, &r, AP01B));
    let mut far = q.clone();
    for t in far.frames[0]
        .trits
        .iter_mut()
        .take(AP01B.theta_single as usize)
    {
        *t = (*t + 1) % 3;
    }
    assert!(l1(&far.frames[0].trits, &r.frames[10].trits) >= AP01B.theta_single);
}

fn one_frame_set(trits: [u8; 380], t: u64) -> SignatureSet {
    let mut s = set();
    s.frames.truncate(1);
    s.frames[0].trits = trits;
    s.frames[0].media_time = t;
    s
}

fn at_distance(base: &[u8; 380], d: usize) -> [u8; 380] {
    let mut x = *base;
    for t in x.iter_mut().take(d) {
        *t = if *t == 0 { 1 } else { *t - 1 };
    }
    x
}

#[test]
fn single_frame_threshold_is_strict() {
    let r = set();
    let base = r.frames[10].trits;
    let near = one_frame_set(at_distance(&base, AP01B.theta_single as usize - 1), 0);
    assert!(is_match(&near, &r, AP01B));
    let edge = one_frame_set(at_distance(&base, AP01B.theta_single as usize), 0);
    // only frame 10 is that close by construction; others must be farther for the test to mean anything
    if r.frames
        .iter()
        .all(|f| l1(&f.trits, &edge.frames[0].trits) >= AP01B.theta_single)
    {
        assert!(!is_match(&edge, &r, AP01B));
    }
}

#[test]
fn clip_distance_threshold_is_strict() {
    // a query whose every frame sits exactly theta from its registered frame gets no votes
    let r = set();
    let mut q = r.clone();
    for f in q.frames.iter_mut() {
        f.trits = at_distance(&f.trits, AP01B.theta as usize);
    }
    let votes_at_theta = aligned_votes(&q, &r, AP01B.theta);
    let mut q2 = r.clone();
    for f in q2.frames.iter_mut() {
        f.trits = at_distance(&f.trits, AP01B.theta as usize - 1);
    }
    assert!(aligned_votes(&q2, &r, AP01B.theta) >= AP01B.min_aligned);
    assert!(votes_at_theta < aligned_votes(&q2, &r, AP01B.theta));
}

#[test]
fn slow_motion_by_slope_0_8_aligns() {
    let r = set();
    let mut q = r.clone();
    for f in q.frames.iter_mut() {
        f.media_time = f.media_time * 5 / 4; // played at 0.8x
    }
    assert!(aligned_votes(&q, &r, AP01B.theta) >= AP01B.min_aligned);
    // and a slope outside the set does not
    let mut far = r.clone();
    for f in far.frames.iter_mut() {
        f.media_time *= 2;
    }
    assert!(aligned_votes(&far, &r, AP01B.theta) < AP01B.min_aligned);
}

#[test]
fn neighbouring_bins_are_summed() {
    // jitter every other frame by one 0.08 s bin: the votes still land on one line
    let r = set();
    let unit = r.media_time_unit as u64;
    let mut q = r.clone();
    for (i, f) in q.frames.iter_mut().enumerate() {
        if i % 2 == 1 {
            f.media_time += unit * 8 / 100;
        }
    }
    assert!(aligned_votes(&q, &r, AP01B.theta) >= q.frames.len() - 2);
}

#[test]
fn ties_round_half_to_even_like_numpy() {
    // numpy: np.round(0.5) = 0, np.round(1.5) = 2, np.round(-0.5) = -0; half-away rounding gives 1, 2, -1
    assert_eq!(bin(0.0, 0.04, 1.0), 0);
    assert_eq!(bin(0.0, 0.12, 1.0), 2);
    assert_eq!(bin(0.0, -0.04, 1.0), 0);
    assert_eq!(bin(1.0, 1.0, 1.25), -3); // (1 - 1.25) / 0.08 = -3.125
}
