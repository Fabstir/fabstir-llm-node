// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Serve-back (interface E.1/E.2, TD9): the session-scoped LoRA adapter —
//! fetch, verify, stage 0600, register per SESSION, evict at session end.
//!
//! ISOLATION is the wire guarantee (E.2): "applied at scale 1.0 to that
//! session's requests only — one adapter per session in M0, never visible
//! to concurrent sessions on the same base model, unloaded at session end".
//! TD9 records why that matters: the v0.1 model-keyed design would have
//! applied one user's PRIVATE adapter to every concurrent session on the
//! same base. The registry below is keyed on session id for exactly that
//! reason, and `adapter_for` is the only resolution path.
//!
//! Verification chain (E.2): `manifestSha256` over the stored bytes → the
//! named file's own sha256 over the reassembled shards → the serving
//! session's model equals the template's `baseServingModelId` pin. Serve-back
//! is additionally gated on the manifest actually carrying the named file
//! (TD12/E.1(b): a run whose GGUF conversion failed ships safetensors-only
//! and cannot serve back).
//!
//! TRUST BOUNDARY — state it plainly, because the first cut of this module
//! did not and that is why its bounds were missing (T5 converge round 1):
//! **the manifest is UNTRUSTED CLIENT INPUT.** Nothing here reads an
//! attestation or the chain; `manifestSha256` proves only that the bytes
//! match the CLIENT'S OWN claim. That is the intended M0 division of labour
//! (the capability CID is the authorisation, per the LTX capability model;
//! CK-7 provenance is the client's own record-keeping), but it has three
//! consequences this module must therefore enforce itself:
//!   * every size/count in the manifest is attacker-chosen, so each is
//!     bounded here (the `staging.rs` discipline, mirrored);
//!   * every NAME in it, and the session id, are attacker-chosen strings
//!     that must never reach a path join unvalidated;
//!   * arbitrary attacker bytes reach llama.cpp's GGUF parser in-process.
//!     That is accepted M0 attack surface, recorded rather than hidden.
//! A session may also legitimately serve ANOTHER customer's adapter if it
//! holds that capability CID; possession is the authorisation, so the base
//! pin is the only semantic check.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

/// The session-init `lora` field (E.2). `Serialize` only so the enclosing
/// `SessionInitData` can derive it; the node never emits this on the wire.
#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct LoraRequest {
    #[serde(rename = "manifestCID")]
    pub manifest_cid: String,
    #[serde(rename = "manifestSha256")]
    pub manifest_sha256: String,
    /// Which manifest entry to load (M0: `adapter.gguf`).
    pub file: String,
}

/// Why a serve-back request was refused. Kept distinct from the training
/// pipeline's `StageError`: these answer a SESSION-INIT, not a train job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeError {
    /// Schema/shape/base-pin violations, and a missing named file (the
    /// gguf-failure gate).
    Validation(String),
    /// A cryptographic claim failed (manifest bytes or file bytes).
    Integrity(String),
    /// Fetch failure — infrastructure, never a claim about the artifact.
    Transport(String),
    /// Local staging-volume failure.
    Io(String),
    /// The session ended while its adapter was still staging (round-3 F1).
    /// Its bytes are deleted before this is returned.
    Cancelled(String),
    /// The stage ran past its wall-clock budget. Round-6 F-R6-3: this used to
    /// be a `Transport`, so the whitelist suppressed a message that is a pure
    /// compile-time constant — the client lost the one reason it can act on
    /// ("your adapter is too big to stage in the window") for no security gain.
    Budget(String),
    /// A node-side chain read failed. Split out for the same reason: the
    /// CLIENT's correct response is to re-shop to another host, which it
    /// cannot know while this is indistinguishable from its own bad CID.
    Chain(String),
}

