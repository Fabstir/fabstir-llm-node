// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Checkpoint Publishing for Conversation Recovery
//!
//! Publishes signed conversation checkpoints to S5 storage
//! for SDK recovery after session timeout.
//!
//! ## Flow
//! 1. Buffer conversation messages during inference
//! 2. At each proof submission (~1000 tokens):
//!    - Create delta with messages since last checkpoint
//!    - Sign with EIP-191
//!    - Upload to S5
//!    - Update checkpoint index
//! 3. THEN submit proof to chain
//!
//! ## Critical
//! Checkpoint publishing MUST complete BEFORE proof submission.
//! If S5 upload fails, proof submission is blocked.
//!
//! ## SDK Compatibility
//! - JSON must use alphabetically sorted keys (recursive)
//! - Compact format (no spaces)
//! - Raw CID format without s5:// prefix

pub mod cleanup;
pub mod delta;
pub mod index;
pub mod publisher;
pub mod signer;

pub use cleanup::{cleanup_checkpoints, CleanupConfig, CleanupResult, CleanupTask};
pub use delta::{CheckpointDelta, CheckpointMessage, MessageMetadata};
pub use index::{CheckpointEntry, CheckpointIndex, SessionState};
pub use publisher::{CheckpointPublisher, SessionCheckpointState};
pub use signer::sign_checkpoint_data;
