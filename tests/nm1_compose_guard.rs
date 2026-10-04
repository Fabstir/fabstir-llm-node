// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NM1 D1 / D5: the production compose's LTX sidecar runs a TAGGED image, never builds,
//! defaults to today's image (`ltx-sidecar:v25`), mounts the 2.5 weights and the model-paths
//! yaml read-only, and passes host flags through a STRING-form `command:` (the list form would
//! pass `""` for an empty `LTX_COMFY_ARGS` and ComfyUI's strict argument parser would
//! crash-loop). The image is built by `scripts/build-ltx-sidecar.sh`, context-free, with the
//! probe's three pins as build args — the Dockerfile's own defaults stay at today's pins
//! because the Phala run-2 recipe builds FROM it (D15).
//!
//! Line-based, as tests/phase5_compose_guard.rs: no YAML dependency.

use std::path::PathBuf;

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// The lines of the `ltx-sidecar` service block, comments included.
fn sidecar_block() -> Vec<String> {
    let text = read("docker-compose.prod.yml");
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line == "  ltx-sidecar:" {
            inside = true;
            continue;
        }
        if inside {
            // The next service (two-space indent) or a top-level key ends the block.
            let is_key = |indent: usize| {
                line.len() > indent
                    && line[..indent].chars().all(|c| c == ' ')
                    && !line[indent..].starts_with(' ')
                    && !line[indent..].starts_with('#')
            };
            if is_key(2) || is_key(0) {
                break;
            }
            out.push(line.to_string());
        }
    }
    assert!(!out.is_empty(), "ltx-sidecar service not found");
    out
}

fn has(block: &[String], needle: &str) -> bool {
    block.iter().any(|l| l.trim() == needle)
}

#[test]
fn ltx_sidecar_runs_a_tagged_image_never_builds() {
    let b = sidecar_block();
    assert!(!b.iter().any(|l| l.trim_start().starts_with("build:")), "no build: block");
    assert!(has(&b, "image: ${LTX_SIDECAR_IMAGE:-ltx-sidecar:v25}"), "default is today's image");
}

#[test]
fn ltx_sidecar_command_is_string_form() {
    let b = sidecar_block();
    assert!(
        has(
            &b,
            r#"command: "--listen 0.0.0.0 --port 8188 --output-directory /opt/ComfyUI/output ${LTX_COMFY_ARGS:-}""#
        ),
        "string-form command with the production arguments"
    );
    assert!(!b.iter().any(|l| l.trim_start().starts_with("command: [")), "never the list form");
}

#[test]
fn ltx_sidecar_mounts_both_weight_trees_and_the_yaml() {
    let b = sidecar_block();
    for mount in [
        "- ${LTX_MODELS_DIR:-./models/ltx}:/opt/ComfyUI/models:ro",
        "- ${LTX25_MODELS_DIR:-./models/ltx25}:/models_ltx25:ro",
        "- ./docker/ltx-extra_model_paths.yaml:/opt/ComfyUI/extra_model_paths.yaml:ro",
        "- ltx-output:/opt/ComfyUI/output",
    ] {
        assert!(has(&b, mount), "missing mount {mount}");
    }
}

#[test]
fn model_paths_yaml_maps_the_2_5_tree() {
    let y = read("docker/ltx-extra_model_paths.yaml");
    assert!(y.contains("base_path: /models_ltx25/"));
    for key in ["diffusion_models", "text_encoders", "vae", "loras", "latent_upscale_models", "embeddings"] {
        assert!(y.lines().any(|l| l.trim() == format!("{key}: {key}")), "yaml lacks {key}");
    }
}

#[test]
fn build_script_pins_the_probe_stack_and_dockerfile_defaults_stay() {
    let s = read("scripts/build-ltx-sidecar.sh");
    for arg in [
        "COMFYUI_REPO=https://github.com/Comfy-Org/ComfyUI.git",
        "COMFYUI_COMMIT=6b747c0428c343e1417219641db93a4fb7cb69ae",
        "LTXVIDEO_COMMIT=bf2ca0264f706db64cb8931155695ca481fc9d91",
    ] {
        assert!(s.contains(arg), "build script lacks {arg}");
    }
    assert!(s.contains("- < docker/Dockerfile.ltx-sidecar"), "context-free build (Dockerfile on stdin)");
    let d = read("docker/Dockerfile.ltx-sidecar");
    assert!(d.contains("ARG COMFYUI_COMMIT=1377a2f72925ed7a5518c1900ff71c6740217b0d"));
    assert!(d.contains("ARG LTXVIDEO_COMMIT=4f45fd6c222eb06eb3e46605da62e7c889e4be5c"));
}
