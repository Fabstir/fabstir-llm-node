# Phala Cloud deployment (Phase 5 attested inference)

What goes on the Phala deploy form, and nothing else. Design and gates:
`docs/development/PHASE5-GATE-CHECKLIST.md`, `EXECUTION-PHASE5-ATTESTATION.md`.

## Files

| File | Purpose |
|---|---|
| `Dockerfile` | one image for both CVMs: node binary from a release tarball + nvtrust 2.6.3 (evidence collection) + CUDA driver stub for the CPU variant |
| `entrypoint.sh` | stub only under `TEE_CPU_ONLY_STUB=1`; refuses if a GPU is present; shouts if `TEE_GPU_EVIDENCE=canned` |
| `collect_gpu_evidence.py` | `nvidia_payload` for the broker; `"canned": true` label in test mode |
| `compose.gpu.yml` | the paid day. Guarded by `cargo test --test phase5_compose_guard` |
| `compose.cpu.yml` | the free gates on a `dstack-v0.5.9` CPU CVM (`tdx.large`) |
| `build.sh` | build + push from a tarball, prints the digest to paste (needs docker) |

## Steps

1. On a box with docker: `deployment/phala/build.sh <release tarball>`. Paste the printed
   `@sha256:` into both composes. Run `cargo test --test phase5_compose_guard`; it must be
   green before either compose goes on the form.
2. Phala Cloud → Deploy → Custom (docker compose). Paste the compose. Image: `dstack-v0.5.9`
   for the CPU rounds (`tdx.large`, 40 GB disk); **`dstack-nvidia-0.5.9`** for the paid day
   (Phala, 2026-09-17: the current stable release; 0.6.0 is still in testing), single H200.
   **The form's image selection must equal the compose's `ai.platformless.dstack_os_image`
   label**: `dstack-v0.5.9` on the CPU compose, `dstack-nvidia-0.5.9` on the GPU compose. The
   two names differ by one word and measure differently; the guard test asserts the label
   per file, the form is on you. The label is operator hygiene, not a security control: it
   says which image was intended, and only the measurement comparison (checklist A-24 vs
   A-12/B-3) says which image booted.
3. Encrypted environment variables (the form's secrets, one per `${VAR}` in the compose):
   `TEE_MODEL_ID`, `TEE_KBS_URL`, `TEE_POLICY_URL`, `TEE_BLOB_URL`, `HOST_PRIVATE_KEY`,
   `RPC_URL`, optional `RUST_LOG`. Nothing else is a secret; the contract addresses are
   public and live in the compose on purpose (they are then measured).
4. CPU rounds: stop the CVM between sessions (compute bills only while running), delete it
   after column A is signed off (disk bills always).
5. GPU day: follow the day order in the gate checklist; `nvidia-smi conf-compute -q` first.

## Things that will bite

- Relative paths mean nothing inside a CVM. Everything is in the image or fetched over
  `TEE_BLOB_URL` / `TEE_POLICY_URL`.
- `shm_size` is the decrypt dir. Smaller than the model = `ENOSPC`, fail-closed, day wasted.
- The compose file is hashed into RTMR3. Changing one byte changes `compose_hash` and
  invalidates the pinned policy. That is the point; plan edits before pinning.
- `TEE_GPU_EVIDENCE=canned` in the GPU compose is refused by the guard test and, if it ever
  ran, by a real-keyring broker. Both are deliberate.