impl ServeError {
    /// A stable, machine-readable cause for the client to branch on. The
    /// human message may be suppressed by the whitelist; this never is,
    /// because it is a fixed vocabulary and carries nothing.
    pub fn reason(&self) -> &'static str {
        match self {
            ServeError::Validation(_) | ServeError::Integrity(_) => "invalid",
            ServeError::Transport(_) => "fetch",
            ServeError::Io(_) => "write",
            ServeError::Cancelled(_) => "cancelled",
            ServeError::Budget(_) => "budget",
            ServeError::Chain(_) => "chain",
        }
    }
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, detail) = match self {
            ServeError::Validation(d) => ("validation", d),
            ServeError::Integrity(d) => ("integrity", d),
            ServeError::Transport(d) => ("transport", d),
            ServeError::Io(d) => ("io", d),
            ServeError::Cancelled(d) => ("cancelled", d),
            ServeError::Budget(d) => ("budget", d),
            ServeError::Chain(d) => ("chain", d),
        };
        write!(f, "{kind}: {detail}")
    }
}

/// The one file M0 serves back (E.2/TD12). Anything else is refused before
/// it can reach a path join (T5 round-1 F1: `request.file` is a wire string
/// and `Path::join` with `"../.."` or an absolute path escapes the staging
/// root, giving an arbitrary file write as the node user).
pub const M0_ADAPTER_FILE: &str = "adapter.gguf";

/// Ceiling on an adapter file. A rank-16 LoRA over a 27B base is tens of MB;
/// 1 GiB is generous and still refuses the `u64::MAX` `with_capacity` panic
/// and the OOM-by-many-shards shape (T5 round-1 F4). Halved from 2 GiB in
/// T5.3 round 1 (F6): staging buffers the plaintext AND the ciphertext.
pub const ADAPTER_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// How many adapters may be LIVE at once, node-wide. Round-3 R3-4: minting
/// the registry key per connection was the right call for isolation, but it
/// removed the incidental cap the session-keyed design gave — one client
/// holding K sockets could stage K adapters of up to `ADAPTER_MAX_BYTES` each
/// and fill the staging volume, since the concurrency bound below limits
/// simultaneity, not the total.
pub const ADAPTER_MAX_LIVE: usize = 16;

/// How many adapters may be staged at once, node-wide (round-1 F6: staging
/// buffers the whole file AND the whole ciphertext in memory, and the route
/// that triggers it needs no credentials, so N connections meant ~2N GiB of
/// RSS with nothing bounding N).
pub const ADAPTER_MAX_CONCURRENT_STAGES: usize = 2;

/// Ceiling on the adapter manifest blob itself (a small JSON document).
pub const ADAPTER_MANIFEST_MAX_BYTES: u64 = 1_048_576;

/// Ceiling on shard count (fetch-storm bound), matching `staging.rs`.
pub const ADAPTER_MAX_SHARDS: usize = 64;

/// A path component supplied by a client (a session id, a manifest file
/// name) must be exactly ONE normal component: no `..`, no `/`, no root, no
/// NUL, not empty. Round-1 F1/F2: `""` made the staged dir the shared
/// `adapters/` root and eviction then deleted EVERY session's adapter;
/// `".."` reached the staging root itself.
fn safe_component(value: &str, what: &str) -> Result<(), ServeError> {
    // Round-6 F-R6-6: echoing `{value:?}` unbounded meant a client could put
    // 64 MiB (tungstenite's default frame cap; no `max_message_size` is set)
    // in `lora.file` and make the node build several more copies of it in the
    // format, the json and the to_string — on an unauthenticated route, before
    // the reservation and before the concurrency semaphore. And the session-id
    // call passes the MINTED KEY, so it must not be echoed at all: the
    // whitelist's key-safety should not rest on an invariant held in another
    // file.
    let shown = if what == "session id" {
        String::from("<redacted>")
    } else {
        let mut t: String = value.chars().take(64).collect();
        if value.chars().count() > 64 {
            t.push('…');
        }
        format!("{t:?}")
    };
    if value.is_empty() || value.contains('\0') || value.len() > 128 {
        return Err(ServeError::Validation(format!(
            "{what} {shown} is empty, over-long, or contains NUL — refused"
        )));
    }
    let path = Path::new(value);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(part)), None) if part == value => Ok(()),
        _ => Err(ServeError::Validation(format!(
            "{what} {shown} is not a single normal path component — refused"
        ))),
    }
}

