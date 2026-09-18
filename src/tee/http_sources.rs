// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 (P3.2) — the node's HTTP(S) sources for the signed policy and the
//! encrypted container.
//!
//! Neither source is a trust boundary: the policy is authenticated by the
//! provider's EIP-191 signature (`policy.rs`) and the container by its chunked
//! AEAD bound to `model_id ‖ policy_hash` (`container.rs`). TLS here defends
//! against one thing only: a path attacker serving an OLDER but still-valid
//! signed policy to dodge version-based revocation. So `https://` is required
//! for both, except to loopback, which the simulator rounds and the tests use.
//!
//! Trust roots for those fetches: reqwest's bundled Mozilla snapshot
//! (`webpki-roots`) PLUS the container's own store (`rustls-tls-native-roots`,
//! so a root installed with `update-ca-certificates` in the image, e.g. Let's
//! Encrypt's 2026 "ISRG Root YE" that the snapshot predates, is honoured) PLUS
//! the private broker root (`TEE_KBS_CA_FILE`, default
//! `/etc/fabstir/kbs-root.pem`, the one rule in `kbs_http::ca_file_path`), so
//! the policy and the container may be served from `kbs.fabstir.net` itself
//! (its nginx has static `/policies/` and `/blobs/` locations) with no extra
//! client configuration. The broker client (`kbs_http.rs`) is stricter
//! and pins the private root alone.
//!
//! Bodies are bounded: a policy is a few KB ([`MAX_POLICY_BYTES`]); a container
//! is the model, capped by `TEE_BLOB_MAX_BYTES` (default 2 GiB) because it is
//! held in RAM until decrypt (the streaming decrypt is P3 follow-up work before
//! the target model; see the tracker).

use crate::tee::kbs_http::read_ca_pem;
use crate::tee::model_source::BlobSource;
use crate::tee::policy::SignedModelPolicy;
use crate::tee::policy_source::PolicySource;
use crate::tee::types::{TeeError, TeeResult};
use async_trait::async_trait;
use std::time::Duration;
use tokio_stream::StreamExt;

/// Env: where the signed policy is. May contain `{model_id}` (hex, no 0x); if it
/// does not, the URL is fetched as-is and the returned policy must be for the
/// requested model.
pub const POLICY_URL_ENV: &str = "TEE_POLICY_URL";
/// Env: base URL the container's `encrypted_ref` is resolved against. A
/// relative ref is joined under it; an absolute `https://` ref is accepted
/// only if it resolves strictly under this base on the same origin (see
/// `HttpBlobSource::url_for`).
pub const BLOB_URL_ENV: &str = "TEE_BLOB_URL";
/// Env: cap on the container size in bytes (default [`DEFAULT_BLOB_MAX`]).
pub const BLOB_MAX_ENV: &str = "TEE_BLOB_MAX_BYTES";
pub const MAX_POLICY_BYTES: usize = 256 * 1024;
pub const DEFAULT_BLOB_MAX: u64 = 2 * 1024 * 1024 * 1024;

fn require_https_or_loopback(url: &str, what: &str) -> TeeResult<()> {
    // A real parse, not a prefix scan: userinfo (`http://localhost:1@evil/`) and
    // bracketed IPv6 (`[::1]`) both defeat splitting on ':' and '/'.
    let parsed = url::Url::parse(url)
        .map_err(|e| TeeError::Fetch(format!("{what}: not a URL: {e} ({url})")))?;
    match (parsed.scheme(), parsed.host()) {
        ("https", Some(_)) => Ok(()),
        ("http", Some(url::Host::Ipv4(ip))) if ip.is_loopback() => Ok(()),
        ("http", Some(url::Host::Ipv6(ip))) if ip.is_loopback() => Ok(()),
        ("http", Some(url::Host::Domain("localhost"))) => Ok(()),
        _ => Err(TeeError::Fetch(format!(
            "{what} must be https:// (or http:// to loopback for the simulator/tests); got {url}"
        ))),
    }
}

