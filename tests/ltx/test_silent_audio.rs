// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NM1 D23 — control clips whose audio cannot be passed through. On ComfyUI 0.38 a linked
//! VHS_LoadVideo AUDIO output is iterated by the core's PromptModelTracker, and VHS's LazyAudioMap
//! raises when the clip has no audio, an empty track, or a layout it cannot map (anything but
//! mono/stereo); core SaveVideo fails AFTER the full render on 3/4/8-channel audio (sdr2hdr). The
//! node reads the clip's audio tracks (`audio_passthrough_ok`) and, for a clip whose audio cannot
//! pass, removes the pass-through link (`drop_silent_audio`) — never an error. Every test names the
//! mutation that turns it red.

use fabstir_llm_node::ltx::mp4::audio_passthrough_ok;
use fabstir_llm_node::ltx::patcher::drop_silent_audio;
use fabstir_llm_node::ltx::Graph;
use serde_json::{json, Value};

// ── synthetic mp4 builders ─────────────────────────────────────────────────────────────

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

/// One AudioSampleEntry: size + `mp4a` + reserved(6) + data_reference_index(2) + reserved(8) +
/// channelcount(2) + samplesize(2) + pre_defined(2) + reserved(2) + samplerate(4) = 36 bytes.
fn mp4a_entry(channels: u16) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend_from_slice(&1u16.to_be_bytes());
    p.extend_from_slice(&[0u8; 8]);
    p.extend_from_slice(&channels.to_be_bytes());
    p.extend_from_slice(&16u16.to_be_bytes());
    p.extend_from_slice(&[0u8; 4]);
    p.extend_from_slice(&(48_000u32 << 16).to_be_bytes());
    boxed(b"mp4a", &p)
}

/// stsd payload: version/flags(4) + entry_count(4) + entries.
fn stsd(entry_count: u32, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut p = vec![0u8; 4];
    p.extend_from_slice(&entry_count.to_be_bytes());
    for e in entries {
        p.extend_from_slice(e);
    }
    boxed(b"stsd", &p)
}

fn trak(handler: &[u8; 4], stsd_box: Option<Vec<u8>>, samples: Option<u32>) -> Vec<u8> {
    let mut stbl = Vec::new();
    if let Some(s) = stsd_box {
        stbl.extend_from_slice(&s);
    }
    if let Some(n) = samples {
        stbl.extend_from_slice(&stsz(n));
    }
    let minf = boxed(b"minf", &boxed(b"stbl", &stbl));
    let mut mdia = hdlr(handler);
    mdia.extend_from_slice(&minf);
    boxed(b"trak", &boxed(b"mdia", &mdia))
}

fn video() -> Vec<u8> {
    trak(b"vide", None, Some(121))
}

/// A usable audio track: `channels` in its first sample entry, `samples` in its stsz.
fn audio(channels: u16, samples: u32) -> Vec<u8> {
    trak(
        b"soun",
        Some(stsd(1, &[mp4a_entry(channels)])),
        Some(samples),
    )
}

fn mp4_with(traks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = boxed(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    out.extend_from_slice(&boxed(b"moov", &traks.concat()));
    out
}

// ── audio_passthrough_ok ───────────────────────────────────────────────────────────────

#[test]
fn test_stereo_audio_passes() {
    // Mutation: always Ok(false) → red.
    assert!(audio_passthrough_ok(&mp4_with(&[video(), audio(2, 237)])).unwrap());
}

#[test]
fn test_mono_audio_passes() {
    // Mutation: accept only 2 channels → red.
    assert!(audio_passthrough_ok(&mp4_with(&[video(), audio(1, 237)])).unwrap());
}

#[test]
fn test_video_only_does_not_pass() {
    // Mutation: always Ok(true) / "no soun track → true" → red.
    assert!(!audio_passthrough_ok(&mp4_with(&[video()])).unwrap());
}

#[test]
fn test_a_usable_stereo_track_under_another_handler_is_not_audio() {
    // Mutation: count any non-vide handler as audio → reads as usable stereo, true, red.
    let text = trak(b"text", Some(stsd(1, &[mp4a_entry(2)])), Some(237));
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), text])).unwrap());
}

