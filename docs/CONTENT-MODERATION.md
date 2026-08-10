# Content moderation

How content moderation works on the Fabstir network: what runs today, what is
being built, and what an interface built on top of it has to handle.

**Status, plainly.** There are two moderation layers and they are in very
different states. The **hash-matching layer** — deterministic matching of
media against block lists of known illegal content — is **built and
operating**: every video transcode is scanned and receives a recorded
verdict, though enforcement (actually withholding content) is staged behind
a host-side switch and not yet turned on. The **policy layer** — AI
judgment of novel content — is built but **cannot be switched on**; section
6 says exactly why, and that limitation does not affect the hash layer.
If you are building a UI against this, read sections 1a, 4 and 5.

---

## 1. The shape of the system

Moderation runs as a **sidecar**: a separate, pinned process next to the node
rather than code inside it. The node hands it a path to a file and gets back a
verdict.

```
   node  ──POST /v1/moderate {"sourcePath": …}──▶  moderation sidecar
         ◀───────── verdict or error ──────────    (pinned image, own GPU work)
```

Three properties matter more than the mechanism:

**It is pinned, not floating.** The sidecar runs from a specific container
image, recorded by digest in a signed manifest. The manifest's hash
(`bundleHash`) identifies the exact policy-and-model combination that produced
any given verdict. Two hosts running the same bundle are expected to reach the
same answer on the same file; a verdict without its `bundleHash` is not
meaningful, because it cannot be reproduced.

**It talks over a Unix socket, not the network.** The sidecar is not reachable
off-host. It is not an API you can call, and it is not a service the network
exposes.

**It never sees anything the host cannot already see.** Moderation happens where
the plaintext already exists on the host during processing. It does not create
new access to user content; it reads what is already there.

## 1a. The hash-matching layer (operating)

Separately from the sidecar above, the media pipeline carries a
**deterministic** moderation layer that involves no AI judgment at all.
During every video transcode, sampled keyframes are handed to the node,
which matches them against **block lists of known illegal content**: exact
matching on SHA-256 of source files, and perceptual matching (PDQ) on the
keyframes themselves, which survives re-encoding. Lists are supplied by the
host operator from documented evidence procedures; when feeds from
recognised reporting bodies are adopted they ride an encrypted store, never
plain files.

Every transcode job receives a verdict — `cleared`, `blocked` or `flagged`
— recorded against the job and carried on the completion message a client
receives. Every verdict is logged with a fingerprint of the exact list it
was produced against, so a `cleared` is always attributable. An **absent
verdict means "not scanned", never "clean"** — the same rule as the
evidence sink below.

Three properties to rely on:

- **Fail-closed everywhere.** If the list cannot be loaded, frames cannot
  be scanned, or the scan service is unreachable, the outcome is a hold,
  not a pass. A broken configuration degrades the host visibly (its health
  endpoint says so) rather than silently weakening moderation.
- **Three independent gates** close on a non-cleared verdict: the node
  withholds completion and billing, the transcoder withholds the upload of
  playable output, and the client SDK refuses the publish step. Each is
  fail-closed on its own inputs. All three currently run in **observing
  mode** — verdicts are produced and recorded, nothing is withheld yet —
  with enforcement staged deliberately after a soak period.
- **Matched material is preserved, and humans decide.** A genuine match
  quarantines the evidence; nothing is auto-reported. A reviewer checks a
  case's provenance (which list produced the hit) before anything is filed
  with the applicable reporting body.

Wire-level shapes (the completion's `moderation` field and the hold error
codes) are documented in `WEBSOCKET_API_SDK_GUIDE.md`.

## 2. What the policy sidecar looks at

(Sections 2 and 3 describe the **policy layer** — the AI sidecar of
section 1 — not the hash layer above.)

Video is **sampled**, not watched frame by frame. The sampler takes every
I-frame plus a one-frame-per-second grid, which catches cuts and scene changes
without decoding everything.

Sampled frames go through a fast screen, and anything the screen flags goes to a
slower judge that evaluates it against every policy. The judge costs roughly
**0.64 seconds per frame**. For scale: sampling and screening a two-hour title
takes about **84 seconds**.

Some honest limits on coverage:

- **Audio is not examined.** Not sampled, not judged. A later milestone.
- **Still images are unproven.** Whether the sampler handles PNG, JPEG and WebP
  correctly has not been measured. Until it is, image-input paths are not
  covered.
- **Only the first video track is read.** A file carrying a second, hidden video
  track would have that track ignored.
- **Text prompts are not moderated.** This is about pixels.

## 3. What comes back

The sidecar returns one of two things: a **verdict**, or an **error explaining
why it could not produce one**. Both are normal outcomes and callers must handle
both.

A response is a verdict **only if** it carries a `verdict` field and no `error`
field. HTTP status is not the discriminator — several errors are returned with
`200`, because the request succeeded even though the analysis did not.

The verdicts:

| Verdict | Meaning |
|---|---|
| `ALLOW` | Nothing found against any policy |
| `BLOCK_ILLEGAL` | Matched a prohibited category |
| `BLOCK_UNRESOLVED` | Could not reach a confident answer |

The error kinds, all of which mean *no verdict was produced*:

