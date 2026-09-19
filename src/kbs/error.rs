// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The broker's refusal type and the wire error kinds (design D10, §4).
//!
//! The kinds are exactly the five the node documents in
//! [`crate::tee::kbs_http::ErrorInner`]: `freshness` 401, `verification` 403,
//! `no_provider` 404, `invalid` 400/413, `unavailable` 500/502/503. There is no
//! `internal`. `detail` names the failed step(s) for the node's log and never
//! carries key material or quote bytes.

use std::fmt;

/// Wire error kind (design D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Freshness,
    Verification,
    NoProvider,
    Invalid,
    Unavailable,
}

impl Kind {
    /// The string on the wire.
    pub fn wire(self) -> &'static str {
        match self {
            Kind::Freshness => "freshness",
            Kind::Verification => "verification",
            Kind::NoProvider => "no_provider",
            Kind::Invalid => "invalid",
            Kind::Unavailable => "unavailable",
        }
    }

    /// The default HTTP status for the kind (design §4); `KbsError::with_status`
    /// overrides it for the 413/502/503 cases.
    pub fn default_status(self) -> u16 {
        match self {
            Kind::Freshness => 401,
            Kind::Verification => 403,
            Kind::NoProvider => 404,
            Kind::Invalid => 400,
            Kind::Unavailable => 500,
        }
    }
}

/// A refusal: kind + status + detail. Every policy-row refusal lists EVERY failing
/// row (design D11); the first row is the headline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbsError {
    pub kind: Kind,
    pub status: u16,
    pub detail: String,
}

impl KbsError {
    pub fn new(kind: Kind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            status: kind.default_status(),
            detail: detail.into(),
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn freshness(detail: impl Into<String>) -> Self {
        Self::new(Kind::Freshness, detail)
    }

    pub fn verification(detail: impl Into<String>) -> Self {
        Self::new(Kind::Verification, detail)
    }

    /// A `verification` refusal over several failed rows: the first is the headline.
    pub fn verification_rows(rows: &[String]) -> Self {
        Self::new(Kind::Verification, rows.join("; "))
    }

    pub fn no_provider(detail: impl Into<String>) -> Self {
        Self::new(Kind::NoProvider, detail)
    }

    pub fn invalid(detail: impl Into<String>) -> Self {
        Self::new(Kind::Invalid, detail)
    }

    /// Egress failed with no memo, or the request wall was exceeded: 502.
    pub fn unavailable_egress(detail: impl Into<String>) -> Self {
        Self::new(Kind::Unavailable, detail).with_status(502)
    }

    /// In-flight permits exhausted: 503.
    pub fn busy() -> Self {
        Self::new(Kind::Unavailable, "busy").with_status(503)
    }

    /// A broker fault (IO on the policy dir, a bug): 500.
    pub fn fault(detail: impl Into<String>) -> Self {
        Self::new(Kind::Unavailable, detail).with_status(500)
    }
}

impl fmt::Display for KbsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.kind.wire(), self.status, self.detail)
    }
}

impl std::error::Error for KbsError {}
