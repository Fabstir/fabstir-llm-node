# Training M0 — private LoRA fine-tuning as a marketplace job

Status (2026-08-26): **demonstrated end to end.** Complete fine-tunes have run on
TEST_HOST_1 under `TRAIN_MOCK_CHAIN`: encrypted dataset in, tokens counted exactly (8701,
three independent counts agreeing on every run), QLoRA train, GGUF conversion, encrypted
adapter out. The adapter demonstrably changed the model: asked about an invented "Meridian
Ledger" protocol, the tuned model answers with the dataset's facts ("512 seconds", "89")
where the base model calls the subject fictional. The same adapter was then staged and
served from host2's L40S, a different GPU generation from the Blackwell card that trained
it; the only thing that moved between the hosts was the capability CID.

The pre-escrow surface a client needs is served. `GET /v1/training/advert` publishes
`tokenizerSha256`, `baseServingModelId` and `alphas` plus the bounds, `modelId` and
`pricePerToken` — a route of its own because the LTX `AllowListBundle` cannot carry them.
`GET /v1/training/tokenizer` serves the tokenizer this host counts with, verified against
the template pin at boot; see "the tokenizer contract" below.

What remains before this can take **paid** jobs:

1. **No registered training model id.** `TRAINING_MODEL_ID` below is still a placeholder.
   It needs a ModelRegistry entry plus a `NodeRegistry` advert with its per-model price;
   the node never registers itself. The T7 paid gates follow registration, not before.
2. **No durable run record or deposit reclaim** (F5.5). The client holds the only copy of
   the capability CID, and one browser-profile loss has already cost a run record; the
   rescue is under serve-back below.
3. **The disconnect double-complete guard.** The inference path's disconnect settlement
   does not skip training-tracker jobs and has sent a real transaction from under the
   mock chain twice. Known, recorded, not yet fixed.

A client submits an encrypted JSONL dataset, the node trains a LoRA adapter against a
pinned base model, settles on-chain per slice, and returns an encrypted adapter the client
alone can decrypt. Zero smart-contract changes: it rides the existing session-job
machinery with its own registered model id.

## What the host can see (read this before writing any privacy copy)

**The host decrypts the dataset in order to train on it.** The capability CID in the job
payload *is* the decryption key (`parse_capability_cid` in `src/ltx/input_image.rs` decodes
`ct_hash`, a 32-byte key, padding and the plaintext CID), so there is no key-release step to
gate. `src/training/staging.rs` fetches each shard, decrypts it, and writes the **plaintext
to `TRAINING_STAGING_ROOT` on the host's filesystem** for the sidecar to read.

**The host also produces the adapter in the clear.** The sidecar writes it to
`TRAINING_WORK_ROOT`; the node reads it back off that path (`core.rs:1158`) and only then
mints a random key and encrypts it (`artifact.rs:75`). The adapter is derived from the
dataset, so leaving it exposed partially undoes the dataset's protection rather than merely
failing to extend it.

So the honest statement is: **one host operator can read every byte of a customer's training
data and holds the adapter it produced.** The public cannot — the on-chain commitment carries
`manifestSha256` hashes, never capabilities, so no key touches the chain.

What is true today: encrypted in transit, encrypted at rest in storage, adapter returned
encrypted, every slice settled on-chain against a proof. What is **not** true today: that the
host cannot see it. Do not write copy implying otherwise; see
`docs/development/DESIGN-CONFIDENTIAL-TRAINING.md` for what would close it.

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
| Template hash | `0x257c3bc96e0b53e285dacf58efdd7e93a7186b3fd02a841c8b7f0955d7ed2f3b` **(CURRENT — see the rule below)** |
| Base repo / revision | `unsloth/Qwen3.8-27B` @ `3ea932cee0a432ae86e9c7826cbe8aef52323a28` |
| `base.ggufArch` / `base.numLayers` | `qwen35` / `64` |
| `tokenizerSha256` | `0x0997f410c57a1f4e53b09e4be8f4a172d90edd9564368fb0847030937229b9f3` |
| `baseServingModelId` | `0x892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650` |

**Stop calling any template hash "final"** — this document did, twice, and both were
superseded within a day (`0x43e1efbd…` by T1.4's `base.files`; `0xec40b499…` by restoring
`base.ggufArch`/`numLayers`, which a T1.4 copy-back had silently clobbered and which cost
runs 1123 and 1124 their GGUF conversion). The rule that actually holds: the hash is
canonical **per template content**, every change to it is a changelogged, breaking event,
and clients must read it from the advert at run time — never pin it. Both client teams
already comply.

**Template copies must be diffed in BOTH directions.** The clobber happened because a
deployed copy (from a pre-`ggufArch` tarball) was copied back over the repo copy after
T1.4 ran on it — the semantic diff was applied to the outbound edit and skipped on the
copy-back. `python3 -c` one-liner before any template copy: compare `base` key sets.

