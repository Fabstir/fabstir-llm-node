// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! dstack event-log parse, digest recompute, RTMR3 replay and first-occurrence
//! extraction (design §9, gate A-10). Pure: bytes in, `Replayed` out.
//!
//! The live guest agent's `/GetQuote` serves the log stripped PER ENTRY: all IMRs
//! are present; runtime-typed entries (`0x08000001`) keep `event`/`event_payload`
//! and carry `digest: ""`; every other entry keeps its `digest` and carries
//! `event_payload: ""`. The simulator serves the full recorded log with digests.
//! Both verify: the served digest is ignored when empty and must equal the
//! recomputed one when present.
//!
//! Extraction follows dstack's `find_event`: FIRST occurrence, scanning stops at
//! the first `system-ready`. The guest agent's `EmitEvent` RPC accepts any event
//! name, so a hostile compose can emit a second `compose-hash` equal to our pin
//! after boot; it is measured and replays fine, and it is never extracted.

use serde::Deserialize;
use sha2::{Digest, Sha384};

/// dstack's runtime event type (`DSTACK_RUNTIME_EVENT_TYPE`).
pub const RUNTIME_EVENT_TYPE: u32 = 0x0800_0001;

pub const EV_COMPOSE_HASH: &str = "compose-hash";
pub const EV_OS_IMAGE_HASH: &str = "os-image-hash";
pub const EV_APP_ID: &str = "app-id";
pub const EV_KEY_PROVIDER: &str = "key-provider";
pub const EV_SYSTEM_READY: &str = "system-ready";
pub const EV_BOOT_MR_DONE: &str = "boot-mr-done";

/// The pinned names: a second occurrence of any of them before `system-ready`
/// refuses.
const PINNED: [&str; 4] = [
    EV_COMPOSE_HASH,
    EV_OS_IMAGE_HASH,
    EV_APP_ID,
    EV_KEY_PROVIDER,
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("event log: {0}")]
pub struct EventLogError(pub String);

/// One entry as the guest agent serialises it (hex strings). Extra fields a future
/// dstack might add are tolerated: this is dstack's wire, not ours.
#[derive(Debug, Clone, Deserialize)]
pub struct WireEvent {
    pub imr: u32,
    pub event_type: u32,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub event_payload: String,
}

/// A runtime event selected for replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub event: String,
    pub payload: Vec<u8>,
    /// The digest as served (`None` when empty, the live form).
    pub served_digest: Option<[u8; 48]>,
}

/// What the broker decides on (design D16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    pub rtmr3: [u8; 48],
    pub compose_hash: [u8; 32],
    pub os_image_hash: [u8; 32],
    pub app_id: Option<Vec<u8>>,
    pub key_provider: Option<Vec<u8>>,
}

/// Parse the JSON array (UTF-8) into wire entries.
pub fn parse(event_log: &[u8]) -> Result<Vec<WireEvent>, EventLogError> {
    serde_json::from_slice::<Vec<WireEvent>>(event_log)
        .map_err(|e| EventLogError(format!("parse: {e}")))
}

/// dstack `RuntimeEvent::digest`: `sha384(u32 LE type ‖ ":" ‖ event ‖ ":" ‖ payload)`
/// (`to_ne_bytes` on x86-64 is little-endian).
pub fn runtime_digest(event: &str, payload: &[u8]) -> [u8; 48] {
    let mut h = Sha384::new();
    h.update(RUNTIME_EVENT_TYPE.to_le_bytes());
    h.update(b":");
    h.update(event.as_bytes());
    h.update(b":");
    h.update(payload);
    h.finalize().into()
}

/// Select the runtime events (rule 1): the `imr == 3` set must equal the
/// `event_type == 0x08000001` set; any asymmetry refuses. Decodes the hex fields.
pub fn select_runtime(entries: &[WireEvent]) -> Result<Vec<RuntimeEvent>, EventLogError> {
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let is_imr3 = e.imr == 3;
        let is_runtime = e.event_type == RUNTIME_EVENT_TYPE;
        if is_imr3 != is_runtime {
            return Err(EventLogError(format!(
                "foreign event type/IMR at entry {i} (imr {}, type 0x{:08x})",
                e.imr, e.event_type
            )));
        }
        if !is_imr3 {
            continue;
        }
        let payload = hex::decode(&e.event_payload)
            .map_err(|_| EventLogError(format!("entry {i}: event_payload is not hex")))?;
        let served_digest = if e.digest.is_empty() {
            None
        } else {
            let d = hex::decode(&e.digest)
                .map_err(|_| EventLogError(format!("entry {i}: digest is not hex")))?;
            let d: [u8; 48] = d
                .try_into()
                .map_err(|_| EventLogError(format!("entry {i}: digest is not 48 bytes")))?;
            Some(d)
        };
        out.push(RuntimeEvent {
            event: e.event.clone(),
            payload,
            served_digest,
        });
    }
    Ok(out)
}

