//! Design §12.1: the two capture rings evict only their own oldest; files are
//! capped; nothing secret is ever written by the caller's record.

use fabstir_llm_node::kbs::capture::{Capture, CaptureRecord, Ring, FILE_CAP};

fn rec(tag: &str) -> CaptureRecord {
    CaptureRecord {
        request: format!("{{\"tag\":\"{tag}\"}}").into_bytes(),
        nras: Some(b"[]".to_vec()),
        verified: Some(serde_json::json!({"tag": tag})),
        decision: format!("released {tag}"),
    }
}

fn count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map(|r| r.count()).unwrap_or(0)
}

#[test]
fn writes_the_four_files_into_the_right_ring() {
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::new(dir.path().to_path_buf(), 100, 20);
    let d = c.write(Ring::Verified, &[0xab; 32], &rec("a")).unwrap();
    assert!(d.starts_with(dir.path().join("verified")));
    assert!(
        d.file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(&format!("-{}", "ab".repeat(8))),
        "model_id[..8] = 16 hex"
    );
    for f in ["request.json", "nras.json", "verified.json", "decision.txt"] {
        assert!(d.join(f).is_file(), "{f}");
    }
    let d2 = c
        .write(
            Ring::Preverify,
            &[0xcd; 32],
            &CaptureRecord {
                nras: None,
                ..rec("b")
            },
        )
        .unwrap();
    assert!(d2.starts_with(dir.path().join("preverify")));
    assert!(!d2.join("nras.json").exists());
}

#[test]
fn each_ring_evicts_only_its_own_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::new(dir.path().to_path_buf(), 3, 2);
    let first_verified = c.write(Ring::Verified, &[1; 32], &rec("v1")).unwrap();
    for i in 0..10 {
        c.write(Ring::Preverify, &[2; 32], &rec(&format!("p{i}")))
            .unwrap();
    }
    assert!(
        first_verified.is_dir(),
        "a shared ring would have evicted the verified capture"
    );
    assert_eq!(count(&dir.path().join("preverify")), 2);
    for i in 0..5 {
        c.write(Ring::Verified, &[1; 32], &rec(&format!("v{i}")))
            .unwrap();
    }
    assert_eq!(count(&dir.path().join("verified")), 3);
    assert!(
        !first_verified.is_dir(),
        "the verified ring evicts its own oldest"
    );
}

#[test]
fn zero_cap_disables_a_ring_and_files_are_capped() {
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::new(dir.path().to_path_buf(), 1, 0);
    assert!(c.write(Ring::Preverify, &[1; 32], &rec("x")).is_none());
    assert!(c.enabled());
    let big = CaptureRecord {
        request: vec![b'x'; FILE_CAP + 10],
        ..rec("big")
    };
    let d = c.write(Ring::Verified, &[1; 32], &big).unwrap();
    // over the cap the file is `<name>.truncated`, never a `.json` that does not parse
    // (mutation: keep the name → the runbook's JSON read fails)
    assert!(!d.join("request.json").exists());
    let written = std::fs::read(d.join("request.json.truncated")).unwrap();
    assert!(written.len() < FILE_CAP + 100);
    assert!(written.ends_with(b"[truncated at 1 MiB]\n"));
    assert!(
        d.join("decision.txt").is_file(),
        "the other files are untouched"
    );
    let off = Capture::new(dir.path().to_path_buf(), 0, 0);
    assert!(!off.enabled());
}

#[test]
fn a_capture_is_never_evicted_while_still_being_written() {
    // Concurrent writers under a tiny cap: every completed capture has all its files,
    // no ".tmp-" leftovers, and the ring holds exactly the cap afterwards.
    let dir = tempfile::tempdir().unwrap();
    let c = std::sync::Arc::new(Capture::new(dir.path().to_path_buf(), 100, 3));
    let handles: Vec<_> = (0..12)
        .map(|i| {
            let c = c.clone();
            std::thread::spawn(move || {
                c.write(Ring::Preverify, &[i as u8; 32], &rec(&format!("t{i}")))
            })
        })
        .collect();
    let written: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(
        written.iter().all(|w| w.is_some()),
        "every write completed (mutation: write in place → eviction races a writer)"
    );
    let ring = dir.path().join("preverify");
    let names: Vec<String> = std::fs::read_dir(&ring)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.starts_with(".tmp-")), "{names:?}");
    assert_eq!(names.len(), 3);
    for n in &names {
        for f in ["request.json", "decision.txt"] {
            assert!(ring.join(n).join(f).is_file(), "{n}/{f}");
        }
    }
}

#[test]
fn orphaned_temp_directories_are_swept_at_start_and_never_counted() {
    // A SIGKILL between write_files and rename leaves `.tmp-<name>`; eviction skips
    // it by design, so the next start must reclaim it (mutation: drop the sweep →
    // the orphan outlives every cap).
    let dir = tempfile::tempdir().unwrap();
    let ring = dir.path().join("preverify");
    let orphan = ring.join(".tmp-0000000000001-000001-0101010101010101");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("request.json"), b"{}").unwrap();
    let live = ring.join("0000000000002-000002-0101010101010101");
    std::fs::create_dir_all(&live).unwrap();
    let c = Capture::new(dir.path().to_path_buf(), 100, 20);
    assert!(!orphan.exists(), "the orphan is reclaimed at construction");
    assert!(live.is_dir(), "a completed capture is untouched");
    assert_eq!(c.sweep_orphans(), 0);
}

#[test]
fn two_captures_in_one_millisecond_get_distinct_directories() {
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::new(dir.path().to_path_buf(), 100, 20);
    let a = c.write(Ring::Verified, &[1; 32], &rec("a")).unwrap();
    let b = c.write(Ring::Verified, &[1; 32], &rec("b")).unwrap();
    assert_ne!(a, b);
    assert_eq!(count(&dir.path().join("verified")), 2);
}