The template hash is `SHA256` over the template's canonical bytes. It is load-bearing: it
feeds `inputCommitment` and `sigDigest` in the on-chain attestation, so any template edit
moves it and is a breaking, changelogged event. Since T1.4 (2026-08-26) `base.files`
carries 21 pins — 18 safetensors shards plus the index, `config.json` and
`generation_config.json` — so the sidecar's weight verification is non-vacuous.

`baseServingModelId` is the **serving** GGUF already registered on host2, which is a
different artifact from the **training** base above. The training base is safetensors plus
`tokenizer.json`; the serving base is the `.gguf`. Confusing the two is the single easiest
mistake in this feature.

The template lives at `templates/train-qlora-qwen38-27b-v1/v1.json` and since T1.4 it
carries the full `base.files` pin list, making it both node-loadable and sidecar-loadable.
The sidecar's `pins.py` refuses at boot a template missing `base.ggufArch`,
`base.numLayers` or sane bounds — added after a copy-back clobbered exactly those fields
(see the diff-both-directions rule above) and two full runs failed at the conversion
step, hours after their training had succeeded.

## Running it

```
TRAIN_ENABLED=true
TRAINER_SOCKET=/var/run/fabstir-trainer/trainer.sock   # must equal the deploy doc's value; compose environment overrides a stale .env line SILENTLY
TRAINING_STAGING_ROOT=/var/lib/fabstir/training/staging
TRAINING_WORK_ROOT=/var/lib/fabstir/training/work
TRAINING_TEMPLATE_PATH=/opt/fabstir/templates/train-qlora-qwen38-27b-v1/v1.json
TRAINING_TOKENIZER_PATH=…/tokenizer.json   # OPTIONAL: needed for counting, NOT for serve-back
TRAINING_MODEL_ID=0x…              # the TRAINING model id, from the registration record
TRAINING_PRICE_PER_TOKEN=10000     # must equal the on-chain per-model price — see the mock-chain trap
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


## Serve-back: eviction, portability, and the traps when testing it

**A reconnect must re-send `lora`, or the session silently uses the base model.** The
node keeps no state across connections, by design: every WebSocket init mints a fresh
key and stages fresh from S5. So after a node restart (or any reconnect) a client that
re-sends `lora` simply re-stages and carries on. A client that OMITS it gets
`SessionAdapter::None`, the resolve path short-circuits before touching the registry,
and the prompt is answered from the BASE MODEL with no error and no frame.

One consequence for client authors: an ordinary socket drop is fine if every re-init
re-sends `lora`, but a **rebuilt session manager** (page reload, or an SDK rebuild when a
wallet reconnects) loses the pointer and cannot re-send what it no longer holds. The fix
is one layer up: persist `manifestCID` + `manifestSha256` + the file name alongside the
conversation the client already rebuilds. None of it is secret and the client supplies it
on every init anyway.

That is not fixable node-side. The protocol has no notion of "this session previously
had an adapter" — that state would have to be keyed on something the client supplies,
which is exactly the wire-settable key the isolation defects came from. The fail-closed
guarantee applies only to a connection that ASKED for an adapter; one that did not ask
has nothing to fail closed about.

**The capability CID is the ONLY key, and the node keeps no copy.** Non-custodial cuts
both ways: lose the client-side record — one browser-profile reset has already done it —
and the adapter is unrecoverable from S5. The rescue that worked: the node's `STAGED` log
lines carry the manifest sha256, so an adapter file the client still holds locally can be
verified against it and republished with `publish-adapter` below, minting a fresh
capability CID. This is why F5.5's durable run record is on the remaining list.

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

**The `🧬` line is the oracle for "the adapter had no effect".** Since v8.53.1 every
session init logs exactly one of `🧬 Session init carries lora … registry key … minted`
or `🧬 Session init carries NO lora … base model only`. Check it FIRST: it splits the
failure space between "client never sent `lora`" (the reconnect trap above) and
"staged and applied, but the adapter is too light to notice" (the alpha rule under
traps). Three consecutive runs were once mis-diagnosed as a staging failure when every
one had staged fine and the adapter was a 0.34% perturbation.

**Size client timeouts from adapter bytes, not a constant.** A rank-32 adapter for the
27B base is 638 MB in 26 shards and stages in 76–84 s through the bridge (~8 MB/s
observed), so a 30 s client timeout guarantees a mid-stage abort on any adapter of real
rank. (The `aead::Error` saga that pointed here turned out to be an SDK defect — one
crypto key shared across every flow, fixed in sdk-core 1.38.3 — but the timeout
arithmetic stands on its own.)

**Adapters are host-portable by construction.** Serve-back needs the training env block,
a loadable template and the base GGUF; not training hardware, not the sidecar, not the
host that trained it. The evidence adapter was trained on TEST_HOST_1's Blackwell card
and served from host2's Ada L40S the same evening.

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
cargo test --test training_api_tests -- --test-threads=1   # 155
cargo test --test training_tests -- --test-threads=1       #  10, the cross-language vectors
RISC0_SKIP_BUILD=1 cargo test --lib training::             #   9, redaction + advert units

cd fabstir-trainer && .venv/bin/pytest tests/ -q           # 139
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
its base check. Serve-back must be exercised against a real chain session. Two more edges
the mock does NOT cover: the CLIENT still reads the real chain, so
`TRAINING_PRICE_PER_TOKEN` must equal the registered per-model price or the client refuses
with `ESTIMATE_MISMATCH` before the node sees anything; and the disconnect settlement path
is not mocked, so a wedged or disconnected client makes the node send a **real**
transaction (the double-complete guard in the status list is the open fix).

**The counting fixture needs no GPU.** Its generator imports the tokenizers library and
nothing else. An earlier usage line in that script said otherwise and blocked the SDK for
days on a dependency that did not exist.

**llama-cpp-2 never frees a LoRA adapter** — no `Drop`, no free call — so every adapter
load leaks. Serve-back loads per request, which multiplies it. The end-to-end runs never
soaked it long enough to measure; holding the adapter per session is the recorded fix.

**The serve-back registry key is minted server-side** and never read from the wire. Two
CRITICAL isolation defects came from keying it on a client-supplied session id, once for
eviction and once for resolution. Do not reintroduce a wire-derived key.

**Client-facing text goes through `src/training/redact.rs`.** A foreign error's `Display`
is never echoed: reqwest writes the request URL into it, and RPC URLs carry API keys. Use
`opaque` for foreign errors, `echo` for client strings, `echo_error` where the diagnosis
sits at the tail. Patching individual sites did not converge; the choke point did.

**Raising LoRA rank WITHOUT raising alpha WEAKENS the adapter.** PEFT scales the learned
update by `alpha/r`, so doubling `r` alone halves the applied weight of everything
learned. Rank 8 / alpha 16 / lr 1e-4 produced an adapter that loaded, applied and changed
nothing observable (~0.34% perturbation); rank 32 / alpha 64 / lr 5e-4 / 5 epochs — the
ratio held at 2.0 — is the recipe that surfaced the dataset's facts (~2.1%). Keep the
alpha:rank ratio when scaling rank, and treat "applies cleanly but answers like base" as
a training-strength symptom, not a serve-back bug; the `🧬` line settles which you have.

**A think-only turn looks like a freeze and is not one.** Qwen3.8 can put its entire
answer inside the `<think>` block and then emit EOS: the node logs a completed generation
(42 tokens, `stop_reason=eos_token`, about 2 s) while a UI that hides thinking shows
"Thinking…" forever. Check the node log before blaming inference or VRAM. Re-ask, or send
per-request thinking `disabled`; rendering a think-only completion is a client concern
(session 1140 is the repro).

**Editing a host `.env` by paste is how one deploy broke three ways.** The file is parsed
by the node, not a shell: an inline `# comment` becomes part of the value, and a pasted
annotation left a literal Unicode `…` inside `TRAINING_MODEL_ID`, a fatal hex parse at
boot. Values are full-width or absent, never annotated. From the same evening: an
8.46-era `.env` predates `USDC_TOKEN`, without which the all-or-nothing wiring quietly
prints `training disabled`; and a host may mount a template SUBDIRECTORY (host2 mounts
`./templates/ltx`), so the training template must be copied inside it, not beside it.

## Where the rest lives

Several of these are **not** version controlled, which is itself worth knowing.

| Document | Location | Tracked? |
|---|---|---|
| Client wire contract | `docs/sdk-reference/DESIGN-TRAINING-M0-INTERFACE.md` | **no** |
| Node/sidecar contract | `fabstir-trainer/docs/CONTRACT-TRAINING-SERVICE.md` | yes |
| Build tracker and round records | `docs/development/EXECUTION-PLAN-TRAINING-M0.md` | **no** |
| Design decisions | `docs/development/IMPLEMENTATION-TRAINING-M0.md` | **no** |
| GPU runbook | `docs/archive/t6-training-gpu-command-sheet.txt` | **no** |
| First-fine-tune runbook and recipe | `docs/development/EXECUTION-CUSTOM-FINETUNE.md` | **no** |

The client wire contract being untracked is a live gap: its own changelog is the only
record of its history, and nothing attests that record is complete. Node and SDK currently
exchange sha256 hashes of each version in correspondence as a stopgap.