/// Rules 2–3: recompute every digest (a present served digest must match), chain
/// from 48 zero bytes. Returns the replayed RTMR3.
pub fn replay(events: &[RuntimeEvent]) -> Result<[u8; 48], EventLogError> {
    let mut mr = [0u8; 48];
    for (i, ev) in events.iter().enumerate() {
        let d = runtime_digest(&ev.event, &ev.payload);
        if let Some(served) = ev.served_digest {
            if served != d {
                return Err(EventLogError(format!(
                    "entry {i} ({}): served digest does not match its event and payload",
                    ev.event
                )));
            }
        }
        let mut h = Sha384::new();
        h.update(mr);
        h.update(d);
        mr = h.finalize().into();
    }
    Ok(mr)
}

/// Rules 4–6: first occurrence, stop at `system-ready`; `system-ready` and
/// `boot-mr-done` required; a duplicate pinned name before `system-ready` refuses;
/// `compose-hash` and `os-image-hash` required and 32 bytes; `app-id` and
/// `key-provider` optional (raw payload bytes).
pub fn extract(events: &[RuntimeEvent], rtmr3: [u8; 48]) -> Result<Replayed, EventLogError> {
    let mut compose_hash = None;
    let mut os_image_hash = None;
    let mut app_id = None;
    let mut key_provider = None;
    let mut boot_mr_done = false;
    let mut system_ready = false;
    let mut seen: Vec<&str> = Vec::new();

    for ev in events {
        if ev.event == EV_SYSTEM_READY {
            system_ready = true;
            break;
        }
        if ev.event == EV_BOOT_MR_DONE {
            boot_mr_done = true;
            continue;
        }
        if PINNED.contains(&ev.event.as_str()) {
            if seen.contains(&ev.event.as_str()) {
                return Err(EventLogError(format!(
                    "duplicate {} before {}",
                    ev.event, EV_SYSTEM_READY
                )));
            }
            seen.push(PINNED.iter().find(|p| **p == ev.event).expect("pinned"));
        }
        match ev.event.as_str() {
            EV_COMPOSE_HASH => compose_hash = Some(fixed32(&ev.payload, EV_COMPOSE_HASH)?),
            EV_OS_IMAGE_HASH => os_image_hash = Some(fixed32(&ev.payload, EV_OS_IMAGE_HASH)?),
            EV_APP_ID => app_id = Some(ev.payload.clone()),
            EV_KEY_PROVIDER => key_provider = Some(ev.payload.clone()),
            _ => {}
        }
    }
    if !system_ready {
        return Err(EventLogError(format!("{EV_SYSTEM_READY} event absent")));
    }
    if !boot_mr_done {
        return Err(EventLogError(format!("{EV_BOOT_MR_DONE} event absent")));
    }
    let compose_hash =
        compose_hash.ok_or_else(|| EventLogError(format!("{EV_COMPOSE_HASH} event absent")))?;
    let os_image_hash = os_image_hash
        .ok_or_else(|| EventLogError(format!("os image: {EV_OS_IMAGE_HASH} event absent")))?;
    Ok(Replayed {
        rtmr3,
        compose_hash,
        os_image_hash,
        app_id,
        key_provider,
    })
}

/// The whole pipeline over the wire bytes: parse → select → replay → extract.
pub fn replay_and_extract(event_log: &[u8]) -> Result<Replayed, EventLogError> {
    let entries = parse(event_log)?;
    let events = select_runtime(&entries)?;
    let rtmr3 = replay(&events)?;
    extract(&events, rtmr3)
}

fn fixed32(payload: &[u8], name: &str) -> Result<[u8; 32], EventLogError> {
    payload.try_into().map_err(|_| {
        EventLogError(format!(
            "{name} payload is {} bytes, want 32",
            payload.len()
        ))
    })
}
