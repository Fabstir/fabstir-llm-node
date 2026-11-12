// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N2/N3/N5/N7/N8, the parts on their own: the cool-down counter (D13), sealing (D15), the deep-EXR
//! sampling and conversion (D6), the staging area (D8) and renderer-folder deletion (D9). Each test names the
//! mutation that turns it red.
use fabstir_llm_node::ltx::checks::config::RendererDirs;
use fabstir_llm_node::ltx::checks::cooldown::{Cooldown, PAUSE, WINDOW};
use fabstir_llm_node::ltx::checks::deep_png::{exr_to_png, sampled_indices, MAX_SAMPLED};
use fabstir_llm_node::ltx::checks::files::{delete_job_files, safe_join, InUse, JobFiles};
use fabstir_llm_node::ltx::checks::seal;
use fabstir_llm_node::ltx::checks::staging::{StageError, Staging};
use std::io::Cursor;
use std::time::{Duration, Instant};

#[test]
fn two_refusals_are_free_and_the_third_pauses_for_ten_minutes() {
    // threshold 2 or 4, or a pause that does not restart on a further refusal -> red
    let c = Cooldown::default();
    let t0 = Instant::now();
    let a = "0xAbC0000000000000000000000000000000000001";
    assert_eq!(c.record_at(a, t0).count, 1);
    let second = c.record_at(a, t0 + Duration::from_secs(60));
    assert!(
        !second.alert && !c.paused_at(a, t0 + Duration::from_secs(61)),
        "two are free"
    );
    let third = c.record_at(a, t0 + Duration::from_secs(120));
    assert!(third.alert, "the third alerts once");
    let after = |s: u64| t0 + Duration::from_secs(120 + s);
    assert!(
        c.paused_at(&a.to_lowercase(), after(1)),
        "case-insensitive address"
    );
    assert!(c.paused_at(a, after(PAUSE.as_secs() - 1)));
    assert!(
        !c.paused_at(a, after(PAUSE.as_secs())),
        "ten minutes, then free"
    );
    let fourth = c.record_at(a, after(700));
    assert!(!fourth.alert && fourth.count == 4);
    assert!(
        c.paused_at(a, after(700 + PAUSE.as_secs() - 1)),
        "each further refusal pauses again"
    );
    assert!(
        !c.paused_at("0xother", after(1)),
        "another address is untouched"
    );
}

#[test]
fn refusals_older_than_a_day_no_longer_count() {
    // never pruning the window -> red
    let c = Cooldown::default();
    let t0 = Instant::now();
    c.record_at("a", t0);
    c.record_at("a", t0 + Duration::from_secs(10));
    let late = c.record_at("a", t0 + WINDOW + Duration::from_secs(11));
    assert_eq!(late.count, 1);
    assert!(!c.paused_at("a", t0 + WINDOW + Duration::from_secs(12)));
}

fn keypair() -> ([u8; 32], [u8; 33]) {
    let sk: [u8; 32] = k256::SecretKey::random(&mut rand::thread_rng())
        .to_bytes()
        .into();
    (sk, seal::public_key_of(&sk).unwrap())
}

#[test]
fn a_sealed_item_opens_with_the_private_key_and_nothing_else() {
    // encrypt to a fixed key, skip the AEAD, or leave the item in clear -> red
    let (sk, pk) = keypair();
    let item = b"MARKER-ITEM-BYTES-0123456789".repeat(40);
    let record = serde_json::json!({"session": "s1", "job": 7, "stage": "c3", "ext": "mp4"});
    let sealed = seal::seal(&pk, &record, &item).unwrap();
    assert!(
        !sealed.windows(11).any(|w| w == b"MARKER-ITEM"),
        "the item is not readable in the file"
    );
    assert!(
        !sealed.windows(5).any(|w| w == b"stage"),
        "nor is its record"
    );
    let (r, i) = seal::open(&sk, &sealed).unwrap();
    assert_eq!((r, i), (record.clone(), item.clone()));
    let (other, _) = keypair();
    assert!(
        seal::open(&other, &sealed).is_err(),
        "another key does not open it"
    );
    let mut tampered = sealed.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(
        seal::open(&sk, &tampered).is_err(),
        "a changed byte is detected"
    );
    let mut header = sealed.clone();
    header[10] ^= 1;
    assert!(
        seal::open(&sk, &header).is_err(),
        "the ephemeral key is bound"
    );
    assert_ne!(
        sealed,
        seal::seal(&pk, &record, &item).unwrap(),
        "fresh key and nonce each time"
    );
    assert_eq!(seal::parse_public_key(&hex::encode(pk)).unwrap(), pk);
    assert!(seal::parse_public_key("zz").is_err());
}

