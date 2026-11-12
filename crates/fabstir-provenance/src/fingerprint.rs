// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The fingerprint value format (plan D3, frozen in AP0.3).
//!
//! Algorithm `org.platformlessai.mpeg7vs.1`: the MPEG-7 video signature (ISO/IEC 15938-3) as computed by the
//! pinned ffmpeg's `signature` filter, default thresholds, after `scale='min(1280,iw)':-2`, over the whole
//! picture stream with any duration limit on the INPUT side (the filter writes its output only on a natural
//! end of input). This is exactly the configuration measured in AP0.1b.
//!
//! Packed set (`PAIFP` v1), all integers big-endian:
//! ```text
//! magic "PAIFP" | version u8 = 1 | media_time_unit u32 (ticks per second) | frame_count u32
//! frame_count x { media_time u64 | confidence u8 | 76 bytes: 380 trits, 5 per byte, t0 + 3 t1 + 9 t2 + 27 t3 + 81 t4 }
//! ```
//! The soft-binding value in the manifest is SHA-256 of these bytes (base64 in the assertion); the signer
//! recomputes it from the set it stores.

use sha2::{Digest, Sha256};

pub const ALG: &str = "org.platformlessai.mpeg7vs.1";
/// The ffmpeg filter graph that defines the algorithm; `{out}` is replaced by the XML output path.
pub const FFMPEG_FILTER: &str = "scale='min(1280,iw)':-2,signature=format=xml:filename={out}";
pub const MAGIC: &[u8; 5] = b"PAIFP";
pub const VERSION: u8 = 1;
pub const TRITS_PER_FRAME: usize = 380;
pub const PACKED_TRIT_BYTES: usize = 76;
pub const FRAME_RECORD_BYTES: usize = 8 + 1 + PACKED_TRIT_BYTES;
pub const HEADER_BYTES: usize = 5 + 1 + 4 + 4;
/// 20 minutes at 60 fps; far beyond any LTX clip, small enough to bound a request.
pub const MAX_FRAMES: usize = 72_000;
/// No frame time beyond one day (a hostile time would overflow the matcher's time-line bins).
pub const MAX_MEDIA_SECS: u64 = 86_400;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FingerprintError {
    #[error("signature XML has no MediaTimeUnit")]
    NoTimeUnit,
    #[error("signature has no frames")]
    Empty,
    #[error("too many frames: {0}")]
    TooManyFrames(usize),
    #[error("frame {frame}: {msg}")]
    BadFrame { frame: usize, msg: String },
    #[error("packed set: {0}")]
    BadPacked(String),
}

/// One frame's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSig {
    pub media_time: u64,
    pub confidence: u8,
    pub trits: [u8; TRITS_PER_FRAME],
}

/// The full signature set of one picture output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureSet {
    pub media_time_unit: u32,
    pub frames: Vec<FrameSig>,
}

fn tag<'a>(s: &'a str, name: &str) -> Option<(&'a str, usize)> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&close)? + a;
    Some((&s[a..b], b + close.len()))
}

/// Parse the XML that ffmpeg's `signature=format=xml` writes. Strict: every `<VideoFrame>` must carry a time,
/// a confidence and exactly 380 values in {0, 1, 2}; times must not decrease.
pub fn parse_ffmpeg_xml(xml: &str) -> Result<SignatureSet, FingerprintError> {
    let unit: u32 = tag(xml, "MediaTimeUnit")
        .and_then(|(v, _)| v.trim().parse().ok())
        .filter(|u| *u > 0)
        .ok_or(FingerprintError::NoTimeUnit)?;
    let mut frames = Vec::new();
    let mut rest = xml;
    while let Some((body, end)) = tag(rest, "VideoFrame") {
        let n = frames.len();
        let bad = |msg: &str| FingerprintError::BadFrame {
            frame: n,
            msg: msg.to_string(),
        };
        let media_time: u64 = tag(body, "MediaTimeOfFrame")
            .and_then(|(v, _)| v.trim().parse().ok())
            .ok_or_else(|| bad("MediaTimeOfFrame"))?;
        let confidence: u8 = tag(body, "FrameConfidence")
            .and_then(|(v, _)| v.trim().parse().ok())
            .ok_or_else(|| bad("FrameConfidence"))?;
        if media_time / unit as u64 > MAX_MEDIA_SECS {
            return Err(bad("time beyond one day"));
        }
        let (sig, _) = tag(body, "FrameSignature").ok_or_else(|| bad("FrameSignature"))?;
        let mut trits = [0u8; TRITS_PER_FRAME];
        let mut k = 0;
        for tok in sig.split_ascii_whitespace() {
            if k == TRITS_PER_FRAME {
                return Err(bad("more than 380 values"));
            }
            trits[k] = match tok {
                "0" => 0,
                "1" => 1,
                "2" => 2,
                _ => return Err(bad("value outside {0,1,2}")),
            };
            k += 1;
        }
        if k != TRITS_PER_FRAME {
            return Err(bad("fewer than 380 values"));
        }
        if let Some(prev) = frames.last().map(|f: &FrameSig| f.media_time) {
            if media_time < prev {
                return Err(bad("time decreases"));
            }
        }
        frames.push(FrameSig {
            media_time,
            confidence,
            trits,
        });
        if frames.len() > MAX_FRAMES {
            return Err(FingerprintError::TooManyFrames(frames.len()));
        }
        rest = &rest[end..];
    }
    if frames.is_empty() {
        return Err(FingerprintError::Empty);
    }
    Ok(SignatureSet {
        media_time_unit: unit,
        frames,
    })
}