/// The client: bundled + native roots, plus `extra_root_pem` (one or more PEM
/// certificates) when given. No redirects: a 302 to http:// would undo the
/// https rule, which exists precisely to stop a downgrade to an older
/// still-valid policy.
fn client(timeout: Duration, extra_root_pem: Option<&[u8]>) -> TeeResult<reqwest::Client> {
    let extra: Vec<reqwest::Certificate> = match extra_root_pem {
        Some(pem) => {
            let certs = reqwest::Certificate::from_pem_bundle(pem)
                .map_err(|e| TeeError::Fetch(format!("extra root PEM: {e}")))?;
            if certs.is_empty() {
                return Err(TeeError::Fetch(
                    "extra root PEM holds no certificate".into(),
                ));
            }
            certs
        }
        None => Vec::new(),
    };
    let build = |built_in_roots: bool| {
        let mut b = reqwest::Client::builder()
            .use_rustls_tls()
            .redirect(reqwest::redirect::Policy::none())
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10))
            .no_proxy()
            .tls_built_in_root_certs(built_in_roots)
            .user_agent(format!(
                "fabstir-llm-node/{}",
                crate::version::VERSION_NUMBER
            ));
        for c in extra.iter().cloned() {
            b = b.add_root_certificate(c);
        }
        b.build()
    };
    // With built-in roots on, reqwest also loads the OS store and fails the
    // build if that store is unusable (a stray SSL_CERT_FILE, a store of
    // unparsable certificates). Probe the store EXPLICITLY, so the decision is
    // about the store and any other builder error surfaces as itself. The
    // private broker root alone is enough to reach kbs.fabstir.net, so an
    // attested boot must not depend on OS-store health when it has that root.
    // reqwest 0.11 gates the bundled webpki snapshot behind the same flag as
    // the OS store, so in that mode a policy/blob host under a public CA is
    // unreachable (the warning says so; fix the store).
    let built_in_roots = match rustls_native_certs::load_native_certs() {
        Ok(_) => true,
        Err(e) if !extra.is_empty() => {
            eprintln!(
                "⚠️  TEE fetch client: OS root store unusable ({e}); continuing with the private broker root only \
                 (a policy/blob host under a public CA is unreachable until the store is fixed)"
            );
            false
        }
        Err(e) => {
            return Err(TeeError::Fetch(format!(
                "OS root store unusable and no private root to fall back on: {e}"
            )))
        }
    };
    match build(built_in_roots) {
        Ok(c) => Ok(c),
        // reqwest applies one more rule the probe cannot see: a store that
        // loads but holds no parseable certificate fails the build too. Same
        // decision, made on reqwest's rule: with a private root, fall back.
        Err(e) if built_in_roots && !extra.is_empty() => {
            eprintln!(
                "⚠️  TEE fetch client: OS root store rejected by the TLS builder ({e}); continuing with the private broker root only \
                 (a policy/blob host under a public CA is unreachable until the store is fixed)"
            );
            build(false).map_err(|e| TeeError::Fetch(format!("build http client: {e}")))
        }
        Err(e) => Err(TeeError::Fetch(format!("build http client: {e}"))),
    }
}

/// The private broker root, by the same rule the broker client uses
/// (`TEE_KBS_CA_FILE`, else the image's default path). Missing = error: these
/// sources are only built on the attested path, which needs the root anyway.
fn kbs_root_from_env() -> TeeResult<Vec<u8>> {
    read_ca_pem().map_err(|e| TeeError::Fetch(e.to_string()))
}

