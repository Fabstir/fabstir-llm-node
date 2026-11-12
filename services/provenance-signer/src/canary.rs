// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Daily canary (T12): sign a test manifest through the production signing path (same key, chain, TSAs;
//! no reservation, nothing stored) and validate it against the configured trust anchors, asserting the
//! explicit `signingCredential.trusted` and `timeStamp.trusted` codes (the overall state stays `Trusted`
//! with an untrusted TSA). Any failure alarms. Optional external validators (`c2patool`, c2pa-python
//! scripts) run on the same file and must exit 0.

use crate::notify::Notifier;
use crate::service::Service;
use c2pa::{Context, Reader};
use fabstir_provenance::class::lookup;
use fabstir_provenance::defs::Params;
use fabstir_provenance::flow::{self, ReserveOnly};
use std::io::Cursor;

/// A 2-second synthetic test pattern (no third-party content).
pub const CANARY_MP4: &[u8] =
    include_bytes!("../../../crates/fabstir-provenance/tests/fixtures/tiny.mp4");

pub const REQUIRED_CODES: &[&str] = &[
    "signingCredential.trusted",
    "timeStamp.trusted",
    "claimSignature.validated",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanaryReport {
    pub ok: bool,
    pub validation_state: String,
    pub missing: Vec<String>,
    pub failures: Vec<String>,
    pub file: Option<std::path::PathBuf>,
}

fn codes(v: &serde_json::Value, key: &str) -> Vec<String> {
    v["activeManifest"][key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s["code"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Sign and validate once. `external` = commands run as `cmd <file>`; `out_dir` keeps the canary file.
pub fn run(
    svc: &Service,
    anchors_pem: &str,
    external: &[String],
    out_dir: &std::path::Path,
    notifier: &dyn Notifier,
) -> CanaryReport {
    let rep = match sign_and_validate(svc, anchors_pem, external, out_dir) {
        Ok(r) => r,
        Err(e) => CanaryReport {
            ok: false,
            validation_state: "error".into(),
            missing: vec![],
            failures: vec![e.to_string()],
            file: None,
        },
    };
    let now = (svc.clock)();
    svc.store.audit(
        now,
        None,
        "canary",
        None,
        None,
        None,
        if rep.ok { "ok" } else { "failed" },
        Some(&format!("{:?} {:?}", rep.missing, rep.failures)),
    );
    if !rep.ok {
        notifier.alarm(
            "Provenance canary FAILED",
            &format!(
                "state {}; missing {:?}; failures {:?}",
                rep.validation_state, rep.missing, rep.failures
            ),
        );
    }
    rep
}

fn sign_and_validate(
    svc: &Service,
    anchors_pem: &str,
    external: &[String],
    out_dir: &std::path::Path,
) -> anyhow::Result<CanaryReport> {
    let row = lookup("ltx-t2v-hdr").expect("t2v is classed");
    let uuid = uuid::Uuid::new_v4().to_string();
    let p = Params::for_mp4(row, "canary", &uuid, &svc.settings.informational_url, None);
    let digest = [0u8; 32];
    let node_ctx = Context::new()
        .with_settings(flow::settings(None, false))?
        .with_signer(Box::new(ReserveOnly(svc.settings.reserve_recommended)));
    let (h, prep) = flow::prepare_mp4(
        node_ctx,
        &p,
        Some(&digest),
        &[],
        CANARY_MP4,
        svc.settings.reserve_recommended,
    )?;

    let signer = crate::claim_signer::ClaimSigner::new(
        svc.key.clone(),
        svc.chain.clone(),
        svc.tsa.clone(),
        svc.settings.reserve_recommended,
    );
    let ctx = Context::new()
        .with_settings(flow::settings(Some(anchors_pem), false))?
        .with_signer(Box::new(signer));
    let mut b = c2pa::Builder::from_context(ctx).with_archive(Cursor::new(h.archive))?;
    b.set_format(flow::FMT_MP4);
    b.add_assertion(flow::BMFF_LABEL, &flow::bmff_hash(Some(h.bmff_hash))?)?;
    fabstir_provenance::defs::check_builder(&b, &p, Some(&digest))
        .map_err(|e| anyhow::anyhow!("gate: {e}"))?;
    let _ = b.placeholder(flow::FMT_MP4)?;
    let signed = b.sign_embeddable(flow::FMT_MP4)?;
    let out = flow::patch(&prep, &signed)?;

    let rctx = Context::new().with_settings(flow::settings(Some(anchors_pem), false))?;
    let reader = Reader::from_context(rctx).with_stream(flow::FMT_MP4, Cursor::new(out.clone()))?;
    let vr = reader
        .validation_results()
        .map(|r| serde_json::to_value(r).unwrap_or_default())
        .unwrap_or_default();
    let success = codes(&vr, "success");
    let mut failures = codes(&vr, "failure");
    let missing: Vec<String> = REQUIRED_CODES
        .iter()
        .filter(|c| !success.iter().any(|s| s == *c))
        .map(|s| s.to_string())
        .collect();
    let state = format!("{:?}", reader.validation_state());

    std::fs::create_dir_all(out_dir)?;
    let file = out_dir.join("canary-latest.mp4");
    std::fs::write(&file, &out)?;
    for cmd in external {
        let mut parts = cmd.split_whitespace();
        let Some(bin) = parts.next() else { continue };
        let st = std::process::Command::new(bin)
            .args(parts)
            .arg(&file)
            .stdin(std::process::Stdio::null())
            .output();
        match st {
            Ok(o) if o.status.success() => {}
            Ok(o) => failures.push(format!("{bin}: exit {:?}", o.status.code())),
            Err(e) => failures.push(format!("{bin}: {e}")),
        }
    }
    let ok = state == "Trusted" && missing.is_empty() && failures.is_empty();
    Ok(CanaryReport {
        ok,
        validation_state: state,
        missing,
        failures,
        file: Some(file),
    })
}
