// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Seam-#1 ingest boundary: `FrameSource` + `IngestItem` + orchestration.
//! Real transport is owned by Part A (§4-Q3); `MockFrameSource` drives tests. The
//! [`process`] decision step is injected so ingest is testable before the Track-1
//! matcher lands (Phase 3/4) — production wires the `csam` matcher entry point there.

use std::collections::VecDeque;

use crate::moderation::types::{ModerationResult, Pdq256};
use crate::moderation::verdict_store::VerdictStore;

/// A decoded video/image frame handed across the seam-#1 ingest boundary.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB8 pixels (`len == width * height * 3`).
    pub rgb: Vec<u8>,
}

/// An audio track accompanying a transcode (carried, not scanned — Track 2, deferred).
#[derive(Debug, Clone)]
pub struct AudioTrack {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

/// What the transcoder emits across seam #1 (parent A2): decoded frames, inline
/// pre-computed hashes, or both. The matcher handles all three shapes (§3.5).
#[derive(Debug)]
pub enum IngestItem {
    Frames {
        job_id: u64,
        frames: Vec<DecodedFrame>,
        audio: Option<AudioTrack>,
    },
    Hashes {
        job_id: u64,
        sha256: Vec<[u8; 32]>,
        pdq: Vec<Pdq256>,
    },
    Both {
        job_id: u64,
        frames: Vec<DecodedFrame>,
        audio: Option<AudioTrack>,
        sha256: Vec<[u8; 32]>,
        pdq: Vec<Pdq256>,
    },
}

impl IngestItem {
    /// The job this item belongs to (present in every variant).
    pub fn job_id(&self) -> u64 {
        match self {
            IngestItem::Frames { job_id, .. }
            | IngestItem::Hashes { job_id, .. }
            | IngestItem::Both { job_id, .. } => *job_id,
        }
    }
}

/// Seam-#1 ingest source. `MockFrameSource` drives tests; real transport deferred (§4-Q3).
pub trait FrameSource {
    /// Yield the next ingest item, or `None` when the source is exhausted.
    fn next_item(&mut self) -> Option<IngestItem>;
}

/// In-memory `FrameSource` for tests.
pub struct MockFrameSource {
    items: VecDeque<IngestItem>,
}

impl MockFrameSource {
    pub fn new(items: Vec<IngestItem>) -> Self {
        Self {
            items: items.into(),
        }
    }
    pub fn with_frames(job_id: u64, frames: Vec<DecodedFrame>) -> Self {
        Self::new(vec![IngestItem::Frames {
            job_id,
            frames,
            audio: None,
        }])
    }
    pub fn with_hashes(job_id: u64, sha256: Vec<[u8; 32]>, pdq: Vec<Pdq256>) -> Self {
        Self::new(vec![IngestItem::Hashes {
            job_id,
            sha256,
            pdq,
        }])
    }
    pub fn with_both(
        job_id: u64,
        frames: Vec<DecodedFrame>,
        sha256: Vec<[u8; 32]>,
        pdq: Vec<Pdq256>,
    ) -> Self {
        Self::new(vec![IngestItem::Both {
            job_id,
            frames,
            audio: None,
            sha256,
            pdq,
        }])
    }
}

impl FrameSource for MockFrameSource {
    fn next_item(&mut self) -> Option<IngestItem> {
        self.items.pop_front()
    }
}

/// Mark a job pending moderation — a HOLD recorded the moment ingest begins, so a
/// crash before the verdict lands still holds (absent also holds).
pub fn record_pending(store: &VerdictStore, job_id: u64) {
    store.set(job_id, ModerationResult::blocked("pending moderation"));
}

/// Drive one ingest item through moderation and record the result. `decide` is the
/// injected Track-1 decision (production passes the `csam` matcher entry point,
/// which routes every shape per §3.5).
pub fn process<F>(store: &VerdictStore, item: &IngestItem, decide: F)
where
    F: FnOnce(&IngestItem) -> ModerationResult,
{
    let job_id = item.job_id();
    record_pending(store, job_id);
    let result = decide(item);
    store.set(job_id, result);
}
