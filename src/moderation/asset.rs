// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Asset intake moderation (B8): image → CSAM hash+match (via the narrow `csam`
//! entry point); `.vtt` subtitle → text scan. Fail-closed everywhere.

use crate::moderation::csam;
use crate::moderation::csam::hashlist::HashListSnapshot;
use crate::moderation::csam::ownhash::OwnHashList;
use crate::moderation::types::{AssetKind, ModerationResult};

/// Launch text-scan list (Q2): mock known-bad URLs + illegal-speech keyword
/// placeholders, swapped for the real admin-vetted list at go-live with no code
/// change. Entries are obvious placeholders — NOT real illegal content.
pub struct TextScanList {
    bad_urls: Vec<String>,
    bad_keywords: Vec<String>,
}

impl TextScanList {
    pub const MOCK_BAD_URLS: &'static [&'static str] = &[
        "http://blocklist.test/abuse-1",
        "http://blocklist.test/abuse-2",
        "http://blocklist.test/abuse-3",
        "https://known-bad.example/csae-a",
        "https://known-bad.example/csae-b",
        "http://takedown.test/url-6",
        "http://takedown.test/url-7",
        "http://takedown.test/url-8",
        "http://takedown.test/url-9",
        "http://takedown.test/url-10",
    ];
    pub const MOCK_BAD_KEYWORDS: &'static [&'static str] = &[
        "BANNED_TEST_PHRASE_1",
        "BANNED_TEST_PHRASE_2",
        "BANNED_TEST_PHRASE_3",
        "BANNED_TEST_PHRASE_4",
        "BANNED_TEST_PHRASE_5",
    ];

    pub fn launch_mock() -> Self {
        Self {
            bad_urls: Self::MOCK_BAD_URLS.iter().map(|s| s.to_string()).collect(),
            bad_keywords: Self::MOCK_BAD_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    /// Scan text; `Some(reason)` if a known-bad URL or keyword is present.
    pub fn scan(&self, text: &str) -> Option<&'static str> {
        let lower = text.to_lowercase();
        if self
            .bad_urls
            .iter()
            .any(|u| lower.contains(&u.to_lowercase()))
        {
            return Some("known-bad-url");
        }
        if self
            .bad_keywords
            .iter()
            .any(|k| lower.contains(&k.to_lowercase()))
        {
            return Some("illegal-speech-keyword");
        }
        None
    }
}

/// Bundles the moderation state so callers (tests, the HTTP handler) get the
/// plan's 2-arg `moderate(kind, bytes)`. Holds a point-in-time list snapshot for
/// launch; periodic refresh is Phase-7 glue.
pub struct AssetModerator {
    snapshot: HashListSnapshot,
    ownhash: OwnHashList,
    max_distance: u32,
    text_list: TextScanList,
}

impl AssetModerator {
    pub fn new(
        snapshot: HashListSnapshot,
        ownhash: OwnHashList,
        max_distance: u32,
        text_list: TextScanList,
    ) -> Self {
        Self {
            snapshot,
            ownhash,
            max_distance,
            text_list,
        }
    }

    /// Moderate one asset. Images go through the narrow `csam` entry point;
    /// subtitles are text-scanned. Fail-closed on any error.
    pub fn moderate(&self, kind: AssetKind, bytes: &[u8]) -> ModerationResult {
        match kind {
            AssetKind::Image | AssetKind::VideoKeyframe => {
                csam::moderate_asset_bytes(bytes, &self.snapshot, &self.ownhash, self.max_distance)
            }
            AssetKind::Subtitle => self.moderate_subtitle(bytes),
        }
    }

    fn moderate_subtitle(&self, bytes: &[u8]) -> ModerationResult {
        match std::str::from_utf8(bytes) {
            Ok(text) => match self.text_list.scan(text) {
                Some(reason) => ModerationResult::flagged(reason),
                None => ModerationResult::cleared(),
            },
            // Unscannable bytes ⇒ fail-closed hold.
            Err(_) => ModerationResult::blocked("subtitle not valid UTF-8"),
        }
    }
}
