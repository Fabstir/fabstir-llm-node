// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Headless ComfyUI client (`/prompt`, `/ws`, `/history`, `/system_stats`,
//! `/interrupt`). Mirrors `TranscoderClient` (reqwest) but adds the WS progress
//! stream ComfyUI uses, which the transcoder did not have.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::debug;

use crate::ltx::template::Graph;

const HTTP_TIMEOUT_SECS: u64 = 120;

/// A ComfyUI execution-progress event parsed off the `/ws` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// A node started; `node == None` is ComfyUI's "prompt finished" signal.
    Executing { node: Option<String> },
    /// Sampler step progress.
    Progress { value: u32, max: u32 },
    /// A node produced output.
    Executed,
}

/// A reference to one output file from `/history/{prompt_id}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExrRef {
    pub filename: String,
    pub subfolder: String,
    pub type_: String,
}

/// HTTP+WS client for one headless ComfyUI instance.
pub struct ComfyClient {
    client: reqwest::Client,
    endpoint: String,
    client_id: String,
}

impl ComfyClient {
    /// Build a client. `client_id` is unique per instance so concurrent jobs
    /// multiplex on `/ws` without crosstalk.
    pub fn new(endpoint: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()?;
        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            client_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    fn ws_endpoint(&self) -> String {
        // http -> ws, https -> wss
        self.endpoint.replacen("http", "ws", 1)
    }

    /// POST `/prompt` with the pinned graph; returns ComfyUI's `prompt_id`.
    pub async fn submit(&self, graph: &Graph) -> Result<String> {
        let url = format!("{}/prompt", self.endpoint);
        let body = serde_json::json!({ "prompt": graph.0, "client_id": self.client_id });
        let response = self.client.post(&url).json(&body).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("comfyui /prompt returned {}: {}", status, text));
        }
        #[derive(Deserialize)]
        struct PromptResponse {
            prompt_id: String,
        }
        let parsed: PromptResponse = response.json().await?;
        Ok(parsed.prompt_id)
    }

    /// POST `/upload/image` (multipart) so an input image lands in ComfyUI's
    /// `input/` folder for a `LoadImage` node to reference (M1a image-to-video).
    /// Returns the name ComfyUI stored it under — which the patcher then
    /// substitutes into the graph, so we always follow ComfyUI's authoritative
    /// name rather than assuming `filename` survived unchanged. Pass a
    /// content-addressed `filename` (e.g. keccak of the plaintext) so identical
    /// images map to one stable input file under `overwrite`.
    pub async fn upload_image(&self, filename: &str, bytes: Vec<u8>) -> Result<String> {
        let url = format!("{}/upload/image", self.endpoint);
        let part = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());
        let form = reqwest::multipart::Form::new()
            .part("image", part)
            .text("type", "input")
            .text("overwrite", "true");
        let response = self.client.post(&url).multipart(form).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "comfyui /upload/image returned {}: {}",
                status,
                text
            ));
        }
        #[derive(Deserialize)]
        struct UploadResponse {
            name: String,
        }
        let parsed: UploadResponse = response.json().await?;
        Ok(parsed.name)
    }

    /// Stream progress for `prompt_id` over `/ws` into `tx` until the prompt
    /// finishes (`executing{node:null}`). Enforces `timeout_secs`, hard-killing a
    /// stuck graph via `/interrupt`. One `ComfyClient` should own one in-flight
    /// prompt (the Phase-9 handler builds a fresh client per job, giving a unique
    /// `clientId`); the consumer should drain `tx` promptly so WS keepalive pongs
    /// keep flowing.
    pub async fn watch(
        &self,
        prompt_id: &str,
        tx: mpsc::Sender<Progress>,
        timeout_secs: u64,
    ) -> Result<()> {
        let inner = self.watch_inner(prompt_id, &tx);
        match tokio::time::timeout(Duration::from_secs(timeout_secs), inner).await {
            Ok(result) => result,
            Err(_) => {
                let _ = self.interrupt().await;
                Err(anyhow!(
                    "ltx job {prompt_id} timed out after {timeout_secs}s"
                ))
            }
        }
    }

    async fn watch_inner(&self, prompt_id: &str, tx: &mpsc::Sender<Progress>) -> Result<()> {
        let url = format!("{}/ws?clientId={}", self.ws_endpoint(), self.client_id);
        let (mut ws, _) = connect_async(&url)
            .await
            .with_context(|| format!("connecting comfyui ws {url}"))?;
        let mut finished = false;
        while let Some(frame) = ws.next().await {
            let Message::Text(text) = frame? else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            // Ignore frames for other prompts multiplexed on this clientId.
            if let Some(pid) = v.pointer("/data/prompt_id").and_then(Value::as_str) {
                if pid != prompt_id {
                    continue;
                }
            }
            // A failed graph is signalled out-of-band (no `executing{node:null}`);
            // fail fast with the real cause instead of riding the timeout to its end.
            if let Some(kind) = v.get("type").and_then(Value::as_str) {
                if kind == "execution_error" || kind == "execution_interrupted" {
                    let msg = v
                        .pointer("/data/exception_message")
                        .and_then(Value::as_str)
                        .unwrap_or(kind);
                    return Err(anyhow!("comfyui {kind} for {prompt_id}: {msg}"));
                }
            }
            if let Some(p) = parse_progress(&v) {
                let done = matches!(&p, Progress::Executing { node: None });
                // A dropped receiver means nobody is listening: stop watching.
                if tx.send(p).await.is_err() {
                    return Ok(());
                }
                if done {
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            Ok(())
        } else {
            // Stream closed before the terminal frame: a premature disconnect is a
            // failure, not a success (else the handler fetches partial outputs).
            Err(anyhow!(
                "comfyui ws for {prompt_id} closed before completion"
            ))
        }
    }

    /// GET `/history/{prompt_id}`; returns the produced EXR file references.
    pub async fn outputs(&self, prompt_id: &str) -> Result<Vec<ExrRef>> {
        let url = format!("{}/history/{}", self.endpoint, prompt_id);
        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("comfyui /history returned {}", response.status()));
        }
        let body: Value = response.json().await?;
        Ok(parse_history(&body, prompt_id))
    }

    /// GET `/view` for one rendered output file; returns its raw bytes. Lets the
    /// node pull results over HTTP from any reachable ComfyUI (`COMFY_URL`), with no
    /// shared output volume — so ComfyUI can run on the host while the node runs in a
    /// container. `query` URL-encodes each param (filenames/subfolders may have spaces).
    pub async fn download(&self, r: &ExrRef) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(format!("{}/view", self.endpoint))
            .query(&[
                ("filename", r.filename.as_str()),
                ("subfolder", r.subfolder.as_str()),
                ("type", r.type_.as_str()),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "comfyui /view returned {} for {:?}",
                response.status(),
                r.filename
            ));
        }
        Ok(response.bytes().await?.to_vec())
    }

    /// POST `/interrupt` to hard-kill the running graph.
    pub async fn interrupt(&self) -> Result<()> {
        self.client
            .post(format!("{}/interrupt", self.endpoint))
            .send()
            .await?;
        Ok(())
    }

    /// GET `/system_stats`; `true` iff the sidecar answers 2xx.
    pub async fn health(&self) -> bool {
        match self
            .client
            .get(format!("{}/system_stats", self.endpoint))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                debug!("comfyui health check failed: {e}");
                false
            }
        }
    }
}

