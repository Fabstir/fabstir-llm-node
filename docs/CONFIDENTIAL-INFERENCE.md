# Confidential Inference on Untrusted GPUs: The Whole Story, End to End

> **What this is, in one breath:** This is the narrative of a feature that lets a model owner ship their *encrypted* AI model to a GPU machine they don't trust, have that machine *cryptographically prove* it's a genuine sealed box running unmodified code, hand it the decryption key *only then*, decrypt the weights *only into encrypted RAM*, run inference on the GPU, and securely wipe everything afterward — all while the machine's root-level operator can run the model and bill for it but can never read the plaintext weights. It works end-to-end on real GPU hardware today (behind a mock attestation backend), and the final 20% — real hardware-rooted attestation on a confidential VM — is Phase 5.

> **⚠️ Currency note (updated 2026-07-16).** Sections 1–5 (the software story, the crypto design, and "what's been proven") remain accurate: the TEE feature is still Phases 1–4 complete behind a mock backend, GPU-proven, with Phase 5 outstanding. **Section 6's IONOS specifics are superseded** by later research (2026-06-16). In short: IONOS runs **H200 + DGX B300** (CC-capable), **not** H100/B200; **CPU confidential computing is not exposed to tenants** (their "attestation" marketing means BSI C5 compliance, not a hardware quote); so standing up a CC-On confidential VM with guest attestation on IONOS is a **co-engineering / partnership ask, not self-service**. Do not pitch from Section 6's "just confirm they offer CC and provision" framing.

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
- **`types.rs`** — the *neutral shared rulebook*. Holds `Evidence`, `Policy`, and — crucially — the single canonical `cross_bind_report_data()` function so no two components can compute the cross-binding differently.
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
                                        gpu_report, gpu_report_hash,
                                        report_data = sha256(pk_att‖
                                          gpu_report_hash‖nonce),
                                        cpu_quote[0..32]=report_data,
                                        image_measurement, pk_att, nonce
                                      submit Evidence  ──────────────────────────────►  burn nonce, then
                                                                                        10 checks (fail-closed):
                                                                                        nonce, quote len, cross-bind,
                                                                                        measurement, SKU, CC-On,
                                                                                        prod-TCB, TCB age, validity
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

**(c) Gather evidence — the cross-binding.** The node builds the `Evidence` structure. It serializes the GPU report fields (SKU, `cc_on`, `production_tcb`, `tcb_age_days`) into `gpu_report`, computes `gpu_report_hash = sha256(gpu_report)`, and then the **security linchpin**:

```
report_data[0..32]  = sha256(pk_att ‖ gpu_report_hash ‖ nonce)
report_data[32..64] = 0x00…00
```

This is the **cross-binding**. It fuses the GPU report, the attestation key, and the freshness nonce into a single SHA-256 hash, and stuffs it into the CPU quote's signed `report_data` field. Why it matters: without it, a hostile operator could grab a *genuine* CPU quote from its real confidential VM and a *genuine* GPU report from a *different* CC GPU it controls, present them together, and pass both independent checks — even though they're from two different physical machines. Cross-binding makes them **inseparable**: pairing GPU #2's report with CPU #1's quote produces a hash mismatch, and the hardware signature over `report_data` means the attacker can't forge a fix. (In Phases 1–4 the `cpu_quote` is a synthetic 64-byte blob where bytes 0–63 *are* `report_data`; in Phase 5 it's a real TDX/SNP quote from which `report_data` is extracted. The cross-bind formula is computed by the *one shared* `cross_bind_report_data()` in `types.rs`, so the mock and the real verifier can never diverge.)

**(d) Submit and verify.** The node sends the evidence to the KBS's `request_key()`. The KBS first **burns the nonce** (marks it consumed *before* verifying — so a failed attempt can't be retried with the same nonce), checks it was issued and unexpired, then calls `DefaultVerifier::verify()`, which runs **ten checks in fail-closed order**:

