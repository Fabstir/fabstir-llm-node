# Confidential Inference on Untrusted GPUs: The Whole Story, End to End

> **What this is, in one breath:** This is the narrative of a feature that lets a model owner ship their *encrypted* AI model to a GPU machine they don't trust, have that machine *cryptographically prove* it's a genuine sealed box running unmodified code, hand it the decryption key *only then*, decrypt the weights *only inside a confidential VM whose memory the operator cannot read*, run inference on the GPU, and securely wipe everything afterward, so that the machine's root-level operator can run the model and bill for it without holding the key. The attested load path is wired into the node binary's live request path and has run end to end on real confidential-computing hardware (Phala Cloud, Intel TDX with an NVIDIA H200: run 1 on 23 September 2026, run 2 on 30 September 2026; Section 5.1). By NVIDIA's design, the GPU memory the model runs in is sealed from the host in confidential-computing mode, and the GPU's signed measurements, checked against NVIDIA's reference measurements before the key is released, confirm that mode (Sections 5.1 and 6).

> **Currency note (updated 2026-10-01).** Sections 1–4 still describe the design. Section 5 records Phase 4 (integration, mock attestation) as it stood in June; **Section 5.1 records Phase 5 on real hardware**, which is where the feature now stands. Section 6 lists what is left today. Phase 5 ran on **Phala Cloud** (dstack, Intel TDX, one NVIDIA H200 in confidential-computing mode); the earlier Azure and IONOS planning is superseded and survives only in `../development/PHASE-4-TO-5-READINESS.md`.

---

## 1. The problem

Imagine you've spent a fortune building a proprietary large language model — the actual numeric **weights** (the trained parameters, stored as GGUF tensor data) are the valuable asset. You want to make money by running it on a *decentralized marketplace* of rented GPUs. But here's the catch: **you cannot trust the people who own those GPUs.**

The operator of any given GPU host has **root access** (full administrator control), physical access to the machine, and control over the **hypervisor** (the software that runs virtual machines) and the **NVIDIA kernel driver** (the host-side software that talks to the GPU). A hostile operator can reboot the box, attach a debugger, dump host RAM, and — on an ordinary GPU — read the GPU's on-board memory (VRAM) directly, snoop the PCIe link (the bus connecting CPU and GPU), or mount DMA attacks (direct memory access reads that bypass the OS). If you simply ship encrypted weights and then hand over a decryption key, the operator reads that key straight out of memory. Game over.

The naive defenses all fail:

- **Encrypt-at-rest, decrypt-in-process:** the operator (root) reads the key from process memory.
- **A CPU-only TEE** (Trusted Execution Environment — a hardware-protected, memory-encrypted enclave, e.g. AMD SEV-SNP or Intel TDX): the moment decrypted weights flow across PCIe into VRAM, the host driver reads VRAM unencrypted.

The only thing that actually closes every door is **NVIDIA Confidential Computing (CC)**: the node runs inside a CPU-TEE confidential VM *and* the GPU is in **CC-On mode**, where VRAM is access-controlled (only the one designated confidential VM can touch it) and the CPU↔GPU link is encrypted with AES-256-GCM. Inside that boundary, the node performs **remote attestation** — a hardware-signed proof that it's a genuine TEE running the exact, measured software the provider approved — and only after that proof checks out does a **Key Broker Service (KBS)** release the decryption key.

The target guarantee, stated bluntly:

> **The host can execute inference jobs and bill for them, but cannot obtain the plaintext weights.**

---

## 2. The cast

Before we follow a model through its life, meet the building blocks. Each is a module with one job.

- **The Model Provider (MP)** — owns the weights, sets the **policy** (the rules), holds the **Data Encryption Key (DEK)** — the 256-bit symmetric key that locks the weights.
- **The GPU Host (H)** — runs the confidential VM. *Assumed hostile.* The adversary in our story.
- **The Client (C)** — sends inference requests; in later phases can demand a TEE-attested node.
- **The Key Broker Service (KBS)** — the gatekeeper. Issues freshness challenges, checks attestation evidence against policy, and releases the DEK *wrapped* (re-encrypted) so the host can't read it. Trusted, acts for the provider.
- **The Verifier** — the judge that runs the actual checks on the evidence. In testing it's `DefaultVerifier`; in production it'll be a real NVIDIA-backed verifier.
- **Hardware roots of trust** — NVIDIA (GPU identity + attestation) and Intel/AMD (CPU TEE identity).

And the code modules, as characters:

