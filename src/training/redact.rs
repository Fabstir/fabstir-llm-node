// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Client-facing text hygiene for the training and serve-back surfaces.
//!
//! **Why this module exists.** Three converge rounds running, a leak of this
//! class was fixed at one site and missed at its sibling: round 5 closed the
//! serve-back staging frame (F-R5-1), round 6 then found the same thing on the
//! session-auth denial path and in `map_stage` (F-R6-1/F-R6-2), and round 7
//! found it again on the training chain read, in `map_sidecar`, and in the run
//! failure detail (F-R7-1/F-R7-2/F-R7-3). Patching sites does not converge,
//! because it leaves the UNSAFE thing as the short, natural thing to write.
//!
//! Two rules, and everything that reaches a client frame should go through one
//! of them:
//!
//!   * a FOREIGN error's `Display` is never echoed. It may carry an RPC or S5
//!     URL (reqwest writes `" for url ({url})"`, and those URLs commonly hold
//!     an API key), an absolute path from this node's filesystem, or an entire
//!     HTTP response body from the sidecar. Use [`opaque`].
//!   * a CLIENT-SUPPLIED string is echoed only bounded. The WebSocket sets no
//!     `max_message_size`, so tungstenite's 64 MiB default applies, and an
//!     unbounded echo multiplies that by every formatting step before any
//!     funding or fetch gate. Use [`echo`].

/// How much of a client-supplied value is worth showing back. Enough to
/// identify which field was wrong, never enough to be an amplifier.
const ECHO_MAX_CHARS: usize = 96;

/// Bound a CLIENT-SUPPLIED string before echoing it in a client-visible
/// message. Truncates on CHARACTERS, so it cannot split a multi-byte sequence.
pub fn echo(value: &str) -> String {
    let mut out: String = value.chars().take(ECHO_MAX_CHARS).collect();
    if value.chars().nth(ECHO_MAX_CHARS).is_some() {
        out.push('…');
    }
    out
}

/// Log a FOREIGN error in full and return fixed text safe to send onward.
///
/// `context` must itself be a constant or node-authored string; it is the part
/// the client sees, so it should say what failed without saying anything about
/// where or how.
pub fn opaque(context: &str, error: impl std::fmt::Display) -> String {
    tracing::error!("{context}: {error}");
    format!("{context} (details are in the host log)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_bounds_and_never_splits_a_char() {
        assert_eq!(echo("short"), "short");
        let wide = "🙂".repeat(200);
        let out = echo(&wide);
        assert_eq!(out.chars().count(), ECHO_MAX_CHARS + 1);
        assert!(out.ends_with('…'));
        // 64 MiB in, a bounded string out: the amplification is what matters.
        assert!(out.len() < 1024);
    }

    #[test]
    fn opaque_keeps_the_context_and_drops_the_error() {
        let out = opaque(
            "session read unavailable",
            "error sending request for url (https://rpc.example/v2/SECRET)",
        );
        assert!(out.starts_with("session read unavailable"));
        assert!(!out.contains("SECRET"));
        assert!(!out.contains("rpc.example"));
    }
}