#[test]
fn test_four_channels_do_not_pass() {
    // Blender writes "quad" AAC at Audio Channels 4; VHS maps only mono/stereo.
    // Mutation: drop the channel check → red.
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), audio(4, 237)])).unwrap());
}

#[test]
fn test_an_empty_audio_track_does_not_pass() {
    // Mutation: drop the sample check → red.
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), audio(2, 0)])).unwrap());
}

#[test]
fn test_entry_count_zero_is_unusable_even_with_an_entry_present() {
    // Mutation: ignore entry_count → reads the 2-channel entry, true, red.
    let soun = trak(b"soun", Some(stsd(0, &[mp4a_entry(2)])), Some(237));
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), soun])).unwrap());
}

#[test]
fn test_missing_stsd_or_stsz_is_unusable_not_an_error() {
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), trak(b"soun", None, Some(237))])).unwrap());
    assert!(!audio_passthrough_ok(&mp4_with(&[
        video(),
        trak(b"soun", Some(stsd(1, &[mp4a_entry(2)])), None)
    ]))
    .unwrap());
    // an entry too short to hold channelcount
    let short = trak(
        b"soun",
        Some(stsd(1, &[boxed(b"mp4a", &[0u8; 10])])),
        Some(237),
    );
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), short])).unwrap());
}

#[test]
fn test_every_soun_track_must_be_usable() {
    // ffmpeg (and so VHS) picks the stream with the most channels; deliverables often put a quad
    // or 5.1 track before a stereo downmix. Mutations: check only the first, only the last, or
    // "any track usable" → each reads true, red.
    let bytes = mp4_with(&[video(), audio(2, 237), audio(4, 237), audio(2, 237)]);
    assert!(!audio_passthrough_ok(&bytes).unwrap());
}

#[test]
fn test_a_box_overrunning_moov_after_the_video_track_is_an_error() {
    // video_sample_count stops at the first vide trak, so this clip passes the control gate and
    // is first met here. Mutation: swallow walk errors into Ok(..) → red.
    let mut bad = boxed(b"free", &[0u8; 8]);
    bad[0..4].copy_from_slice(&1000u32.to_be_bytes()); // declares 1000 bytes inside a small moov
    let bytes = mp4_with(&[video(), bad]);
    assert!(fabstir_llm_node::ltx::mp4::video_sample_count(&bytes).is_ok());
    assert!(audio_passthrough_ok(&bytes).is_err());
}