#[test]
fn sealed_files_are_owner_only_and_written_whole() {
    let dir = tempfile::tempdir().unwrap();
    let p = seal::write_sealed(dir.path(), "x.sealed", b"abc").unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "no .part left"
    );
}

#[test]
fn deep_sampling_keeps_the_ends_and_draws_the_rest_at_random() {
    // fix F6. A fixed pattern (multiples of the fps) -> two draws are equal and most frames are never judged
    // -> red; the last frame dropped, or the cap lifted -> red
    let mut rng = rand::rngs::OsRng;
    let a = sampled_indices(121, 24, &mut rng);
    assert!(a.contains(&0) && a.contains(&120), "the first and the last");
    assert_eq!(a.len(), 8, "one from each of the six seconds, and the ends");
    let b = sampled_indices(121, 24, &mut rng);
    let c = sampled_indices(121, 24, &mut rng);
    assert!(a != b || a != c, "the draw is not a fixed pattern");
    // every frame has a chance: over many draws no index of a two-second clip is left out
    let mut union = std::collections::BTreeSet::new();
    for _ in 0..3000 {
        let s = sampled_indices(48, 24, &mut rng);
        assert_eq!(s.len(), 4, "at least one per second on average");
        union.extend(s);
    }
    assert_eq!(union.len(), 48, "no frame is safe from being judged");
    // one judged frame in each second of the clip, whatever the draw
    let s = sampled_indices(240, 24, &mut rng);
    for second in 0..10 {
        let (lo, hi) = (1 + second * 238 / 10, 1 + (second + 1) * 238 / 10);
        assert!(s.range(lo..hi).next().is_some(), "second {second}");
    }
    let long = sampled_indices(4000, 24, &mut rng);
    assert_eq!(
        long.len(),
        MAX_SAMPLED,
        "at most 64, however long the input"
    );
    assert!(long.contains(&0) && long.contains(&3999));
    assert!(sampled_indices(0, 24, &mut rng).is_empty());
    assert_eq!(sampled_indices(1, 0, &mut rng).len(), 1);
    assert_eq!(
        sampled_indices(2, 0, &mut rng).len(),
        2,
        "fps 0 never divides by zero"
    );
    assert_eq!(sampled_indices(3, 24, &mut rng).len(), 3);
}

fn exr(w: usize, h: usize, value: impl Fn(usize, usize) -> f32 + Sync) -> Vec<u8> {
    use exr::prelude::*;
    let pixels = SpecificChannels::rgb(|pos: Vec2<usize>| {
        let v = value(pos.x(), pos.y());
        (v, v, v)
    });
    let mut c = Cursor::new(Vec::new());
    Image::from_channels((w, h), pixels)
        .write()
        .to_buffered(&mut c)
        .unwrap();
    c.into_inner()
}

fn mean_and_max(png: &[u8]) -> (f64, u8) {
    let img = image::load_from_memory(png).unwrap().to_rgb8();
    let px = img.as_raw();
    (
        px.iter().map(|v| *v as f64).sum::<f64>() / px.len() as f64,
        *px.iter().max().unwrap(),
    )
}

#[test]
fn a_dark_frame_is_brightened_to_its_own_99th_percentile() {
    // no gain (a plain clamp) -> the dark frame stays near black -> red
    let dark = exr(64, 32, |x, _| 0.004 * (x as f32 / 63.0));
    let (mean, max) = mean_and_max(&exr_to_png(&dark, false).unwrap());
    assert!(
        max >= 250 && mean > 100.0,
        "display-referred ramp fills the range: mean {mean}, max {max}"
    );
    // an over-bright frame is brought down, not clipped to white
    let hot = exr(64, 32, |x, _| 40.0 * (x as f32 / 63.0));
    let (mean, _) = mean_and_max(&exr_to_png(&hot, false).unwrap());
    assert!((100.0..160.0).contains(&mean), "mean {mean}");
    // a black frame stays black and does not divide by zero
    assert_eq!(
        mean_and_max(&exr_to_png(&exr(8, 8, |_, _| 0.0), true).unwrap()).1,
        0
    );
    // NaN and infinity do not poison the frame
    let odd = exr(8, 8, |x, y| {
        if x == 0 {
            f32::NAN
        } else if y == 0 {
            f32::INFINITY
        } else {
            0.5
        }
    });
    exr_to_png(&odd, true).unwrap();
}

