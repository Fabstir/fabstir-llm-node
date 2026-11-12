// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Retention: the log of requests (which host asked for which signature) is cut to a configured period; the
//! repository of records, which detection needs, is never touched. Each test names the mutation that turns it red.
use provenance_signer::config::Config;
use provenance_signer::repo::{Kind, Meta, Record, Repo};
use provenance_signer::store::{request_log_cutoff, Pruned, Reservation, Store};

const DAY: u64 = 86_400;
const NOW: u64 = 1_800_000_000;

fn reservation(id: &str, expires_at: u64) -> Reservation {
    Reservation {
        id: id.into(),
        host: "ab".repeat(20),
        template_id: "ltx-t2v-hdr".into(),
        bundle_version: 50,
        params_json: "{}".into(),
        sequence_params_json: None,
        sequence_json: None,
        created_at: expires_at - 3_600,
        expires_at,
    }
}

fn audit(store: &Store, ts: u64, uuid: &str) {
    store.audit(
        ts,
        Some(&"ab".repeat(20)),
        "/v1/sign",
        Some("r"),
        Some("mp4"),
        Some(uuid),
        "signed",
        None,
    );
}

#[test]
fn the_request_log_is_cut_to_the_period_and_nothing_younger_goes() {
    // no delete -> the counts stay; `<=` for `<` or a cut measured from the wrong field -> a kept row goes -> red
    let store = Store::in_memory().unwrap();
    let cutoff = request_log_cutoff(NOW, 365);
    assert_eq!(cutoff, NOW - 365 * DAY);
    let (old, edge, young) = (cutoff - DAY, cutoff, NOW - DAY);
    for (name, t) in [("old", old), ("edge", edge), ("young", young)] {
        audit(&store, t, name);
        store.insert_reservation(&reservation(name, t)).unwrap();
        assert!(store.consume_slot(name, "mp4", "sha", name, t).unwrap());
    }
    // a reservation that expired long ago but whose slot was signed inside the period keeps both rows
    store
        .insert_reservation(&reservation("late-slot", old))
        .unwrap();
    assert!(store
        .consume_slot("late-slot", "mp4", "sha", "late-slot", young)
        .unwrap());
    let gone = store.prune_request_log(cutoff).unwrap();
    assert_eq!(
        gone,
        Pruned {
            audit: 1,
            reservations: 1,
            slots: 1
        }
    );
    assert_eq!(
        store.request_log_rows().unwrap(),
        Pruned {
            audit: 2,
            reservations: 3,
            slots: 3
        }
    );
    assert!(store.reservation("old").unwrap().is_none());
    assert!(store.slot("old", "mp4").unwrap().is_none());
    for kept in ["edge", "young", "late-slot"] {
        assert!(store.reservation(kept).unwrap().is_some(), "{kept}");
        assert!(store.slot(kept, "mp4").unwrap().is_some(), "{kept}");
    }
    // the reconciliation count of the last day is as before
    assert_eq!(
        store.signatures_per_host(NOW - 2 * DAY, NOW).unwrap(),
        vec![("ab".repeat(20), 1)]
    );
    // a second run finds nothing more
    assert_eq!(store.prune_request_log(cutoff).unwrap(), Pruned::default());
}

#[test]
fn the_register_of_records_is_never_pruned() {
    // a prune that reached into the repository -> the record or its files are gone -> red
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("signer.sqlite")).unwrap();
    let repo = Repo::open(&dir.path().join("repository")).unwrap();
    let uuid = "0a0a0a0a-0b0b-4c0c-8d0d-0e0e0e0e0e0e";
    let meta = Meta {
        kind: Kind::Sequence,
        class: fabstir_provenance::class::lookup("ltx-t2v-hdr")
            .unwrap()
            .class,
        has_visual_input: false,
        time: 1, // a record as old as records get
        public: true,
        request_sha256: "00".repeat(32),
    };
    repo.put(
        uuid,
        &Record {
            manifest: b"manifest".to_vec(),
            meta: meta.clone(),
            fingerprint: None,
            frames_index: Some(b"[]".to_vec()),
        },
    )
    .unwrap();
    audit(&store, 1, uuid);
    let files = |root: &std::path::Path| {
        let mut out = vec![];
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    stack.push(e.path());
                } else {
                    out.push((e.path(), std::fs::read(e.path()).unwrap()));
                }
            }
        }
        out.sort();
        out
    };
    let before = files(&dir.path().join("repository"));
    assert!(before.len() >= 3, "manifest, meta and frames index");
    assert_eq!(store.prune_request_log(u64::MAX).unwrap().audit, 1);
    assert_eq!(files(&dir.path().join("repository")), before);
    assert_eq!(repo.get_public(uuid).unwrap().1, meta);
    assert_eq!(repo.get_frames_index(uuid).unwrap(), b"[]");
    // and the prune's own statements name the three request-log tables only
    let src = include_str!("../src/store.rs");
    let body = &src[src.find("pub fn prune_request_log").unwrap()..];
    let body = &body[..body.find("pub fn request_log_rows").unwrap()];
    let deleted: Vec<&str> = body
        .match_indices("DELETE FROM ")
        .map(|(i, m)| body[i + m.len()..].split_whitespace().next().unwrap())
        .collect();
    assert_eq!(deleted, ["audit", "slots", "reservations"]);
}

fn config(extra: &str) -> anyhow::Result<Config> {
    let dir = tempfile::tempdir().unwrap();
    let json = include_str!("../deploy/config.example.json")
        .replace("0x<host1 node address>", &format!("0x{}", "ab".repeat(20)))
        .replace("  \"request_log_retention_days\": 365,\n", extra);
    let path = dir.path().join("config.json");
    std::fs::write(&path, json).unwrap();
    Config::load(&path)
}

#[test]
fn the_period_is_a_setting_with_a_year_as_its_default() {
    // another default -> red; no lower bound -> a typo of 0 would empty the log at the next start -> red
    assert_eq!(config("").unwrap().request_log_retention_days, 365);
    assert_eq!(
        config("  \"request_log_retention_days\": 400,\n")
            .unwrap()
            .request_log_retention_days,
        400
    );
    for bad in [0, 6] {
        let e = config(&format!("  \"request_log_retention_days\": {bad},\n")).unwrap_err();
        assert!(e.to_string().contains("request_log_retention_days"), "{e}");
    }
    assert!(config("  \"request_log_retention_days\": 7,\n").is_ok());
    // the serving process runs the prune with that setting
    let main: String = include_str!("../src/main.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(main.contains("cfg.request_log_retention_days"));
    assert!(
        main.contains(".prune_request_log(provenance_signer::store::request_log_cutoff(now,days))")
    );
}
