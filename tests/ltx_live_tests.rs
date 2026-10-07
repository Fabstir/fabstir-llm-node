// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NM1 D19 — the node's OWN client against a live sidecar (E-NM1.0, E-NM1.2).
//!
//! Every live test is `#[ignore]` and runs only with `LTX_LIVE_COMFY_URL` set, from the dev
//! container through Jules's SSH tunnel (WSL: `ssh -N -L 8189:127.0.0.1:8189 <host1>`), which the
//! container reaches at host.docker.internal:
//!
//!   LTX_LIVE_COMFY_URL=http://host.docker.internal:8189 LTX_LIVE_OUT_DIR=<dir> LTX_LIVE_MANIFEST=<runs.json> \
//!     cargo test --test ltx_live_tests live_manifest -- --exact --ignored --nocapture --test-threads=1
//!
//! (`--exact`: the filter `live_manifest` would otherwise also run `live_manifest_dry_run`, which fails
//! without `LTX_LIVE_MANIFEST_DIR` and turns every run's result red.)
//!
//! Before the window: `LTX_LIVE_MANIFEST_DIR=temp/nm1-probe/manifests cargo test --test ltx_live_tests
//! live_manifest_dry_run -- --ignored --nocapture` checks every manifest offline (inputs present, labels
//! unique, each template run prepared through the real store and patcher).
//!
//! A manifest is a JSON array of runs (paths are relative to the working directory):
//!
//!   {"label": "t2v-768-5s", "template": "ltx-t2v-hdr", "w": 768, "h": 512, "fps": 24, "frames": 121,
//!    "seed": "42", "prompt": "…", "output": "exr-sequence" | "exr-frames",
//!    "images": ["still.png"], "videos": ["clip.mp4"], "expectOrderRefusal": false, "timeoutSecs": 2400,
//!    "expectAudioKept": true, "deepFrames": "frames-dir/"}
//!
//! `expectAudioKept` (NM1 D23): the dry run asserts the clip's audio pass-through (a `VHS_LoadVideo`
//! slot-2 or `GetVideoComponents` slot-1 consumer) survives or is dropped, from the clip's REAL bytes.
//! `deepFrames` (NM1.0b): a folder of EXR frames sent through the deep-conform wire (`exrseq-linear`,
//! production's) in place of `videos`.
//!
//! or, for a raw graph (the D8 ramp pass-through check), `"graph": "temp/nm1-probe/ramp.json"` in place of
//! `template`, whose `LoadImage` nodes take the uploaded images in order.
//!
//! Each template run uses the real patcher (with D23's `drop_silent_audio` for clips whose audio cannot
//! pass), `ComfyClient` (`/upload/image`, `/prompt`, the `/ws` end signal), `outputs` (D22's ONE delivery
//! filter, shared with the handler) and `order_refs`; every output file is written to
//! `$LTX_LIVE_OUT_DIR/<label>/`, and one JSON line per run (template hash, billed frames, output names,
//! the `order_refs` verdict, wall seconds, error) is appended to `$LTX_LIVE_OUT_DIR/results.jsonl`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use fabstir_llm_node::ltx::exr::order_refs;
use fabstir_llm_node::ltx::mp4::audio_passthrough_ok;
use fabstir_llm_node::ltx::patcher::{drop_silent_audio, patch};
use fabstir_llm_node::ltx::types::{InputWire, LtxJob, OutputKind, Resolution};
use fabstir_llm_node::ltx::{ComfyClient, Graph, TemplateStore};
use serde::Deserialize;
use serde_json::{json, Value};

const TEMPLATES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Run {
    label: String,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    graph: Option<String>,
    #[serde(default)]
    w: u32,
    #[serde(default)]
    h: u32,
    #[serde(default)]
    fps: u32,
    #[serde(default)]
    frames: u32,
    #[serde(default = "default_seed")]
    seed: String,
    #[serde(default)]
    prompt: String,
    #[serde(default = "default_output")]
    output: OutputKind,
    #[serde(default)]
    images: Vec<String>,
    #[serde(default)]
    videos: Vec<String>,
    #[serde(default)]
    strength: Option<f64>,
    #[serde(default)]
    expect_order_refusal: bool,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
    #[serde(default)]
    expect_audio_kept: Option<bool>,
    #[serde(default)]
    deep_frames: Option<String>,
}

