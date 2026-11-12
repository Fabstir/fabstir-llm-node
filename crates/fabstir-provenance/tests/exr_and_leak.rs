// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! D9 EXR header surgery and D8(c) leak guard. Mutations: a part left unmarked, offsets not shifted, pixel
//! bytes touched, D9a padding skipped, the guard matching only the unescaped token or scanning mdat.
use exr::prelude::*;
use fabstir_provenance::exr_header::{mark, read_records, ExrError};
use fabstir_provenance::exr_record::needs_pad;
use fabstir_provenance::leak_guard::{check_mp4, Leak};
use std::io::Cursor;

fn rgba_layer(name: &str, w: usize, h: usize, seed: f32) -> Layer<AnyChannels<FlatSamples>> {
    let n = w * h;
    let ch = |c: &str, k: f32| AnyChannel::new(c, FlatSamples::F16((0..n).map(|i| f16::from_f32(((i as f32) * 0.37 + k + seed).sin())).collect()));
    let chans = AnyChannels::sort(smallvec::smallvec![ch("R", 0.0), ch("G", 1.0), ch("B", 2.0), ch("A", 3.0)]);
    Layer::new((w, h), LayerAttributes::named(name), Encoding::SMALL_LOSSLESS, chans)
}

fn write(image: &Image<Layers<AnyChannels<FlatSamples>>>) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    image.write().to_buffered(&mut c).unwrap();
    c.into_inner()
}

fn single() -> Vec<u8> {
    write(&Image::from_layers(ImageAttributes::new(IntegerBounds::from_dimensions((64, 40))), smallvec::smallvec![rgba_layer("rgba", 64, 40, 0.0)]))
}

fn multi() -> Vec<u8> {
    write(&Image::from_layers(
        ImageAttributes::new(IntegerBounds::from_dimensions((32, 24))),
        smallvec::smallvec![rgba_layer("beauty", 32, 24, 0.0), rgba_layer("matte", 32, 24, 5.0)],
    ))
}

fn pixels(b: &[u8]) -> Vec<Vec<FlatSamples>> {
    let img = read().no_deep_data().largest_resolution_level().all_channels().all_layers().all_attributes().from_buffered(Cursor::new(b.to_vec())).unwrap();
    img.layer_data.iter().map(|l| l.channel_data.list.iter().map(|c| c.sample_data.clone()).collect()).collect()
}

const REC: &[u8] = br#"{"class":"generated","schema":"platformlessai.exr-provenance.v1"}"#;

fn rec(pad: usize) -> Vec<u8> {
    let mut v = REC.to_vec();
    v.extend(std::iter::repeat_n(b' ', pad));
    v
}

#[test]
fn exr_frame_has_provenance_record() {
    let marked = mark(&single(), &rec).unwrap();
    assert_eq!(read_records(&marked).unwrap(), vec![Some(REC.to_vec())]);
    assert_eq!(read_records(&single()).unwrap(), vec![None]);
}

#[test]
fn every_part_of_a_multipart_file_has_the_record() {
    let marked = mark(&multi(), &rec).unwrap();
    assert_eq!(read_records(&marked).unwrap(), vec![Some(REC.to_vec()), Some(REC.to_vec())]);
}

#[test]
fn exr_surgery_is_lossless() {
    for src in [single(), multi()] {
        let marked = mark(&src, &rec).unwrap();
        // an independent reader decodes the same pixels from the marked file
        assert_eq!(pixels(&marked), pixels(&src));
        // and the chunk bytes after the tables are byte-identical
        let tail = src.len() - 64;
        assert_eq!(&marked[marked.len() - (src.len() - tail)..], &src[tail..]);
    }
}

#[test]
fn a_frame_is_marked_once() {
    let marked = mark(&single(), &rec).unwrap();
    assert!(matches!(mark(&marked, &rec), Err(ExrError::AlreadyMarked(_))));
}

#[test]
fn deep_files_are_refused() {
    // a minimal single-part header whose `type` attribute says deepscanline
    let mut b = vec![0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0];
    b.extend_from_slice(b"type\0string\0");
    b.extend_from_slice(&12i32.to_le_bytes());
    b.extend_from_slice(b"deepscanline");
    b.push(0);
    b.extend_from_slice(&[0u8; 16]);
    assert_eq!(mark(&b, &rec), Err(ExrError::Unsupported("deep data")));
}

#[test]
fn not_exr_and_truncated_files_are_refused() {
    assert_eq!(mark(b"not an exr file at all", &rec), Err(ExrError::NotExr));
    let s = single();
    assert!(mark(&s[..s.len() / 2], &rec).is_err());
    assert!(mark(&s[..40], &rec).is_err());
}

#[test]
fn marked_frames_never_land_on_a_256k_multiple() {
    // choose a record length that makes the first attempt land exactly on a multiple, then check D9a
    let src = single();
    let first = mark(&src, &rec).unwrap().len();
    let target = first.div_ceil(262_144) * 262_144;
    let extra = target - first;
    let long = |pad: usize| {
        let mut v = REC.to_vec();
        v.extend(std::iter::repeat_n(b' ', extra + pad));
        v
    };
    let out = mark(&src, &long).unwrap();
    assert!(!needs_pad(out.len()));
    assert_eq!(out.len(), target + 1);
}

// ------------------------------------------------------------------------------------------ leak guard

fn bx(t: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(t);
    v.extend_from_slice(body);
    v
}

#[test]
fn refuses_mp4_with_escaped_comfy_graph() {
    // VHS writes the graph double-encoded into the moov/udta metadata
    let escaped = br#"{\"3\": {\"inputs\": {\"seed\": 42}, \"class_type\": \"KSampler\"}}"#;
    let mp4 = [bx(b"ftyp", b"isom\0\0\x02\0"), bx(b"moov", &bx(b"udta", escaped)), bx(b"mdat", &[0u8; 64])].concat();
    assert_eq!(check_mp4(&mp4), Err(Leak::Graph("moov".into())));
    let weights = [bx(b"ftyp", b"isom\0\0\x02\0"), bx(b"moov", b"ltx-2.3-22b-dev-fp8.safetensors"), bx(b"mdat", &[0u8; 8])].concat();
    assert!(check_mp4(&weights).is_err());
}

#[test]
fn guard_ignores_picture_data_and_passes_clean_files() {
    let mdat_lookalike = [bx(b"ftyp", b"isom\0\0\x02\0"), bx(b"moov", b"clean"), bx(b"mdat", b"...class_type... random bytes")].concat();
    assert_eq!(check_mp4(&mdat_lookalike), Ok(()));
    let tiny = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.mp4")).unwrap();
    assert_eq!(check_mp4(&tiny), Ok(()));
    assert_eq!(check_mp4(&tiny[..tiny.len() - 3]), Err(Leak::Malformed));
}
