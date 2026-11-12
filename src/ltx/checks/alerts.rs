// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Alerts (D11, D13, D15). An alert names what happened and to which job; it never carries prompt text, picture
//! data or a file path. Sending never blocks a job: the HTTP sink posts from its own task with its own timeout.

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where alerts go. `kind` is a fixed word, `detail` a line of derived figures and identifiers.
pub trait AlertSink: Send + Sync {
    fn alert(&self, kind: &str, detail: &str);
}

/// A log line (always on).
pub struct LogAlerts;
impl AlertSink for LogAlerts {
    fn alert(&self, kind: &str, detail: &str) {
        tracing::error!("LTX-CHECKS-ALERT kind={kind} {detail}");
    }
}

/// HTTP POST of a plain-text line to `LTX_CHECKS_ALERT_URL` (an ntfy topic by default, question 2).
pub struct HttpAlerts {
    url: String,
    client: reqwest::Client,
}

impl HttpAlerts {
    pub fn new(url: &str) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(HttpAlerts {
            url: url.to_string(),
            client,
        })
    }
}

impl AlertSink for HttpAlerts {
    fn alert(&self, kind: &str, detail: &str) {
        let (client, url) = (self.client.clone(), self.url.clone());
        let body = format!("[content checks] {kind}: {detail}");
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(async move {
            let sent = client
                .post(&url)
                .header("Title", "Platformless content checks")
                .header("Priority", "high")
                .body(body)
                .send()
                .await;
            if let Err(e) = sent {
                // the URL may hold a private topic name: log the failure class only
                tracing::warn!(
                    "content-check alert not delivered (timeout={})",
                    e.is_timeout()
                );
            }
        });
    }
}

/// Several sinks at once (the log line plus the push).
pub struct Fanout(pub Vec<Arc<dyn AlertSink>>);
impl AlertSink for Fanout {
    fn alert(&self, kind: &str, detail: &str) {
        for s in &self.0 {
            s.alert(kind, detail);
        }
    }
}

/// Keeps what was sent (tests assert on it; nothing in `src/` builds one).
#[derive(Default)]
pub struct RecordingAlerts(pub Mutex<Vec<(String, String)>>);
impl AlertSink for RecordingAlerts {
    fn alert(&self, kind: &str, detail: &str) {
        if let Ok(mut g) = self.0.lock() {
            g.push((kind.to_string(), detail.to_string()));
        }
    }
}
impl RecordingAlerts {
    pub fn kinds(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|g| g.iter().map(|(k, _)| k.clone()).collect())
            .unwrap_or_default()
    }
    pub fn all(&self) -> Vec<(String, String)> {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }
}
