// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! WP-N2 Phase 0/1 tests: operator list-file parsers + env resolution
//! (`IMPLEMENTATION-MODERATION-LISTS.md` §4 Phase 0/1).

use fabstir_llm_node::moderation::csam::hashlist::ListState;
use fabstir_llm_node::moderation::csam::listfile::{
    parse_list_file, parse_ownhash_file, resolve_pdq_max_distance, snapshot_from_parsed,
    FramesMatchState,
};

fn sha_line(bytes: [u8; 32]) -> String {
    format!("sha256:{}", hex::encode(bytes))
}

fn pdq_line(bytes: [u8; 32]) -> String {
    format!("pdq:{}", hex::encode(bytes))
}

// ---------------------------------------------------------------- Sub-phase 0.1

#[test]
fn mixed_lines_parse_to_exact_counts() {
    let content = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        sha_line([0x11; 32]),
        sha_line([0x22; 32]),
        pdq_line([0x33; 32]),
        pdq_line([0x44; 32]),
        pdq_line([0x55; 32]),
    );
    let (sha, pdq) = parse_list_file(&content).expect("mixed file must parse");
    assert_eq!(sha.len(), 2, "exactly the two sha256 entries");
    assert_eq!(pdq.len(), 3, "exactly the three pdq entries");
    assert!(sha.contains(&[0x11; 32]));
    assert!(sha.contains(&[0x22; 32]));
    assert!(pdq.iter().any(|p| p.0 == [0x33; 32]));
}

#[test]
fn comments_blanks_and_crlf_tolerated() {
    let content = format!(
        "# operator block list\r\n\r\n{}\r\n  \r\n# trailing comment\r\n",
        sha_line([0xab; 32])
    );
    let (sha, pdq) = parse_list_file(&content).expect("comments/blanks/CRLF must parse");
    assert_eq!(sha.len(), 1);
    assert!(pdq.is_empty());
    assert!(sha.contains(&[0xab; 32]));
}

#[test]
fn utf8_bom_is_tolerated_like_crlf() {
    // Windows-transit tolerance: an invisible BOM must not produce a baffling
    // rejection of a visually-valid first line.
    let content = format!("\u{feff}{}\n", sha_line([0xbc; 32]));
    let (sha, _) = parse_list_file(&content).expect("BOM'd file must parse");
    assert!(sha.contains(&[0xbc; 32]));
    let own = format!("\u{feff}{}\n", hex::encode([0xbd; 32]));
    assert!(parse_ownhash_file(&own)
        .expect("BOM'd ownhash parses")
        .contains(&[0xbd; 32]));
}

#[test]
fn uppercase_hex_accepted() {
    let content = format!("sha256:{}\n", hex::encode([0xcd; 32]).to_uppercase());
    let (sha, _) = parse_list_file(&content).expect("uppercase hex is legal");
    assert!(
        sha.contains(&[0xcd; 32]),
        "uppercase entry must decode to the same bytes"
    );
}

#[test]
fn malformed_hex_errs_naming_line() {
    let content = format!(
        "{}\n{}\nsha256:zz{}\n",
        sha_line([0x11; 32]),
        pdq_line([0x22; 32]),
        &hex::encode([0x33; 32])[2..]
    );
    let err = parse_list_file(&content).expect_err("bad hex must reject the file");
    assert!(
        err.to_string().contains("line 3"),
        "error must name the 1-based line, got: {err}"
    );
}

#[test]
fn wrong_length_errs_naming_line() {
    let content = format!("sha256:{}\n", &hex::encode([0x11; 32])[..62]);
    let err = parse_list_file(&content).expect_err("62 hex chars must reject");
    assert!(
        err.to_string().contains("line 1"),
        "error must name the 1-based line, got: {err}"
    );
}

