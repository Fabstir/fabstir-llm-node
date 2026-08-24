# Training M0 — private LoRA fine-tuning as a marketplace job

Status: node side complete. GPU end-to-end (T6) and paid gates (T7) outstanding.

A client submits an encrypted JSONL dataset, the node trains a LoRA adapter against a
pinned base model, settles on-chain per slice, and returns an encrypted adapter the client
alone can decrypt. Zero smart-contract changes: it rides the existing session-job
machinery with its own registered model id.

## Shape

Two processes, and the split is load bearing.

**The node** (this repo, `src/training/`) does everything on the money and trust path:
accept-time validation against the chain, dataset staging and hash verification, the slice
loop, proof submission, settlement, and serve-back. It never trains.

**The sidecar** (`fabstir-trainer`, its own repo) does the training and only the training.
It counts tokens, scans content, and runs the loop. It is reached over a **Unix domain
socket** and is never exposed to a network.

They share two directories **by absolute path**, and the handover works by one process
deleting what the other wrote. That is the whole interface besides the socket.

## The pinned values

Copy these, never retype them. This is the same rule the LTX model ids follow, and it
exists because a retyped id has already cost this project a debugging session.

| What | Value |
|---|---|
| Template id | `train-qlora-qwen38-27b-v1` |
| Template hash | `0x43e1efbd802c7cd761dfd2ff24ff4e1a48b47d08a6ee12d24936ea3aa487c904` **(PROVISIONAL)** |

The template hash is `SHA256` over the template's canonical bytes, so it **will change** when `base.files` is added at T1.4. It is load-bearing: it feeds `inputCommitment` and `sigDigest` in the on-chain attestation. Do not quote it outside this repo, and do not register it, until the file list lands.

| Base repo / revision | `unsloth/Qwen3.8-27B` @ `3ea932cee0a432ae86e9c7826cbe8aef52323a28` |
| `tokenizerSha256` | `0x0997f410c57a1f4e53b09e4be8f4a172d90edd9564368fb0847030937229b9f3` |
| `baseServingModelId` | `0x892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650` |

`baseServingModelId` is the **serving** GGUF already registered on host2, which is a
different artifact from the **training** base above. The training base is safetensors plus
`tokenizer.json`; the serving base is the `.gguf`. Confusing the two is the single easiest
mistake in this feature.

The template lives at `templates/train-qlora-qwen38-27b-v1/v1.json`. It deliberately omits
`base.files`, the per-file safetensors sha256 list, which needs the weights provisioned.
That makes it node-loadable and **not** sidecar-loadable, which is correct: serve-back
needs no sidecar, and a real training run must not start on unverified weights.

## Running it

```
TRAIN_ENABLED=true
TRAINER_SOCKET=/var/run/fabstir/trainer.sock
TRAINING_STAGING_ROOT=/var/lib/fabstir/training/staging
TRAINING_WORK_ROOT=/var/lib/fabstir/training/work
TRAINING_TEMPLATE_PATH=/opt/fabstir/templates/train-qlora-qwen38-27b-v1/v1.json
TRAINING_MODEL_ID=0x…              # the TRAINING model id, from the registration record
TRAINING_PRICE_PER_TOKEN=904       # must equal the registered price
TRAINING_ALLOWLIST_VERSION=1
TRAIN_JOB_TIMEOUT_SECS=12600
TRAIN_WS_WRITE_TIMEOUT_SECS=900
TRAIN_ACCEPT_COOLDOWN_SECS=60
TRAINING_RATE_LIMIT_TOKENS_PER_SEC=10000
TRAINER_CLIENT_TIMEOUT_SECS=600    # optional, bounds every call to the sidecar
```

`HOST_PRIVATE_KEY` and `USDC_TOKEN` are also hard required; without either the whole
feature disables itself and `GET /v1/training/capacity` answers 404. The wiring is
all-or-nothing, and all three disable paths print a line ending `training disabled`, so
grep for that rather than for any one message.

**Serve-back needs no sidecar and no training hardware.** The startup wiring never
connects to the sidecar, and serve-back staging does not reference it once. A host with
`TRAIN_ENABLED=true`, the env block and a loadable template will stage adapters, publish
its training bundle section, and honestly report `available: false` on the capacity route.
Answering with an adapter does need the base model loaded, so that part wants the GGUF.

