// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks, fix pass (F4, F8 to F11): a staging write that outlives its caller, folders that must not
//! overlap, write access wherever job files are deleted, deletion that a symlink cannot steer, durable sealing
//! with a fallback, and the offline opener's output folder. Each test names the mutation that turns it red.
use fabstir_llm_node::ltx::checks::config::{ChecksConfig, Mode, RendererDirs};
use fabstir_llm_node::ltx::checks::files::{
    delete_job_files, resolve_under, InUse, JobFiles, Target,
};
use fabstir_llm_node::ltx::checks::seal;
use fabstir_llm_node::ltx::checks::staging::{is_staged_name, StageError, Staging};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[tokio::test]
async fn a_staging_write_that_outlives_its_caller_cleans_up_after_itself() {
    // fix F4. A writer that ignores the abandoned flag -> the file appears after the guard is gone -> red
    let dir = tempfile::tempdir().unwrap();
    let files = || std::fs::read_dir(dir.path()).unwrap().count();
    let mut slow = Staging::new(dir.path(), dir.path(), 1 << 20);
    slow.write_delay = Duration::from_millis(400);
    // the caller's own limit fires while the blocking write has not started writing
    let dropped = tokio::time::timeout(Duration::from_millis(50), slow.stage(&[9u8; 4096], "mp4"));
    assert!(dropped.await.is_err(), "the caller gave up");
    assert_eq!(files(), 0);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        files(),
        0,
        "the write finished after the drop and removed its file"
    );
    // the staging area's own bound: the same guarantee, and an error instead of a hang
    let bounded = slow
        .clone()
        .with_write_bound(Duration::from_millis(50))
        .stage(&[9u8; 4096], "mp4")
        .await;
    assert_eq!(
        bounded.unwrap_err(),
        StageError::Io("write timed out".to_string())
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(files(), 0);
    // and an unhurried write is there until its guard goes
    let kept = slow.stage(&[9u8; 4096], "mp4").await.unwrap();
    assert_eq!(files(), 1);
    drop(kept);
    assert_eq!(files(), 0);
}

fn env(pairs: &[(&'static str, &'static str)]) -> Result<ChecksConfig, String> {
    let map: HashMap<&str, &str> = pairs.iter().copied().collect();
    ChecksConfig::from_lookup(true, true, |k| map.get(k).map(|v| v.to_string()))
}

const KEY: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
const BASE: [(&str, &str); 11] = [
    ("LTX_CONTENT_CHECKS", "enforce"),
    ("LTX_CHECKS_SOCKET", "/run/mod/mod.sock"),
    ("LTX_CHECKS_STAGING_DIR", "/run/checks/staging"),
    ("LTX_CHECKS_STAGING_DIR_SERVICE", "/staging"),
    ("COMFY_INPUT_DIR", "/app/comfy-input"),
    ("COMFY_OUTPUT_DIR", "/app/comfy-output"),
    ("RELIGHT_INPUT_DIR", "/app/relight-input"),
    ("RELIGHT_OUTPUT_DIR", "/app/relight-output"),
    ("LTX_EVIDENCE_PUBKEY", KEY),
    ("LTX_EVIDENCE_DIR", "/var/lib/evidence"),
    (
        "LTX_CHECKS_BUNDLE_HASH",
        "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12",
    ),
];

fn with(changes: &[(&'static str, &'static str)]) -> Result<ChecksConfig, String> {
    let mut pairs: Vec<(&str, &str)> = BASE.to_vec();
    for &(k, v) in changes {
        pairs.retain(|(name, _)| *name != k);
        pairs.push((k, v));
    }
    env(&pairs)
}

#[test]
fn the_folders_must_be_distinct_and_none_inside_another() {
    // fix F9. No overlap check -> the sweep, the job-end deletion and the evidence share a folder -> red
    with(&[]).expect("distinct folders load");
    for (key, value, other) in [
        ("LTX_EVIDENCE_DIR", "/app/comfy-output", "COMFY_OUTPUT_DIR"),
        (
            "LTX_EVIDENCE_DIR",
            "/app/comfy-output/evidence",
            "COMFY_OUTPUT_DIR",
        ),
        (
            "COMFY_INPUT_DIR",
            "/app/comfy-output/../comfy-output/in",
            "COMFY_OUTPUT_DIR",
        ),
        (
            "LTX_CHECKS_STAGING_DIR",
            "/app/comfy-input/staging",
            "COMFY_INPUT_DIR",
        ),
        ("LTX_CHECKS_STAGING_DIR", "/var/lib", "LTX_EVIDENCE_DIR"),
        (
            "RELIGHT_OUTPUT_DIR",
            "/app/comfy-output",
            "COMFY_OUTPUT_DIR",
        ),
        (
            "RELIGHT_INPUT_DIR",
            "/app/relight-output/./",
            "RELIGHT_OUTPUT_DIR",
        ),
    ] {
        let e = with(&[(key, value)]).unwrap_err();
        assert!(
            e.contains("overlap") && e.contains(key) && e.contains(other),
            "{key}={value}: {e}"
        );
    }
    // a name that only starts the same is another folder
    with(&[("COMFY_INPUT_DIR", "/app/comfy-output-2")]).unwrap();
    // the refusals file belongs with the evidence (its default), never where files are swept or deleted
    let cfg = with(&[]).unwrap();
    assert_eq!(
        cfg.refusals_file.as_deref(),
        Some(std::path::Path::new("/var/lib/evidence/refusals.jsonl"))
    );
    with(&[("LTX_REFUSALS_FILE", "/var/log/node/refusals.jsonl")]).unwrap();
    for inside in [
        "/run/checks/staging/r.jsonl",
        "/app/comfy-output/video/r.jsonl",
    ] {
        let e = with(&[("LTX_REFUSALS_FILE", inside)]).unwrap_err();
        assert!(e.contains("LTX_REFUSALS_FILE lies inside"), "{e}");
    }
    // with checks off the renderer folders still may not overlap: files are deleted there in any mode
    let e = with(&[
        ("LTX_CONTENT_CHECKS", "off"),
        ("COMFY_INPUT_DIR", "/app/comfy-output/input"),
    ])
    .unwrap_err();
    assert!(e.contains("overlap"), "{e}");
}

