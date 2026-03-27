// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sidecar-based transcode capacity tracking with cached status.

use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

use super::client::TranscoderClient;
use super::types::SidecarStatus;

/// Cached wrapper around sidecar status with TTL-based refresh.
pub struct CachedSidecarStatus {
    cache: RwLock<Option<(Instant, SidecarStatus)>>,
    ttl: Duration,
}

impl CachedSidecarStatus {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(None),
            ttl,
        }
    }

    /// Return cached status if within TTL, otherwise fetch from sidecar.
    /// On fetch error, returns stale cache if available, `None` otherwise.
    pub async fn get_or_fetch(&self, client: &TranscoderClient) -> Option<SidecarStatus> {
        let stale = {
            let cached = self.cache.read().await;
            match *cached {
                Some((ts, status)) if ts.elapsed() < self.ttl => return Some(status),
                Some((_, status)) => Some(status),
                None => None,
            }
        };
        match client.get_sidecar_status().await {
            Ok(status) => {
                let mut w = self.cache.write().await;
                *w = Some((Instant::now(), status));
                Some(status)
            }
            Err(e) => {
                debug!("Sidecar status fetch failed: {e}");
                stale
            }
        }
    }

    pub async fn has_capacity(&self, client: &TranscoderClient) -> bool {
        self.get_or_fetch(client)
            .await
            .map_or(false, |s| s.has_capacity())
    }
}