1. Nonce matches the expected nonce.
2. `cpu_quote.len() >= 64`.
3. **Cross-binding:** recompute `report_data`, confirm `cpu_quote[0..32]` matches *and* `cpu_quote[32..64]` is all zeros.
4. Decode `gpu_report` into `GpuReportFields`.
5. **Measurement:** `image_measurement == policy.expected_measurement` (the 48-byte SHA-384 launch measurement matches the provider's pinned value — proof the node runs *exactly* the approved code).
6. **SKU allowlist:** the GPU model is approved.
7. **CC-On:** if required, `cc_on == true`.
8. **Production TCB:** if required, no debug TCB.
9. **TCB age:** `tcb_age_days <= max_tcb_age_days` (firmware isn't dangerously stale).
10. **Validity window:** broken clock fails; `not_before ≤ now ≤ expiry`.

Any failure → `TeeError::VerificationFailed`, no key released.

### Step 3 — The key is released, wrapped (ECIES)

If all ten checks pass, the KBS wraps the DEK to the node's `pk_att` using **ECIES** (Elliptic Curve Integrated Encryption Scheme):

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

The recurring discipline is **fail-closed**: deny by default unless *every* check passes. A mis-set clock (`u64::MAX`) fails closed. An expired policy (`now > expiry`, or `expiry = 0` for instant revocation) fails closed. A mismatched measurement, a disallowed SKU, CC-Off, stale TCB, a cross-bind mismatch, an unknown or stale nonce — all return an error and withhold the DEK. No plaintext is ever written on a failure path.

The supporting promises:

- **Authentication** — the hardware proves the node holds `pk_att_secret` and that the nonce was KBS-issued; cross-binding ties `pk_att` to the GPU report.
- **Integrity** — Poly1305 tags everywhere; tampering breaks decryption.
- **Confidentiality + forward secrecy** — DEK wrapped under ephemeral ECDH; compromising `pk_att_secret` later can't decrypt past captures.
- **Freshness + replay protection** — single-use, TTL-bounded nonces; burned up-front.
- **Two distinct nonces, no overlap** — the 32-byte *KBS nonce* (attestation freshness + cross-binding) and the 16-byte container *nonce_base* (AEAD chunk encryption) never mix, avoiding a false sense of single-nonce safety.
- **Provider control via signed policy** — pin the measurement, allowlist SKUs, require CC-On / production TCB, cap TCB age, set a validity window. Policies are off-chain and signed, so they can be rotated (tighten, revoke) *without re-encrypting the weights*.
- **Capability discovery** — a node advertises `tee-attested` (in registration metadata and the WebSocket handshake) **iff** `HOST_TEE_ENABLED`, so clients select only nodes that will honor encrypted models. Legacy-registry deployments emit no `capabilities` key at all, so they can't accidentally claim TEE support.

---

## 5. What's been proven

The GPU end-to-end test (`/workspace/fabstir-llm-node/tests/tee_e2e.rs`) ran on **real hardware** (TEST_HOST_1 / 3XS-Z, real NVIDIA GPU with CUDA) and exercised the **complete pipeline as shipped** — driving the production entry point `prepare_attested_model` with *no production edits*:

1. **Provider-side offline:** sign a policy (ECDSA via k256, address via `recover_client_address`), encrypt a real 1B-parameter GGUF (`tiny-vicuna-1b.q4_k_m.gguf`) with XChaCha20-Poly1305 in 8 MiB chunks.
2. **Node-side:** validate the policy, attest (mock backend), receive the DEK from `MockKeyBroker`, decrypt to tmpfs.
3. **Hardware proof:** assert plaintext lives *only* on tmpfs (`is_tmpfs`) and round-trips byte-exact.
4. **Real GPU inference:** `LlmEngine::load_model` with `gpu_layers: 99` (all layers on the GPU) on the prompt "The capital of France is" → " Paris, and it is the capital of the Île-de-France region", with `tokens_generated > 0`.
5. **Secure teardown:** unload, `secure_delete`, assert the file is gone.

**Result: 1 passed in 147 seconds.** This proves the security-critical path (encrypt → policy → attest → DEK → decrypt-to-tmpfs → hash-bind → GPU load → infer → secure_delete) is real, not theoretical.

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

---

## 6. What's left — the final 20% (Phase 5)

> **⚠️ The IONOS details in this section are superseded (2026-07-16).** See the currency note at the top of this doc. The hardware/SDK/software points below are still valid; the IONOS *positioning* (candidate vs. default, "just confirm they offer CC") is not — IONOS has H200 + DGX B300, does not expose CPU CC to tenants, and Phase 5 there is a co-engineering ask.

Phase 5 is the hardware-dependent remainder: swap the mocks for real, hardware-rooted components, run on a genuine confidential VM with CC-On, and prove the host truly can't read VRAM.

**The hardware:**

- **Default (decision D-HW):** Azure **NCCadsH100v5-series** — a managed confidential VM (H100 + AMD SEV-SNP, CC-On, attestation handled by Azure). No hardware purchase; resolves the open question of validated H100 silicon.
- **Candidate:** **IONOS** — H200 (validated CC silicon) at ~$3.26/€3 per GPU-hr, with £200 in credits (≈ 50–60 GPU-hr) and a direct support contact. The decisive question to ask them: *"Do you offer a confidential VM (AMD SEV-SNP or Intel TDX) with the GPU in CC-On mode and customer-accessible remote attestation (NVIDIA NRAS or local RIM)?"* If yes, IONOS becomes preferred (budget + hands-on support); if not, fall back to Azure.

**The real components (behind the existing trait boundaries):**

- **`NvidiaCcProvider`** — real GPU report + real CPU TDX/SNP quote (wraps `nvtrust` / the NVIDIA Attestation SDK).
- **Real `AttestationVerifier`** — self-hosted **RIM** (Reference Integrity Measurements) verification, CPU/GPU certificate chains, production-TCB + CC=On checks, cross-bind extracted from *real* `report_data`. (Self-hosted RIM is the production target so NVIDIA's **NRAS** isn't permanently in the critical path.)
- **Real KBS** — no backdoors, a trusted host-independent clock, nonces bound to `model_id` (preventing cross-model replay), one-time-use, TTL-bounded.

**Vendor specs to pin (open questions):** NVIDIA Attestation SDK version, CC-driver branch, guest-kernel version; the exact byte sequence of `gpu_report`, whether `gpu_report_hash` covers the full DER blob or parsed fields, nonce composition, and DER/PEM parsing libraries.

**Reproducible measured-CVM image (D6):** Fabstir publishes deterministic node-CVM images so the launch measurement is stable; providers pin those reference measurements into `SignedModelPolicy.expected_measurement`. This is flagged as the item *most likely to slip* — if the image builds non-deterministically, providers can't pin a measurement and the whole attestation guarantee collapses.

**Deploy:** VFIO GPU passthrough; enable CC + ready-state (`nvidia-smi conf-compute -srs 1`); decrypted weights *only* to in-CVM tmpfs (`TEE_DECRYPT_DIR`), sized for the full multi-GB model plus KV cache plus headroom.

**Carried-forward hardening (must land in Phase 5):**

- **`pk_att` hardware binding** — today the mock simply echoes `pk_att`; the real verifier MUST confirm `pk_att` against the hardware quote and validate it as a canonical 33-byte compressed point. *Until this lands, Phases 1–4 do not meet the full threat model.*
- DEK / key-material `Zeroize`; mutex-poison recovery; cross-bind length-prefix + domain tag.
- **Close TOCTOU** — fd-based load / `F_ADD_SEALS` / re-verify-before-mmap.

**Final proof:** validate the mock→real pipeline on real CC hardware, run a `/security-review` of the full Phase-5 wiring, and demonstrate "host cannot read VRAM" with CC-On enabled.

The handoff lives in **`PHASE-4-TO-5-READINESS.md`**, sized for the Phase-5 team (Azure or IONOS) to execute.

**Where things stand:** the software security perimeter is built, clippy-clean, and *proven on real GPU hardware* end-to-end behind a mock backend. Version `8.30.0-tee-confidential-inference` is committable under the relaxed gate. What remains is anchoring that perimeter to a hardware root of trust — real attestation on a confidential VM with CC-On — which is the difference between "the pipeline works" and "the host genuinely cannot steal the weights."

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
- **Report data:** the 64-byte signed CPU-quote field carrying the cross-binding hash (bytes 0–31) plus zero padding (bytes 32–63).
- **RIM (Reference Integrity Measurement):** NVIDIA's authentic-firmware baseline used by verifiers.
- **S5:** decentralized storage holding the encrypted container.
- **Secure delete:** single-pass zeroize (RAM is TEE-encrypted) then unlink; idempotent.
- **Signed model policy:** the provider-signed off-chain authorization to release the DEK for a model.
- **Silent-truncation vector:** dropping chunks and editing `num_chunks`; defeated by binding the full header into every chunk's AAD.
- **SKU:** a GPU model identifier (e.g. H100, H200) the provider can allowlist.
- **TCB (Trusted Computing Base):** the security-critical firmware/microcode/kernel the TEE relies on; "age" measures patch staleness.
- **`tee-attested`:** a capability string advertised iff `HOST_TEE_ENABLED`, letting clients select TEE-honoring nodes.
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