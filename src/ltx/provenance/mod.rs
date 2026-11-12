// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AI provenance on the node (IMPLEMENTATION-AI-PROVENANCE.md AP2): every delivered LTX output is marked
//! before upload (signed C2PA manifest; EXR header record + sequence manifest) and its picture fingerprint is
//! registered with the signer. Shared definitions: `crates/fabstir-provenance`.

pub mod ffmpeg;
pub mod finalise;
pub mod signer_client;

use crate::ltx::types::LtxJob;
use anyhow::{bail, Result};
use fabstir_provenance::{Class, TemplateClass};

/// A 2-second synthetic test pattern (no third-party content) for the startup fingerprint self-test.
pub const SELF_TEST_MP4: &[u8] =
    include_bytes!("../../../crates/fabstir-provenance/tests/fixtures/tiny.mp4");

/// The resolver page written into manifests and header records (the signer issues the manifest's own copy).
pub const RESOLVER_URL: &str = "https://platformlessai.org/verify";

/// The ingredient formats of a job, in the order the manifest lists them (contract v1.0): a Modified output's
/// parent is the user's clip (`videos[0]`; an EXR frame list on the deep wire), then every other visual input;
/// a Generated output lists its stills and videos as inputs; Data lists none. Formats only: the node never
/// lets c2pa parse an input's bytes.
pub fn ingredient_formats(row: &TemplateClass, job: &LtxJob) -> Result<Vec<&'static str>> {
    let images = job.images.as_deref().unwrap_or_default().len();
    let videos = job.videos.as_deref().unwrap_or_default().len();
    let video_fmt = if job.input_wire.is_some() {
        "image/x-exr"
    } else {
        "video/mp4"
    };
    let mut out = vec![];
    match row.class {
        Class::Data => return Ok(out),
        Class::Modified => {
            if videos == 0 {
                bail!(
                    "{}: a modified output needs the user's clip",
                    row.template_id
                );
            }
            out.push(video_fmt);
            out.extend(std::iter::repeat_n("video/mp4", videos - 1));
        }
        // only videos[0] can arrive on the deep EXR wire; a chained part's previous part is always an mp4
        Class::Generated if videos > 0 => {
            out.push(video_fmt);
            out.extend(std::iter::repeat_n("video/mp4", videos - 1));
        }
        Class::Generated => {}
    }
    out.extend(std::iter::repeat_n("image/png", images));
    Ok(out)
}

/// Everything a job needs to be marked (D18: LTX is enabled only when this exists).
pub struct Runtime {
    pub signer: std::sync::Arc<dyn signer_client::ProvenanceSigner>,
    pub ffmpeg: ffmpeg::Ffmpeg,
    /// The reserve this node uses for its placeholder (the signer's recommendation at startup).
    pub reserve: usize,
}

impl Runtime {
    /// From the environment: `PROVENANCE_SIGNER_URL`, `HOST_PRIVATE_KEY`, `PROVENANCE_FFMPEG` (pinned, with the
    /// signature filter), and a signer that speaks this crate's contract and c2pa version.
    pub async fn from_env() -> Result<Runtime> {
        let url = std::env::var(signer_client::ENV_URL)
            .map_err(|_| anyhow::anyhow!("{} is not set", signer_client::ENV_URL))?;
        let key = crate::crypto::extract_node_private_key()?;
        let signer = signer_client::HttpSigner::new(&url, &key)?;
        let ffmpeg = ffmpeg::Ffmpeg::from_env().await?;
        ffmpeg::sweep_scratch();
        // exercise the real fingerprint path once (temp dir writable and filter-safe, the filter writes its
        // file): a broken scratch area would otherwise fail every picture job after its GPU run
        let set = ffmpeg
            .fingerprint(SELF_TEST_MP4)
            .await
            .map_err(|e| anyhow::anyhow!("ffmpeg self-test: {e:#}"))?;
        anyhow::ensure!(
            !set.frames.is_empty(),
            "ffmpeg self-test produced no signature"
        );
        Self::check(std::sync::Arc::new(signer), ffmpeg).await
    }

    /// The signer must answer `/v1/limits` with our API, c2pa and provenance-crate versions.
    pub async fn check(
        signer: std::sync::Arc<dyn signer_client::ProvenanceSigner>,
        ffmpeg: ffmpeg::Ffmpeg,
    ) -> Result<Runtime> {
        let l = signer
            .limits()
            .await
            .map_err(|e| anyhow::anyhow!("signer unreachable: {e}"))?;
        if l.api != fabstir_provenance::contract::API_VERSION
            || l.c2pa_crate != fabstir_provenance::defs::C2PA_CRATE_VERSION
            || l.provenance_crate != fabstir_provenance::CRATE_VERSION
        {
            bail!(
                "signer versions differ (api {}, c2pa {}, provenance {})",
                l.api,
                l.c2pa_crate,
                l.provenance_crate
            );
        }
        // a dry-run reservation proves the host key and this node version are allow-listed (it just expires)
        let probe = fabstir_provenance::contract::ReserveRequest {
            template_id: "ltx-t2v-hdr".into(),
            bundle_version: u32::MAX,
            node_version: crate::version::VERSION.to_string(),
            slots: fabstir_provenance::contract::Slots {
                mp4: 1,
                sequence: 0,
            },
            deadline_secs: 60,
        };
        signer
            .reserve(&probe)
            .await
            .map_err(|e| anyhow::anyhow!("signer refuses this node: {e}"))?;
        Ok(Runtime {
            signer,
            ffmpeg,
            reserve: clamp_reserve(l.reserve_recommended),
        })
    }
}

/// D7/D18: every allow-listed template must have a class-table row, or the node does not load the bundle (a
/// bundle can ship without a node release; an unclassed template would deliver unmarked output).
pub fn check_templates_classed(store: &crate::ltx::TemplateStore) -> Result<()> {
    let missing: Vec<&str> = store
        .bundle()
        .templates
        .iter()
        .map(|t| t.template_id.as_str())
        .filter(|id| fabstir_provenance::class::lookup(id).is_none())
        .collect();
    if !missing.is_empty() {
        bail!("templates without a provenance class: {missing:?}");
    }
    Ok(())
}

/// D8b: the graph saves through core `SaveVideo` (needs ComfyUI's `--disable-metadata`).
pub fn graph_uses_save_video(g: &crate::ltx::Graph) -> bool {
    g.0.as_object()
        .is_some_and(|m| m.values().any(|n| n["class_type"] == "SaveVideo"))
}

/// Whether a job's reservation needs the sequence slot: the template can deliver EXR frames and this job does.
pub fn wants_sequence(row: &TemplateClass, job: &LtxJob, relight: bool) -> bool {
    row.exr_role.is_some() && (job.output == crate::ltx::types::OutputKind::ExrFrames || relight)
}

/// A job's provenance grant, made at accept: the reservation and what `run` needs to mark its outputs.
pub struct Grant {
    pub reservation: fabstir_provenance::contract::ReserveResponse,
    pub formats: Vec<&'static str>,
    pub row: &'static TemplateClass,
    pub runtime: std::sync::Arc<Runtime>,
}

/// UTC now as RFC 3339 seconds (the EXR header record's time).
pub fn utc_now_rfc3339() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A signer-supplied reserve size, bounded to what any chain + TSA token needs (AP0.2 measured at most about
/// 9 KB; a huge value would make c2pa allocate a huge placeholder).
pub fn clamp_reserve(r: usize) -> usize {
    r.clamp(9_216, 65_536)
}
