// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingResponse {
    pub content: String,
    pub tokens: u32,
    pub finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_token: Option<String>,
}

pub struct StreamingHandler {
    receiver: mpsc::Receiver<StreamingResponse>,
}

impl StreamingHandler {
    pub fn new(receiver: mpsc::Receiver<StreamingResponse>) -> Self {
        Self { receiver }
    }
}

impl Stream for StreamingHandler {
    type Item = Result<StreamingResponse, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some(response)) => Poll::Ready(Some(Ok(response))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub fn format_sse(response: &StreamingResponse) -> String {
    if response.finish_reason.as_deref() == Some("stop") {
        "data: [DONE]\n\n".to_string()
    } else {
        format!(
            "data: {}\n\n",
            serde_json::to_string(response).unwrap_or_default()
        )
    }
}

pub fn format_websocket_message(msg_type: &str, content: serde_json::Value) -> String {
    serde_json::json!({
        "type": msg_type,
        "content": content,
    })
    .to_string()
}
