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
   `HOST_PRIVATE_KEY`, `RPC_URL`, optional `RUST_LOG`, and (both composes carry
   `HOST_TEE_ENABLED: "true"` since the v8.55.0 image) the attested-load set: `TEE_MODEL_ID`
   (32-byte hex, no `0x`; on `compose.cpu.yml` it MUST begin `7435743a`, the bytes `t5t:`,
   because the canned GPU half is released only by a TEST-keyring broker and the v8.56.0
   node refuses a test release for any other id, and a real-keyring release for a `t5t:`
   id, before downloading anything),
   `TEE_MODEL_PROVIDER` (the `0x` address whose signature the policy must carry),
   `TEE_KBS_URL` (`https://kbs.fabstir.net/v1/kbs`), `TEE_POLICY_URL` (may contain
   `{model_id}`), `TEE_BLOB_URL`, optional `TEE_EXPECTED_MODEL_SHA256` (the on-chain hash
   of the plaintext; absent = a CRITICAL warning). Not a secret but in the same block:
   `TEE_BLOB_MAX_BYTES` (container cap; default 2 GiB, a larger container is exit 78, so
   raise it for the real model). The binary's boot decision table is
   `src/tee/live.rs`: the flag and `TEE_MODEL_ID` are all-or-nothing (a compose with the flag
   `"false"` must have the `TEE_*` block commented out and the form must not supply them;
   the guard test enforces the pairing either way).
   `REQUIRE_MODEL_VALIDATION=true` on the attested path REQUIRES `TEE_EXPECTED_MODEL_SHA256`
   (the plaintext is bound to it; the plain path's filename-keyed registry check does not
   apply to a tmpfs decrypt), else exit 78. Nothing else is a secret; the contract addresses
   are public and live in the compose on purpose (they are then measured).
4. CPU rounds: stop the CVM between sessions (compute bills only while running), delete it
   after column A is signed off (disk bills always).
5. GPU day: follow the day order in the gate checklist; `nvidia-smi conf-compute -q` first.

## Things that will bite

- Relative paths mean nothing inside a CVM. Everything is in the image or fetched over
  `TEE_BLOB_URL` / `TEE_POLICY_URL`.
- TLS roots for those two fetches: reqwest's bundled Mozilla snapshot + the image's own
  store (`/etc/ssl/certs`, so a root added with `update-ca-certificates` in the Dockerfile
  counts) + the private broker root (`TEE_KBS_CA_FILE`, default `/etc/fabstir/kbs-root.pem`,
  the same rule the broker client uses), so the policy and the container can be served from
  `kbs.fabstir.net`'s static `/policies/` and `/blobs/` locations (`nginx-kbs.conf`, files
  under `/var/lib/fabstir-kbs/public/`) with no extra client configuration. A Let's Encrypt host on the 2026
  "ISRG Root YE" hierarchy needs that root baked into the image first (the snapshot predates
  it); `tests/tee_fetch_roots.rs` proves the store is honoured. The broker client itself
  pins the private root alone.
- `shm_size` is the decrypt dir. Smaller than the model = `ENOSPC`, fail-closed, day wasted.
  `TEE_DECRYPT_DIR` MUST be tmpfs on the attested path (exit 78 otherwise): shutdown unlinks
  the plaintext without overwriting it, which is only safe where the pages die with the mount.
- The compose file is hashed into RTMR3. Changing one byte changes `compose_hash` and
  invalidates the pinned policy. That is the point; plan edits before pinning.
- `stop_grace_period: 30s` in both composes: the node's stop path is API drain (≤ 5 s) +
  P2P leave, bounded at 8 s by its watchdog, which then ends the process itself. That
  bound runs from the signal, so a stop landing during a slow post-start step (LTX bundle
  publish, sidecar probes) logs "Orderly shutdown did not complete in time: exiting now."
  and exits 0 with no drain; that is by design, not a hang.
- A refused attested load exits 78 after a 20 s pause, and both composes use
  `restart: on-failure:5` (guard-enforced): a permanent refusal (hash mismatch, broker
  refusal, container over the cap) stops after five tries instead of re-downloading the
  container and burning a broker nonce forever. Read the log, fix the cause, start again.
- `TEE_GPU_EVIDENCE=canned` in the GPU compose is refused by the guard test and, if it ever
  ran, by a real-keyring broker. Both are deliberate. The mirror image is covered too: a
  broker LEFT on its test keyring (`KBS_GPU_EVIDENCE=canned`) after a gate day labels every
  release `test_release: true`, and the node refuses such a release unless its compose says
  `TEE_ACCEPT_TEST_RELEASE: "1"`, which only `compose.cpu.yml` does (the guard forbids it on
  the GPU compose). A GPU node meeting a canned broker exits 78 instead of serving, and from
  v8.56.0 it finds out from `GET /v1/kbs/info` BEFORE downloading the container (`broker
  /info: keyring is TEST …` in its log; `/info` is tried three times on transport failures,
  10 s budget each and 10 s apart, so a broker restart is ridden out).
- A `t5t:`-prefixed `TEE_MODEL_ID` without `TEE_ACCEPT_TEST_RELEASE: "1"` is refused at boot,
  offline (no broker can ever release it to a node that refuses test releases); the message
  blames the form.
- The image digest re-pin: `tests/phase5_release_pins.rs` holds the `(version, digest)`
  history; a release that will be re-cut adds a `("x.y.z", "PENDING")` row with its version
  bump and the paste fills it. Before pasting, prove the pushed image carries the
  post-P4.5 collector: `docker run --rm --entrypoint grep <image@digest> -c 'DevTools (CC
  development) mode' /usr/local/bin/collect_gpu_evidence.py` prints `1` (the Dockerfile
  copies the collector from the CHECKOUT, not the tarball; a stale checkout bakes the old
  one).
- `request_key` waits up to 180 s (`TEE_KBS_URL` client, `DEFAULT_REQUEST_KEY_TIMEOUT`): the
  broker's side spans the DCAP collateral fetch and the NRAS round trip, and a nonce is burned
  on arrival, so a client that gave up early could never retry. `challenge` keeps 30 s.