#[test]
fn start_up_sees_through_a_symlink_and_checks_delete_access_in_any_mode() {
    // fix F9: comparing the paths only as written -> a symlinked alias of the evidence folder passes -> red.
    // fix F11: write access checked in `enforce` only -> the read-only folder passes with checks off -> red
    let tmp = tempfile::tempdir().unwrap();
    let p = |n: &str| tmp.path().join(n);
    for d in ["staging", "in", "out", "evidence"] {
        std::fs::create_dir(p(d)).unwrap();
    }
    let mut cfg = ChecksConfig::off();
    cfg.mode = Mode::Enforce;
    cfg.staging_node = Some(p("staging"));
    cfg.evidence_dir = Some(p("evidence"));
    cfg.comfy_dirs = Some(RendererDirs {
        input: p("in"),
        output: p("out"),
    });
    cfg.verify_access().unwrap();
    // another name for the evidence folder, handed over as the renderer's output folder
    std::os::unix::fs::symlink(p("evidence"), p("alias")).unwrap();
    cfg.comfy_dirs.as_mut().unwrap().output = p("alias");
    cfg.check_distinct(false).expect("as written they differ");
    let e = cfg.verify_access().unwrap_err();
    assert!(e.contains("overlap"), "{e}");

    // checks off, folders set: job files are deleted, so the node must be able to
    let mut off = ChecksConfig::off();
    off.comfy_dirs = Some(RendererDirs {
        input: p("in"),
        output: p("out"),
    });
    off.verify_access().unwrap();
    std::fs::set_permissions(p("out"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let e = off.verify_access().unwrap_err();
    assert!(
        e.contains("COMFY_OUTPUT_DIR") && e.contains("not writable"),
        "{e}"
    );
    std::fs::set_permissions(p("out"), std::fs::Permissions::from_mode(0o700)).unwrap();
    // and with no folders set there is nothing to check
    ChecksConfig::off().verify_access().unwrap();
}

#[test]
fn a_symlink_in_a_renderer_folder_cannot_steer_a_delete() {
    // fix F10. Deleting through the path as written -> the sealed file is gone -> red
    let root = tempfile::tempdir().unwrap();
    let p = |n: &str| root.path().join(n);
    for d in ["in", "out/video", "evidence"] {
        std::fs::create_dir_all(p(d)).unwrap();
    }
    std::fs::write(p("evidence/1.sealed"), b"sealed").unwrap();
    std::fs::write(p("out/video/real.mp4"), b"x").unwrap();
    // planted where the renderer can write: folders that lead out, and a link that is itself a "file"
    std::os::unix::fs::symlink(p("evidence"), p("out/escape")).unwrap();
    std::os::unix::fs::symlink(p("evidence"), p("in/deadbeef")).unwrap();
    std::os::unix::fs::symlink(p("evidence/1.sealed"), p("out/video/link.mp4")).unwrap();
    assert_eq!(resolve_under(&p("out"), "escape/1.sealed"), Target::Outside);
    assert_eq!(resolve_under(&p("out"), "gone/x.mp4"), Target::Missing);
    assert!(matches!(
        resolve_under(&p("out"), "video/real.mp4"),
        Target::Inside(_)
    ));
    let dirs = RendererDirs {
        input: p("in"),
        output: p("out"),
    };
    let files = JobFiles {
        inputs: vec![],
        input_dirs: vec!["deadbeef".into()],
        outputs: vec![
            ("escape".into(), "1.sealed".into()),
            ("video".into(), "link.mp4".into()),
            ("video".into(), "real.mp4".into()),
        ],
    };
    let (removed, failed) = delete_job_files(&dirs, &files, &InUse::default());
    assert_eq!(
        (removed, failed),
        (3, 1),
        "the escape is refused; links are removed as links"
    );
    assert_eq!(
        std::fs::read(p("evidence/1.sealed")).unwrap(),
        b"sealed",
        "nothing outside the renderer's folders was touched"
    );
    assert!(!p("out/video/real.mp4").exists() && !p("out/video/link.mp4").exists());
    assert!(std::fs::symlink_metadata(p("in/deadbeef")).is_err());
    assert!(
        p("out/escape").exists(),
        "the planted folder link is left for the operator"
    );
}

#[test]
fn only_names_this_node_stages_are_swept() {
    assert!(is_staged_name("0123456789abcdef0123456789abcdef.mp4"));
    assert!(is_staged_name("0123456789abcdef0123456789abcdef.webp.part"));
    for other in [
        "0123456789abcdef0123456789abcdef.sealed",
        "0123456789ABCDEF0123456789ABCDEF.mp4",
        "0123456789abcdef0123456789abcde.mp4",
        "refusals.jsonl",
        "20261112T101500Z-12-c3-0123456789abcdef0123456789abcdef.sealed",
        ".mp4",
        "0123456789abcdef0123456789abcdef",
    ] {
        assert!(!is_staged_name(other), "{other}");
    }
}

#[test]
fn sealed_and_unsealed_copies_are_written_durably_and_owner_only() {
    // fix F8. No directory sync, or a fallback written group-readable -> red
    let dir = tempfile::tempdir().unwrap();
    let kept = seal::write_unsealed(dir.path(), "item.mp4", b"the item").unwrap();
    assert_eq!(kept, dir.path().join("unsealed/item.mp4"));
    assert_eq!(std::fs::read(&kept).unwrap(), b"the item");
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        (mode(&kept), mode(&dir.path().join("unsealed"))),
        (0o600, 0o700)
    );
    assert!(
        seal::write_unsealed(dir.path(), "item.mp4", b"again").is_err(),
        "a kept item is never overwritten"
    );
    let src: String = include_str!("../../src/ltx/checks/seal.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let durable = &src[src.find("fnwrite_durable(").unwrap()..];
    let (sync_file, rename, sync_dir) = (
        durable.find("f.sync_all()").unwrap(),
        durable.find("std::fs::rename(&part,&dest)").unwrap(),
        durable.find("sync_dir(dir)").unwrap(),
    );
    assert!(
        sync_file < rename && rename < sync_dir,
        "file, rename, then the directory"
    );
}

#[test]
fn the_offline_opener_keeps_what_it_opens_to_the_owner() {
    // fix F8. An output folder created with the default mode -> other users on the machine can list it -> red
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_open-evidence"))
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let printed = run(&["keygen", "--out-key", "k.key"]);
    let pubkey = printed
        .lines()
        .find_map(|l| l.strip_prefix("LTX_EVIDENCE_PUBKEY="))
        .expect("the public key is printed");
    run(&["seal-test", "--pubkey", pubkey, "--out", "t.sealed"]);
    run(&[
        "open",
        "--key-file",
        "k.key",
        "t.sealed",
        "--out-dir",
        "opened/deep",
    ]);
    let mode = |p: &str| {
        std::fs::metadata(dir.path().join(p))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("opened/deep"), 0o700);
    assert_eq!(
        (mode("opened/deep/t.item.txt"), mode("k.key")),
        (0o600, 0o600)
    );
    assert!(
        std::fs::read_to_string(dir.path().join("opened/deep/t.item.txt"))
            .unwrap()
            .contains("synthetic evidence item")
    );
}

#[tokio::test]
async fn a_kept_staged_file_survives_the_start_up_sweep() {
    // fix H3. Kept under its staging name -> the next start-up sweep deletes the only copy -> red
    let dir = tempfile::tempdir().unwrap();
    let st = Staging::new(dir.path(), std::path::Path::new("/svc"), 1 << 20);
    let staged = st.stage(b"the only copy", "mp4").await.unwrap();
    let was = staged.node_path().to_path_buf();
    let kept = staged.keep();
    let name = kept.file_name().unwrap().to_str().unwrap().to_string();
    assert!(!was.exists(), "no longer under a name the sweep matches");
    assert_eq!(
        name,
        format!("kept-{}", was.file_name().unwrap().to_str().unwrap())
    );
    assert!(!is_staged_name(&name));
    assert_eq!(st.sweep(), 0);
    assert_eq!(std::fs::read(&kept).unwrap(), b"the only copy");
}

#[test]
fn the_refusals_file_must_be_an_absolute_path() {
    // fix H4. A relative path accepted -> it cannot be compared with the folders, so the overlap rule is skipped -> red
    for relative in ["refusals.jsonl", "staging/refusals.jsonl", "./r.jsonl"] {
        let e = with(&[("LTX_REFUSALS_FILE", relative)]).unwrap_err();
        assert!(
            e.contains("LTX_REFUSALS_FILE must be an absolute path"),
            "{relative}: {e}"
        );
    }
    let ok = with(&[("LTX_REFUSALS_FILE", "/var/log/fabstir/refusals.jsonl")]).unwrap();
    assert_eq!(
        ok.refusals_file.as_deref(),
        Some(std::path::Path::new("/var/log/fabstir/refusals.jsonl"))
    );
    // and what a relative path would have slipped past is still refused when written in full
    assert!(with(&[("LTX_REFUSALS_FILE", "/run/checks/staging/refusals.jsonl")]).is_err());
}