fn default_seed() -> String {
    "42".to_string()
}
fn default_output() -> OutputKind {
    OutputKind::ExrSequence
}
fn default_timeout() -> u64 {
    2400
}

fn content_name(bytes: &[u8], path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    format!("{}.{ext}", hex::encode(ethers::utils::keccak256(bytes)))
}

/// D23: the names (paired with `paths` by position) of the clips whose audio cannot pass through —
/// `audio_passthrough_ok` false OR unreadable, exactly as the handler treats them.
fn silent_names(paths: &[String], names: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (p, n) in paths.iter().zip(names) {
        let bytes = std::fs::read(p).map_err(|e| format!("reading input {p}: {e}"))?;
        if !audio_passthrough_ok(&bytes).unwrap_or(false) {
            out.push(n.clone());
        }
    }
    Ok(out)
}

/// The deep-conform wire: every EXR frame of `dir`, uploaded in name order into ONE content-addressed
/// subfolder under node-style sequential names (as the handler stages them); returns the subfolder.
async fn upload_deep(client: &ComfyClient, dir: &str) -> Result<String, String> {
    let mut frames: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "exr"))
        .collect();
    frames.sort();
    let mut all = Vec::with_capacity(frames.len());
    for f in &frames {
        all.push(std::fs::read(f).map_err(|e| format!("{}: {e}", f.display()))?);
    }
    // Content-addressed, as the handler's subfolder is: a re-run with different frames never reuses
    // the old ones (a stale frame cannot fill a gap — frame_limit caps only the top end).
    let mut digest = Vec::with_capacity(all.len() * 32);
    for b in &all {
        digest.extend_from_slice(&ethers::utils::keccak256(b));
    }
    let sub = format!(
        "deep-{}",
        &hex::encode(ethers::utils::keccak256(&digest))[..16]
    );
    for (i, (f, bytes)) in frames.iter().zip(all).enumerate() {
        client
            .upload_input_in(Some(&sub), &format!("frame_{:05}.exr", i + 1), bytes)
            .await
            .map_err(|e| format!("uploading {}: {e}", f.display()))?;
    }
    Ok(sub)
}

/// A graph link `[source id, output slot]`, or None.
fn link(v: &Value) -> Option<(&str, u64)> {
    match v.as_array().map(|a| a.as_slice()) {
        Some([src, slot]) => src.as_str().zip(slot.as_u64()),
        _ => None,
    }
}

/// Whether the graph still carries a clip-audio pass-through (D23's dry-run check): a consumer of a
/// `VHS_LoadVideo`'s slot 2, or of slot 1 of a `GetVideoComponents` whose video comes from a core
/// `LoadVideo` (directly or through `Video Slice`) — never a GetVideoComponents fed by generated video.
fn loader_audio_present(g: &Graph) -> bool {
    let obj = g.0.as_object().unwrap();
    let class = |id: &str| {
        obj.get(id)
            .and_then(|n| n["class_type"].as_str())
            .unwrap_or("")
    };
    let from_loader = |gvc: &str| {
        let mut node = gvc.to_string();
        for _ in 0..16 {
            match obj.get(&node).and_then(|n| link(&n["inputs"]["video"])) {
                Some((src, 0)) if class(src) == "LoadVideo" => return true,
                Some((src, 0)) if class(src) == "Video Slice" => node = src.to_string(),
                _ => return false,
            }
        }
        false
    };
    obj.values().any(|n| {
        n["inputs"].as_object().is_some_and(|ins| {
            ins.values().any(|v| match link(v) {
                Some((src, 2)) => class(src) == "VHS_LoadVideo",
                Some((src, 1)) => class(src) == "GetVideoComponents" && from_loader(src),
                _ => false,
            })
        })
    })
}

