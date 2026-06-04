// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Track-1 matching engine — exact (SHA-256 + own-hash) now; PDQ near-match in
//! Phase 4. 🚨
//!
//! Fail-closed: matching against an `Unavailable` NCMEC list yields
//! `Err(ListUnavailable)` (§3.4). Exact-match is **bit-identical only** — it does
//! NOT detect re-encoded/transcoded CSAM (§3.5); that is PDQ's job. An own-hash
//! (locally-confirmed) hit is a definitive block regardless of NCMEC availability.

use sha2::{Digest, Sha256};

use crate::moderation::csam::hashlist::HashListSnapshot;
use crate::moderation::csam::ownhash::OwnHashList;
use crate::moderation::csam::pdq;
use crate::moderation::types::{MatchResult, Pdq256, Result};

pub struct Matcher<'a> {
    snapshot: &'a HashListSnapshot,
    ownhash: &'a OwnHashList,
}

impl<'a> Matcher<'a> {
    pub fn new(snapshot: &'a HashListSnapshot, ownhash: &'a OwnHashList) -> Self {
        Self { snapshot, ownhash }
    }

    /// SHA-256 of the given bytes.
    pub fn sha256(bytes: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().into()
    }

    /// Exact-match a precomputed SHA-256. Own-hash (locally-confirmed) is a
    /// definitive block regardless of NCMEC list state; otherwise the NCMEC list
    /// must be available (fail-closed `Err(ListUnavailable)` if not).
    pub fn match_sha256(&self, sha256: &[u8; 32]) -> Result<MatchResult> {
        if self.ownhash.contains(sha256) {
            return Ok(MatchResult::exact());
        }
        self.snapshot.require_available()?;
        if self.snapshot.sha256.contains(sha256) {
            Ok(MatchResult::exact())
        } else {
            Ok(MatchResult::no_match())
        }
    }

    /// Exact-match raw bytes (hashes them first).
    pub fn match_bytes(&self, bytes: &[u8]) -> Result<MatchResult> {
        let h = Self::sha256(bytes);
        self.match_sha256(&h)
    }

    /// PDQ near-match against the list within `max_distance` (config-driven, §4.2).
    /// Fail-closed: an Unavailable list ⇒ `Err(ListUnavailable)`, never "no match".
    /// Reports the minimum Hamming distance when it matches (§3.5: the supplied
    /// hash may be computed in-node from frames OR pre-supplied by the transcoder).
    pub fn match_pdq(&self, query: &Pdq256, max_distance: u32) -> Result<MatchResult> {
        self.snapshot.require_available()?;
        match self
            .snapshot
            .pdq
            .iter()
            .map(|h| pdq::hamming(query, h))
            .min()
        {
            Some(d) if d <= max_distance => Ok(MatchResult::near(d)),
            _ => Ok(MatchResult::no_match()),
        }
    }

    /// Full Track-1 decision: SHA-256/own-hash exact prefilter (short-circuits on a
    /// hit — PDQ is not consulted), then PDQ near-match if a `pdq` hash is supplied.
    pub fn match_content(
        &self,
        sha256: &[u8; 32],
        pdq: Option<&Pdq256>,
        max_distance: u32,
    ) -> Result<MatchResult> {
        let exact = self.match_sha256(sha256)?;
        if exact.is_match {
            return Ok(exact); // bit-identical / own-hash hit ⇒ no need for PDQ
        }
        match pdq {
            Some(p) => self.match_pdq(p, max_distance),
            None => Ok(MatchResult::no_match()),
        }
    }
}
