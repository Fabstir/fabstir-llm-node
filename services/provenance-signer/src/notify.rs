// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Alarms (canary, reconciliation). ntfy in production; a recorder in tests.

use std::sync::Mutex;

pub trait Notifier: Send + Sync {
    fn alarm(&self, title: &str, body: &str);
}

pub struct Ntfy {
    pub url: String,
}

impl Notifier for Ntfy {
    fn alarm(&self, title: &str, body: &str) {
        let r = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .post(&self.url)
            .set("Title", title)
            .set("Priority", "high")
            .set("Tags", "rotating_light")
            .send_string(body);
        if let Err(e) = r {
            tracing::error!("ntfy alarm failed: {e}");
        }
    }
}

/// Logs only (no ntfy configured).
pub struct LogOnly;
impl Notifier for LogOnly {
    fn alarm(&self, title: &str, body: &str) {
        tracing::error!("ALARM {title}: {body}");
    }
}

#[derive(Default)]
pub struct Recorder {
    pub alarms: Mutex<Vec<(String, String)>>,
}
impl Notifier for Recorder {
    fn alarm(&self, title: &str, body: &str) {
        self.alarms
            .lock()
            .unwrap()
            .push((title.to_string(), body.to_string()));
    }
}