async fn upload_all(client: &ComfyClient, paths: &[String]) -> Result<Vec<String>, String> {
    let mut names = Vec::with_capacity(paths.len());
    for p in paths {
        let bytes = std::fs::read(p).map_err(|e| format!("reading input {p}: {e}"))?;
        let name = client
            .upload_input(&content_name(&bytes, p), bytes)
            .await
            .map_err(|e| format!("uploading {p}: {e}"))?;
        names.push(name);
    }
    Ok(names)
}

/// The graph a run submits, and the job `order_refs` judges it by (template runs only).
fn prepare(
    store: &TemplateStore,
    run: &Run,
    image_names: &[String],
    video_names: &[String],
    silent: &[String],
) -> Result<(Graph, Option<LtxJob>, String), String> {
    if let Some(path) = &run.graph {
        let mut g: Value =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)
                .map_err(|e| format!("{path}: {e}"))?;
        let mut loaders: Vec<String> = g
            .as_object()
            .ok_or("graph is not an object")?
            .iter()
            .filter(|(_, n)| n["class_type"] == "LoadImage")
            .map(|(k, _)| k.clone())
            .collect();
        loaders.sort();
        if loaders.len() != image_names.len() {
            return Err(format!(
                "{} LoadImage nodes for {} images",
                loaders.len(),
                image_names.len()
            ));
        }
        for (id, name) in loaders.iter().zip(image_names) {
            g[id]["inputs"]["image"] = json!(name);
        }
        return Ok((Graph(g), None, "raw-graph".to_string()));
    }
    let template = run
        .template
        .as_deref()
        .ok_or("a run needs a template or a graph")?;
    let hash = store
        .template_hash(template)
        .ok_or_else(|| format!("{template} is not in the allow-list"))?
        .to_string();
    let graph = store.verify(template, &hash).map_err(|e| e.to_string())?;
    let job = LtxJob {
        template_id: template.to_string(),
        template_hash: hash.clone(),
        prompt: run.prompt.clone(),
        seed: run.seed.clone(),
        frames: run.frames,
        fps: run.fps,
        resolution: Resolution { w: run.w, h: run.h },
        lora: format!("{template}@v1"),
        output: run.output,
        images: None,
        videos: None,
        strength: run.strength,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: run.deep_frames.as_ref().map(|_| InputWire::ExrseqLinear),
    };
    let patched =
        patch(&graph, &job, image_names, video_names).map_err(|e| format!("patch: {e}"))?;
    // D23: after patch, exactly as the handler does.
    let patched =
        drop_silent_audio(&patched, silent).map_err(|e| format!("silent-audio drop: {e}"))?;
    Ok((patched, Some(job), hash))
}

/// The run's output folder, created if missing and refused if it already holds files.
fn fresh_label_dir(out_dir: &Path, label: &str) -> Result<PathBuf, String> {
    let dir = out_dir.join(label);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .next()
        .is_some()
    {
        return Err(format!(
            "{} is not empty: use a fresh LTX_LIVE_OUT_DIR, or another label",
            dir.display()
        ));
    }
    Ok(dir)
}

