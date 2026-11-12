// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! RFC 3161 timestamps (plan D10, T7, contract section 7): the configured list in order, a per-attempt
//! timeout, failover on any error, first success wins. The reply is verified by c2pa before it is used.
//! Nodes never contact a TSA.

use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TsaConfig {
    pub urls: Vec<String>,
    pub timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsaAttempt {
    pub url: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, thiserror::Error)]
#[error("all TSAs failed: {last}")]
pub struct TsaUnavailable {
    pub last: String,
    pub attempts: Vec<TsaAttempt>,
}

/// POST one timestamp query. Returns the TimeStampResp bytes.
fn post(url: &str, body: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(timeout)
        .redirects(0)
        .build();
    let resp = agent
        .post(url)
        .set("Content-Type", "application/timestamp-query")
        .send_bytes(body)
        .map_err(|e| e.to_string())?;
    let ct = resp
        .header("Content-Type")
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ct.starts_with("application/timestamp-reply")
        && !ct.starts_with("application/timestamp-response")
    {
        return Err(format!("unexpected content type {ct:?}"));
    }
    let mut out = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .take(64 * 1024)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    if !granted(&out) {
        return Err("reply is not a granted TimeStampResp with a token".into());
    }
    Ok(out)
}

/// DER: TimeStampResp ::= SEQUENCE { status PKIStatusInfo ::= SEQUENCE { status INTEGER, ... }, timeStampToken ... }.
/// True when status is granted (0) or grantedWithMods (1) and a token follows. c2pa verifies the token itself.
pub fn granted(b: &[u8]) -> bool {
    fn len(b: &[u8], i: usize) -> Option<(usize, usize)> {
        let first = *b.get(i)?;
        if first < 0x80 {
            return Some((first as usize, i + 1));
        }
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 {
            return None;
        }
        let mut l = 0usize;
        for k in 0..n {
            l = (l << 8) | *b.get(i + 1 + k)? as usize;
        }
        Some((l, i + 1 + n))
    }
    (|| -> Option<bool> {
        if *b.first()? != 0x30 {
            return None;
        }
        let (outer, i) = len(b, 1)?;
        if i + outer != b.len() {
            return None;
        }
        if *b.get(i)? != 0x30 {
            return None;
        }
        let (status_len, j) = len(b, i + 1)?;
        if *b.get(j)? != 0x02 {
            return None;
        }
        let (int_len, k) = len(b, j + 1)?;
        if int_len != 1 {
            return None;
        }
        let status = *b.get(k)?;
        let after_status = j + status_len;
        Some((status == 0 || status == 1) && after_status < b.len())
    })()
    .unwrap_or(false)
}

/// Query the TSAs in order for the c2pa `message`; returns (reply, url, attempts).
pub fn timestamp(
    cfg: &TsaConfig,
    message: &[u8],
) -> Result<(Vec<u8>, String, Vec<TsaAttempt>), TsaUnavailable> {
    let body = match c2pa::crypto::time_stamp::default_rfc3161_message(message) {
        Ok(b) => b,
        Err(e) => {
            return Err(TsaUnavailable {
                last: format!("rfc3161 message: {e}"),
                attempts: vec![],
            })
        }
    };
    let mut attempts = vec![];
    let mut last = "no TSA configured".to_string();
    for url in &cfg.urls {
        match post(url, &body, cfg.timeout) {
            Ok(r) => {
                attempts.push(TsaAttempt {
                    url: url.clone(),
                    ok: true,
                    detail: "ok".into(),
                });
                return Ok((r, url.clone(), attempts));
            }
            Err(e) => {
                last = e.clone();
                attempts.push(TsaAttempt {
                    url: url.clone(),
                    ok: false,
                    detail: e,
                });
            }
        }
    }
    Err(TsaUnavailable { last, attempts })
}
