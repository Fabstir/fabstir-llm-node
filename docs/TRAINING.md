# Training M0 — private LoRA fine-tuning as a marketplace job

Status: the node's training and serve-back paths are built and pinned by tests, and the
pre-escrow surface a client needs is now served. What remains before anything can run end
to end is deployment rather than node code:

1. ~~No training advert.~~ **Built.** `GET /v1/training/advert` publishes
   `tokenizerSha256`, `baseServingModelId` and `alphas` plus the bounds, `modelId` and
   `pricePerToken`. It is a route of its own because the LTX `AllowListBundle` cannot carry
   them: that bundle is built from ComfyUI workflow graphs (`src/ltx/template.rs`), and the
   training template is a different shape.
2. ~~No tokenizer route.~~ **Built.** `GET /v1/training/tokenizer` serves the tokenizer this
   host counts with, verified against the template pin at boot. See "the tokenizer contract"
   below: the client MUST hash what it fetches and compare against the advert.
3. **No registered training model id.** `TRAINING_MODEL_ID` below is a placeholder. It
   needs a ModelRegistry entry plus a `NodeRegistry` advert with its per-model price; the
   node never registers itself. Host address and WS endpoint follow that, not before.

GPU end-to-end (T6) and paid gates (T7) remain outstanding after that.

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
TRAINING_TOKENIZER_PATH=…/tokenizer.json   # OPTIONAL: needed for counting, NOT for serve-back
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


## Serve-back: what evicts an adapter, and two traps when testing it

**Publishing a test adapter.** A serve-back `manifestCID` must be a `u`-multibase
CAPABILITY CID (base64url of the `0xae` envelope carrying the ciphertext hash, the
XChaCha20-Poly1305 key and the plaintext CID). A plain `PUT /s5/fs/…` to the s5-bridge
returns a bare `blob…` content address with **no key in it**, which serve-back refuses
(`src/ltx/input_image.rs`). The LTX allow-list bundle uses `blob…` happily because it is
fetched as plaintext; an adapter never is. Do **not** widen the validator to accept both.

Use the node's own publisher rather than a script, so the sharding, encryption and
canonical manifest are the same tested code a real training run uses:

```bash
cargo build --release --features dev-tools --bin publish-adapter
# run it where the bridge resolves, i.e. inside the node's container:
docker cp target/release/publish-adapter llm-node-prod:/tmp/
docker cp adapter.gguf llm-node-prod:/tmp/
docker compose -f docker-compose.prod.yml exec llm-node /tmp/publish-adapter /tmp/adapter.gguf
```

It is behind `dev-tools` because it links the whole lib; leaving it in the default set
would relink a second full binary on every `cargo build`.


**Session end is the ONLY path that evicts a live adapter.** There is no TTL, no idle
timeout, no memory-pressure eviction and no LRU: `src/training/serve.rs` contains no
timer of any kind. At `ADAPTER_MAX_LIVE` (16) the node **refuses the new stage** rather
than evicting an existing one, so no session can lose its adapter because another
session wanted one. A `Reservation`'s `Drop` removes its own failed stage, never
another's.

The one exception worth telling users about: **a node restart clears everything.** The
registry is in-memory, and `sweep_orphan_adapter_dirs` deletes staged files at boot. So
a chat that was working an hour ago will refuse if the host restarted in between.

Two traps when testing serve-back through a browser, both of which make the test
silently measure nothing while appearing to pass:

* **The concurrent session must be a SECOND BROWSER TAB.** Navigating to it inside a
  single-page app unmounts the first chat, which ends its session, so the two sessions
  are never concurrent and the isolation test proves nothing. Not UI-specific; anyone
  driving this through a browser hits it.
* **Container logs and in-container `ls` are UTC; screenshots carry local time** (BST in
  summer). On the naive reading an eviction can appear to precede the isolation test that
  actually ran before it. Realign before drawing conclusions about ordering.

Note also that isolation does not depend on that ordering: a session whose init carries
no `lora` never reaches the registry at all, because the connection-local
`SessionAdapter` is `None` and the first match arm in `src/api/server.rs` returns no
adapter without consulting it. Ordering decides the EVICTION evidence, not the
isolation evidence.

## The tokenizer contract

A client counts tokens **before** escrow. If its count disagrees with the host's, the result
is an `ESTIMATE_MISMATCH` on a session the customer has already funded, so the client must
count with exactly the bytes the host bills with.

That is guaranteed by the **pin**, not by the source:

1. `GET /v1/training/advert` returns `template.tokenizerSha256` and `tokenizer.sha256`.
2. `GET /v1/training/tokenizer` returns the bytes.
3. The client hashes what it received and compares. **A fetch that skips the comparison
   defeats the mechanism entirely** — the hash is published next to the URL for this reason.

**`TRAINING_TOKENIZER_PATH` is optional on purpose.** Serve-back (E.2) loads an adapter and
counts nothing, so a host that only serves adapters is not made to carry a 12 MB counting
asset. If it is unset, or set to a file that fails the pin, training still wires and
serve-back still works; the advert reports `tokenizer.available: false` with reason
`notServed`, and `GET /v1/training/tokenizer` returns **503** (the route exists, it has
nothing verified to serve) rather than 404. What never happens is serving unverified bytes.

Node side, the file is read and verified against `template.tokenizerSha256` at boot, then
held resident. Resident rather than re-read per request so that nothing can swap the file
underneath a request after the advertised hash was computed. A host whose tokenizer does not
match its template **fails training wiring and serves no training at all**, rather than
serving a tokenizer it does not bill with.

The route carries a strong `ETag` and `Cache-Control: immutable`, and honours `If-None-Match`
(including a comma-separated list) with a 304. That matters because it is a public
unauthenticated route serving roughly 12 MB.

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