/// Run one manifest entry end to end; returns its results line.
async fn run_one(url: &str, out_dir: &Path, store: &TemplateStore, run: &Run) -> Value {
    let started = Instant::now();
    let mut line = json!({ "label": run.label, "template": run.template, "graph": run.graph,
                           "w": run.w, "h": run.h, "fps": run.fps, "frames": run.frames });
    let result: Result<(), String> = async {
        // A fresh client per run: a unique clientId, as the handler builds per job.
        let dir = fresh_label_dir(out_dir, &run.label)?;
        let client = ComfyClient::new(url).map_err(|e| e.to_string())?;
        let image_names = upload_all(&client, &run.images).await?;
        let (video_names, silent) = match &run.deep_frames {
            Some(dir) => (vec![upload_deep(&client, dir).await?], Vec::new()),
            None => {
                let names = upload_all(&client, &run.videos).await?;
                let silent = silent_names(&run.videos, &names)?;
                (names, silent)
            }
        };
        line["silentAudio"] = json!(silent);
        let (graph, job, hash) = prepare(store, run, &image_names, &video_names, &silent)?;
        line["templateHash"] = json!(hash);
        let prompt_id = client
            .submit(&graph)
            .await
            .map_err(|e| format!("submit: {e}"))?;
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        client
            .watch(&prompt_id, tx, run.timeout_secs)
            .await
            .map_err(|e| format!("watch: {e}"))?;
        let _ = drain.await;
        line["renderSecs"] = json!(started.elapsed().as_secs_f64());
        let refs = client
            .outputs(&prompt_id)
            .await
            .map_err(|e| format!("outputs: {e}"))?;
        let mut names = Vec::new();
        for r in &refs {
            let bytes = client
                .download(r)
                .await
                .map_err(|e| format!("download {}: {e}", r.filename))?;
            let name = Path::new(&r.filename)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
            names.push(name);
        }
        line["outputs"] = json!(names);
        if let Some(job) = job {
            let verdict = order_refs(&job, refs);
            line["orderRefs"] = json!(match &verdict {
                Ok(v) => format!("ok: {} refs", v.len()),
                Err(e) => format!("refused: {e}"),
            });
            if verdict.is_err() != run.expect_order_refusal {
                return Err(format!(
                    "order_refs {} but the run expected {}",
                    if verdict.is_err() {
                        "refused"
                    } else {
                        "accepted"
                    },
                    if run.expect_order_refusal {
                        "a refusal"
                    } else {
                        "acceptance"
                    }
                ));
            }
        }
        Ok(())
    }
    .await;
    line["wallSecs"] = json!(started.elapsed().as_secs_f64());
    line["error"] = json!(result.err());
    line
}

fn live_env() -> Option<(String, PathBuf)> {
    let url = std::env::var("LTX_LIVE_COMFY_URL")
        .ok()
        .filter(|s| !s.is_empty())?;
    let out = PathBuf::from(
        std::env::var("LTX_LIVE_OUT_DIR").expect("LTX_LIVE_OUT_DIR must be set for live runs"),
    );
    Some((url, out))
}

fn read_manifest(path: &str) -> Vec<Run> {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("manifest {path}: {e}")))
        .unwrap_or_else(|e| panic!("manifest {path}: {e}"))
}

/// Runs every entry of `$LTX_LIVE_MANIFEST` in order (one ComfyUI job at a time, as the window runs
/// them), records each, and fails at the end listing every run that errored.
#[tokio::test]
#[ignore]
async fn live_manifest() {
    let Some((url, out_dir)) = live_env() else {
        panic!("set LTX_LIVE_COMFY_URL (and LTX_LIVE_OUT_DIR, LTX_LIVE_MANIFEST) to run live");
    };
    let manifest = std::env::var("LTX_LIVE_MANIFEST").expect("LTX_LIVE_MANIFEST must be set");
    let runs = read_manifest(&manifest);
    let store = TemplateStore::new(TEMPLATES).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut failed = Vec::new();
    for run in &runs {
        let line = run_one(&url, &out_dir, &store, run).await;
        println!("{line}");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out_dir.join("results.jsonl"))
            .unwrap();
        use std::io::Write;
        writeln!(f, "{line}").unwrap();
        if !line["error"].is_null() {
            failed.push(format!("{}: {}", run.label, line["error"]));
        }
    }
    assert!(failed.is_empty(), "failed runs:\n{}", failed.join("\n"));
}