#[test]
fn scene_linear_input_gets_the_display_curve() {
    // skip the curve for scene-linear -> the two means are equal -> red
    let ramp = exr(64, 32, |x, _| x as f32 / 63.0);
    let (flat, _) = mean_and_max(&exr_to_png(&ramp, false).unwrap());
    let (curved, _) = mean_and_max(&exr_to_png(&ramp, true).unwrap());
    assert!(
        curved > flat + 25.0,
        "linear mid-grey is lifted: {flat} -> {curved}"
    );
    assert!(
        exr_to_png(b"\x76\x2f\x31\x01 not an exr", true).is_err(),
        "unreadable fails closed"
    );
}

#[tokio::test]
async fn staging_writes_part_then_renames_and_deletes_with_the_guard() {
    // no delete on drop -> the last assertion goes red; no cap -> the Full assertion goes red
    let dir = tempfile::tempdir().unwrap();
    let st = Staging::new(dir.path(), std::path::Path::new("/mnt/service-view"), 1000);
    let staged = st.stage(&[7u8; 600], "mp4").await.unwrap();
    assert!(
        staged.service_path().starts_with("/mnt/service-view/")
            && staged.service_path().ends_with(".mp4")
    );
    assert_eq!(std::fs::read(staged.node_path()).unwrap().len(), 600);
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.len() == 1 && !names[0].ends_with(".part"),
        "{names:?}"
    );
    assert_eq!(
        st.stage(&[0u8; 500], "png").await.unwrap_err(),
        StageError::Full
    );
    drop(staged);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "deleted after the answer"
    );
    // the start-up sweep clears what a previous run staged, and nothing else (fix F9)
    let ours = "0123456789abcdef0123456789abcdef";
    for name in [
        format!("{ours}.mp4"),
        format!("{ours}.png.part"),
        format!("{ours}.sealed"),
        format!("{}.mp4", &ours[1..]),
        "refusals.jsonl".to_string(),
    ] {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    std::fs::create_dir(dir.path().join(format!("{}.webp", ours.replace('0', "1")))).unwrap();
    assert_eq!(st.sweep(), 2, "only the two names this node stages");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[test]
fn deletion_never_leaves_the_renderer_folders() {
    // join without the component check -> the outside file is deleted -> red
    let root = tempfile::tempdir().unwrap();
    let (input, output, evidence) = (
        root.path().join("in"),
        root.path().join("out"),
        root.path().join("evidence"),
    );
    for d in [
        &input,
        &output,
        &evidence,
        &input.join("deadbeef"),
        &output.join("video"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    for f in [
        "in/a.png",
        "in/deadbeef/frame_00001.exr",
        "out/video/x.mp4",
        "evidence/1.sealed",
        "keep.txt",
    ] {
        std::fs::write(root.path().join(f), b"x").unwrap();
    }
    assert!(safe_join(&input, "../keep.txt").is_none());
    assert!(safe_join(&input, "/etc/passwd").is_none());
    assert!(safe_join(&input, "").is_none() && safe_join(&input, ".").is_none());
    let dirs = RendererDirs {
        input: input.clone(),
        output: output.clone(),
    };
    let files = JobFiles {
        inputs: vec![
            "a.png".into(),
            "../keep.txt".into(),
            "gone-already.png".into(),
        ],
        input_dirs: vec!["deadbeef".into(), "../evidence".into()],
        outputs: vec![
            ("video".into(), "x.mp4".into()),
            ("../evidence".into(), "1.sealed".into()),
        ],
    };
    let (removed, failed) = delete_job_files(&dirs, &files, &InUse::default());
    assert_eq!((removed, failed), (4, 3));
    assert!(!input.join("a.png").exists() && !input.join("deadbeef").exists());
    assert!(!output.join("video/x.mp4").exists());
    assert!(root.path().join("keep.txt").exists() && evidence.join("1.sealed").exists());
}

#[test]
fn a_shared_input_is_deleted_only_when_its_last_job_ends() {
    // no use count -> the first job's end deletes the second job's input -> red
    let root = tempfile::tempdir().unwrap();
    let dirs = RendererDirs {
        input: root.path().to_path_buf(),
        output: root.path().to_path_buf(),
    };
    std::fs::write(root.path().join("same.mp4"), b"x").unwrap();
    let in_use = InUse::default();
    in_use.hold("f:same.mp4");
    in_use.hold("f:same.mp4");
    let files = JobFiles {
        inputs: vec!["same.mp4".into()],
        ..Default::default()
    };
    assert_eq!(delete_job_files(&dirs, &files, &in_use), (0, 0));
    assert!(root.path().join("same.mp4").exists());
    assert_eq!(delete_job_files(&dirs, &files, &in_use), (1, 0));
    assert!(!root.path().join("same.mp4").exists());
}