/// A staged, verified, session-scoped adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedAdapter {
    pub session_id: String,
    /// 0600 file under `<staging_root>/adapters/<sessionId>/<file>`.
    pub path: PathBuf,
    pub file: String,
    pub sha256: String,
    /// Reassembled plaintext size, for the operator log. Without a positive
    /// staged line, a stage that SUCCEEDED and was then rejected by llama.cpp
    /// reads identically to a stage that failed, and inferring success from
    /// silence is the failure this surface exists to avoid.
    pub bytes: u64,
    /// How many shards the manifest declared for this file.
    pub shards: usize,
}

/// How a WebSocket connection resolves its serve-back adapter.
///
/// Round-1 F1/F1(b): the first cut cached a raw `PathBuf` in the connection.
/// That bypassed the registry entirely — `adapter_for` ended up with ZERO
/// production callers, so the one component that knows an eviction happened
/// was never consulted, and a request could be served from a path whose bytes
/// had been replaced underneath it. Resolution now goes back through the
/// registry on every request, keyed on an id the WIRE CANNOT SET.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SessionAdapter {
    /// No `lora` was requested: an ordinary session on the base model.
    #[default]
    None,
    /// A `lora` WAS requested. The request must be refused if the registry
    /// no longer holds it — staging failed, or it was evicted — because
    /// answering from the base model would silently give the customer the
    /// wrong weights on a paid session.
    Required(String),
}

/// What a registry key holds.
///
/// `Reserved` = a stage is IN FLIGHT for that session id (round-2 R2-3). The
/// reservation is taken under the lock before any fetch, so a second
/// concurrent stage on the same id is refused rather than racing the first
/// to the same temp path and the same registry key.
///
/// Round-3 F1: this was a bare `Option`, and `evict`'s `remove().flatten()`
/// then deleted an in-flight reservation's KEY while deleting no FILE. The
/// stage went on to commit an adapter for a session that had already ended —
/// private weights with nothing left to evict them, surviving until the boot
/// sweep — and in that window a second session could claim the freed id and
/// have its entry overwritten by the first stage's commit, which is the TD9
/// isolation failure the reservation was added to close. The flag lets
/// eviction and commit see each other under one lock instead.
enum Slot {
    Reserved { cancelled: bool },
    Ready(StagedAdapter),
}

/// Per-SESSION adapter registry (TD9's isolation requirement).
pub struct AdapterRegistry {
    inner: Mutex<HashMap<String, Slot>>,
    stages: tokio::sync::Semaphore,
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            stages: tokio::sync::Semaphore::new(ADAPTER_MAX_CONCURRENT_STAGES),
        }
    }
}