#[test]
fn unknown_prefix_errs_naming_line() {
    let content = format!(
        "{}\nmd5:{}\n",
        sha_line([0x11; 32]),
        hex::encode([0x22; 32])
    );
    let err = parse_list_file(&content).expect_err("unknown prefix must reject");
    assert!(
        err.to_string().contains("line 2"),
        "error must name the 1-based line, got: {err}"
    );
}

#[test]
fn inline_comment_makes_line_malformed() {
    let content = format!("{} # reviewed 2026-07-30\n", sha_line([0x11; 32]));
    let err = parse_list_file(&content).expect_err("inline comments are not allowed");
    assert!(
        err.to_string().contains("line 1"),
        "error must name the 1-based line, got: {err}"
    );
}

#[test]
fn empty_file_errs() {
    // Rule 2: no accidental empty clean list.
    parse_list_file("").expect_err("zero entries without directive is an error");
}

#[test]
fn comments_only_errs() {
    parse_list_file("# nothing here\n\n# still nothing\n")
        .expect_err("comments-only is empty ⇒ error");
}

#[test]
fn duplicates_dedupe_without_error() {
    let content = format!(
        "{}\n{}\n{}\n{}\n",
        sha_line([0x11; 32]),
        sha_line([0x11; 32]),
        pdq_line([0x22; 32]),
        pdq_line([0x22; 32]),
    );
    let (sha, pdq) = parse_list_file(&content).expect("duplicates are not an error");
    assert_eq!(sha.len(), 1, "sha256 duplicates deduplicate (HashSet)");
    assert_eq!(
        pdq.len(),
        2,
        "pdq duplicates kept (Vec) — matcher takes min distance, inert"
    );
}

#[test]
fn directive_plus_entry_errs() {
    // Rule 2: the directive is legal ONLY as the sole non-comment line —
    // truncating a populated list must never yield a valid directive-only file.
    let after = format!("#!allow-empty\n{}\n", sha_line([0x11; 32]));
    parse_list_file(&after).expect_err("directive + entry (entry after) must reject");
    let before = format!("{}\n#!allow-empty\n", sha_line([0x11; 32]));
    parse_list_file(&before).expect_err("directive + entry (entry before) must reject");
}

#[test]
fn directive_alone_accepts_empty() {
    let (sha, pdq) = parse_list_file("# day-1, no takedowns yet\n#!allow-empty\n")
        .expect("sole directive accepts an explicitly-empty list");
    assert!(sha.is_empty());
    assert!(pdq.is_empty());
}

#[test]
fn directive_variants_are_ordinary_comments() {
    // Case-sensitive, no interior space: near-misses are comments, so the file
    // is empty and fails (flag NOT set).
    parse_list_file("#! allow-empty\n").expect_err("space variant is a comment ⇒ empty ⇒ Err");
    parse_list_file("#!ALLOW-EMPTY\n").expect_err("uppercase variant is a comment ⇒ empty ⇒ Err");
}

// ---------------------------------------------------------------- Sub-phase 0.2

#[test]
fn ownhash_entries_parse_with_comments_blanks_crlf() {
    let content = format!(
        "# locally-confirmed hashes\r\n\r\n{}\r\n{}\r\n",
        hex::encode([0x11; 32]),
        hex::encode([0x22; 32]).to_uppercase(),
    );
    let own = parse_ownhash_file(&content).expect("ownhash file must parse");
    assert_eq!(own.len(), 2);
    assert!(own.contains(&[0x11; 32]));
    assert!(own.contains(&[0x22; 32]), "uppercase hex is legal");
}

#[test]
fn ownhash_malformed_errs_naming_line() {
    let content = format!("{}\nnot-hex-at-all\n", hex::encode([0x11; 32]));
    let err = parse_ownhash_file(&content)
        .err()
        .expect("malformed line must reject the file");
    assert!(
        err.to_string().contains("line 2"),
        "error must name the 1-based line, got: {err}"
    );
}