/// GET `url`, refusing anything above `max` bytes (never truncating).
async fn get_bounded(client: &reqwest::Client, url: &str, max: u64) -> TeeResult<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| TeeError::Fetch(format!("GET {url}: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(TeeError::Fetch(format!("GET {url}: HTTP {status}")));
    }
    let mut out: Vec<u8> = Vec::new();
    if let Some(len) = resp.content_length() {
        if len > max {
            return Err(TeeError::Fetch(format!(
                "GET {url}: {len} bytes exceeds the {max}-byte bound"
            )));
        }
        // Reserve once: growth by doubling would hold ~3× the bytes so far at
        // each reallocation, on top of the tmpfs plaintext written next, inside
        // a CVM whose RAM also backs the decrypt dir. Fallible, so a hostile
        // Content-Length is a clean error, never an abort.
        out.try_reserve_exact(len as usize)
            .map_err(|e| TeeError::Fetch(format!("GET {url}: cannot reserve {len} bytes: {e}")))?;
    }
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| TeeError::Fetch(format!("GET {url}: body: {e}")))?;
        if (out.len() + chunk.len()) as u64 > max {
            return Err(TeeError::Fetch(format!(
                "GET {url}: body exceeds the {max}-byte bound"
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Fetches the provider's `SignedModelPolicy` JSON over HTTP(S).
#[derive(Debug, Clone)]
pub struct HttpPolicySource {
    url_template: String,
    timeout: Duration,
    client: reqwest::Client,
}

impl HttpPolicySource {
    fn build(url_template: &str, timeout: Duration, extra_root: Option<&[u8]>) -> TeeResult<Self> {
        let url_template = url_template.trim().to_string();
        require_https_or_loopback(&url_template, POLICY_URL_ENV)?;
        Ok(Self {
            url_template,
            timeout,
            client: client(timeout, extra_root)?,
        })
    }

    pub fn new(url_template: &str, timeout: Duration) -> TeeResult<Self> {
        Self::build(url_template, timeout, None)
    }

    /// As [`new`](Self::new), also trusting `pem` (the private broker root),
    /// built once; the production constructor (`from_env`).
    pub fn new_with_extra_root(
        url_template: &str,
        timeout: Duration,
        pem: &[u8],
    ) -> TeeResult<Self> {
        Self::build(url_template, timeout, Some(pem))
    }

    /// Also trust the certificate(s) in `pem` (the private broker root).
    /// Rebuilds the client; production goes through `from_env`, which builds
    /// once with the root in place.
    pub fn with_extra_root(self, pem: &[u8]) -> TeeResult<Self> {
        Self::build(&self.url_template, self.timeout, Some(pem))
    }

    pub fn from_env() -> TeeResult<Self> {
        let url = std::env::var(POLICY_URL_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| TeeError::Fetch(format!("{POLICY_URL_ENV} is not set")))?;
        Self::new_with_extra_root(&url, Duration::from_secs(30), &kbs_root_from_env()?)
    }

    fn url_for(&self, model_id: [u8; 32]) -> String {
        self.url_template
            .replace("{model_id}", &hex::encode(model_id))
    }
}

#[async_trait]
impl PolicySource for HttpPolicySource {
    async fn fetch_policy(&self, model_id: [u8; 32]) -> TeeResult<SignedModelPolicy> {
        let url = self.url_for(model_id);
        let body = get_bounded(&self.client, &url, MAX_POLICY_BYTES as u64).await?;
        let signed: SignedModelPolicy = serde_json::from_slice(&body)
            .map_err(|e| TeeError::Fetch(format!("GET {url}: policy JSON: {e}")))?;
        // Whatever the URL served, it must be THIS model's policy; the signature
        // check (`fetch_validated_policy`) comes after and stays the authority.
        if signed.policy.model_id != model_id {
            return Err(TeeError::VerificationFailed(format!(
                "policy at {url} is for model {}, not {}",
                hex::encode(signed.policy.model_id),
                hex::encode(model_id)
            )));
        }
        Ok(signed)
    }
}

/// Fetches the encrypted container over HTTP(S), bounded.
#[derive(Debug, Clone)]
pub struct HttpBlobSource {
    base: String,
    max_bytes: u64,
    timeout: Duration,
    client: reqwest::Client,
}

impl HttpBlobSource {
    fn build(
        base: &str,
        max_bytes: u64,
        timeout: Duration,
        extra_root: Option<&[u8]>,
    ) -> TeeResult<Self> {
        let base = base.trim().trim_end_matches('/').to_string();
        require_https_or_loopback(&base, BLOB_URL_ENV)?;
        // Relative refs are string-joined under the base path, so the base must
        // be a plain origin + path: a query or fragment would end up in the
        // middle of every joined URL and make every ref look malformed.
        let parsed = url::Url::parse(&base)
            .map_err(|e| TeeError::Fetch(format!("{BLOB_URL_ENV}: not a URL: {e}")))?;
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(TeeError::Fetch(format!(
                "{BLOB_URL_ENV} must be an origin plus path with no query or fragment; got {base}"
            )));
        }
        Ok(Self {
            base,
            max_bytes,
            timeout,
            client: client(timeout, extra_root)?,
        })
    }

    pub fn new(base: &str, max_bytes: u64, timeout: Duration) -> TeeResult<Self> {
        Self::build(base, max_bytes, timeout, None)
    }

    /// As [`new`](Self::new), also trusting the certificate(s) in `pem` (the
    /// private broker root), built once. This is the production constructor
    /// (`from_env`); with the root present the client survives an unusable OS
    /// root store (see `client`).
    pub fn new_with_extra_root(
        base: &str,
        max_bytes: u64,
        timeout: Duration,
        pem: &[u8],
    ) -> TeeResult<Self> {
        Self::build(base, max_bytes, timeout, Some(pem))
    }

    /// Also trust the certificate(s) in `pem` (the private broker root).
    /// Rebuilds the client; production goes through `from_env`, which builds
    /// once with the root in place.
    pub fn with_extra_root(self, pem: &[u8]) -> TeeResult<Self> {
        Self::build(&self.base, self.max_bytes, self.timeout, Some(pem))
    }

    pub fn from_env() -> TeeResult<Self> {
        let base = std::env::var(BLOB_URL_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| TeeError::Fetch(format!("{BLOB_URL_ENV} is not set")))?;
        let max = match std::env::var(BLOB_MAX_ENV) {
            Ok(v) if !v.trim().is_empty() => v
                .trim()
                .parse::<u64>()
                .map_err(|_| TeeError::Fetch(format!("{BLOB_MAX_ENV}: not a number: {v}")))?,
            _ => DEFAULT_BLOB_MAX,
        };
        // A large model takes minutes to fetch; the whole-request timeout must
        // cover it. 30 min is generous for anything the tmpfs could hold.
        Self::new_with_extra_root(
            &base,
            max,
            Duration::from_secs(30 * 60),
            &kbs_root_from_env()?,
        )
    }

    /// Resolve `encrypted_ref`. An absolute URL is accepted only on the SAME
    /// origin as `TEE_BLOB_URL`: `encrypted_ref` is outside the policy signature
    /// (`SignedModelPolicy` signs `policy` alone), so an unrestricted absolute
    /// ref would let a compromised policy host steer every boot into a
    /// 30-minute, up-to-`TEE_BLOB_MAX_BYTES` download from anywhere before the
    /// AEAD binding fails. Same origin = the operator's own configured host.
    /// Resolve `encrypted_ref` to the URL to GET. Absolute or relative is decided
    /// by a real parse (schemes are case-insensitive: `HTTPS://…` is absolute
    /// too), never a prefix scan; only `http(s)://host` counts as absolute.
    /// EITHER form must resolve, after the URL parser has normalised it (`..`,
    /// `%2e%2e`, `\` as `/` in special schemes), to a path UNDER the base path
    /// on the base's origin with no query or fragment: `encrypted_ref` is
    /// outside the policy signature, so it must not be able to aim the
    /// 30-minute, `TEE_BLOB_MAX_BYTES` fetch at another object anywhere,
    /// including elsewhere on the operator's own origin.
    fn url_for(&self, path: &str) -> TeeResult<String> {
        if path.trim().is_empty() {
            return Err(TeeError::Fetch(format!(
                "encrypted_ref is empty; the policy must name an object under {BLOB_URL_ENV}"
            )));
        }
        let base = url::Url::parse(&self.base)
            .map_err(|e| TeeError::Fetch(format!("{BLOB_URL_ENV}: not a URL: {e}")))?;
        let u = match url::Url::parse(path) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.has_host() => {
                require_https_or_loopback(path, "encrypted_ref")?;
                u
            }
            // `scheme://host…` in another scheme: the S5 source's `s5://…` and the
            // like. Not a path on this origin; say what to use instead.
            Ok(u) if u.has_host() => {
                return Err(TeeError::Fetch(format!(
                "encrypted_ref {path} is a `{}://` reference, which this HTTP blob source cannot \
                     fetch; the policy must carry a path relative to {BLOB_URL_ENV} or an absolute \
                     https URL under it",
                u.scheme()
            )))
            }
            // Everything else is a path relative to the base, including names with
            // a colon in a segment (`llama-3.1:8b-q4.enc`), which `Url::parse`
            // would otherwise read as a scheme.
            _ => {
                let joined = format!("{}/{}", self.base, path.trim_start_matches('/'));
                url::Url::parse(&joined)
                    .map_err(|e| TeeError::Fetch(format!("encrypted_ref {path}: {e}")))?
            }
        };
        let prefix = format!("{}/", base.path().trim_end_matches('/'));
        // Strictly under the base: the base directory itself (`u.path() == prefix`)
        // is "another object" too. No percent-encoding in the resolved path
        // either: the parser keeps `%2F` as a segment character, but a server
        // that decodes before dot-segment removal (nginx) would read
        // `..%2F..%2Fother` as a climb the prefix check cannot see. Model file
        // names are plain ASCII paths; anything needing encoding is refused.
        let under_base = u.scheme() == base.scheme()
            && u.host() == base.host()
            && u.port_or_known_default() == base.port_or_known_default()
            && u.path().starts_with(&prefix)
            && u.path().len() > prefix.len()
            && !u.path().contains('%')
            && u.query().is_none()
            && u.fragment().is_none();
        if !under_base {
            return Err(TeeError::Fetch(format!(
                "encrypted_ref {path} must resolve under {BLOB_URL_ENV} \
                 (resolved to {u}, not under {prefix} on the same origin, or carries \
                 percent-encoding, a query or a fragment)"
            )));
        }
        Ok(u.to_string())
    }
}

#[async_trait]
impl BlobSource for HttpBlobSource {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>> {
        let url = self.url_for(path)?;
        get_bounded(&self.client, &url, self.max_bytes).await
    }
}