- **`container.rs`** — the *vault builder*. Defines the encrypted container format and the chunked AEAD encryption/decryption. (The raw XChaCha20-Poly1305 primitive itself is *reused* from `src/crypto/encryption.rs`, the existing session-encryption layer — there is no TEE-specific `encryption.rs`.)
- **`provider.rs`** — the *witness stand*. The `AttestationProvider` trait: the seam the mock backend fills today and the real `NvidiaCcProvider` fills in Phase 5.
- **`types.rs`** — the *neutral shared rulebook*. Holds `Evidence`, `Policy`, and — crucially — the single canonical `report_data()` layout function so no two components can build or check `report_data` differently.
- **`keywrap.rs`** — the *key courier*. ECDH + HKDF + AEAD to wrap and unwrap the DEK.
- **`mock.rs` / `verifier.rs`** — the *judge and the stand-in witness*. The mock attestation provider/KBS and the `DefaultVerifier`.
- **`key_broker.rs`** — the *choreographer of the handshake* (`obtain_dek`).
- **`model_source.rs`** — the *careful butler*. Fetches the ciphertext, decrypts to tmpfs, manages the cache, and securely deletes.
- **`policy.rs` / `policy_source.rs`** — the *notary*. Validates the provider's signed policy.
- **`orchestration.rs`** — the *director* (`prepare_attested_model`), tying everything together into one fail-closed path.

---

## 3. The life of one encrypted model

Here's the whole journey at a glance:

```
PROVIDER (offline)                      HOST / CONFIDENTIAL VM                       KBS / VERIFIER
─────────────────                       ─────────────────────                       ──────────────
 random DEK (256-bit)
 encrypt GGUF -> container  ──S5──►   fetch ciphertext
 sign Policy (EIP-191)      ──────►   fetch + validate policy (signer == provider,
                                       validity window)  [fail-closed]
                                      generate ephemeral pk_att (secp256k1)
                                      ask for a challenge nonce  ───────────────────►  mint 32-byte nonce
                                                                                       (issued_at, consumed=false)
                                      gather Evidence:                  ◄────nonce────
                                        GPU evidence collected under
                                          the nonce (nonce inside the
                                          signed GPU report),
                                        report_data = sha256(pk_att)‖nonce,
                                        cpu_quote signs report_data,
                                        event_log, vm_config, pk_att, nonce
                                      submit Evidence  ──────────────────────────────►  burn nonce, then
                                                                                        checks (fail-closed):
                                                                                        nonce, real-payload guard,
                                                                                        quote len, identity,
                                                                                        nonce (CPU half), decode,
                                                                                        nonce (GPU half),
                                                                                        mrtd, hwmodel, CC mode,
                                                                                        TD debug, TCB status,
                                                                                        secure boot/debug, version
                                                                                        floors, validity
                                      WrappedKey (ECIES)               ◄──wrap DEK──────  wrap_key(dek, pk_att)
                                      unwrap with pk_att_secret -> DEK
                                      stream-decrypt container -> tmpfs (0600)
                                      sha256(plaintext) == on-chain hash? [fail-closed]
                                      LlmEngine::load_model on CUDA (CC-On VRAM)
                                      run inference -> tokens
                                      unload + secure_delete (zeroize + unlink)
```

Now the same journey, told slowly.

### Step 0 — The provider seals the vault (offline)

The provider generates a random 256-bit **DEK** and encrypts the GGUF weights with **XChaCha20-Poly1305** — an **AEAD** cipher (Authenticated Encryption with Additional Data: it both hides the data *and* detects tampering). XChaCha20 is a stream cipher with a generous 24-byte **nonce** (a number-used-once; reusing one under the same key would be catastrophic), and Poly1305 appends a 16-byte **authentication tag** that fails decryption if even one bit is altered.

The weights are split into **chunks** of 8 MiB. Chunking does two jobs: it gives each chunk a *unique nonce* without per-chunk randomness overhead, and it lets the node decrypt by streaming rather than loading the whole multi-GB model into memory at once.

The result is the **encrypted container** — a 98-byte fixed header followed by chunked ciphertext. The header (laid out by hand, no serde) is:

- 8 bytes magic `"FABS-TEE"`, 2 bytes version (`1`)
- 32 bytes `model_id`
- 4 bytes `chunk_size`, 4 bytes `num_chunks`
- 16 bytes `nonce_base` (CSPRNG-random)
- 32 bytes `policy_hash` (SHA-256 of the policy)

Two clever security details live here:

1. **Per-chunk nonce:** `nonce_base (16) ‖ chunk_idx_u32_be (4) ‖ 0x00×4` = 24 bytes. Deterministic, unique per chunk. Because the chunk count must fit in a `u32`, the 4-byte counter can never overflow (≥ 2³² chunks fails closed).
2. **The full header is bound into every chunk's AAD** (`chunk_aad = header_bytes ‖ chunk_idx`). This makes the header *tamper-evident*: if an attacker drops the last chunk and decrements `num_chunks`, the AAD changes and *every* remaining chunk's authentication tag breaks. This closes the **silent-truncation** attack.

The ciphertext is uploaded to **S5** (decentralized storage), and only the encrypted reference (path/CID) is published — *outside* the policy, so a swapped pointer is caught by header validation.

### Step 1 — The host fetches and validates the policy

The provider has separately signed a **`SignedModelPolicy`** — an off-chain authorization that says "this model may be decrypted under these conditions." It's signed with **EIP-191 personal_sign** (the Ethereum wallet-signature standard, the same thing MetaMask users click "sign" on; its magic prefix prevents the signature from being replayed as an on-chain transaction).

