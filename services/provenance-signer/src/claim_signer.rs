// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The c2pa `Signer` used for every manifest: the sealed key, the chain (EE first, root excluded), the
//! node-chosen reserve, and the TSA list. One instance per signing call (it records which TSA answered).

use crate::keystore::KeyStore;
use crate::tsa::{self, TsaConfig};
use c2pa::{Signer, SigningAlg};
use std::sync::{Arc, Mutex};

pub struct ClaimSigner {
    pub key: Arc<KeyStore>,
    pub chain: Arc<Vec<Vec<u8>>>,
    pub tsa: TsaConfig,
    pub reserve: usize,
    /// Set when a TSA answered; None if timestamping failed or was not attempted.
    pub tsa_used: Arc<Mutex<Option<String>>>,
    pub tsa_error: Arc<Mutex<Option<String>>>,
}

impl ClaimSigner {
    pub fn new(
        key: Arc<KeyStore>,
        chain: Arc<Vec<Vec<u8>>>,
        tsa: TsaConfig,
        reserve: usize,
    ) -> ClaimSigner {
        ClaimSigner {
            key,
            chain,
            tsa,
            reserve,
            tsa_used: Arc::new(Mutex::new(None)),
            tsa_error: Arc::new(Mutex::new(None)),
        }
    }
}

impl Signer for ClaimSigner {
    fn sign(&self, data: &[u8]) -> c2pa::Result<Vec<u8>> {
        self.key
            .sign(data)
            .map_err(|e| c2pa::Error::BadParam(format!("key: {e}")))
    }
    fn alg(&self) -> SigningAlg {
        SigningAlg::Es256
    }
    fn certs(&self) -> c2pa::Result<Vec<Vec<u8>>> {
        Ok(self.chain.as_ref().clone())
    }
    fn reserve_size(&self) -> usize {
        self.reserve
    }
    fn send_timestamp_request(&self, message: &[u8]) -> Option<c2pa::Result<Vec<u8>>> {
        // Never sign without a timestamp: an empty list is an error, not "skip".
        Some(match tsa::timestamp(&self.tsa, message) {
            Ok((reply, url, _)) => {
                *self.tsa_used.lock().unwrap() = Some(url);
                Ok(reply)
            }
            Err(e) => {
                *self.tsa_error.lock().unwrap() = Some(e.last.clone());
                Err(c2pa::Error::BadParam(format!(
                    "tsa_unavailable: {}",
                    e.last
                )))
            }
        })
    }
}

/// Load a PEM chain file (EE first).
pub fn load_chain(path: &std::path::Path) -> anyhow::Result<Vec<Vec<u8>>> {
    let data = std::fs::read(path)?;
    let v: Vec<Vec<u8>> = pem::parse_many(&data)?
        .into_iter()
        .filter(|p| p.tag() == "CERTIFICATE")
        .map(|p| p.into_contents())
        .collect();
    anyhow::ensure!(!v.is_empty(), "chain file has no certificates");
    Ok(v)
}
