// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T3 validation matrix (interface A.1/A.3/C.6). Wave A = the pure accept
//! core (snapshot decode, A.3 gates, attempt registry, plausibility, wire
//! strictness); wave B (T3.3+) adds the pipeline rows that assert zero
//! `ProofSubmit` + the zero-token `SessionComplete` per terminal reject.

mod support;

mod test_accept;
mod test_artifact;
mod test_attestation_t4;
mod test_execute;
mod test_execute_ends;
mod test_handler;
mod test_pipeline;
mod test_pipeline_scan;
mod test_run_loop;
mod test_serve;
mod test_run_loop_ends;
mod test_staging;
mod test_tracker;
mod test_train_stream_client;
mod test_trainer_client;
mod test_wiring;