`TRAIN_ENABLED` alone does **not** trigger cross-slot exclusion. Only an active training
run takes the shared GPU permit.

## Containers

The design pins the trainer as a container image by digest. Three things must line up, and
they are the whole of the difficulty.

The **socket must be on a real Linux filesystem**. A Unix domain socket does not work over
9p or virtiofs, so anything under a Windows mount fails, and fails silently rather than
loudly.

The **two roots must be mounted at identical paths in both containers**. They exchange work
by absolute path; mounting the same volume at different points looks right and does nothing.

Both containers must **run as the same uid**, or share a gid with write permission on both
roots. Consumption is signalled by deletion, and removing a directory entry needs write
permission on the containing directory. Mismatched uids stall the first handover silently.

## Tests and gates

```bash
cargo test --test training_api_tests -- --test-threads=1   # 150
cargo test --test training_tests -- --test-threads=1       #  10, the cross-language vectors
RISC0_SKIP_BUILD=1 cargo test --lib training::             #   3, the redaction unit tests

cd fabstir-trainer && .venv/bin/pytest tests/ -q           # 111
```

**The counting parity gate is opt-in and matters.** The sidecar verifies its counting
against the node's frozen fixture only when handed the pinned tokenizer:

```bash
FABSTIR_PINNED_TOKENIZER=/path/to/tokenizer.json .venv/bin/pytest tests/ -q
```

Without it the test skips; with the **wrong** file it fails rather than skipping, because a
mismatched tokenizer would make the suite green while proving the opposite. This is the
check that makes node and client counting parity measured rather than assumed, and a
disagreement here is a `DECLARED_TOKENS_MISMATCH` on a funded job.

## Traps that have already cost time

**The in-repo client ABI is stale for `sessionJobs`.** It carries a phantom `requester` at
index 2 and shifts every field after it, so `host` decodes as the payment token and
`deposit` as the price. Never verify that struct against the ABI. `accept.rs` reads raw
words at fixed offsets deliberately, pinned by a real `eth_call` fixture, and the SDK
independently confirmed the same layout byte for byte.

**`TRAIN_MOCK_CHAIN` is authoritative**, and only exactly `true` or `1` enables it. In mock
mode the session model reads back as the *training* model id, so serve-back can never pass
its base check. Serve-back must be exercised against a real chain session.

**The counting fixture needs no GPU.** Its generator imports the tokenizers library and
nothing else. An earlier usage line in that script said otherwise and blocked the SDK for
days on a dependency that did not exist.

**llama-cpp-2 never frees a LoRA adapter** — no `Drop`, no free call — so every adapter
load leaks. Serve-back loads per request, which multiplies it. Measuring that is a T6 item
and holding the adapter per session is the recorded fix.

**The serve-back registry key is minted server-side** and never read from the wire. Two
CRITICAL isolation defects came from keying it on a client-supplied session id, once for
eviction and once for resolution. Do not reintroduce a wire-derived key.

**Client-facing text goes through `src/training/redact.rs`.** A foreign error's `Display`
is never echoed: reqwest writes the request URL into it, and RPC URLs carry API keys. Use
`opaque` for foreign errors, `echo` for client strings, `echo_error` where the diagnosis
sits at the tail. Patching individual sites did not converge; the choke point did.

## Where the rest lives

Several of these are **not** version controlled, which is itself worth knowing.

| Document | Location | Tracked? |
|---|---|---|
| Client wire contract | `docs/sdk-reference/DESIGN-TRAINING-M0-INTERFACE.md` | **no** |
| Node/sidecar contract | `fabstir-trainer/docs/CONTRACT-TRAINING-SERVICE.md` | yes |
| Build tracker and round records | `docs/development/EXECUTION-PLAN-TRAINING-M0.md` | **no** |
| Design decisions | `docs/development/IMPLEMENTATION-TRAINING-M0.md` | **no** |
| GPU runbook | `docs/archive/t6-training-gpu-command-sheet.txt` | **no** |

The client wire contract being untracked is a live gap: its own changelog is the only
record of its history, and nothing attests that record is complete. Node and SDK currently
exchange sha256 hashes of each version in correspondence as a stopgap.