#[test]
fn ownhash_wrong_length_errs_naming_line() {
    let content = format!("{}\n", &hex::encode([0x11; 32])[..60]);
    let err = parse_ownhash_file(&content)
        .err()
        .expect("60 hex chars must reject");
    assert!(
        err.to_string().contains("line 1"),
        "error must name the 1-based line, got: {err}"
    );
}

#[test]
fn ownhash_empty_or_comments_only_errs() {
    assert!(
        parse_ownhash_file("").is_err(),
        "empty ownhash file is an error"
    );
    assert!(
        parse_ownhash_file("# nothing\n").is_err(),
        "comments-only ownhash file is an error"
    );
}

#[test]
fn ownhash_duplicates_dedupe_without_error() {
    let content = format!("{h}\n{h}\n", h = hex::encode([0x33; 32]));
    let own = parse_ownhash_file(&content).expect("duplicates are not an error");
    assert_eq!(own.len(), 1, "OwnHashList is set-backed");
}

#[test]
fn ownhash_directive_plus_entry_errs() {
    let after = format!("#!allow-empty\n{}\n", hex::encode([0x11; 32]));
    assert!(
        parse_ownhash_file(&after).is_err(),
        "directive + entry (entry after) must reject"
    );
    let before = format!("{}\n#!allow-empty\n", hex::encode([0x11; 32]));
    assert!(
        parse_ownhash_file(&before).is_err(),
        "directive + entry (entry before) must reject"
    );
}

#[test]
fn ownhash_directive_alone_accepts_empty_list() {
    // The 0.1 pin's translation: "empty Loaded" reads as "accepted empty list"
    // (OwnHashList has no availability state).
    let own = parse_ownhash_file("#!allow-empty\n").expect("sole directive accepts empty");
    assert!(own.is_empty());
}

#[test]
fn ownhash_directive_variants_are_ordinary_comments() {
    assert!(
        parse_ownhash_file("#! allow-empty\n").is_err(),
        "space variant ⇒ comment ⇒ empty ⇒ Err"
    );
    assert!(
        parse_ownhash_file("#!ALLOW-EMPTY\n").is_err(),
        "uppercase variant ⇒ comment ⇒ empty ⇒ Err"
    );
}

// ---------------------------------------------------------------- Sub-phase 0.3

#[test]
fn snapshot_is_loaded_and_carries_parsed_entries() {
    let content = format!("{}\n{}\n", sha_line([0x11; 32]), pdq_line([0x22; 32]));
    let (sha, pdq) = parse_list_file(&content).expect("must parse");
    let snap = snapshot_from_parsed(sha, pdq, content.as_bytes());
    assert_eq!(
        snap.state,
        ListState::Loaded,
        "an operator list installs as Loaded"
    );
    assert!(snap.sha256.contains(&[0x11; 32]));
    assert!(snap.pdq.iter().any(|p| p.0 == [0x22; 32]));
    assert!(
        snap.require_available().is_ok(),
        "Loaded must pass the availability gate"
    );
}

#[test]
fn snapshot_version_is_content_fingerprint() {
    // Rule 6: version = first 8 bytes of the SHA-256 of the file bytes —
    // same file ⇒ same value on any host; any change ⇒ visible change.
    let content = format!("{}\n", sha_line([0x11; 32]));
    let (s1, p1) = parse_list_file(&content).expect("must parse");
    let (s2, p2) = parse_list_file(&content).expect("must parse");
    let a = snapshot_from_parsed(s1, p1, content.as_bytes());
    let b = snapshot_from_parsed(s2, p2, content.as_bytes());
    assert_eq!(a.version, b.version, "same bytes ⇒ same fingerprint");
    assert_ne!(
        a.version, 0,
        "fingerprint of real content is not the empty default"
    );

    let changed = content.replace("sha256:1", "sha256:2");
    let (s3, p3) = parse_list_file(&changed).expect("must parse");
    let c = snapshot_from_parsed(s3, p3, changed.as_bytes());
    assert_ne!(
        a.version, c.version,
        "one-byte change ⇒ different fingerprint"
    );
}