There are deliberately **two hashes**:

- **Policy hash (SHA-256)** of the canonical policy bytes — bound into the container's AAD (integrity).
- **Signature digest (Keccak256 with the EIP-191 prefix)** — what the wallet actually signed (authenticity).

Both provider and node compute **byte-identical canonical bytes** (`serde_json::to_value` → sort keys alphabetically → `to_string` → bytes), so any tampering invalidates the signature.

`fetch_validated_policy` is the single fail-closed gate: it fetches the policy, **recovers the signer address** from the signature, compares it case-insensitively to the **bound provider** (from on-chain `proposals(modelId).proposer`, with a config-fallback `ProviderRegistry` in Phase 4), and checks the **validity window** (`not_before ≤ now ≤ expiry`). Any mismatch → `VerificationFailed`, and nothing is decrypted. (A broken clock returns `u64::MAX` from `now_unix()`, which fails the window unconditionally — fail-closed by construction.)

### Step 2 — The host proves it's trustworthy (the attestation handshake)

This is the heart of the story, driven by `NodeAttestationClient::obtain_dek()`.

**(a) Challenge.** The node asks the KBS for a fresh **nonce**. The KBS mints a 32-byte CSPRNG value, records it with `issued_at` and `consumed: false`, and a TTL (default 300 seconds) starts ticking. Without this, an attacker could replay old evidence forever.

**(b) Ephemeral key.** The node generates a fresh **secp256k1** keypair `(pk_att_secret, pk_att_pub)` — the **attestation key**. The secret *never leaves encrypted RAM*; the public key (33 bytes, compressed) gets bound into the hardware proof. It's used once and discarded (forward secrecy).

