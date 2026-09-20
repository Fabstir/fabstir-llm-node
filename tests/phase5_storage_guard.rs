// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design S6): the two composes carry the streaming-load storage
//! layout — two named volumes declared at top level and mounted, the env
//! literals that point at them, the two form placeholders, and a tmpfs cap
//! large enough for the dummy/target. Own file: `phase5_compose_guard.rs` is
//! over the 400-line cap.

use std::path::PathBuf;

const COMPOSES: &[&str] = &["compose.gpu.yml", "compose.cpu.yml"];
const CONTAINERS: &str = "/var/lib/fabstir/containers";
const PLAINTEXT: &str = "/var/lib/fabstir/plaintext";

fn compose(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("deployment/phala")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Non-comment content of each line (a `#` line dropped, an inline ` #`
/// comment removed).
fn code_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            if l.trim_start().starts_with('#') {
                return String::new();
            }
            match l.find(" #") {
                Some(i) => l[..i].to_string(),
                None => l.to_string(),
            }
        })
        .filter(|l| !l.trim().is_empty())
        .collect()
}

/// `- ` items under the SERVICE's `volumes:` key (indented), not the top-level one.
fn service_mounts(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_volumes = false;
    let mut indent = 0usize;
    for l in lines {
        let lead = l.len() - l.trim_start().len();
        let t = l.trim();
        if t == "volumes:" && lead > 0 {
            in_volumes = true;
            indent = lead;
            continue;
        }
        if in_volumes {
            if lead > indent && t.starts_with("- ") {
                out.push(t[2..].trim().trim_matches('"').to_string());
                continue;
            }
            in_volumes = false;
        }
    }
    out
}

/// Keys under the TOP-LEVEL `volumes:` block.
fn declared_volumes(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for l in lines {
        let lead = l.len() - l.trim_start().len();
        let t = l.trim();
        if t == "volumes:" && lead == 0 {
            in_block = true;
            continue;
        }
        if in_block {
            if lead > 0 {
                out.push(t.trim_end_matches(':').to_string());
                continue;
            }
            in_block = false;
        }
    }
    out.sort();
    out
}

fn env_value(lines: &[String], key: &str) -> Option<String> {
    lines
        .iter()
        .filter_map(|l| l.trim().strip_prefix(key))
        .filter_map(|rest| rest.trim_start().strip_prefix(':'))
        .map(|v| v.trim().trim_matches('"').to_string())
        .next()
}

#[test]
fn both_composes_declare_and_mount_exactly_the_two_named_volumes() {
    for name in COMPOSES {
        let lines = code_lines(&compose(name));
        assert_eq!(
            declared_volumes(&lines),
            vec![
                "fabstir-containers".to_string(),
                "fabstir-plaintext".to_string()
            ],
            "{name}: top-level volumes: must declare exactly the two"
        );
        let mounts = service_mounts(&lines);
        let mut named: Vec<&str> = mounts
            .iter()
            .filter(|m| !m.starts_with('/'))
            .map(|m| m.as_str())
            .collect();
        named.sort();
        assert_eq!(
            named,
            vec![
                format!("fabstir-containers:{CONTAINERS}"),
                format!("fabstir-plaintext:{PLAINTEXT}"),
            ],
            "{name}: the service must mount exactly the two named volumes at the design's paths"
        );
    }
}

#[test]
fn the_env_literals_point_at_the_mounted_paths() {
    for name in COMPOSES {
        let lines = code_lines(&compose(name));
        assert_eq!(
            env_value(&lines, "TEE_CONTAINER_DIR").as_deref(),
            Some(CONTAINERS),
            "{name}: TEE_CONTAINER_DIR must be the literal mount path of fabstir-containers"
        );
        assert_eq!(
            env_value(&lines, "TEE_PLAINTEXT_VOLUME").as_deref(),
            Some(PLAINTEXT),
            "{name}: TEE_PLAINTEXT_VOLUME must be the literal mount path of fabstir-plaintext"
        );
        assert_eq!(
            env_value(&lines, "TEE_DECRYPT_DIR").as_deref(),
            Some("/dev/shm"),
            "{name}: the tmpfs home stays the /dev/shm literal"
        );
    }
}

#[test]
fn the_two_form_values_are_templated_with_their_defaults() {
    for name in COMPOSES {
        let lines = code_lines(&compose(name));
        assert_eq!(
            env_value(&lines, "TEE_DECRYPT_ON_DISK").as_deref(),
            Some("${TEE_DECRYPT_ON_DISK:-0}"),
            "{name}: the mode is ONE form value defaulting to tmpfs"
        );
        assert_eq!(
            env_value(&lines, "TEE_BLOB_MAX_BYTES").as_deref(),
            Some("${TEE_BLOB_MAX_BYTES:-2147483648}"),
            "{name}: the container bound comes from the form, defaulting to 2 GiB"
        );
    }
}

#[test]
fn shm_size_is_at_least_the_design_floor() {
    for (name, floor_g) in [("compose.gpu.yml", 16u64), ("compose.cpu.yml", 6u64)] {
        let lines = code_lines(&compose(name));
        let v = env_value(&lines, "shm_size").unwrap_or_else(|| panic!("{name}: shm_size"));
        let g: u64 = v
            .trim_end_matches(['g', 'G'])
            .parse()
            .unwrap_or_else(|_| panic!("{name}: shm_size `{v}` is not <n>g"));
        assert!(
            g >= floor_g,
            "{name}: shm_size {g}g is below the design floor {floor_g}g (the tmpfs-mode cap)"
        );
    }
}