#[test]
fn one_kind_only_lists_are_fine() {
    let (sha, pdq) = parse_list_file(&pdq_line([0x66; 32])).expect("pdq-only list is legal");
    assert!(sha.is_empty());
    assert_eq!(pdq.len(), 1);
    let (sha2, pdq2) = parse_list_file(&sha_line([0x77; 32])).expect("sha-only list is legal");
    assert_eq!(sha2.len(), 1);
    assert!(pdq2.is_empty());
}

// ---------------------------------------------------------------- Sub-phase 1.1

const LIST_VAR: &str = "MODERATION_LIST_FILE";
const OWN_VAR: &str = "MODERATION_OWNHASH_FILE";
const PDQ_VAR: &str = "MODERATION_PDQ_MAX_DISTANCE";

/// Restore-on-drop guard: env vars are process-global and this serial binary
/// has no env mutex — a failing assert mid-table must not leak vars into later
/// tests (the Phase-1.2 pin tests depend on a clean environment).
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new(vars: &[&'static str]) -> Self {
        let saved = vars.iter().map(|v| (*v, std::env::var(v).ok())).collect();
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (var, val) in &self.saved {
            match val {
                Some(v) => std::env::set_var(var, v),
                None => std::env::remove_var(var),
            }
        }
    }
}

fn clear_moderation_env() {
    std::env::remove_var(LIST_VAR);
    std::env::remove_var(OWN_VAR);
    std::env::remove_var(PDQ_VAR);
}

