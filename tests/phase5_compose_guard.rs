// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 gate A-19, automated: the Phala composes are deploy-safe.
//!
//! `deployment/phala/compose.gpu.yml` is the paid-day configuration. It is hashed
//! into RTMR3 and pinned by the key broker's policy, so it must pull by digest,
//! never build, never bind-mount a host path that does not exist inside a CVM,
//! and never carry the GPU-half test mode (`TEE_GPU_EVIDENCE=canned`), the
//! broker's counterpart (`KBS_GPU_EVIDENCE`), the CPU-only driver stub
//! (`TEE_CPU_ONLY_STUB`), or the test-release opt-in (`TEE_ACCEPT_TEST_RELEASE`).
//! `compose.cpu.yml` must carry exactly those markers, so the two files can
//! never be swapped on the deploy form.
//!
//! Line-based on purpose: no YAML dependency, and a compose is short enough
//! that structure-by-indentation is unambiguous. Comments are stripped first
//! (the file headers name the forbidden variables in prose).
//!
//! `gpu_compose_images_are_digest_pinned` stays RED until `build.sh` has pushed
//! and the digest has been pasted in. That is the gate doing its job.

use std::path::PathBuf;

fn compose(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("deployment/phala")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Non-comment content of each line, with inline `# ...` comments removed.
fn code_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            let trimmed = l.trim_start();
            if trimmed.starts_with('#') {
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

fn contains_token(lines: &[String], token: &str) -> bool {
    lines.iter().any(|l| l.contains(token))
}

/// Values of every `image:` key.
fn image_refs(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| l.trim().strip_prefix("image:"))
        .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string())
        .collect()
}