/// Every manifest in `$LTX_LIVE_MANIFEST_DIR`, checked offline before the window: each input file
/// exists, labels are unique across the step-4 manifests, and each template run prepares through the
/// real store and patcher (with placeholder input names, one per input).
#[test]
#[ignore]
fn live_manifest_dry_run() {
    let dir = std::env::var("LTX_LIVE_MANIFEST_DIR").expect("LTX_LIVE_MANIFEST_DIR must be set");
    let store = TemplateStore::new(TEMPLATES).unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut step4_labels = std::collections::HashSet::new();
    let mut problems = Vec::new();
    for f in &files {
        let runs = read_manifest(f.to_str().unwrap());
        for run in &runs {
            for p in run.images.iter().chain(&run.videos).chain(run.graph.iter()) {
                if !Path::new(p).is_file() {
                    problems.push(format!("{}: {}: missing {p}", f.display(), run.label));
                }
            }
            if let Some(d) = &run.deep_frames {
                let n = std::fs::read_dir(d).map(|r| r.count()).unwrap_or(0);
                if n == 0 {
                    problems.push(format!(
                        "{}: {}: deepFrames {d} empty or missing",
                        f.display(),
                        run.label
                    ));
                }
            }
            let stem = f.file_stem().unwrap().to_string_lossy();
            if stem.starts_with("s4-") && !step4_labels.insert(run.label.clone()) {
                problems.push(format!("{}: duplicate label {}", f.display(), run.label));
            }
            if run.graph.is_none() {
                let imgs: Vec<String> =
                    (0..run.images.len()).map(|i| format!("i{i}.png")).collect();
                let vids: Vec<String> = if run.deep_frames.is_some() {
                    vec!["deep-sub".to_string()]
                } else {
                    (0..run.videos.len()).map(|i| format!("v{i}.mp4")).collect()
                };
                // D23 on the REAL bytes, mapped to the placeholder names.
                let silent = if run.deep_frames.is_some() {
                    Ok(Vec::new())
                } else {
                    silent_names(&run.videos, &vids)
                };
                match silent.and_then(|s| prepare(&store, run, &imgs, &vids, &s)) {
                    Err(e) => problems.push(format!("{}: {}: {e}", f.display(), run.label)),
                    Ok((g, _, _)) => {
                        if let Some(want) = run.expect_audio_kept {
                            let got = loader_audio_present(&g);
                            println!(
                                "  {}: audio pass-through {}",
                                run.label,
                                if got { "kept" } else { "dropped" }
                            );
                            if got != want {
                                problems.push(format!(
                                    "{}: {}: expectAudioKept {want} but the pass-through was {}",
                                    f.display(),
                                    run.label,
                                    if got { "kept" } else { "dropped" }
                                ));
                            }
                        }
                    }
                }
            }
        }
        println!("{}: {} run(s)", f.display(), runs.len());
    }
    assert!(!files.is_empty(), "no manifests in {dir}");
    assert!(
        problems.is_empty(),
        "manifest problems:\n{}",
        problems.join("\n")
    );
}

/// The manifest format, checked offline so a typo cannot first surface inside the window.
#[test]
fn manifest_format_parses() {
    let runs: Vec<Run> = serde_json::from_value(json!([
        {"label": "t2v", "template": "ltx-t2v-hdr", "w": 768, "h": 512, "fps": 25, "frames": 126,
         "output": "exr-frames", "expectOrderRefusal": true},
        {"label": "alpha", "template": "ltx-alpha-hdr", "w": 1920, "h": 1088, "fps": 25, "frames": 145,
         "output": "exr-frames", "videos": ["clip.mp4"], "timeoutSecs": 1200},
        {"label": "ramp", "graph": "temp/nm1-probe/ramp.json", "images": ["temp/nm1-probe/ramp.png"]}
    ]))
    .unwrap();
    assert_eq!(runs.len(), 3);
    assert!(runs[0].expect_order_refusal);
    assert_eq!(runs[1].output, OutputKind::ExrFrames);
    assert_eq!(runs[2].timeout_secs, 2400);
    // An unknown key is refused, not silently ignored.
    assert!(
        serde_json::from_value::<Vec<Run>>(json!([{"label": "x", "templat": "ltx-t2v-hdr"}]))
            .is_err()
    );
    // A template run prepares through the real store and patcher.
    let store = TemplateStore::new(TEMPLATES).unwrap();
    let (g, job, hash) = prepare(&store, &runs[1], &[], &["clip.mp4".to_string()], &[]).unwrap();
    assert_eq!(hash, store.template_hash("ltx-alpha-hdr").unwrap());
    assert_eq!(job.unwrap().frames, 145);
    assert!(g
        .0
        .as_object()
        .unwrap()
        .values()
        .any(|n| n["inputs"]["frame_load_cap"] == 145));
}

