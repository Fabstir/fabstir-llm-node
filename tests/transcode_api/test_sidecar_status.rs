// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! 8.62.2: the transcoder sidecar (d645145 and later) reports a failed job in
//! GetTranscoded's `status_code` while the HTTP response stays 200: 400 means
//! bad input (for example an invalid source CID), any other non-200 means the
//! job failed for good (download failed, every format failed, a panic). The
//! node checks it BEFORE progress, which means nothing on a failed job, and ends
//! the run at once instead of waiting out its 30 minute timeout. `metadata` is a
//! plain-text reason for logs only and is never parsed.
//!
//! Live evidence (host2, 2026-11-05): a bare source CID submitted to d645145
//! read {"status_code": 400, "metadata": "Invalid source CID", "progress": 0}
//! on the first poll; the old sidecar said 200 "Transcoding in progress" for ever.

use fabstir_llm_node::api::websocket::handlers::transcode::sidecar_failure_code;

#[test]
fn test_200_is_not_a_failure() {
    assert_eq!(sidecar_failure_code(200), None);
}

#[test]
fn test_400_is_bad_input() {
    assert_eq!(sidecar_failure_code(400), Some("VALIDATION_FAILED"));
}

#[test]
fn test_every_other_status_is_a_permanent_failure() {
    for code in [500, 404, 409, 503, 0, -1, 201, 299] {
        assert_eq!(
            sidecar_failure_code(code),
            Some("TRANSCODE_FAILED"),
            "status_code {code}"
        );
    }
}