| Kind | Cause |
|---|---|
| `SOURCE_NOT_FOUND` | The file is not there |
| `SOURCE_UNREADABLE` | It is there but cannot be read |
| `SOURCE_INVALID` | Not a media file it can parse |
| `SOURCE_OUTSIDE_ROOT` | Path outside the permitted directory (usually a misconfigured mount) |
| `SOURCE_MUTATED` | The file changed while being read |
| `PIPELINE_FAILURE` | The analysis itself failed |
| `CLIENT_GONE` | The caller disconnected while waiting |
| `CONTENT_ID_MISMATCH` | The file is not the one the caller named |

A `5xx` always means a genuine server fault and takes precedence over anything
in the body.

Successful scans are recorded to an evidence sink as
`{contentId, report, scoreRecord}`, where `contentId` is the SHA-256 of the file
content. **Failed scans write no row at all** — an absent record means "not
scanned", never "scanned and clean".

## 4. What runs today

Be precise about this, because the three paths differ.

| Path | Moderation today |
|---|---|
| **Transcode / media pipeline — hash matching** | **Operating.** Every transcode is scanned against the loaded block lists and a verdict is recorded and returned. Enforcement (withholding non-cleared content) is staged behind host-side switches, currently off |
| **Transcode / media pipeline — policy scan** | Built, off (section 6) |
| **AI video generation (LTX)** | **None.** No moderation call exists anywhere on this path |
| **Audio, text prompts** | None |

The AI-generation path deserves emphasis because it is easy to assume otherwise:
today, a video generated through the Blender extension or any other LTX client
is **not moderated at any point**. What governs that path currently is the
acceptable-use terms users agree to, and the fact that access is invite-only.

## 5. What it will be, and what a UI must handle

The next stage adds moderation to the AI-generation path. Two things about its
design will shape any interface built on it.

**It is retrospective, not preventive.** The scan happens *after* the render is
produced, delivered and paid for. It runs off the critical path so that
moderation cannot delay, block or fail a render. It produces a **record**, not a
decision. Nothing a user does is refused because of it.

For a UI, that means:

- There is **no moderation state to show at generation time**. No "pending
  review", no spinner, no gate. The render completes exactly as it does now.
- A verdict may arrive **seconds to minutes after** delivery, and may never
  arrive at all — the scan can be shed under load or fail, and that is a normal
  outcome, not an error to surface.
- **Do not build UI that implies content was approved.** A missing verdict means
  nothing was recorded. Presenting that as "clean" would be false.

**Verdicts are not currently trustworthy enough to act on.** See section 6.

The realistic UI surface for the first release is therefore **an operator view,
not a user-facing one**: a way for whoever runs the service to see what was
scanned, what was found, and what could not be scanned. Anything user-facing —
notices, appeals, takedown flows — depends on decisions not yet made, listed in
section 7.

## 6. Why policy-based refusal is not switched on

This section is about the **policy layer only** — the AI judgment of novel
content. The hash layer of section 1a is unaffected: matching a file
against a block list involves no policy judgment, which is why it can
operate while this limitation stands.

This is the single most important limitation of the policy layer, and it is
not a matter of finish or polish.

One of the policies is **prompt-inexpressible**: it cannot be stated to the
model in a way that separates the prohibited category from lawful adult
content. In practice the policy fires on lawful material. The decision layer
turns two positive frames within a two-minute window into `BLOCK_ILLEGAL`, and a
short clip sits entirely inside that window.

Switching enforcement on today would therefore **refuse lawful content from
paying customers, and label them with the most serious category the system
has**. That is a worse failure than not enforcing.

Two consequences follow, and both are deliberate:

- **Enforcement stays off** until that policy is redesigned. This is a blocking
  dependency, tracked as its own milestone, not a configuration flag.
- **Verdicts produced before the redesign are known-defective.** Retaining
  unreviewed "suspected illegal" findings about identifiable customers is itself
  a liability, which is why the retrospective scan has not simply been switched
  on in the meantime.

The system is built so that when the policy is fixed, the same components run
in enforcing mode. The gap is the policy, not the plumbing.

## 7. Open decisions

Genuinely undecided, and each changes what gets built:

- **What happens to flagged material.** Preserve it as evidence, or delete it?
  Preservation means deliberately retaining the material; deletion may destroy
  evidence. This needs legal input and drives the storage design. The existing
  quarantine mechanism holds items in memory only and does not survive a
  restart, so durable preservation is unbuilt.
- **Retention.** How long records and any preserved material are kept.
- **Review.** Who looks at a flagged item, and how quickly. A finding nobody
  reviews has little value and some cost.
- **Notice and appeal.** Whether users are told, and how they contest a finding.
- **Reporting.** Where confirmed findings are reported, and by whom.

## 8. What this does not claim

- It does not detect everything. Sampling, single-track reading, no audio and no
  still-image coverage all leave gaps.
- It does not prevent anything today, on any path: the hash layer records
  and returns verdicts but enforcement is not yet switched on anywhere.
- It does not make the operator a moderator of the wider network. Verdicts are
  local to the host that produced them.
- Two hosts agreeing depends on running the same pinned bundle. Across different
  bundles, verdicts are not comparable.

---

*If you are integrating against this, the parts that will not change are the
verdict/error split in section 3 and the retrospective, non-blocking design in
section 5. The rest is subject to the decisions in section 7.*
