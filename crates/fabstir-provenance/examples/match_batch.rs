// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP4 fingerprint re-measurement tool (not part of any service): the PRODUCTION matcher
//! (`matcher::is_match` with `AP01B`, exactly what the signer's index runs) over a registry directory of packed
//! `PAIFP` sets (`<name>.pfp`). Reads one query `.pfp` path per stdin line and prints, per line, the query path, a
//! tab, and the comma-separated names of every registered set it matches.
//!   cargo run --release --example match_batch -- <registry-dir> < queries.txt
use fabstir_provenance::fingerprint::SignatureSet;
use fabstir_provenance::matcher::{is_match, AP01B};
use std::io::{BufRead, Write};

fn main() {
    let dir = std::env::args().nth(1).expect("usage: match_batch <registry-dir>");
    let mut reg = vec![];
    for e in std::fs::read_dir(&dir).expect("registry dir") {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("pfp") {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let set = SignatureSet::unpack(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        reg.push((name, set));
    }
    reg.sort_by(|a, b| a.0.cmp(&b.0));
    eprintln!("registry: {} sets", reg.len());
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines() {
        let q_path = line.unwrap();
        let q_path = q_path.trim();
        if q_path.is_empty() {
            continue;
        }
        let hits: Vec<&str> = match std::fs::read(q_path).ok().and_then(|b| SignatureSet::unpack(&b).ok()) {
            Some(q) => reg.iter().filter(|(_, r)| is_match(&q, r, AP01B)).map(|(n, _)| n.as_str()).collect(),
            None => vec!["!unreadable"],
        };
        writeln!(out, "{q_path}\t{}", hits.join(",")).unwrap();
        out.flush().unwrap();
    }
}
