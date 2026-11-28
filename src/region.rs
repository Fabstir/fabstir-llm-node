// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `FABSTIR_REGION` start check (UKH1 U3, plan D17, SR1 D3b).
//!
//! Unset or `global` is today's node: nothing is checked. `uk` is a host
//! whose user content must stay in the UK, so the node refuses to start with
//! anything that would send it elsewhere:
//! - web search enabled (queries go to outside search providers). The same
//!   reading as `SearchConfig::from_env`, so an UNSET `WEB_SEARCH_ENABLED`
//!   (which turns search on) is refused too;
//! - video generation configured (`COMFY_URL` non-empty or `RELIGHT_URL`
//!   non-empty, as `main.rs` decides it) without `LTX_CHECKS_ALERT_URL` set
//!   explicitly (present and non-empty). Video off needs no alert sink.
//!
//! Any other region value (including `eu`, not configured yet) refuses.

/// The regions this node knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Global,
    Uk,
}

impl Region {
    pub fn as_str(&self) -> &'static str {
        match self {
            Region::Global => "global",
            Region::Uk => "uk",
        }
    }
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Decide the region and check the settings it forbids. `Err` carries the
/// message the node stops with.
pub fn check_region_start(get: impl Fn(&str) -> Option<String>) -> Result<Region, String> {
    let region = match non_empty(get("FABSTIR_REGION")) {
        None => return Ok(Region::Global),
        Some(v) if v.eq_ignore_ascii_case("global") => return Ok(Region::Global),
        Some(v) if v.eq_ignore_ascii_case("uk") => Region::Uk,
        Some(v) => {
            return Err(format!(
                "FABSTIR_REGION={v:?} is not a configured region; use global (or unset) or uk"
            ))
        }
    };

    if crate::search::config::web_search_enabled(get("WEB_SEARCH_ENABLED").as_deref()) {
        return Err(format!(
            "FABSTIR_REGION={}: web search sends queries to providers outside the region; set WEB_SEARCH_ENABLED=false (unset means enabled)",
            region.as_str()
        ));
    }

    let video_configured =
        non_empty(get("COMFY_URL")).is_some() || non_empty(get("RELIGHT_URL")).is_some();
    if video_configured && non_empty(get("LTX_CHECKS_ALERT_URL")).is_none() {
        return Err(format!(
            "FABSTIR_REGION={}: video generation is configured (COMFY_URL or RELIGHT_URL) but LTX_CHECKS_ALERT_URL is not set; set the content-check alert sink for this region explicitly, or switch video off",
            region.as_str()
        ));
    }

    Ok(region)
}