/// Releases a stage reservation unless it was committed, so an early `?`
/// anywhere in `stage` cannot strand a session id as permanently unusable.
struct Reservation<'a> {
    registry: &'a AdapterRegistry,
    session_id: String,
    committed: bool,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.registry
                .inner
                .lock()
                .expect("adapter registry poisoned")
                .remove(&self.session_id);
        }
    }
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch → verify → stage 0600 → register for `session_id`.
    ///
    /// Order matters: the BASE pin is checked before any byte moves (a
    /// session on the wrong base can never use this adapter, so fetching
    /// would be wasted work and a needless read of another job's artifact),
    /// then the manifest, then the named file.
    #[allow(clippy::too_many_arguments)]
    pub async fn stage(
        &self,
        s5_base: &str,
        staging_root: &Path,
        session_id: &str,
        session_model_id: &str,
        base_serving_model_id: &str,
        request: &LoraRequest,
    ) -> Result<StagedAdapter, ServeError> {
        // Client-controlled strings NEVER reach a path join unvalidated
        // (round-1 F1/F2). The file name is additionally pinned to the one
        // M0 serves back.
        safe_component(session_id, "session id")?;
        safe_component(&request.file, "lora.file")?;
        if request.file != M0_ADAPTER_FILE {
            return Err(ServeError::Validation(format!(
                "lora.file {:?} is not the M0 serve-back artifact {M0_ADAPTER_FILE:?}",
                request.file
            )));
        }
        // E.2: "The session's model must equal the template's
        // baseServingModelId pin." Checked BEFORE the reservation (round-2
        // R2-18) so a caller on the wrong base model never learns whether
        // the session id it named is already staged.
        if !session_model_id.eq_ignore_ascii_case(base_serving_model_id) {
            return Err(ServeError::Validation(format!(
                "serving base mismatch: session model {session_model_id} != the template's \
                 baseServingModelId {base_serving_model_id}"
            )));
        }

        // E.2: one adapter per session in M0. A re-stage is REFUSED rather
        // than silently replacing (round-1 F3: `insert` let a second stage
        // overwrite the first session's entry AND its file). Round-2 R2-3:
        // the check and the insert used to be separate lock acquisitions
        // with the whole fetch-and-write between them, so two concurrent
        // stages on one id both passed, clobbered each other's temp file and
        // both returned Ok — leaving one session's sha256 recorded against
        // the other's bytes. Reserving under the lock closes that window;
        // the guard releases it on every error path.
        // The cap and the reservation take ONE lock between them (round-4
        // F-R4-2: as two acquisitions this was a check-then-reserve race —
        // K tasks all read `len() == CAP - 1`, all passed, and the registry
        // overshot by however many were inside the window, each overshoot
        // costing up to ADAPTER_MAX_BYTES of staging volume, which is the
        // exact damage the cap exists to bound).
        {
            let mut registry = self.inner.lock().expect("adapter registry poisoned");
            if registry.len() >= ADAPTER_MAX_LIVE {
                return Err(ServeError::Validation(format!(
                    "this node is already serving {ADAPTER_MAX_LIVE} session adapters"
                )));
            }
            match registry.entry(session_id.to_string()) {
                std::collections::hash_map::Entry::Occupied(_) => {
                    return Err(ServeError::Validation(
                        "this session already has an adapter (one per session in M0)"
                            .to_string(),
                    ));
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(Slot::Reserved { cancelled: false });
                }
            }
        }
        let mut reservation = Reservation {
            registry: self,
            session_id: session_id.to_string(),
            committed: false,
        };

        // Bounded concurrency (F6). Taken AFTER the reservation, so a queued
        // stage still owns its session id and a second attempt on the same id
        // is still refused rather than queueing behind the first.
        let _permit = self
            .stages
            .acquire()
            .await
            .map_err(|_| ServeError::Io("adapter stage semaphore closed".to_string()))?;

        let manifest = fetch_and_verify_manifest(s5_base, request).await?;
        let entry = manifest
            .files
            .iter()
            .find(|f| f.name == request.file)
            .ok_or_else(|| {
                // TD12/E.1(b): a run whose GGUF conversion failed ships
                // safetensors-only — serve-back is unavailable for it.
                ServeError::Validation(format!(
                    "adapter manifest carries no {:?} (a gguf-conversion failure ships \
                     safetensors-only and cannot serve back)",
                    request.file
                ))
            })?;

        let bytes = reassemble_and_verify(s5_base, entry).await?;

        let dir = staging_root.join("adapters").join(session_id);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| ServeError::Io(format!("create {dir:?}: {e}")))?;
        restrict_dir(&dir).await?;
        let path = dir.join(&request.file);
        write_private(&path, &bytes).await?;

        let staged = StagedAdapter {
            session_id: session_id.to_string(),
            path,
            file: request.file.clone(),
            sha256: entry.sha256.clone(),
            bytes: bytes.len() as u64,
            shards: entry.shards.len(),
        };
        // Commit, unless an `evict` overtook us (round-3 F1). Both sides
        // decide under the same lock, so the id is never free while either
        // is still acting on it.
        let cancelled = {
            let mut registry = self.inner.lock().expect("adapter registry poisoned");
            match registry.get(session_id) {
                Some(Slot::Reserved { cancelled: true }) => {
                    registry.remove(session_id);
                    true
                }
                _ => {
                    registry.insert(session_id.to_string(), Slot::Ready(staged.clone()));
                    false
                }
            }
        };
        reservation.committed = true;
        if cancelled {
            // Nothing will evict this now, so delete it here rather than
            // leaving a customer's private weights for the boot sweep.
            if let Err(error) = tokio::fs::remove_dir_all(&dir).await {
                tracing::warn!(
                    "failed to clean up the cancelled stage at {dir:?}: {error} — private \
                     weights may remain on disk"
                );
            }
            return Err(ServeError::Cancelled(
                "the session ended while its adapter was staging".to_string(),
            ));
        }
        Ok(staged)
    }

    /// The ONLY resolution path: an adapter is visible to its own session
    /// and to nothing else.
    pub fn adapter_for(&self, session_id: &str) -> Option<StagedAdapter> {
        match self
            .inner
            .lock()
            .expect("adapter registry poisoned")
            .get(session_id)
        {
            // A reservation is a stage IN FLIGHT, not an adapter.
            Some(Slot::Ready(staged)) => Some(staged.clone()),
            _ => None,
        }
    }

    /// TEST ONLY: is anything staged at all? The registry key is minted
    /// server-side, so a test driving the router cannot name it.
    #[doc(hidden)]
    pub fn is_empty_for_test(&self) -> bool {
        self.inner
            .lock()
            .expect("adapter registry poisoned")
            .values()
            .all(|slot| !matches!(slot, Slot::Ready(_)))
    }

    /// Resolve a connection's serve-back adapter for one request.
    ///
    /// `Required` that the registry no longer holds is an ERROR, never a
    /// silent `None`. Falling back to the base model there would hand the
    /// customer the wrong weights on a session they are paying for and
    /// believe is serving their fine-tune, and it would do so invisibly.
    pub fn resolve(&self, want: &SessionAdapter) -> Result<Option<PathBuf>, ServeError> {
        match want {
            SessionAdapter::None => Ok(None),
            // Round-3 R3-2: the message must NOT embed the key. It is minted
            // precisely so the wire never sees one, and this text is shipped
            // to the client verbatim. The key is logged by the caller instead.
            SessionAdapter::Required(session_id) => match self.adapter_for(session_id) {
                Some(staged) => Ok(Some(staged.path)),
                None => Err(ServeError::Validation(
                    "this session asked for a LoRA adapter and none is staged (staging \
                     failed, or the session ended) — refusing to answer from the base model"
                        .to_string(),
                )),
            },
        }
    }

    /// Session end: deregister AND delete the staged file (E.2 "unloaded at
    /// session end"; TD9 "deleted at eviction"). A no-op for an unknown
    /// session.
    pub async fn evict(&self, session_id: &str) {
        let staged = {
            let mut registry = self.inner.lock().expect("adapter registry poisoned");
            match registry.get_mut(session_id) {
                // Round-3 F1: a stage is in flight. Leave the KEY in place so
                // no other session can claim the id, and let that stage
                // delete its own bytes when it sees the flag.
                Some(Slot::Reserved { cancelled }) => {
                    *cancelled = true;
                    None
                }
                Some(Slot::Ready(_)) => match registry.remove(session_id) {
                    Some(Slot::Ready(staged)) => Some(staged),
                    _ => None,
                },
                None => None,
            }
        };
        if let Some(staged) = staged {
            // The whole per-session directory goes, not just the file. The
            // path shape is guaranteed by `safe_component` at stage time, so
            // this can only ever remove `<root>/adapters/<one component>`.
            if let Some(dir) = staged.path.parent() {
                if let Err(error) = tokio::fs::remove_dir_all(dir).await {
                    // Round-1 F10: a FAILED deletion of a customer's private
                    // weights must not be silent.
                    tracing::warn!(
                        "failed to evict session adapter dir {dir:?}: {error} — private weights may remain on disk"
                    );
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdapterManifest {
    schema: String,
    kind: String,
    files: Vec<AdapterFile>,
}

#[derive(Debug, Deserialize)]
struct AdapterFile {
    name: String,
    sha256: String,
    #[serde(rename = "sizeBytes")]
    size_bytes: u64,
    shards: Vec<AdapterShard>,
}

#[derive(Debug, Deserialize)]
struct AdapterShard {
    cid: String,
    sha256: String,
    #[serde(rename = "sizeBytes")]
    size_bytes: u64,
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("0x{}", hex::encode(Sha256::digest(data)))
}

fn hex_eq(a: &str, b: &str) -> bool {
    a.trim_start_matches("0x").eq_ignore_ascii_case(b.trim_start_matches("0x"))
}

/// Fetch the manifest and verify `manifestSha256` over its EXACT stored
/// bytes (never re-canonicalised — the D.2 stored-bytes rule), then its
/// schema/kind literals.
async fn fetch_and_verify_manifest(
    s5_base: &str,
    request: &LoraRequest,
) -> Result<AdapterManifest, ServeError> {
    // Pre-fetch bound on the CLIENT-DECLARED length (round-1 F4: without it
    // a capability CID claiming gigabytes is streamed into memory before the
    // sha256 check can fail — the exact hole `staging.rs` closed).
    gate_declared_len(&request.manifest_cid, ADAPTER_MANIFEST_MAX_BYTES, "adapter manifest")?;
    let (_h, bytes) = crate::ltx::input_image::fetch_image_hash(s5_base, &request.manifest_cid)
        .await
        .map_err(|e| ServeError::Transport(format!("adapter manifest fetch: {e}")))?;
    let actual = sha256_hex(&bytes);
    // Round-7 F-R7-5: these three are wire strings echoed through the
    // whitelisted `Integrity`/`Validation` arms, so they are bounded. The
    // whitelist says the text is built from the client's own claims; it does
    // not say the client's claims are a sensible length.
    if !hex_eq(&actual, &request.manifest_sha256) {
        return Err(ServeError::Integrity(format!(
            "manifestSha256 mismatch: stored bytes hash {actual}, session claims {}",
            crate::training::redact::echo(&request.manifest_sha256)
        )));
    }
    let manifest: AdapterManifest = serde_json::from_slice(&bytes)
        .map_err(|e| ServeError::Validation(format!("adapter manifest parse: {e}")))?;
    if manifest.schema != "artifact-manifest-v1" {
        return Err(ServeError::Validation(format!(
            "adapter manifest schema {:?} is not artifact-manifest-v1",
            crate::training::redact::echo(&manifest.schema)
        )));
    }
    if manifest.kind != "adapter" {
        return Err(ServeError::Validation(format!(
            "manifest kind {:?} is not an adapter manifest",
            crate::training::redact::echo(&manifest.kind)
        )));
    }
    Ok(manifest)
}

/// Fetch every shard in order, verify each against its own claim, then the
/// reassembled file against the entry's sha256 and size.
async fn reassemble_and_verify(
    s5_base: &str,
    entry: &AdapterFile,
) -> Result<Vec<u8>, ServeError> {
    // Every number below is attacker-chosen (see the module header), so each
    // is bounded BEFORE it is trusted (round-1 F4).
    if entry.shards.is_empty() {
        return Err(ServeError::Validation(format!("{} declares no shards", entry.name)));
    }
    if entry.shards.len() > ADAPTER_MAX_SHARDS {
        return Err(ServeError::Validation(format!(
            "{} declares {} shards > the {ADAPTER_MAX_SHARDS} cap",
            entry.name,
            entry.shards.len()
        )));
    }
    if entry.size_bytes == 0 || entry.size_bytes > ADAPTER_MAX_BYTES {
        return Err(ServeError::Validation(format!(
            "{} declares {} bytes, outside 1..={ADAPTER_MAX_BYTES}",
            entry.name, entry.size_bytes
        )));
    }
    // Round-2 R2-1 (HIGH): this was a raw `.sum()` over attacker-chosen u64s.
    // Release builds have overflow checks OFF (no `[profile]` in Cargo.toml),
    // so shards of `[2^63, 2^63, 1024]` summed to 1024 and MATCHED a declared
    // `sizeBytes` of 1024 — the equality gate below passed, and because the
    // per-shard gate then used `shard.size_bytes` as its own ceiling, an
    // 8 GiB blob was admitted and downloaded before any hash was checked.
    // The wrap silently unbound the one check the per-shard gate relied on:
    // a fix cancelled by a sibling fix. Bound each shard by the CONSTANT
    // first, then saturate the sum, as `staging.rs` has always done.
    let mut declared_total: u64 = 0;
    for (index, shard) in entry.shards.iter().enumerate() {
        if shard.size_bytes == 0 || shard.size_bytes > ADAPTER_MAX_BYTES {
            return Err(ServeError::Validation(format!(
                "{} shard {index} declares {} bytes, outside 1..={ADAPTER_MAX_BYTES}",
                entry.name, shard.size_bytes
            )));
        }
        declared_total = declared_total.saturating_add(shard.size_bytes);
    }
    if declared_total != entry.size_bytes {
        return Err(ServeError::Validation(format!(
            "{} shard sizes sum to {declared_total}, file claims {}",
            entry.name, entry.size_bytes
        )));
    }
    // Clamped: `size_bytes` is bounded above, so this cannot be the
    // `capacity overflow` panic the round-1 probe produced.
    let mut bytes = Vec::with_capacity(entry.size_bytes as usize);
    for (index, shard) in entry.shards.iter().enumerate() {
        // Pre-fetch: bound by the CONSTANT (round-2 R2-2 — passing
        // `shard.size_bytes` as the ceiling made this gate a no-op), then
        // require equality with the manifest's claim, so no blob larger than
        // the claim can even start downloading.
        let what = format!("{} shard {index}", entry.name);
        let declared = gate_declared_len(&shard.cid, ADAPTER_MAX_BYTES, &what)?;
        if declared != shard.size_bytes {
            return Err(ServeError::Integrity(format!(
                "{what} capability declares {declared} bytes but the manifest claims {} — \
                 refused before fetch",
                shard.size_bytes
            )));
        }
        let (_h, part) = crate::ltx::input_image::fetch_image_hash(s5_base, &shard.cid)
            .await
            .map_err(|e| {
                ServeError::Transport(format!("{} shard {index} fetch: {e}", entry.name))
            })?;
        if part.len() as u64 != shard.size_bytes || !hex_eq(&sha256_hex(&part), &shard.sha256) {
            return Err(ServeError::Integrity(format!(
                "{} shard {index} does not match its manifest claim",
                entry.name
            )));
        }
        // Running bound: never accumulate past the (bounded) declared total.
        if bytes.len() as u64 + part.len() as u64 > entry.size_bytes {
            return Err(ServeError::Integrity(format!(
                "{} shards overrun the declared {} bytes",
                entry.name, entry.size_bytes
            )));
        }
        bytes.extend_from_slice(&part);
    }
    if bytes.len() as u64 != entry.size_bytes {
        return Err(ServeError::Integrity(format!(
            "{} reassembled to {} bytes, manifest claims {}",
            entry.name,
            bytes.len(),
            entry.size_bytes
        )));
    }
    let actual = sha256_hex(&bytes);
    if !hex_eq(&actual, &entry.sha256) {
        return Err(ServeError::Integrity(format!(
            "{} sha256 mismatch: reassembled {actual}, manifest claims {}",
            entry.name, entry.sha256
        )));
    }
    Ok(bytes)
}

/// Pre-fetch gate on a capability CID's SELF-DECLARED plaintext length —
/// the `staging.rs` rule, which this module was missing (round-1 F4).
/// Returns the capability's SELF-DECLARED plaintext length, bounded by a
/// CONSTANT. Round-2 R2-2: the ceiling must never be a number from the same
/// attacker-authored manifest the caller is trying to bound — a per-shard
/// gate whose max is `shard.size_bytes` admits any size the attacker likes.
/// The caller pairs this with an equality check, exactly as `staging.rs`
/// does.
fn gate_declared_len(cid: &str, max_bytes: u64, what: &str) -> Result<u64, ServeError> {
    let envelope = crate::ltx::input_image::parse_capability_cid(cid)
        .map_err(|e| ServeError::Validation(format!("{what} capability CID invalid: {e}")))?;
    let declared = envelope.plaintext_len as u64;
    if declared == 0 || declared > max_bytes {
        return Err(ServeError::Validation(format!(
            "{what} declares {declared} plaintext bytes (bound 1..={max_bytes}) — refused before fetch"
        )));
    }
    Ok(declared)
}

/// 0700 on the adapter directories (round-1 F11: `create_dir_all` leaves
/// 0755, letting any local user enumerate live session ids and adapter
/// names on the shared box the whole premise is about).
async fn restrict_dir(dir: &Path) -> Result<(), ServeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for target in [dir.parent().unwrap_or(dir), dir] {
            // Round-2 R2-12: a failure here leaves the directory 0755 and
            // leaks live session ids on the shared box this guard exists
            // for. Not fatal (the file itself is still written 0600), but
            // it must never be silent.
            if let Err(error) =
                tokio::fs::set_permissions(target, std::fs::Permissions::from_mode(0o700)).await
            {
                tracing::warn!(
                    "failed to restrict {target:?} to 0700: {error} — session ids and adapter \
                     names may be world-readable"
                );
            }
        }
    }
    let _ = dir;
    Ok(())
}

/// Write the adapter 0600 and ATOMICALLY (TD9 + round-1 F5/F6): the bytes
/// go to a fresh `create_new` temp in the same directory (so an existing
/// file's looser mode can never be inherited, and a pre-planted SYMLINK
/// cannot be followed — `create_new` fails on any existing path), are
/// fsynced, then renamed into place. A reader therefore sees either nothing
/// or the complete, verified artifact.
async fn write_private(path: &Path, bytes: &[u8]) -> Result<(), ServeError> {
    use tokio::io::AsyncWriteExt;
    let tmp = path.with_extension("staging-tmp");
    let _ = tokio::fs::remove_file(&tmp).await; // a dead attempt's leftover
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        // `mode` is inherent on tokio's unix OpenOptions; it applies because
        // create_new ALWAYS creates (round-1 F5: with create+truncate an
        // existing 0644 file kept its mode and the "never world-readable"
        // claim was false).
        options.mode(0o600);
    }
    let mut file = options
        .open(&tmp)
        .await
        .map_err(|e| ServeError::Io(format!("create {tmp:?}: {e}")))?;
    file.write_all(bytes)
        .await
        .map_err(|e| ServeError::Io(format!("write {tmp:?}: {e}")))?;
    file.sync_all()
        .await
        .map_err(|e| ServeError::Io(format!("sync {tmp:?}: {e}")))?;
    drop(file);
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| ServeError::Io(format!("rename into {path:?}: {e}")))?;
    Ok(())
}


/// Boot sweep for staged adapters (round-1 F10). At startup no session is
/// live, so every `adapters/<sessionId>/` directory is a crash leftover
/// holding one customer's private weights. The job-dir sweep in
/// `staging.rs` matches only `job-*` and never touched these.
pub fn sweep_orphan_adapter_dirs(staging_root: &Path) -> usize {
    let adapters = staging_root.join("adapters");
    let Ok(entries) = std::fs::read_dir(&adapters) else {
        return 0;
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        match std::fs::remove_dir_all(entry.path()) {
            Ok(()) => swept += 1,
            // Round-2 R2-13: this counted only successes, so a FAILED
            // removal of a customer's private weights at boot was invisible
            // — the same silence round-1 F10 closed in `evict`.
            Err(error) => tracing::warn!(
                "failed to sweep orphan adapter dir {:?}: {error} — private weights remain on disk",
                entry.path()
            ),
        }
    }
    swept
}
