// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Deep EXR input (D6): which frames are judged, and the conversion of one frame to an 8-bit PNG the still-image
//! path can read. The conversion scales the frame so its own 99th percentile reaches full scale, then applies the
//! display curve when the values are scene-linear, so a dark (or an over-bright) frame cannot hide content.

use std::collections::BTreeSet;
use std::io::Cursor;

/// D6: at most this many frames of one deep input are judged.
pub const MAX_SAMPLED: usize = 64;
/// Refuse to allocate for an absurd header (8192 x 8192).
const MAX_PIXELS: usize = 8192 * 8192;
/// The longest side sent to the service (its models look at far smaller pictures).
const MAX_SIDE: u32 = 1536;

/// The frames judged (D6, fix F6): the first, the last, and one frame chosen AT RANDOM from each of up to 62 equal
/// stretches of the rest. A stretch is at most one second long until the cap of [`MAX_SAMPLED`] binds, so there is
/// at least one judged frame per second on average, no frame is safe from being judged, and which frames are
/// judged cannot be predicted from the input. The caller draws from the operating system's randomness, once per
/// job.
pub fn sampled_indices(total: usize, fps: u32, rng: &mut impl rand::Rng) -> BTreeSet<usize> {
    let mut picked = BTreeSet::new();
    if total == 0 {
        return picked;
    }
    picked.insert(0);
    picked.insert(total - 1);
    if total <= 2 {
        return picked;
    }
    // the frames strictly between the first and the last
    let inner = total - 2;
    let seconds = total.div_ceil(fps.max(1) as usize);
    let stretches = seconds.clamp(1, MAX_SAMPLED - 2).min(inner);
    for k in 0..stretches {
        let (lo, hi) = (k * inner / stretches, (k + 1) * inner / stretches);
        picked.insert(1 + rng.gen_range(lo..hi.max(lo + 1)));
    }
    picked
}

/// The gain that brings `p99` to full scale. A black frame (nothing above zero) keeps gain 1.
pub fn gain_for(p99: f32) -> f32 {
    if p99.is_finite() && p99 > 1e-6 {
        1.0 / p99
    } else {
        1.0
    }
}

/// One channel value to 8 bits: gain, clamp, then the display curve for scene-linear input.
pub fn to_display_u8(v: f32, gain: f32, scene_linear: bool) -> u8 {
    let v = if v.is_nan() { 0.0 } else { v };
    let mut x = (v * gain).clamp(0.0, 1.0);
    if scene_linear {
        x = x.powf(1.0 / 2.2);
    }
    (x * 255.0).round() as u8
}

/// The 99th percentile of the per-pixel peak channel (finite values only).
pub fn percentile_99(peaks: &mut [f32]) -> f32 {
    if peaks.is_empty() {
        return 0.0;
    }
    let k = ((peaks.len() - 1) as f64 * 0.99).round() as usize;
    let (_, v, _) = peaks.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
    *v
}

/// Convert one EXR frame (first RGB layer) to an 8-bit PNG. Blocking and CPU-bound: call it off the async workers.
pub fn exr_to_png(bytes: &[u8], scene_linear: bool) -> Result<Vec<u8>, String> {
    use exr::prelude::*;
    struct Px {
        w: usize,
        h: usize,
        rgb: Vec<[f32; 3]>,
    }
    let image = read()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |res: Vec2<usize>, _| {
                let n = res.width().saturating_mul(res.height());
                Px {
                    w: res.width(),
                    h: res.height(),
                    // an over-sized header allocates nothing; it is refused below
                    rgb: vec![[0.0; 3]; if n <= MAX_PIXELS { n } else { 0 }],
                }
            },
            |px: &mut Px, pos: Vec2<usize>, (r, g, b, _a): (f32, f32, f32, f32)| {
                if let Some(slot) = px.rgb.get_mut(pos.y() * px.w + pos.x()) {
                    *slot = [r, g, b];
                }
            },
        )
        .first_valid_layer()
        .all_attributes()
        .from_buffered(Cursor::new(bytes))
        .map_err(|e| format!("unreadable EXR ({e})"))?;
    let px = image.layer_data.channel_data.pixels;
    if px.w == 0 || px.h == 0 || px.rgb.len() != px.w * px.h {
        return Err("EXR frame is empty or too large".to_string());
    }
    let mut peaks: Vec<f32> = px
        .rgb
        .iter()
        .map(|c| c[0].max(c[1]).max(c[2]))
        .filter(|v| v.is_finite())
        .collect();
    let gain = gain_for(percentile_99(&mut peaks));
    let mut out = Vec::with_capacity(px.rgb.len() * 3);
    for c in &px.rgb {
        out.extend(c.iter().map(|v| to_display_u8(*v, gain, scene_linear)));
    }
    let img = image::RgbImage::from_raw(px.w as u32, px.h as u32, out)
        .ok_or_else(|| "EXR frame size mismatch".to_string())?;
    let img = if img.width().max(img.height()) > MAX_SIDE {
        let s = MAX_SIDE as f64 / img.width().max(img.height()) as f64;
        image::imageops::thumbnail(
            &img,
            ((img.width() as f64 * s).round() as u32).max(1),
            ((img.height() as f64 * s).round() as u32).max(1),
        )
    } else {
        img
    };
    let mut png = Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| format!("PNG encode failed ({e})"))?;
    Ok(png.into_inner())
}
