# LTX templates

This directory holds pinned LTX ComfyUI templates (API-format graphs).

`ltx-t2v-hdr/v1.json` is a **TEST FIXTURE** used by the Rust unit tests. It is
**NOT** the production author-time template. The real template is built by the
repo owner in desktop ComfyUI and pinned separately; this fixture only needs to
be small and structurally valid enough to exercise the patcher.

## Patch handles

The patcher locates nodes by their `_meta.title`. The three handles are:

- `PROMPT_IN` — text-encoder node; patcher writes `inputs.text`.
- `SAMPLER` — sampler node; patcher writes `inputs.seed`.
- `VIDEO_LATENT` — empty-latent node; patcher writes `inputs.width`,
  `inputs.height`, `inputs.length` (frame count) and `inputs.fps`.

## Contract

Value-substitution ONLY. The patcher replaces literal input values on the
titled nodes; it never adds, removes, re-wires, or otherwise structurally edits
nodes. Connection arrays (`["<node_id>", <output_index>]`) and all other nodes
stay exactly as pinned.