impl SignatureSet {
    /// The packed bytes (format above).
    pub fn pack(&self) -> Result<Vec<u8>, FingerprintError> {
        if self.frames.is_empty() {
            return Err(FingerprintError::Empty);
        }
        if self.frames.len() > MAX_FRAMES {
            return Err(FingerprintError::TooManyFrames(self.frames.len()));
        }
        if self.media_time_unit == 0 {
            return Err(FingerprintError::NoTimeUnit);
        }
        let mut out = Vec::with_capacity(HEADER_BYTES + self.frames.len() * FRAME_RECORD_BYTES);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&self.media_time_unit.to_be_bytes());
        out.extend_from_slice(&(self.frames.len() as u32).to_be_bytes());
        for (n, f) in self.frames.iter().enumerate() {
            out.extend_from_slice(&f.media_time.to_be_bytes());
            out.push(f.confidence);
            for chunk in f.trits.chunks(5) {
                let mut b = 0u8;
                let mut w = 1u8;
                for &t in chunk {
                    if t > 2 {
                        return Err(FingerprintError::BadFrame {
                            frame: n,
                            msg: "value outside {0,1,2}".into(),
                        });
                    }
                    b += t * w;
                    w = w.wrapping_mul(3);
                }
                out.push(b);
            }
        }
        Ok(out)
    }

    /// Parse and validate a packed set (what the signer receives).
    pub fn unpack(bytes: &[u8]) -> Result<SignatureSet, FingerprintError> {
        let bad = |m: &str| FingerprintError::BadPacked(m.to_string());
        if bytes.len() < HEADER_BYTES || &bytes[..5] != MAGIC {
            return Err(bad("magic"));
        }
        if bytes[5] != VERSION {
            return Err(bad("version"));
        }
        let unit = u32::from_be_bytes(bytes[6..10].try_into().unwrap());
        if unit == 0 {
            return Err(FingerprintError::NoTimeUnit);
        }
        let count = u32::from_be_bytes(bytes[10..14].try_into().unwrap()) as usize;
        if count == 0 {
            return Err(FingerprintError::Empty);
        }
        if count > MAX_FRAMES {
            return Err(FingerprintError::TooManyFrames(count));
        }
        if bytes.len() != HEADER_BYTES + count * FRAME_RECORD_BYTES {
            return Err(bad("length does not match frame_count"));
        }
        let mut frames = Vec::with_capacity(count);
        for n in 0..count {
            let r = &bytes[HEADER_BYTES + n * FRAME_RECORD_BYTES
                ..HEADER_BYTES + (n + 1) * FRAME_RECORD_BYTES];
            let media_time = u64::from_be_bytes(r[..8].try_into().unwrap());
            if media_time / unit as u64 > MAX_MEDIA_SECS {
                return Err(FingerprintError::BadFrame {
                    frame: n,
                    msg: "time beyond one day".into(),
                });
            }
            if let Some(prev) = frames.last().map(|f: &FrameSig| f.media_time) {
                if media_time < prev {
                    return Err(FingerprintError::BadFrame {
                        frame: n,
                        msg: "time decreases".into(),
                    });
                }
            }
            let mut trits = [0u8; TRITS_PER_FRAME];
            for (i, &b) in r[9..].iter().enumerate() {
                if b > 242 {
                    return Err(FingerprintError::BadFrame {
                        frame: n,
                        msg: "packed byte > 242".into(),
                    });
                }
                let mut v = b;
                for j in 0..5 {
                    trits[i * 5 + j] = v % 3;
                    v /= 3;
                }
            }
            frames.push(FrameSig {
                media_time,
                confidence: r[8],
                trits,
            });
        }
        Ok(SignatureSet {
            media_time_unit: unit,
            frames,
        })
    }
}

/// SHA-256 of a packed set: the soft-binding value.
pub fn digest(packed: &[u8]) -> [u8; 32] {
    Sha256::digest(packed).into()
}