#[test]
fn env_resolution_permutation_table() {
    let _guard = EnvGuard::new(&[LIST_VAR, OWN_VAR, PDQ_VAR]);
    let dir = tempfile::tempdir().expect("tempdir");
    let list_path = dir.path().join("list.txt");
    let own_path = dir.path().join("own.txt");
    std::fs::write(
        &list_path,
        format!("{}\n{}\n", sha_line([0x11; 32]), pdq_line([0x22; 32])),
    )
    .unwrap();
    std::fs::write(&own_path, format!("{}\n", hex::encode([0x33; 32]))).unwrap();

    // Row 1: neither var set ⇒ byte-for-byte today's state (the no-op proof).
    clear_moderation_env();
    let st = FramesMatchState::from_env().expect("no env vars is never an error");
    assert!(
        st.snapshot.require_available().is_err(),
        "no list ⇒ Unavailable (fail-closed default)"
    );
    assert!(st.ownhash.is_empty());
    assert_eq!(st.max_distance, 31);

    // Row 2: list file only ⇒ Loaded snapshot with the entries, fp stamped.
    std::env::set_var(LIST_VAR, &list_path);
    let st = FramesMatchState::from_env().expect("valid list file must load");
    assert_eq!(st.snapshot.state, ListState::Loaded);
    assert!(st.snapshot.sha256.contains(&[0x11; 32]));
    assert!(st.snapshot.pdq.iter().any(|p| p.0 == [0x22; 32]));
    assert_ne!(st.snapshot.version, 0, "content fingerprint stamped");
    assert!(st.ownhash.is_empty());

    // Row 3: ownhash only ⇒ loaded ownhash, snapshot stays Unavailable (§1:
    // own-hash alone can never clear anything).
    clear_moderation_env();
    std::env::set_var(OWN_VAR, &own_path);
    let st = FramesMatchState::from_env().expect("valid ownhash file must load");
    assert!(st.snapshot.require_available().is_err());
    assert!(st.ownhash.contains(&[0x33; 32]));

    // Row 4: both files ⇒ both loaded.
    std::env::set_var(LIST_VAR, &list_path);
    let st = FramesMatchState::from_env().expect("both files must load");
    assert_eq!(st.snapshot.state, ListState::Loaded);
    assert!(st.ownhash.contains(&[0x33; 32]));

    // Row 5: set-but-missing list file ⇒ Err naming the path (degradable).
    clear_moderation_env();
    std::env::set_var(LIST_VAR, dir.path().join("nope.txt"));
    let err = FramesMatchState::from_env_files(31)
        .err()
        .expect("missing file is an error");
    assert!(
        format!("{err:#}").contains("nope.txt"),
        "error must name the path, got: {err:#}"
    );

    // Row 6: malformed list file ⇒ Err with path AND 1-based line context.
    let bad = dir.path().join("bad.txt");
    std::fs::write(&bad, "sha256:short\n").unwrap();
    std::env::set_var(LIST_VAR, &bad);
    let err = FramesMatchState::from_env_files(31)
        .err()
        .expect("malformed file is an error");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("bad.txt") && msg.contains("line 1"),
        "path + line context required, got: {msg}"
    );

    // Row 6b: malformed ownhash file rides the same degradable channel.
    clear_moderation_env();
    std::env::set_var(OWN_VAR, &bad);
    assert!(FramesMatchState::from_env_files(31).is_err());

    // Rows 7-10: the PDQ knob — the ONLY fatal channel, structurally separate.
    clear_moderation_env();
    assert_eq!(resolve_pdq_max_distance().expect("unset ⇒ default"), 31);
    std::env::set_var(PDQ_VAR, "17");
    assert_eq!(resolve_pdq_max_distance().expect("17 is valid"), 17);
    let st = FramesMatchState::from_env().expect("no files + knob 17");
    assert_eq!(st.max_distance, 17);
    std::env::set_var(PDQ_VAR, "300");
    resolve_pdq_max_distance()
        .err()
        .expect("cap is MAX_PDQ_DISTANCE (256) — 300 must be rejected");
    std::env::set_var(PDQ_VAR, "not-a-number");
    resolve_pdq_max_distance()
        .err()
        .expect("unparseable knob must be rejected");
    // Set-but-EMPTY knob means unset (the shared MODERATION_* convention —
    // a stray `FOO=` env-file line must not take down paid serving).
    std::env::set_var(PDQ_VAR, "");
    assert_eq!(resolve_pdq_max_distance().expect("empty ⇒ default"), 31);
    // Structural separation: a broken knob NEVER surfaces via from_env_files —
    // with a valid file and a GENUINELY broken knob (re-set: the empty-string
    // row above resolves to the default, which would hollow this pin), file
    // loading still succeeds.
    std::env::set_var(PDQ_VAR, "not-a-number");
    std::env::set_var(LIST_VAR, &list_path);
    let st = FramesMatchState::from_env_files(31).expect("file loading ignores the knob");
    assert_eq!(st.snapshot.state, ListState::Loaded);

    // A non-regular file (here: a directory) must DEGRADE loudly — fs::read
    // on e.g. a FIFO would otherwise wedge in open() forever, a silent hang
    // being worse than the loud degrade rule 1 mandates.
    clear_moderation_env();
    std::env::set_var(LIST_VAR, dir.path());
    assert!(
        FramesMatchState::from_env_files(31).is_err(),
        "a non-regular list path must degrade, not wedge or load"
    );

    // Set-but-NOT-UNICODE file vars are set-and-broken ⇒ degradable Err —
    // NEVER silently unset: a non-unicode OWNHASH value beside a Loaded list
    // would otherwise fail OPEN (operator's own blocks silently dropped).
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bad = std::ffi::OsStr::from_bytes(b"/etc/x\xff.txt");
        clear_moderation_env();
        std::env::set_var(LIST_VAR, bad);
        assert!(
            FramesMatchState::from_env_files(31).is_err(),
            "non-unicode MODERATION_LIST_FILE must degrade, not vanish"
        );
        clear_moderation_env();
        std::env::set_var(LIST_VAR, &list_path);
        std::env::set_var(OWN_VAR, bad);
        assert!(
            FramesMatchState::from_env_files(31).is_err(),
            "non-unicode MODERATION_OWNHASH_FILE beside a valid list must degrade (fail-open trap)"
        );
    }
}