/// A run's output folder must start empty: two runs in one folder would mix their EXR sequences and
/// `compare.py` would read the union.
#[test]
fn run_refuses_a_non_empty_label_folder() {
    let out = std::env::temp_dir().join(format!("ltx-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    assert!(
        fresh_label_dir(&out, "a").is_ok(),
        "a missing folder is created"
    );
    assert!(fresh_label_dir(&out, "a").is_ok(), "an empty one is reused");
    std::fs::write(out.join("a").join("frame_00001.exr"), b"x").unwrap();
    let err = fresh_label_dir(&out, "a").unwrap_err();
    assert!(err.contains("not empty"), "{err}");
    let _ = std::fs::remove_dir_all(&out);
}

/// D23 in the harness: `prepare` drops the pass-through for a silent clip and keeps it otherwise, and
/// the two new manifest fields parse. Mutation: `prepare` skipping `drop_silent_audio` → red.
#[test]
fn prepare_drops_audio_only_for_a_silent_clip() {
    let runs: Vec<Run> = serde_json::from_value(json!([
        {"label": "edit", "template": "ltx-edit-hdr", "w": 768, "h": 512, "fps": 24, "frames": 121,
         "videos": ["clip.mp4"], "expectAudioKept": false},
        {"label": "deep", "template": "ltx-edit-hdr", "w": 768, "h": 512, "fps": 24, "frames": 121,
         "deepFrames": "frames/"}
    ]))
    .unwrap();
    assert_eq!(runs[0].expect_audio_kept, Some(false));
    assert_eq!(runs[1].deep_frames.as_deref(), Some("frames/"));
    let store = TemplateStore::new(TEMPLATES).unwrap();
    let v = vec!["v0.mp4".to_string()];
    let (silent, _, _) = prepare(&store, &runs[0], &[], &v, &v).unwrap();
    assert!(
        !loader_audio_present(&silent),
        "a silent clip loses the pass-through"
    );
    let (kept, _, _) = prepare(&store, &runs[0], &[], &v, &[]).unwrap();
    assert!(loader_audio_present(&kept), "a clip with audio keeps it");
    // the deep run goes through the production wire's loader swap
    let (deep, job, _) = prepare(&store, &runs[1], &[], &["deep-sub".to_string()], &[]).unwrap();
    assert_eq!(job.unwrap().input_wire, Some(InputWire::ExrseqLinear));
    assert!(deep
        .0
        .as_object()
        .unwrap()
        .values()
        .all(|n| n["class_type"] != "VHS_LoadVideo"));
}

// ── VFX Passes (VP1.1): the node's own client against a live relight sidecar ──────────────────────────
//
//   RELIGHT_LIVE_URL=http://127.0.0.1:8190 RELIGHT_LIVE_CLIP=<1920x1088 25 fps clip, >= 121 frames> \
//     cargo test --test ltx_live_tests live_relight_standard_121 -- --exact --ignored --nocapture --test-threads=1
//
// A 121-frame Standard job: upload, the real patcher, `/prompt`, the `/ws` end signal, `outputs`, `order_refs`
// (exactly 121 EXRs + 1 preview), then the first EXR's header is parsed BY HAND (no exr crate): channel names and
// the `platformless:conventions` attribute.

/// (attribute name, type, raw value) of a scanline EXR header.
fn exr_header(bytes: &[u8]) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    if bytes.len() < 8 || bytes[..4] != [0x76, 0x2f, 0x31, 0x01] {
        return Err("not an OpenEXR file".into());
    }
    let mut pos = 8;
    let cstr = |pos: &mut usize| -> Result<String, String> {
        let end = bytes[*pos..]
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated header string")?
            + *pos;
        let s = String::from_utf8_lossy(&bytes[*pos..end]).to_string();
        *pos = end + 1;
        Ok(s)
    };
    let mut out = Vec::new();
    loop {
        let name = cstr(&mut pos)?;
        if name.is_empty() {
            return Ok(out);
        }
        let ty = cstr(&mut pos)?;
        let size = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        out.push((name, ty, bytes[pos..pos + size].to_vec()));
        pos += size;
    }
}