#[test]
fn test_every_prefix_returns_without_panicking() {
    // Whole-file prefixes stop at the moov header; so also (a) cut the moov PAYLOAD at every length
    // with the moov size re-declared to match (the trak loop and soun_track_usable see every partial
    // trak), and (b) overwrite every byte inside the moov with 0x00 / 0x7F / 0xFF (garbage sizes,
    // entry counts and channel counts deep in the boxes). Mutation: any unchecked index → panic, red.
    let bytes = mp4_with(&[video(), audio(2, 237)]);
    for n in 0..bytes.len() {
        let _ = audio_passthrough_ok(&bytes[..n]);
    }
    let payload = [video(), audio(2, 237)].concat();
    let ftyp = boxed(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    for n in 0..=payload.len() {
        let mut clip = ftyp.clone();
        clip.extend_from_slice(&boxed(b"moov", &payload[..n]));
        let _ = audio_passthrough_ok(&clip);
    }
    // (c) the audio trak LAST with only an stsd, its entry cut at every length and every enclosing
    // size re-declared, so the entry's reads run up against the very end of the file.
    let entry = mp4a_entry(2);
    for n in 0..=entry.len() {
        let soun = trak(b"soun", Some(stsd(1, &[entry[..n].to_vec()])), None);
        let _ = audio_passthrough_ok(&mp4_with(&[video(), soun]));
    }
    let moov_start = ftyp.len() + 8;
    for i in moov_start..bytes.len() {
        for v in [0x00u8, 0x7F, 0xFF] {
            let mut clip = bytes.clone();
            clip[i] = v;
            let _ = audio_passthrough_ok(&clip);
        }
    }
}

#[test]
fn test_a_short_first_entry_is_unusable_even_when_more_bytes_follow() {
    // A 16-byte first entry followed by a full stereo entry: channelcount's offset (stsd payload + 32)
    // lands inside the SECOND entry. Mutation: drop the entry-size guard → reads 2, true, red.
    // entry 1 is 20 bytes; the bytes after it put 0x0002 exactly at stsd payload + 32 (entry 1's
    // would-be channelcount offset), so only the entry-size guard can tell.
    let short = boxed(b"mp4a", &[0u8; 12]);
    let trailing = vec![0u8, 0, 0, 0, 0, 2, 0, 0, 0, 0];
    let soun = trak(b"soun", Some(stsd(1, &[short, trailing])), Some(237));
    assert!(!audio_passthrough_ok(&mp4_with(&[video(), soun])).unwrap());
}

#[test]
fn test_real_probe_files_when_present() {
    // The window inputs: silent / stereo AAC / quad AAC (skipped when the probe folder is absent).
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/temp/nm1-probe/inputs");
    for (name, want) in [
        ("street_768_24_121.mp4", false),
        ("street_768_24_121_aac.mp4", true),
        ("street_768_24_121_quad.mp4", false),
    ] {
        let Ok(bytes) = std::fs::read(format!("{dir}/{name}")) else {
            eprintln!("skipping {name}: not present");
            continue;
        };
        assert_eq!(audio_passthrough_ok(&bytes).unwrap(), want, "{name}");
    }
}

// ── drop_silent_audio ──────────────────────────────────────────────────────────────────

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

fn template(id: &str) -> Graph {
    let raw = std::fs::read(format!("{DIR}/{id}/v1.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Every (node, input) whose value differs between two graphs (a whole-graph diff).
fn diff(a: &Graph, b: &Graph) -> Vec<String> {
    let (a, b) = (a.0.as_object().unwrap(), b.0.as_object().unwrap());
    let mut out = Vec::new();
    for (id, na) in a {
        let Some(nb) = b.get(id) else {
            out.push(format!("{id} removed"));
            continue;
        };
        if na != nb {
            let (ia, ib) = (
                na["inputs"].as_object().unwrap(),
                nb["inputs"].as_object().unwrap(),
            );
            for k in ia.keys().chain(ib.keys()) {
                if ia.get(k) != ib.get(k) && !out.contains(&format!("{id}.{k}")) {
                    out.push(format!("{id}.{k}"));
                }
            }
            if na.get("class_type") != nb.get("class_type") || na.get("_meta") != nb.get("_meta") {
                out.push(format!("{id} header"));
            }
        }
    }
    for id in b.keys() {
        if !a.contains_key(id) {
            out.push(format!("{id} added"));
        }
    }
    out
}

#[test]
fn test_edit_silent_clip_loses_only_the_videocombine_audio() {
    // Mutation: skip the removal → node 80 keeps `audio`, red; any other change shows in the diff.
    let g = template("ltx-edit-hdr");
    let out = drop_silent_audio(&g, &names(&["control.mp4"])).unwrap();
    assert_eq!(diff(&g, &out), vec!["80.audio".to_string()]);
    assert!(out.0["80"]["inputs"].get("audio").is_none());
}

#[test]
fn test_a_different_name_leaves_the_vhs_graph_unchanged() {
    // Mutation: ignore inputs.video → red. (Non-empty list, so an early return cannot hide it.)
    let g = template("ltx-edit-hdr");
    let out = drop_silent_audio(&g, &names(&["someone-else.mp4"])).unwrap();
    assert!(diff(&g, &out).is_empty());
}

#[test]
fn test_alpha_with_its_own_loader_name_is_unchanged() {
    // Alpha's slot 2 is unlinked. Mutation: fail when a silent loader has no consumer → red.
    let g = template("ltx-alpha-hdr");
    let out = drop_silent_audio(&g, &names(&["control.mp4"])).unwrap();
    assert!(diff(&g, &out).is_empty());
}

#[test]
fn test_a_slot2_consumer_other_than_videocombine_audio_fails_closed() {
    // Mutation: remove without the census → Ok, red.
    let mut g = template("ltx-edit-hdr");
    g.0["999"] = json!({ "class_type": "SomeAudioEncoder", "inputs": { "audio": ["10", 2] } });
    assert!(drop_silent_audio(&g, &names(&["control.mp4"])).is_err());
    let mut g2 = template("ltx-edit-hdr");
    g2.0["80"]["inputs"]["sound"] = g2.0["80"]["inputs"]["audio"].clone();
    assert!(drop_silent_audio(&g2, &names(&["control.mp4"])).is_err());
}

#[test]
fn test_two_vhs_loaders_only_the_silent_one_loses_audio() {
    // Mutation: drop every loader's consumer → red.
    let mut g = template("ltx-edit-hdr");
    g.0["500"] = json!({ "class_type": "VHS_LoadVideo", "inputs": { "video": "other.mp4" } });
    g.0["501"] = json!({ "class_type": "VHS_VideoCombine", "inputs": { "images": ["500", 0], "audio": ["500", 2] } });
    let out = drop_silent_audio(&g, &names(&["control.mp4"])).unwrap();
    assert!(out.0["80"]["inputs"].get("audio").is_none());
    assert_eq!(out.0["501"]["inputs"]["audio"], json!(["500", 2]));
}

#[test]
fn test_sdr2hdr_silent_clip_loses_only_the_createvideo_audio() {
    // The core path: 5106 LoadVideo → 5105 GetVideoComponents slot 1 → CreateVideo 5108.audio.
    // Mutation: handle only VHS_LoadVideo → red.
    let g = template("ltx-sdr2hdr-hdr");
    let out = drop_silent_audio(&g, &names(&["input.mp4"])).unwrap();
    assert_eq!(diff(&g, &out), vec!["5108.audio".to_string()]);
}

#[test]
fn test_sdr2hdr_different_name_is_unchanged() {
    // Mutation: ignore inputs.file on the core path → red.
    let g = template("ltx-sdr2hdr-hdr");
    let out = drop_silent_audio(&g, &names(&["someone-else.mp4"])).unwrap();
    assert!(diff(&g, &out).is_empty());
}

#[test]
fn test_iclora_silent_clip_is_unchanged() {
    // 199 → Video Slice 692 → GetVideoComponents 697:70, slot 1 unlinked (its delivered audio is
    // the model's own). Mutation: fail when a reached GetVideoComponents has no audio consumer → red.
    let g = template("ltx-iclora-hdr");
    let out = drop_silent_audio(&g, &names(&["stone_ruins.mp4"])).unwrap();
    assert!(diff(&g, &out).is_empty());
}

fn core_chain(through_slice: bool, consumer_class: &str) -> Graph {
    let mut g = json!({
        "1": { "class_type": "LoadVideo", "inputs": { "file": "clip.mp4" } },
        "3": { "class_type": "GetVideoComponents", "inputs": { "video": ["1", 0] } },
        "4": { "class_type": consumer_class, "inputs": { "images": ["3", 0], "audio": ["3", 1] } }
    });
    if through_slice {
        g["2"] = json!({ "class_type": "Video Slice", "inputs": { "video": ["1", 0] } });
        g["3"]["inputs"]["video"] = json!(["2", 0]);
    }
    Graph(g)
}

#[test]
fn test_core_path_through_video_slice_is_followed() {
    // Mutation: handle only direct LoadVideo → GetVideoComponents links → red.
    let out = drop_silent_audio(&core_chain(true, "CreateVideo"), &names(&["clip.mp4"])).unwrap();
    assert!(out.0["4"]["inputs"].get("audio").is_none());
    let direct =
        drop_silent_audio(&core_chain(false, "CreateVideo"), &names(&["clip.mp4"])).unwrap();
    assert!(direct.0["4"]["inputs"].get("audio").is_none());
}

#[test]
fn test_core_path_census_fails_closed() {
    // Mutation: remove the core-path census → Ok, red.
    assert!(drop_silent_audio(
        &core_chain(false, "SomeAudioEncoder"),
        &names(&["clip.mp4"])
    )
    .is_err());
}

#[test]
fn test_empty_list_returns_the_graph_unchanged() {
    let g = template("ltx-edit-hdr");
    let out = drop_silent_audio(&g, &[]).unwrap();
    assert!(diff(&g, &out).is_empty());
    let _: &Value = &out.0;
}
