// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The class table (plan D7): per allow-listed template, what Platformless AI says about its output.
//!
//! The signer derives class, model and roles from THIS table keyed by `template_id`; it never takes them
//! from the node. A template missing here cannot be reserved, and the node refuses to load it (D18).

use serde::{Deserialize, Serialize};

/// What the output is, in C2PA terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Class {
    /// Synthesised from a prompt, stills or a layout: `c2pa.created` + trainedAlgorithmicMedia.
    Generated,
    /// The user's footage changed by AI: `c2pa.opened` + `parentOf`, then `c2pa.edited`.
    Modified,
    /// Data images (normal, basecolor, depth, roughness, metallic maps, mattes): trainedAlgorithmicData.
    Data,
}

/// The role of one delivered file (D1). Only a Picture output carries a fingerprint soft binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutputRole {
    Picture,
    Data,
}

/// The model behind a template (the `c2pa.ai-disclosure` assertion).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Model {
    Ltx23,
    Ltx25,
    CosmosDiffusionRenderer,
}

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Model::Ltx23 => "LTX-2.3",
            Model::Ltx25 => "LTX-2.5",
            Model::CosmosDiffusionRenderer => "Cosmos-DiffusionRenderer",
        }
    }

    /// A PURL for the weights repository (D6).
    pub fn identifier(self) -> &'static str {
        match self {
            Model::Ltx23 => "pkg:huggingface/Lightricks/LTX-2.3",
            Model::Ltx25 => "pkg:huggingface/Lightricks/LTX-2.5",
            Model::CosmosDiffusionRenderer => {
                "pkg:huggingface/nvidia/Diffusion_Renderer_Inverse_Cosmos_7B"
            }
        }
    }
}

/// One row of the class table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateClass {
    pub template_id: &'static str,
    pub class: Class,
    pub model: Model,
    /// Role of the mp4 (`frames[0]` for EXR jobs).
    pub mp4_role: OutputRole,
    /// Role of the EXR frames, if the template can deliver them.
    pub exr_role: Option<OutputRole>,
    /// The output is made from the user's images or footage, so its manifest MUST carry a `parentOf` or
    /// `inputTo` visual ingredient; false for prompt-only templates (and for Data, which carries none).
    /// Stored with the fingerprint as `has_visual_input`; drives the detector's wording (D14).
    pub visual_input: bool,
}

/// Bumped whenever a row changes; node and signer must agree (same crate version).
pub const CLASS_TABLE_VERSION: u32 = 1;

const fn row(
    template_id: &'static str,
    class: Class,
    model: Model,
    mp4_role: OutputRole,
    exr_role: Option<OutputRole>,
    visual_input: bool,
) -> TemplateClass {
    TemplateClass {
        template_id,
        class,
        model,
        mp4_role,
        exr_role,
        visual_input,
    }
}

use Class::{Data as D, Generated as G, Modified as M};
use Model::{CosmosDiffusionRenderer as COSMOS, Ltx23 as L23, Ltx25 as L25};
use OutputRole::{Data as RD, Picture as RP};

/// The v48/v49 allow-list (37 templates), D7. Models read from each template's checkpoint names.
pub const CLASS_TABLE: &[TemplateClass] = &[
    // generated
    row("ltx-t2v-hdr", G, L23, RP, Some(RP), false),
    row("ltx-i2v-hdr", G, L23, RP, Some(RP), true),
    row("ltx-flf2v-hdr", G, L23, RP, Some(RP), true),
    row("ltx-ingredients-hdr", G, L23, RP, Some(RP), true),
    row("ltx-slowmo-hdr", G, L25, RP, None, true),
    row("ltx-layout-hdr", G, L25, RP, None, true),
    row("ltx-layout-chain-hdr", G, L25, RP, None, true),
    // modified, EXR picture
    row("ltx-iclora-hdr", M, L23, RP, Some(RP), true),
    row("ltx-iclora-chain-hdr", M, L23, RP, Some(RP), true),
    row("ltx-crossview-hdr", M, L23, RP, Some(RP), true),
    row("ltx-edit-hdr", M, L23, RP, Some(RP), true),
    row("ltx-edit-chain-hdr", M, L23, RP, Some(RP), true),
    row("ltx-outpaint-hdr", M, L23, RP, Some(RP), true),
    row("ltx-outpaint-chain-hdr", M, L23, RP, Some(RP), true),
    row("ltx-water-hdr", M, L23, RP, Some(RP), true),
    row("ltx-water-chain-hdr", M, L23, RP, Some(RP), true),
    row("ltx-daynight-hdr", M, L23, RP, Some(RP), true),
    row("ltx-daynight-chain-hdr", M, L23, RP, Some(RP), true),
    row("ltx-restore-hdr", M, L23, RP, Some(RP), true),
    row("ltx-sdr2hdr-hdr", M, L23, RP, Some(RP), true),
    // modified, mp4 only
    row("ltx-restore25-hdr", M, L25, RP, None, true),
    row("ltx-restore25-ref-hdr", M, L25, RP, None, true),
    row("ltx-cleanplate-hdr", M, L25, RP, None, true),
    row("ltx-cleanplate-chain-hdr", M, L25, RP, None, true),
    row("ltx-colorize-hdr", M, L25, RP, None, true),
    row("ltx-colorize-chain-hdr", M, L25, RP, None, true),
    row("ltx-deblur-hdr", M, L25, RP, None, true),
    row("ltx-decompress-hdr", M, L25, RP, None, true),
    row("ltx-refine-hdr", M, L25, RP, None, true),
    row("ltx-refine-chain-hdr", M, L25, RP, None, true),
    row("ltx-upscale25-hdr", M, L25, RP, None, true),
    row("ltx-upscale25-chain-hdr", M, L25, RP, None, true),
    // modified, EXR data (matte)
    row("ltx-alpha-hdr", M, L25, RP, Some(RD), true),
    row("ltx-alpha-chain-hdr", M, L25, RP, Some(RD), true),
    // data (the preview mp4 is the normal pass)
    // (visual_input false: a Data manifest carries no ingredient and no fingerprint, so the flag is unused)
    row("cosmos-passes-key", D, COSMOS, RD, Some(RD), false),
    row("cosmos-passes-std", D, COSMOS, RD, Some(RD), false),
    row("cosmos-passes-full", D, COSMOS, RD, Some(RD), false),
];

/// The row for a template, or None (unclassed: refuse).
pub fn lookup(template_id: &str) -> Option<&'static TemplateClass> {
    CLASS_TABLE.iter().find(|r| r.template_id == template_id)
}
