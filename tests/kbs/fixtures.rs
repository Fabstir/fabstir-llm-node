//! The vendored real-bytes fixtures are asserted by hash so a silently edited fixture fails loudly.

use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/kbs/fixtures")
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture_dir().join(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

#[test]
fn every_fixture_matches_sha256sums() {
    let sums = String::from_utf8(fixture("SHA256SUMS")).unwrap();
    let mut n = 0;
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (want, name) = line.split_once("  ").expect("sha256sum line");
        let got = hex::encode(Sha256::digest(fixture(name)));
        assert_eq!(got, want, "fixture {name} changed");
        n += 1;
    }
    assert_eq!(n, 12, "SHA256SUMS lists every vendored fixture");
}
