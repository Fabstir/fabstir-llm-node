# LTX templates — the pinned allow-list

This directory holds the pinned LTX ComfyUI templates (API-format graphs) plus
`allowlist.json` (bundle **v7**). The node loads them at startup, computes each
template's canonical keccak256 hash, publishes the bundle to S5 and serves it to
clients; a job is accepted ONLY when its `(templateId, templateHash)` matches the
pinned graph byte-for-byte. That hash chain — on-chain `metadata.ltx` → bundleHash →
templateHash — is what makes a registered model id a truthful provenance claim.

## The seven templates

| templateId | inputs | notes |
|---|---|---|
| `ltx-t2v-hdr` | — | text → video |
| `ltx-i2v-hdr` | 1 still | prompt-enhance baked ON |
| `ltx-flf2v-hdr` | 2 stills | first/last keyframes |
| `ltx-iclora-hdr` | 1 still + 1 clip | IC-LoRA union control (MoGe depth guide), AV output |
| `ltx-outpaint-hdr` | 1 clip | fits + black-letterboxes to job w/h; the outpaint LoRA fills pure-black regions |
| `ltx-edit-hdr` | 1 clip | centre-crop conform; prompt-driven in-video editing (experimental checkpoint) |
| `ltx-restore-hdr` | 1 clip | centre-crop conform; detail recovery / upscale |

The BL4 trio (outpaint/edit/restore) shares one 30-node spine — `ltx-2.3-22b-dev-fp8`
+ distilled-384 LoRA + mode IC-LoRA, 8-step ManualSigmas, LOCAL Gemma text encoder,
radiance gamma pair — and muxes the SOURCE clip's audio into the output. Canvas maths
derive from `GetImageSize` on decoded tensors, never container metadata.

## Host requirements (BL4)

- **ComfyUI ≥ 0.25** (the trio's graphs use post-0.25 core behaviour; proven on
  v0.27.0). Custom node packs: ComfyUI-LTXVideo, KJNodes, VideoHelperSuite, ComfyMath,
  and **radiance** (fxtdstudios/radiance, pinned commit `64fee414` — supplies
  `Float32ColorCorrect`; its requirements.txt lists a non-existent `Imath` dist — skip
  that line, the OpenEXR wheel provides the module).
- **Weights** (models/loras/), sha256-verified; sources + full provenance in the repo
  owner's records:
  - `ltx-2.3-22b-distilled-lora-384.safetensors` — Lightricks/LTX-2.3 (`f5d4953f…`)
  - `ltx-2.3-22b-ic-lora-outpaint.safetensors` — community training checkpoint,
    byte-identical on two independent mirrors (`32c5d3e0…`)
  - `ltx23_edit_anything_global_rank128_v1_9000steps_adamw.safetensors` —
    Alissonerdx/LTX-LoRAs (`36721b39…`)
  - `ltx2.3-video-restoration-general.safetensors` — joyfox/LTX2.3-ICEdit-Insight
    (`0460ace5…`)
- **Licences**: the LTX 2.3 base weights and the Lightricks repos are licence-gated on
  Hugging Face (browser login + acceptance); the mode LoRAs are community-trained
  derivatives — review the LTX-2 derivative-weights terms before third-party rollout.

## Patch handles (the patcher's contract)

The patcher locates nodes by `_meta.title` or `class_type` and substitutes leaf
values ONLY — it never adds, removes or re-wires nodes, so the graph that runs is the
graph that was hashed:

- `Prompt` (title) — the positive prompt (`value` or `text` leaf). REQUIRED.
- `RandomNoise.noise_seed` / `KSampler.seed` (class) — the job seed. REQUIRED (≥1).
- `Width` / `Height` / `Frame Rate` / `Duration` (titles) — optional dims/rate handles.
- `LoadImage.image` + `LoadVideo.file` / `VHS_LoadVideo.video` (class) — input
  bindings, node-id order, count fail-closed.
- `VHS_LoadVideo.frame_load_cap` (class) — capped at the billed frame count
  (defence-in-depth atop the stsz gate).

`ltx-t2v-hdr/v1.json`'s structure doubles as the patcher test fixture; all seven are
production graphs whose hashes are golden-pinned in `tests/ltx/test_template.rs`.