/// List items directly under a `volumes:` key (the service's bind mounts).
fn volume_items(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_volumes = false;
    let mut indent = 0usize;
    for l in lines {
        let lead = l.len() - l.trim_start().len();
        let t = l.trim();
        if t == "volumes:" {
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

/// What both composes must set `HOST_TEE_ENABLED` to. The variable turns on
/// the `tee-attested` advert (WS handshake + registry metadata) and, from
/// v8.55.0, makes an attested load mandatory (`tee::live`: "true" without one
/// is a start-up refusal). It was pinned to "false" while the composes carried
/// the v8.54.0 image (which loaded plain models regardless) and flipped to
/// "true" with the v8.55.0 digest on 2026-09-18, in one change with the `TEE_*`
/// block (`attested_load_variables_move_with_the_flag`). A pinned image that
/// predates the enforcement must never sit under "true".
const EXPECTED_HOST_TEE_ENABLED: &str = "\"true\"";

const FORBIDDEN_ON_GPU: [&str; 6] = [
    "TEE_GPU_EVIDENCE",
    "KBS_GPU_EVIDENCE",
    "TEE_CPU_ONLY_STUB",
    // the node-side opt-in for a test-keyring release: a GPU node must refuse
    // a key from a broker left in canned mode (P3 converge round 3)
    "TEE_ACCEPT_TEST_RELEASE",
    // honoured unconditionally by DstackClient::from_env: a pasted simulator
    // line would make every paid-day quote come from the simulator
    "DSTACK_SIMULATOR_ENDPOINT",
    "build:",
];

#[test]
fn gpu_compose_has_no_test_mode_markers_and_never_builds() {
    let lines = code_lines(&compose("compose.gpu.yml"));
    for token in FORBIDDEN_ON_GPU {
        assert!(
            !contains_token(&lines, token),
            "compose.gpu.yml must not contain `{token}` outside comments (gate A-19)"
        );
    }
}

#[test]
fn gpu_compose_bind_mounts_are_only_the_dstack_socket() {
    let lines = code_lines(&compose("compose.gpu.yml"));
    let vols = volume_items(&lines);
    // "only": the dstack socket and nothing else. Any other host path either
    // does not exist inside a CVM or exposes something that should not be.
    assert_eq!(
        vols,
        vec!["/var/run/dstack.sock:/var/run/dstack.sock".to_string()],
        "compose.gpu.yml must bind-mount exactly the dstack socket; got {vols:?}"
    );
    for v in &vols {
        let src = v.split(':').next().unwrap_or("");
        for bad in ["./", "../", "${", "~"] {
            assert!(
                !src.starts_with(bad),
                "bind mount source `{src}` has nothing to point at inside a CVM (gate A-19)"
            );
        }
        assert!(
            src.starts_with('/'),
            "only absolute in-CVM paths are mountable; got `{src}` (named volumes are not \
             needed on the first run and would need the encrypted data disk sized for them)"
        );
    }
}

#[test]
fn restart_policy_cannot_hot_loop_a_refusal() {
    // A refused attested load exits 78; `unless-stopped`/`always` would retry it
    // forever (container re-download, nonce burn, decrypt per loop). Both
    // composes must cap restarts on failure.
    for name in ["compose.gpu.yml", "compose.cpu.yml"] {
        let lines = code_lines(&compose(name));
        let value = lines
            .iter()
            .filter_map(|l| l.trim().strip_prefix("restart:"))
            .map(|v| v.trim().to_string())
            .next()
            .unwrap_or_else(|| panic!("{name} must set restart: explicitly"));
        assert!(
            value.starts_with("on-failure:"),
            "{name}: restart must be `on-failure:<n>`, got `{value}`"
        );
        let n: u32 = value["on-failure:".len()..]
            .parse()
            .unwrap_or_else(|_| panic!("{name}: restart cap must be a number, got `{value}`"));
        assert!(
            (1..=10).contains(&n),
            "{name}: restart cap {n} out of 1..=10"
        );
    }
}

#[test]
fn stop_grace_period_outlasts_the_orderly_shutdown() {
    // The orderly stop is bounded at 8 s by the watchdog (API drain 5 s + P2P
    // leave); docker's default 10 s grace leaves no margin before SIGKILL.
    for name in ["compose.gpu.yml", "compose.cpu.yml"] {
        let lines = code_lines(&compose(name));
        let value = lines
            .iter()
            .filter_map(|l| l.trim().strip_prefix("stop_grace_period:"))
            .map(|v| v.trim().trim_matches('"').to_string())
            .next()
            .unwrap_or_else(|| panic!("{name} must set stop_grace_period: explicitly"));
        let secs: u64 = value
            .strip_suffix('s')
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("{name}: stop_grace_period must be `<n>s`, got `{value}`"));
        assert!(
            secs >= 20,
            "{name}: stop_grace_period {secs}s must be at least 20s (orderly bound 8 s + margin)"
        );
    }
}

#[test]
fn gpu_compose_has_the_gpu_essentials() {
    let lines = code_lines(&compose("compose.gpu.yml"));
    for needed in [
        "runtime: nvidia",
        "privileged: true",
        "NVIDIA_DRIVER_CAPABILITIES",
        "GPU_LAYERS: \"99\"",
        "TEE_DECRYPT_DIR: /dev/shm",
        "shm_size:",
    ] {
        assert!(
            contains_token(&lines, needed),
            "compose.gpu.yml is missing `{needed}`"
        );
    }
}

#[test]
fn gpu_compose_images_are_digest_pinned() {
    let lines = code_lines(&compose("compose.gpu.yml"));
    let images = image_refs(&lines);
    assert!(!images.is_empty(), "compose.gpu.yml declares no image");
    for image in &images {
        let (_, digest) = image.split_once("@sha256:").unwrap_or_else(|| {
            panic!("image `{image}` is not pinned by @sha256 digest (run deployment/phala/build.sh and paste the digest; gate A-19)")
        });
        assert!(
            digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "image `{image}`: digest must be 64 lowercase hex chars (still the placeholder? run build.sh)"
        );
        assert!(
            digest.chars().any(|c| c != '0'),
            "image `{image}`: all-zero digest is a placeholder"
        );
    }
}

#[test]
fn cpu_compose_is_the_cpu_variant_and_nothing_else() {
    let lines = code_lines(&compose("compose.cpu.yml"));
    for needed in [
        "TEE_CPU_ONLY_STUB: \"1\"",
        "TEE_GPU_EVIDENCE: canned",
        "TEE_ACCEPT_TEST_RELEASE: \"1\"",
        "GPU_LAYERS: \"0\"",
        "/var/run/dstack.sock:/var/run/dstack.sock",
    ] {
        assert!(
            contains_token(&lines, needed),
            "compose.cpu.yml is missing `{needed}`"
        );
    }
    for forbidden in [
        "runtime: nvidia",
        "privileged: true",
        "build:",
        "NVIDIA_VISIBLE_DEVICES",
    ] {
        assert!(
            !contains_token(&lines, forbidden),
            "compose.cpu.yml must not contain `{forbidden}` (a CPU CVM has no GPU)"
        );
    }
}

/// Value of the `ai.platformless.dstack_os_image` label, if present.
fn os_image_label(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .filter_map(|l| l.trim().strip_prefix("ai.platformless.dstack_os_image:"))
        .map(|v| v.trim().trim_matches('"').to_string())
        .next()
}

#[test]
fn each_compose_names_the_dstack_os_image_it_expects() {
    // dstack-v0.5.9 (CPU) and dstack-nvidia-0.5.9 (GPU) differ by one word and
    // measure differently. The label is measured into RTMR3 with the compose,
    // and the deploy form's image selection must equal it (deployment README).
    // It is a mistake-catcher, not a security control: it records the image
    // INTENDED, not the one BOOTED; the verifier's MRTD/RTMR comparison is the
    // only thing that binds a release to the image actually running.
    let gpu = os_image_label(&code_lines(&compose("compose.gpu.yml")));
    let cpu = os_image_label(&code_lines(&compose("compose.cpu.yml")));
    assert_eq!(
        gpu.as_deref(),
        Some("dstack-nvidia-0.5.9"),
        "compose.gpu.yml must label the GPU image dstack-nvidia-0.5.9 (Phala, 2026-09-17: current stable; 0.6.0 is still in testing)"
    );
    assert_eq!(
        cpu.as_deref(),
        Some("dstack-v0.5.9"),
        "compose.cpu.yml must label the CPU image dstack-v0.5.9"
    );
}

/// The active (uncommented) `HOST_TEE_ENABLED:` value of a compose.
fn host_tee_enabled_value(name: &str, lines: &[String]) -> String {
    lines
        .iter()
        .filter_map(|l| l.trim().strip_prefix("HOST_TEE_ENABLED:"))
        .map(|v| v.trim().to_string())
        .next()
        .unwrap_or_else(|| panic!("{name} must set HOST_TEE_ENABLED explicitly"))
}

#[test]
fn host_tee_enabled_matches_what_the_pinned_binary_honours() {
    for name in ["compose.gpu.yml", "compose.cpu.yml"] {
        let value = host_tee_enabled_value(name, &code_lines(&compose(name)));
        assert_eq!(
            value, EXPECTED_HOST_TEE_ENABLED,
            "{name}: HOST_TEE_ENABLED must be {EXPECTED_HOST_TEE_ENABLED} until P3.3 wires the attested load path (see the constant's doc)"
        );
    }
}

/// The variables `src/tee/live.rs` requires on the attested path. Its decision
/// table refuses `TEE_MODEL_ID` under `HOST_TEE_ENABLED` "false" (exit 78), so
/// the block is commented out while the flag is "false" and active once it is
/// "true": the two move in one change, never separately.
const TEE_LOAD_VARS: [&str; 5] = [
    "TEE_MODEL_ID",
    "TEE_MODEL_PROVIDER",
    "TEE_KBS_URL",
    "TEE_POLICY_URL",
    "TEE_BLOB_URL",
];

#[test]
fn attested_load_variables_move_with_the_flag() {
    for name in ["compose.gpu.yml", "compose.cpu.yml"] {
        let text = compose(name);
        let lines = code_lines(&text);
        let enabled = match host_tee_enabled_value(name, &lines).as_str() {
            "\"true\"" => true,
            "\"false\"" => false,
            other => panic!("{name}: HOST_TEE_ENABLED must be quoted true/false, got {other}"),
        };
        for var in TEE_LOAD_VARS {
            let active = lines
                .iter()
                .any(|l| l.trim().starts_with(&format!("{var}:")));
            assert_eq!(
                active,
                enabled,
                "{name}: `{var}:` must be {} while HOST_TEE_ENABLED is {}",
                if enabled { "set" } else { "commented out" },
                if enabled { "\"true\"" } else { "\"false\"" },
            );
        }
        // Optional or not, every variable keeps its `${VAR}` template line in the
        // file, so the flip is an uncomment and the form's allow-list is complete.
        for var in TEE_LOAD_VARS
            .iter()
            .chain(["TEE_EXPECTED_MODEL_SHA256"].iter())
        {
            let template = format!("{var}: ${{{var}}}");
            assert!(
                text.contains(&template),
                "{name}: missing the `{template}` line (active or commented)"
            );
        }
        // The container cap is a literal (measured, not secret) and must be in
        // the file too: its 2 GiB default fails closed on a real-size model.
        assert!(
            text.contains("TEE_BLOB_MAX_BYTES:"),
            "{name}: missing the `TEE_BLOB_MAX_BYTES:` line (active or commented)"
        );
    }
}

#[test]
fn both_composes_reference_the_same_image() {
    let gpu = image_refs(&code_lines(&compose("compose.gpu.yml")));
    let cpu = image_refs(&code_lines(&compose("compose.cpu.yml")));
    assert_eq!(
        gpu, cpu,
        "one image serves both CVMs; the CPU rounds must exercise the exact image the GPU day pulls"
    );
}

#[test]
fn the_broker_root_ca_is_present_and_is_exactly_one_certificate() {
    // The node trusts ONLY this root for kbs.fabstir.net (private CA, expert
    // decision 2026-09-17; deployment/phala/kbs-ca/README.md). It is baked into
    // the image, so it must exist here, be a single CERTIFICATE block (a root,
    // never a chain with an intermediate), and never a private key.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("deployment/phala/kbs-root.pem");
    let pem = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} missing ({e}); generate the root per deployment/phala/kbs-ca/README.md step 1 and commit the .pem (public)",
            path.display()
        )
    });
    let begins = pem.matches("-----BEGIN CERTIFICATE-----").count();
    let ends = pem.matches("-----END CERTIFICATE-----").count();
    assert_eq!(
        (begins, ends),
        (1, 1),
        "kbs-root.pem must hold exactly one certificate (the root, no intermediate); found {begins} BEGIN / {ends} END"
    );
    assert!(
        !pem.contains("PRIVATE KEY"),
        "kbs-root.pem must never contain a private key"
    );
}

#[test]
fn the_binary_is_never_committed_into_the_build_context() {
    // build.sh extracts the 1.2 GB binary into deployment/phala and removes it
    // afterwards; .gitignore is the belt to that brace.
    let gitignore =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".gitignore"))
            .expect("read .gitignore");
    assert!(
        gitignore
            .lines()
            .any(|l| l.trim() == "deployment/phala/fabstir-llm-node"),
        ".gitignore must exclude deployment/phala/fabstir-llm-node"
    );
}
