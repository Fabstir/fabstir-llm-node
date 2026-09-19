// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The broker's only way onto the network (design §12): reqwest 0.11 built with
//! `.use_rustls_tls()` (the crate enables `default-tls` too, so the builder's
//! default would be OpenSSL) and native roots; redirects off; no proxy; GET and
//! POST; per-request timeouts; a host allow-list checked on the PARSED URL
//! (scheme `https`, no userinfo, host and port equal to an allowed pair) before
//! any connection. dcap-qvl fetches the root CRL from a distribution point it
//! extracts from a certificate BEFORE chain validation, so a hostile PCCS controls
//! that string; the allow-list is what bounds it.
//!
//! Test hooks (constructor arguments, as `HttpKeyBrokerClient::new` has): an extra
//! root PEM and a `(host, addr)` resolve pin, so a loopback fake with an rcgen root
//! is reachable as `https://kbs.test:<port>/` and allow-listed by being the
//! configured URL.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum EgressError {
    /// The URL failed the allow-list; no connection was attempted.
    #[error("egress refused: {0}")]
    NotAllowed(String),
    /// Connection, TLS, timeout, or the client could not be built.
    #[error("egress transport: {0}")]
    Transport(String),
    /// The body exceeded the caller's cap.
    #[error("egress body over {0} bytes")]
    TooLarge(usize),
}

/// A complete response (status, headers as UTF-8 strings, bounded body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Test hooks; production passes `EgressOptions::default()`.
#[derive(Debug, Default, Clone)]
pub struct EgressOptions {
    /// An extra trusted root (PEM), for a loopback fake under test.
    pub extra_root_pem: Option<Vec<u8>>,
    /// Pin `host` to `addr` (reqwest ignores the port in `addr`; the URL's port is used).
    pub resolve: Option<(String, SocketAddr)>,
    /// Further pins (a fake standing in for several hosts).
    pub resolve_extra: Vec<(String, SocketAddr)>,
}

#[derive(Clone)]
pub struct EgressClient {
    client: reqwest::Client,
    allowed: Vec<(String, u16)>,
}

impl EgressClient {
    /// `allowed` = `(host lowercase, port)` pairs (`KbsConfig::allowed_hosts`).
    pub fn new(allowed: Vec<(String, u16)>, opts: EgressOptions) -> Result<Self, EgressError> {
        let mut b = reqwest::Client::builder()
            .use_rustls_tls()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(format!("fabstir-kbs/{}", crate::version::VERSION_NUMBER));
        if let Some(pem) = &opts.extra_root_pem {
            let cert = reqwest::Certificate::from_pem(pem)
                .map_err(|e| EgressError::Transport(format!("extra root: {e}")))?;
            b = b.add_root_certificate(cert);
        }
        if let Some((host, addr)) = &opts.resolve {
            b = b.resolve(host, *addr);
        }
        for (host, addr) in &opts.resolve_extra {
            b = b.resolve(host, *addr);
        }
        let client = b
            .build()
            .map_err(|e| EgressError::Transport(format!("client: {e}")))?;
        Ok(Self { client, allowed })
    }

    /// The allow-list rule (design §12). Returns the parsed URL on success.
    pub fn check_url(&self, url: &str) -> Result<Url, EgressError> {
        let u = Url::parse(url).map_err(|e| EgressError::NotAllowed(format!("{url}: {e}")))?;
        if u.scheme() != "https" {
            return Err(EgressError::NotAllowed(format!(
                "{url}: scheme is not https"
            )));
        }
        if !u.username().is_empty() || u.password().is_some() {
            return Err(EgressError::NotAllowed(format!("{url}: userinfo")));
        }
        let host = u
            .host_str()
            .ok_or_else(|| EgressError::NotAllowed(format!("{url}: no host")))?
            .to_ascii_lowercase();
        let port = u.port().unwrap_or(443);
        if !self.allowed.iter().any(|(h, p)| *h == host && *p == port) {
            return Err(EgressError::NotAllowed(format!(
                "{host}:{port} is not an allowed egress host"
            )));
        }
        Ok(u)
    }

    pub async fn get(
        &self,
        url: &str,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Response, EgressError> {
        let u = self.check_url(url)?;
        let req = self.client.get(u).timeout(timeout);
        self.send(req, max_bytes).await
    }

    pub async fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Response, EgressError> {
        let u = self.check_url(url)?;
        let req = self
            .client
            .post(u)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .timeout(timeout);
        self.send(req, max_bytes).await
    }

    async fn send(
        &self,
        req: reqwest::RequestBuilder,
        max_bytes: usize,
    ) -> Result<Response, EgressError> {
        let mut resp = req
            .send()
            .await
            .map_err(|e| EgressError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let mut headers = BTreeMap::new();
        for (k, v) in resp.headers() {
            if let Ok(s) = v.to_str() {
                headers.insert(k.as_str().to_string(), s.to_string());
            }
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| EgressError::Transport(e.to_string()))?
        {
            if body.len() + chunk.len() > max_bytes {
                return Err(EgressError::TooLarge(max_bytes));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}