/// Parse one `/ws` frame into a `Progress` event (pure; unit-tested).
pub fn parse_progress(frame: &Value) -> Option<Progress> {
    match frame.get("type").and_then(Value::as_str)? {
        "progress" => {
            let data = frame.get("data")?;
            Some(Progress::Progress {
                value: data.get("value")?.as_u64()? as u32,
                max: data.get("max")?.as_u64()? as u32,
            })
        }
        "executing" => {
            let node = frame
                .pointer("/data/node")
                .and_then(|n| n.as_str().map(String::from));
            Some(Progress::Executing { node })
        }
        "executed" => Some(Progress::Executed),
        _ => None,
    }
}

/// Parse a `/history/{prompt_id}` body into ordered output file refs (pure).
/// Collects from ANY output bucket a save node emits — `images` (EXR frames),
/// `gifs`/`videos` (a `SaveVideo`/`CreateVideo` clip), etc. — so the pipeline works
/// whether the pinned graph saves an EXR sequence or a single video file.
pub fn parse_history(body: &Value, prompt_id: &str) -> Vec<ExrRef> {
    let mut refs = Vec::new();
    // Index by key directly (a prompt_id is not a JSON-Pointer-safe token).
    let Some(outputs) = body
        .get(prompt_id)
        .and_then(|p| p.get("outputs"))
        .and_then(Value::as_object)
    else {
        return refs;
    };
    for node_out in outputs.values() {
        let Some(node_obj) = node_out.as_object() else {
            continue;
        };
        // Any array-of-file-descriptors bucket counts as produced output.
        for files in node_obj.values().filter_map(Value::as_array) {
            for f in files {
                if let Some(filename) = f.get("filename").and_then(Value::as_str) {
                    refs.push(ExrRef {
                        filename: filename.to_string(),
                        subfolder: f
                            .get("subfolder")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        type_: f
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("output")
                            .to_string(),
                    });
                }
            }
        }
    }
    refs
}
