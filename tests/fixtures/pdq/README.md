# PDQ reference vectors (Q1)

Source: Meta ThreatExchange `pdq` (https://github.com/facebook/ThreatExchange/tree/main/pdq),
the PDQ tech paper (arXiv 1912.07745), and the `pdqhash` Python package.

## Algorithm constants pinned by this port (`src/moderation/csam/pdq.rs`)

1. **Luminance**: BT.601 weights `Y = 0.299 R + 0.587 G + 0.114 B` on raw 8-bit RGB (no gamma).
2. **Downsample to 64×64**: Jarosz tent = 2 passes of a separable box blur, then decimate.
   Window size per axis: `(oldDim + 2·64 − 1) / (2·64)`.
3. **DCT**: full 64×64 DCT-II; extract the **frequency-1..16 × 1..16** block (DC row/col 0 is
   **excluded**), giving 256 coefficients. (Uniform `sqrt(2/64)` scale; scale-invariant under the median.)
4. **Bits**: median of the 256 coefficients via Torben rank `(n+1)/2 = 128` (= `sorted[127]`, a single
   element, **not** the average of the two middles); bit = 1 iff `coeff > median` (strict, ties → 0).
5. **Quality**: integer-truncated gradient sum over the 64×64 buffer —
   `d = (int)((u−v)·100/255)`, `quality = min(100, Σ|d| / 90)`. Meta discards `quality ≤ 49` as low.
6. **Match threshold**: Hamming distance ≤ 31 / 256 (config default `pdq_max_distance = 31`).

## Pinned reference vector (decoder-independent, Meta-exact)

A **constant (solid-colour) image** has all-zero AC DCT coefficients ⇒ **hash = all-zero (32 zero bytes)**
and zero gradient ⇒ **quality = 0**. This holds for ANY correct PDQ implementation including Meta's, so it
is pinned directly in `tests/moderation/test_pdq.rs` (no binary image needed).

## Go-live cross-check (NOT a build gate)

Bit-for-bit parity with Meta on natural images (e.g. the `pdq/data/reg-test-input/dih/bridge-*.jpg`
regression set, expected hashes in `pdq/cpp/reg_test/expected/out`) additionally requires JPEG-decoder
pixel agreement with Meta's CImg. That cross-check (vendor the images + verify with the reference
`pdq-photo-hasher`/`pdqhash`) is a go-live verification item, recorded here, not run in this build.
Sample expected (dih set, all quality 100):
`bridge-1-original.jpg d8f8f0cee0f4a84f0637022a078f67f0b36e2ed596621e1d33e6339c4e9c9b22`