**(c) Gather evidence — the shared nonce.** The node builds the `Evidence` structure. It collects the GPU attestation evidence **under the challenge nonce** (the mock serialises the GPU report fields, including that nonce, into `gpu_report`; the real collector hands the same nonce to NVIDIA's nvtrust, which puts it inside the hardware-signed GPU report), and asks the CPU TEE to sign a 64-byte `report_data`:

```
report_data[0..32]  = sha256(pk_att)      identity: the key the DEK will be wrapped to
report_data[32..64] = nonce               the challenge, in the clear
```

This layout (Phase 5, 2026-09-17; it matches Phala's dstack reference node) replaces the earlier `sha256(pk_att ‖ gpu_report_hash ‖ nonce)` commitment. Two independently signed quotes, one nonce: the CPU quote proves a genuine confidential VM holding `pk_att` answered *this* challenge, and the GPU report proves a genuine CC-mode GPU produced evidence for *the same* challenge. A hostile operator who pairs a genuine CPU quote with GPU evidence collected for another challenge (another session, another box, or a replay) fails the nonce comparison on the GPU half; one who substitutes a different `pk_att` to catch the wrapped key fails the identity comparison against the signed quote body. Neither quote has to exist before the other. (In Phases 1–4 the `cpu_quote` is a synthetic 64-byte blob where bytes 0–63 *are* `report_data`; in Phase 5 it is a real TDX quote from which `report_data` is extracted after signature verification. The layout is built by the *one shared* `report_data()` in `types.rs`, so the mock and the real verifier can never diverge.)

**(d) Submit and verify.** The node sends the evidence to the KBS's `request_key()`. The KBS first **burns the nonce** (marks it consumed *before* verifying — so a failed attempt can't be retried with the same nonce), checks it was issued and unexpired, then calls `DefaultVerifier::verify()`, which runs these checks **in fail-closed order** (Phase 5 layout and Policy schema 2, 2026-09-17):

0. The policy is schema 2 in canonical spelling (lowercase hex, no `0x`, right lengths); nothing non-conforming is coerced, it is refused, because the provider signed those exact bytes.
1. `ev.nonce` matches the KBS-issued nonce.
1b. **Real-payload guard:** if `gpu_report` is a real `nvidia_payload` JSON object, refuse with "needs the Phase-5 verifier" (this verifier is mock-only; the real broker verifier judges real evidence).
2. `cpu_quote.len() >= 64`.
3. **Identity:** `report_data[0..32] == sha256(pk_att)`, so the key the DEK will be wrapped to is the one the CPU TEE signed for.
4. **Nonce, CPU half:** `report_data[32..64] ==` the issued nonce, in the clear.
5. Decode `gpu_report` into `GpuReportFields` (the mock shape; the real path maps NRAS EAT claims into it).
6. **Nonce, GPU half:** the nonce inside the GPU evidence `==` the issued nonce. Two independently signed quotes, one challenge: this is the whole cross-binding.
7. **Measurement:** the mock's `image_measurement` (its stand-in for MRTD) must equal `policy.cvm.mrtd`. The real broker verifier compares MRTD and RTMR0–2 from the *verified* quote and replays RTMR3 for `os_image_hash` / `compose_hash`; the mock cannot, so those are validated for form only.
8. **Hardware model:** `hwmodel` is in `policy.gpu.allowed_hwmodels`.
9. **CC mode:** if required, matched *exactly* (`on` ≠ `devtools`).
10. **TD debug:** if `policy.cvm.require_td_debug_off`, the TD DEBUG attribute must be clear.
11. **TCB status:** in `policy.cvm.allowed_tcb_status` (exact strings such as `UpToDate`; widening is a signed policy change); then GPU secure boot / debug status and the optional driver and VBIOS version floors.
12. **Validity window:** broken clock fails; `not_before ≤ now ≤ expiry`.

Any failure → `TeeError::VerificationFailed`, no key released.

### Step 3 — The key is released, wrapped (ECIES)

If every check passes, the KBS wraps the DEK to the node's `pk_att` using **ECIES** (Elliptic Curve Integrated Encryption Scheme):

- Generate a fresh ephemeral keypair `(eph_secret, eph_pub)`.
- **ECDH** (Elliptic Curve Diffie-Hellman key agreement): `ecdh = diffie_hellman(eph_secret, pk_att)`.
- Hash it: `shared = sha256(ecdh.raw_secret_bytes())`.
- **HKDF-SHA256** expand with `info = b"key-wrap-v1"` (an 11-byte **domain-separation** tag, so this key can never collide with keys derived for other purposes like checkpoint-delta encryption), no salt, to 32 bytes.
- Encrypt the DEK with XChaCha20-Poly1305 under a fresh 24-byte nonce, with `aad = eph_pub`.

The result is a `WrappedKey { eph_pub, nonce (24B), ciphertext (32B DEK + 16B tag = 48B) }`. Only knowledge of `pk_att_secret` can unwrap it.

### Step 4 — Unwrap and decrypt into RAM only

The node calls `unwrap_key(&wrapped, &pk_att_secret)`: it recomputes the identical wrap key (ECDH is symmetric — both sides derive the same shared secret), decrypts, and validates the plaintext is *exactly* 32 bytes. Any tampering, wrong secret, or swapped `eph_pub` → error. Fail-closed; there's no partial decryption.

With the DEK in hand, `prepare_encrypted_model` does the careful work:

- **First, the fail-closed gate:** check `HOST_TEE_ENABLED` (an env flag accepting only `1`/`true`/`yes`/`on`, cached once via `OnceLock`). A non-TEE node logs CRITICAL and returns `NonTeeNodeRefusesEncrypted` *before* any cache lookup, S5 fetch, or decryption. A node never honors a capability it can't deliver.
- **Stream-decrypt to tmpfs.** `decrypt_model` validates `model_id` and `policy_hash` in the header *before* touching chunks, reconstructs each chunk's nonce and AAD identically, verifies the Poly1305 tag *before* writing plaintext, and streams into a **tmpfs** file (RAM-backed, mode `0600`, inside encrypted guest RAM). Plaintext never touches disk or network. If decryption fails on chunk *k*, the caller must `secure_delete` the partial output.
- **Cache + refcount.** The decrypted file is cached by `(model_id, policy_hash)` — so a *policy rotation* (new hash) is a cache miss forcing fresh attestation. Concurrent loads share one file via refcounting; the file is securely deleted only when the last reference drops.

### Step 5 — The on-chain hash check (and an honest TOCTOU note)

The director, `prepare_attested_model`, now hashes the decrypted weights (`sha256_file_hex`) and compares — case-insensitively — to `expected_model_hash` (the hex of the model's on-chain `ModelInfo.sha256_hash`). On **match**, it logs a TOCTOU warning and proceeds. On **mismatch**, it calls `fail_closed()`: release the cache reference, securely delete the plaintext, return `ModelHashMismatch`.

The **TOCTOU** (Time-of-Check-to-Time-of-Use) warning is honest engineering. Between this hash check and the moment llama.cpp `mmap()`s the tmpfs file inside `load_model()`, a host with filesystem access *inside the CVM* could swap the file. Phase 4 *logs* this (in both `orchestration.rs` and `engine.rs`, searchable via `target: "tee"`) but does not yet *close* it. The practical risk is already mitigated by the CVM's encrypted-RAM boundary; Phase 5 closes it airtight via fd-based loading, `F_ADD_SEALS`, or re-verify-before-mmap.

### Step 6 — Load on the GPU, run, and wipe

The caller builds a `ModelConfig { encrypted: true, model_path: <tmpfs path>, .. }` and hands it to `LlmEngine::load_model()`, which loads the plaintext onto CUDA (in CC-On mode the weights land in protected VRAM), and runs inference. When done, the model is unloaded, GPU memory released, and **`secure_delete`** wipes the tmpfs file: a single-pass **zeroize** (overwrite with zeros in 64 KB chunks, then `sync_all`) followed by `unlink`. One pass suffices because the pages are TEE-encrypted RAM. The function is idempotent; if deletion fails, `purge_or_warn` logs CRITICAL rather than swallowing the error.

---

## 4. The promises it keeps (and the rules)

| Aspect | What it covers |
|---|---|
| **Asset** | Proprietary model weights (GGUF tensor data). |
| **Adversary** | GPU host operator: root, physical access, can dump RAM/VRAM, snoop PCIe, reboot. |
| **Defense** | NVIDIA CC (GPU access-control + link encryption) + CPU TEE (encrypted RAM + attestation) + cross-bound attestation + a key-release gate. |
| **Guarantee** | Host cannot obtain plaintext weights; decryption happens only in protected RAM/VRAM under attestation. |
| **Out of scope** | Silicon attacks, side channels, supply-chain compromise, DoS, inference-result correctness (that's Risc0's job). |

The recurring discipline is **fail-closed**: deny by default unless *every* check passes. A mis-set clock (`u64::MAX`) fails closed. An expired policy (`now > expiry`, or `expiry = 0` for instant revocation) fails closed. A mismatched measurement, a disallowed SKU, CC-Off, stale TCB, an identity or nonce mismatch on either half, an unknown or stale nonce — all return an error and withhold the DEK. No plaintext is ever written on a failure path.

The supporting promises:

- **Authentication** — the hardware proves the node holds `pk_att_secret` and that the nonce was KBS-issued; the same nonce inside the signed GPU evidence ties the GPU half to the same challenge.
- **Integrity** — Poly1305 tags everywhere; tampering breaks decryption.
- **Confidentiality + forward secrecy** — DEK wrapped under ephemeral ECDH; compromising `pk_att_secret` later can't decrypt past captures.
- **Freshness + replay protection** — single-use, TTL-bounded nonces; burned up-front.
- **Two distinct nonces, no overlap** — the 32-byte *KBS nonce* (attestation freshness; the value both quotes are bound to) and the 16-byte container *nonce_base* (AEAD chunk encryption) never mix, avoiding a false sense of single-nonce safety.
- **Provider control via signed policy** (schema 2) — pin the CVM registers (`mrtd`, `rtmr0`–`rtmr2`, `os_image_hash`, `compose_hash`), allow-list GPU models (`hwmodel`), require CC mode, secure boot and debug off, set driver/VBIOS floors, allow-list TCB statuses and advisories, set a validity window. Policies are off-chain and signed, so they can be rotated (tighten, revoke) *without re-encrypting the weights*.
- **Capability discovery** — a node advertises `tee-attested` (in registration metadata and the WebSocket handshake) **iff** `HOST_TEE_ENABLED` **and** the model behind it was not a test-keyring release (Phase 5: a CPU gate node keyed against canned GPU evidence runs without the advert), so clients select only nodes that will honor encrypted models against real evidence. Legacy-registry deployments emit no `capabilities` key at all, so they can't accidentally claim TEE support.

---

## 5. What's been proven

*This section records Phase 4 as it stood on 2026-06-03. Its statements about the live request path and the mock backend are historical: Phase 5 (Section 5.1) wired the path into the node binary and replaced the mocks with real attestation.*

The GPU end-to-end test (`/workspace/fabstir-llm-node/tests/tee_e2e.rs`) ran on **real hardware** (TEST_HOST_1 / 3XS-Z, real NVIDIA GPU with CUDA) and exercised the complete attested load path **in integration** — driving the orchestration entry `prepare_attested_model` with *no production edits*. Stated precisely: that test is the only caller of `prepare_attested_model`; the node binary's live request path loads plain models (`src/main.rs`, `encrypted: false`) and does not yet call it. The steps proven are:

1. **Provider-side offline:** sign a policy (ECDSA via k256, address via `recover_client_address`), encrypt a real 1B-parameter GGUF (`tiny-vicuna-1b.q4_k_m.gguf`) with XChaCha20-Poly1305 in 8 MiB chunks.
2. **Node-side:** validate the policy, attest (mock backend), receive the DEK from `MockKeyBroker`, decrypt to tmpfs.
3. **Hardware proof:** assert plaintext lives *only* on tmpfs (`is_tmpfs`) and round-trips byte-exact.
4. **Real GPU inference:** `LlmEngine::load_model` with `gpu_layers: 99` (all layers on the GPU) on the prompt "The capital of France is" → " Paris, and it is the capital of the Île-de-France region", with `tokens_generated > 0`.
5. **Secure teardown:** unload, `secure_delete`, assert the file is gone.

**Result: 1 passed in 147 seconds.** This proves the security-critical path (encrypt → policy → attest → DEK → decrypt-to-tmpfs → hash-bind → GPU load → infer → secure_delete) is real, not theoretical — in integration, on real GPU hardware. Wiring it into the live request path is Phase 5 (2026-09-17 correction; see `docs/archive/PHASE5-ATTESTATION-RECON-REPORT.md`).

**The honest asterisk:** the test used `MockAttestationProvider`, which **accepts any challenge nonce and measurement without verifying them against NVIDIA hardware roots**. The *orchestration* is real; only the cryptographic verification of evidence against hardware is bypassed. This is intentional — it lets the entire software pipeline be tested on non-CC hardware and in CI. But it means **during Phases 1–4 a malicious host could pass mock attestation without actually running in a confidential VM.** The full threat model is *not* satisfied until Phase 5.

**Test counts and version:**

- `tee_tests`: **84 test functions** defined in `tests/tee/` (Phase 1 ~22, Phase 2 ~53, Phase 3 ~66, Phase 4 cumulative); **78 passed on the GPU host** (TEST_HOST_1) on the last host run, the remainder environment-gated.
- TEE code is **fmt-clean and clippy-clean** (zero warnings/errors in `src/tee/` and `tests/tee/`); the lib compiles under test config.
- **Version: `8.30.0-tee-confidential-inference`** — the TEE feature's snapshot version (`src/version.rs`). The repo has since moved to `8.37.0` with unrelated LTX work layered on top; the TEE code is unchanged.
- GPU e2e: `tests/tee_e2e.rs`, 1 passed in 147 s.

A **relaxed baseline-diff gate** was approved for Phase 4: TEE tests green + TEE fmt/clippy-clean + lib compiles + *no new* `--lib` failures. Pre-existing, TEE-unrelated failures (`api::embed` needs the ONNX model; `api::response_formatter`; hanging `ezkl`/`inference`/`contracts`; the Risc0 guest build needs `RISC0_SKIP_BUILD=1`) are accepted because the TEE module is cleanly isolated.

**The two Phase-4 limitations, stated plainly:**

- **Mock attestation** doesn't verify the measured node image against hardware roots.
- **The TOCTOU window** (verify → mmap) is logged, not closed.

For **open-weight models**, none of this is a blocker: the pipeline works end-to-end, and such models skip the KBS entirely, relying on the on-chain `sha256_hash` plus environment attestation. The policy/KBS/key-release machinery only *matters* for proprietary weights with secrets to protect.

### 5.1 Phase 5: real confidential-computing hardware

**Run 1, 23 September 2026 (Phala Cloud, Intel TDX + one NVIDIA H200).** The off-node key broker verified a genuine TDX quote (TCB status up to date, no advisories) and the H200's attestation through NVIDIA's Remote Attestation Service (hardware model `GH100`, driver and VBIOS versions, secure boot on, debug disabled), with the same one-time challenge in both. It released the model key on the first boot after the machine's measurements were pinned; those launch and boot registers had been recomputed independently from the provider's published image and matched the hardware quote before they were pinned, not trusted on first use. The node checked the decrypted Qwen3.8-27B (29 GB) against its on-chain hash, loaded it onto the H200 and served inference; on a clean stop it deleted the plaintext. A replayed key request was refused. Paid testnet chat sessions from the app then ran on that host, with zero-knowledge proofs, checkpoints and settlement on-chain, and a proof published to S5 that an unrelated machine fetched and matched to the on-chain hash.

**Run 2, 30 September 2026 (same hardware): the whole product on one attested machine.** One configuration deployed the node, image generation (FLUX.2), the fine-tuning trainer, the storage bridge and the video pipeline (LTX 2.3) in one confidential VM.

- **The configuration was signed before the machine existed.** Its compose hash was computed in advance from our own compose file and signed into the policy; it matched on the first boot, as did the launch registers pinned after run 1, and the key was released.
- **A changed configuration needs a new signed policy.** A mid-day change (a storage-bridge fix and the video services) changed the compose hash, so release required a newly signed policy naming the new hash and a container resealed under it, because the container header binds the policy hash into every chunk. The encrypted copy cached on the machine, bound to the previous policy, was detected as stale and re-downloaded. Three boots, three releases, each against a fresh quote and fresh GPU evidence; the third boot reused the cached container and was serving about four minutes after it started.
- **The hardware view matches the policy.** The provider's own console showed the running machine's MRTD, RTMR0 to RTMR2 and compose hash identical, digit for digit, to the values in the signed policy the broker held.
- **From the app and the Blender extension:** encrypted chat, images, a fine-tune whose dataset was decrypted only inside the confidential VM and whose adapter came back encrypted and was served into a session on the same machine, and six video-generation modes; every job settled on-chain. Two independent fine-tunes of the same job produced byte-identical adapters.
- **What it does not show.** The video weights are public and were verified against pinned hashes as they downloaded; they are not sealed or released by the broker. The GPU's mode is confirmed through NVIDIA's measurement comparison rather than a named claim (Section 6).

The run records, captured artefacts and every decision are in `../development/EXECUTION-PHASE5-ATTESTATION.md` and `../development/PHASE5-KNOWN-GAPS-TOFU.md`.

---

## 6. What's left

Phase 5 proved the path on real hardware. These items stand between that and a production guarantee; the gap numbers refer to `../development/PHASE5-KNOWN-GAPS-TOFU.md`.

- **GPU memory and the confidential-computing mode (G-6a, closed 2026-10-01 on NVIDIA's documented design; NVIDIA's written confirmation pending).** No NVIDIA claim names the mode; NVIDIA's GTC 2023 deck (S51709) says the attestation report records which of the three modes the GPU is in, lists confidential-computing configuration among the measurements, and describes the verifier's comparison with the golden reference measurements as "a pass/fail report for correct CC configuration". The broker requires that comparison (`measres`) plus the secure-boot and debug claims. Public copy states the GPU-memory protection as NVIDIA's design, with "sealed" or "blocked", never "cannot"; a negative answer from NVIDIA would reopen this.
- **Automatic routing to `tee-attested` hosts.** Both runs routed paid sessions by hand, by pointing a test host's on-chain registration at the confidential VM.
- **Video weights under the attested release.** Only the language model is sealed and broker-released; the LTX weights are public and only hash-checked on download.
- **Host profiles separate from the sealed model binding (policy schema 3, `../development/DESIGN-POLICY-V3-HOST-PROFILES.md`).** Today every new measurement is a new policy and a reseal of every model, as run 2's mid-day change showed. The ratified design moves host measurements into a separately signed, versioned profile set; not yet built.
- **A second, independent signer for host approvals (G-20)**, which first needs the node image to reproduce from public source (G-21): `Cargo.lock` is committed and `scripts/build-release.sh` remaps build paths, but the CUDA kernels still embed per-build temporary names.
- **Deferred hardening:** the verify-then-load window inside the confidential VM (G-12) is logged, not closed; NVIDIA's service is on the release path rather than local verification (G-8); reference-measurement collateral is fetched rather than pinned (G-9); the node-to-broker TLS is one private CA with no revocation path (G-15).
- **Confidential training as a guarantee.** The dataset key travels in the job payload to whichever host the client chose; releasing it only into an attested confidential VM reuses this machinery.

---

## Appendix — Glossary

- **AAD (Additional Authenticated Data):** metadata authenticated by the AEAD tag but not encrypted; tampering with it breaks decryption.
- **AEAD:** Authenticated Encryption with Additional Data — encryption that provides both secrecy and tamper detection.
- **Attestation key (`pk_att`):** an ephemeral secp256k1 public key generated inside the TEE, bound into the attestation; the DEK is wrapped to it. The secret never leaves encrypted RAM.
- **Canonical policy bytes:** byte-stable serialization (sorted JSON keys) so provider and node produce identical bytes.
- **CC-On (Confidential Computing On):** NVIDIA GPU mode where VRAM is access-controlled and the CPU↔GPU link is encrypted; only one confidential VM may access the GPU.
- **Chunking:** splitting the model into fixed 8 MiB pieces, each encrypted independently, enabling unique nonces and streaming decryption.
- **Confidential Computing (CC):** hardware feature encrypting VM memory and GPU VRAM, inaccessible to the host even with root.
- **Confidential VM (CVM):** a VM on a CPU TEE (Intel TDX / AMD SEV-SNP) whose guest RAM is encrypted from the host and hypervisor.
- **Container (encrypted model container):** the file format — 98-byte header + AEAD-sealed chunks.
- **Cross-binding:** fusing GPU report hash, `pk_att`, and nonce into one SHA-256 embedded in the signed CPU quote, so evidence from different machines can't be mixed, swapped, or replayed.
- **DEK (Data Encryption Key):** the 256-bit symmetric key encrypting the weights; released only after attestation, wrapped to `pk_att`.
- **Domain separation:** distinct HKDF `info` tags (e.g. `"key-wrap-v1"`) so the same secret yields independent keys in different contexts.
- **ECDH:** Elliptic Curve Diffie-Hellman key agreement — two parties derive a shared secret from their keypairs.
- **ECIES:** ECDH + HKDF + AEAD to wrap a key to a recipient's public key, so only they can unwrap it.
- **EIP-191 personal_sign:** Ethereum wallet-signature standard with a magic prefix preventing replay as an on-chain transaction.
- **Ephemeral keypair:** a one-use keypair giving forward secrecy.
- **Evidence:** the structure (`gpu_report`, `cpu_quote`, `image_measurement`, `pk_att`, `nonce`) sent to the verifier.
- **Fail-closed:** any error or failed check denies the operation; never falls back to an unsafe default.
- **Freshness nonce:** a one-time KBS-issued random value binding attestation to a moment in time, defeating replay.
- **GPU report:** GPU-hardware-signed evidence (SKU, CC state, identity certs).
- **CPU quote:** CPU-TEE-signed evidence including the launch measurement and the 64-byte `report_data`.
- **Hash bind:** SHA-256 comparison of decrypted weights against the on-chain-approved hash; fail-closed.
- **HKDF:** HMAC-based Key Derivation Function — stretches a shared secret into independent keys via an `info` tag.
- **HOST_TEE_ENABLED:** flag (true only inside a genuine CVM) gating encrypted-model loading and `tee-attested` advertisement; default false.
- **KBS (Key Broker Service):** issues nonces, verifies evidence against policy, and releases the wrapped DEK.
- **Launch measurement:** a 48-byte SHA-384 hash of the node image at boot (AMD `LAUNCH_MEASUREMENT` / Intel `MRTD`), pinned in policy.
- **Measurement (expected):** the provider-pinned value the attested measurement must match.
- **Mock attestation backend:** a test stand-in that accepts evidence without hardware verification — enables non-CC testing but does not satisfy the threat model.
- **Nonce:** a number used once; reuse under the same key breaks AEAD security.
- **NRAS:** NVIDIA Remote Attestation Service (cloud verifier), used during Phase 5 prototyping.
- **One-time-use nonce:** invalid after a single consumption (burned up-front), defeating replay and retry.
- **Policy:** provider-defined rules (allowed SKUs, expected measurement, CC/TCB requirements, TCB-age cap, validity window, model_id).
- **Policy hash (SHA-256):** digest of canonical policy bytes, bound into the container AAD.
- **Production TCB:** non-debug CPU firmware build.
- **`prepare_attested_model`:** the single fail-closed orchestration entry: fetch policy → validate → attested decrypt → hash-bind.
- **`PreparedModel`:** the decrypted, attested, hash-verified result (tmpfs path, model_id, policy_hash, policy), cache-keyed by `(model_id, policy_hash)`.
- **Refcounting:** tracking how many loads share a decrypted file; deleted only when the count hits zero.
- **Remote attestation:** a hardware-signed proof a platform is genuine and running specific measured code in a secure state.
- **Report data:** the 64-byte signed CPU-quote field carrying `sha256(pk_att)` (bytes 0–31) and the challenge nonce in the clear (bytes 32–63).
- **RIM (Reference Integrity Measurement):** NVIDIA's authentic-firmware baseline used by verifiers.
- **S5:** decentralized storage holding the encrypted container.
- **Secure delete:** single-pass zeroize (RAM is TEE-encrypted) then unlink; idempotent.
- **Signed model policy:** the provider-signed off-chain authorization to release the DEK for a model.
- **Silent-truncation vector:** dropping chunks and editing `num_chunks`; defeated by binding the full header into every chunk's AAD.
- **hwmodel:** the GPU model identifier NVIDIA's attestation reports (e.g. H100, H200); the policy's `allowed_hwmodels` allow-lists it.
- **TCB (Trusted Computing Base):** the security-critical firmware/microcode/kernel the TEE relies on; the policy allow-lists the acceptable TCB statuses (e.g. `UpToDate`) and advisory ids rather than an age.
- **`tee-attested`:** a capability string advertised iff `HOST_TEE_ENABLED` and the loaded model was not a test-keyring release, letting clients select TEE-honoring nodes.
- **TEE (Trusted Execution Environment):** a hardware-isolated, memory-encrypted, attestable execution context.
- **tmpfs:** RAM-backed filesystem (mode 0600 here); decrypted weights live only here, never on disk.
- **TOCTOU (Time-of-Check-to-Time-of-Use):** a race where a file is swapped between verification and use; logged in Phase 4, closed in Phase 5.
- **VRAM:** the GPU's on-board memory holding weights during inference; CC-protected in CC-On mode.
- **WrappedKey:** `{ eph_pub, nonce (24B), ciphertext (48B = 32B DEK + 16B tag) }` — the ECIES-sealed DEK.
- **XChaCha20-Poly1305:** the AEAD cipher (24-byte nonce, 16-byte tag, 32-byte key) used for both weights and key-wrap.
- **Zeroize:** overwriting sensitive bytes with zeros so they can't be recovered from memory.

---

## Companion documents

This story is the *narrative* view. When you want the detailed, checkbox-level
account, go to:

- **`../development/IMPLEMENTATION-NVIDIA-TEE.md`** — the full implementation plan
  with every sub-phase (1.1 → 4.4) ticked off, the threat model, the design
  decisions (D1–D8), and a dated execution changelog (including the GPU-proven
  entry and the relaxed-gate policy).
- **`../development/PHASE-4-TO-5-READINESS.md`** — the Phase-4→5 handoff: one-screen
  status, module-by-module test status, and **§4 the exact Phase-5 unblock list**
  (the hardware/SDK/deploy checklist) + **§5 the decisive provider question** for
  IONOS/Azure.
- **`../development/EXECUTION-NVIDIA-TEE.md`** — how the build itself was driven.

Source code: `src/tee/**` (the modules in "The cast"); tests: `tests/tee/**` and
the GPU end-to-end proof `tests/tee_e2e.rs`.

*Written 2026-06-03 for v8.30.0-tee-confidential-inference. Phases 1–4 complete
(mock backend); Phase 5 (real CC-On attestation) is the remaining 20%.*

*Updated 2026-10-01 after Phase 5 runs 1 and 2 on Phala Cloud (Intel TDX + NVIDIA H200):
Section 5.1 added, Section 6 rewritten.*