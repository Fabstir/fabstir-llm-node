// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP4.1 design tool (not part of any service): the evidence behind every candidate match, from the PRODUCTION
//! matcher functions (`matcher::aligned_votes` at AP01B's theta, `matcher::single_distance`). For each query `.pfp`
//! path on stdin prints: path, TAB, query frame count, TAB, then `name:votes:registered_frames` for every registered
//! set with at least <min-votes> aligned frames (multi-frame queries), or `name:distance:1` for every set whose
//! nearest frame is under 150 (single-frame queries).
//!   cargo run --release --example votes_batch -- <registry-dir> <min-votes> <threads> < queries.txt
use fabstir_provenance::fingerprint::SignatureSet;
use fabstir_provenance::matcher::{aligned_votes, single_distance, AP01B};
use std::io::{BufRead, Write};
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (dir, min_votes, threads) = (a[1].clone(), a[2].parse::<usize>().unwrap(), a[3].parse::<usize>().unwrap());
    let mut reg = vec![];
    for e in std::fs::read_dir(&dir).expect("registry dir") {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) == Some("pfp") {
            let name = p.file_stem().unwrap().to_string_lossy().to_string();
            reg.push((name, SignatureSet::unpack(&std::fs::read(&p).unwrap()).unwrap()));
        }
    }
    reg.sort_by(|x, y| x.0.cmp(&y.0));
    eprintln!("registry: {} sets", reg.len());
    let reg = Arc::new(reg);
    let queries: Vec<String> = std::io::stdin().lock().lines().map(|l| l.unwrap().trim().to_string()).filter(|l| !l.is_empty()).collect();
    let queries = Arc::new(queries);
    let n = queries.len();
    let chunk = n.div_ceil(threads.max(1)).max(1);
    let mut handles = vec![];
    for t in 0..threads.max(1) {
        let (reg, queries) = (reg.clone(), queries.clone());
        handles.push(std::thread::spawn(move || {
            let mut out = vec![];
            for q_path in queries.iter().skip(t * chunk).take(chunk) {
                let line = match std::fs::read(q_path).ok().and_then(|b| SignatureSet::unpack(&b).ok()) {
                    None => format!("{q_path}\t0\t!unreadable"),
                    Some(q) if q.frames.len() == 1 => {
                        let hits: Vec<String> = reg.iter().map(|(nm, r)| (nm, single_distance(&q, r))).filter(|(_, d)| *d < 150).map(|(nm, d)| format!("{nm}:{d}:1")).collect();
                        format!("{q_path}\t1\t{}", hits.join(","))
                    }
                    Some(q) => {
                        let hits: Vec<String> = reg
                            .iter()
                            .filter_map(|(nm, r)| {
                                let v = aligned_votes(&q, r, AP01B.theta);
                                (v >= min_votes).then(|| format!("{nm}:{v}:{}", r.frames.len()))
                            })
                            .collect();
                        format!("{q_path}\t{}\t{}", q.frames.len(), hits.join(","))
                    }
                };
                out.push(line);
            }
            out
        }));
    }
    let stdout = std::io::stdout();
    let mut o = stdout.lock();
    for h in handles {
        for l in h.join().unwrap() {
            writeln!(o, "{l}").unwrap();
        }
    }
}