/// Channel names from a `chlist` value: name\0 + 16 bytes, repeated, then \0.
fn chlist_names(v: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut pos = 0;
    while pos < v.len() && v[pos] != 0 {
        let end = v[pos..].iter().position(|b| *b == 0).unwrap() + pos;
        names.push(String::from_utf8_lossy(&v[pos..end]).to_string());
        pos = end + 1 + 16;
    }
    names
}

#[tokio::test]
#[ignore]
async fn live_relight_standard_121() {
    let (Ok(url), Ok(clip)) = (
        std::env::var("RELIGHT_LIVE_URL"),
        std::env::var("RELIGHT_LIVE_CLIP"),
    ) else {
        panic!("set RELIGHT_LIVE_URL and RELIGHT_LIVE_CLIP");
    };
    let store = TemplateStore::new(TEMPLATES).unwrap();
    let run = Run {
        label: "relight-std-121".into(),
        template: Some("cosmos-passes-std".into()),
        graph: None,
        w: 1920,
        h: 1088,
        fps: 25,
        frames: 121,
        seed: "42".into(),
        prompt: String::new(),
        output: OutputKind::ExrFrames,
        images: vec![],
        videos: vec![clip.clone()],
        strength: None,
        expect_order_refusal: false,
        timeout_secs: 2100,
        expect_audio_kept: None,
        deep_frames: None,
    };
    let client = ComfyClient::new(&url).unwrap();
    let video_names = upload_all(&client, &run.videos).await.unwrap();
    let (graph, job, _hash) = prepare(&store, &run, &[], &video_names, &[]).unwrap();
    let started = Instant::now();
    let prompt_id = client.submit(&graph).await.expect("submit");
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    client
        .watch(&prompt_id, tx, run.timeout_secs)
        .await
        .expect("watch");
    let _ = drain.await;
    println!(
        "relight Standard 121 rendered in {:.0} s",
        started.elapsed().as_secs_f64()
    );
    let refs = client.outputs(&prompt_id).await.expect("outputs");
    let ordered = order_refs(&job.unwrap(), refs).expect("order_refs accepts the delivery");
    assert_eq!(ordered.len(), 122, "one preview + 121 EXRs");
    assert!(
        !ordered[0].filename.ends_with(".exr"),
        "frames[0] is the preview"
    );
    assert!(ordered[1..].iter().all(|r| r.filename.ends_with(".exr")));
    let bytes = client
        .download(&ordered[1])
        .await
        .expect("download the first EXR");
    let header = exr_header(&bytes).expect("EXR header");
    let channels = header
        .iter()
        .find(|(n, t, _)| n == "channels" && t == "chlist")
        .expect("channels");
    let mut names = chlist_names(&channels.2);
    names.sort();
    assert_eq!(
        names,
        [
            "basecolor.B",
            "basecolor.G",
            "basecolor.R",
            "normal.B",
            "normal.G",
            "normal.R"
        ]
    );
    let conv = header
        .iter()
        .find(|(n, _, _)| n == "platformless:conventions")
        .expect("conventions attribute");
    let conv: Value = serde_json::from_slice(&conv.2).expect("conventions are JSON");
    assert_eq!(conv["passes"], json!(["normal", "basecolor"]));
    assert_eq!(conv["schema"], "vfx-passes-v1", "{conv}");
    println!("first EXR: {} bytes, conventions {conv}", bytes.len());
}
