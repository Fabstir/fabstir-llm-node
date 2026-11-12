// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Daily count reconciliation (D11): on-chain proofs per host versus mp4 signatures per host over the same
//! window. A mismatch alarms (a vetted host shipping work without marks, or signatures with no billed job).
//! On host1 (LTX only) every proof is an LTX clip; a mixed host needs a model filter (not in L).

use crate::notify::Notifier;
use std::collections::BTreeMap;

/// keccak256("ProofSubmitted(uint256,address,uint256,bytes32,string)")
pub fn proof_submitted_topic() -> String {
    format!(
        "0x{}",
        hex::encode(fabstir_provenance::contract::keccak(
            b"ProofSubmitted(uint256,address,uint256,bytes32,string)"
        ))
    )
}

/// On-chain counts for a time window.
pub trait ProofCounter {
    /// (window start unix, window end unix, proofs per host lower-case hex without 0x)
    fn count(&self, hosts: &[String]) -> anyhow::Result<(u64, u64, BTreeMap<String, u64>)>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub host: String,
    pub proofs: u64,
    pub signatures: u64,
}

/// Compare and alarm. Returns the mismatches.
pub fn reconcile_with_tolerance(
    counter: &dyn ProofCounter,
    signatures: impl Fn(u64, u64) -> anyhow::Result<Vec<(String, u64)>>,
    hosts: &[String],
    tolerance: u64,
    notifier: &dyn Notifier,
) -> anyhow::Result<Vec<Mismatch>> {
    reconcile_inner(counter, signatures, hosts, tolerance, notifier)
}

/// Exact comparison (tolerance 0).
pub fn reconcile(
    counter: &dyn ProofCounter,
    signatures: impl Fn(u64, u64) -> anyhow::Result<Vec<(String, u64)>>,
    hosts: &[String],
    notifier: &dyn Notifier,
) -> anyhow::Result<Vec<Mismatch>> {
    reconcile_inner(counter, signatures, hosts, 0, notifier)
}

fn reconcile_inner(
    counter: &dyn ProofCounter,
    signatures: impl Fn(u64, u64) -> anyhow::Result<Vec<(String, u64)>>,
    hosts: &[String],
    tolerance: u64,
    notifier: &dyn Notifier,
) -> anyhow::Result<Vec<Mismatch>> {
    let (from, to, proofs) = match counter.count(hosts) {
        Ok(x) => x,
        Err(e) => {
            // the error can carry the RPC URL (and a key in it): log it, alarm generically
            tracing::error!("reconciliation: {e:#}");
            notifier.alarm(
                "Provenance reconciliation could not run",
                "the on-chain count failed; see the signer log",
            );
            return Err(e);
        }
    };
    let sigs: BTreeMap<String, u64> = signatures(from, to)?.into_iter().collect();
    let mut all: Vec<&String> = proofs.keys().chain(sigs.keys()).collect();
    all.sort();
    all.dedup();
    let mismatches: Vec<Mismatch> = all
        .into_iter()
        .map(|h| Mismatch {
            host: h.clone(),
            proofs: *proofs.get(h).unwrap_or(&0),
            signatures: *sigs.get(h).unwrap_or(&0),
        })
        .filter(|m| m.proofs.abs_diff(m.signatures) > tolerance)
        .collect();
    if !mismatches.is_empty() {
        let lines: Vec<String> = mismatches
            .iter()
            .map(|m| {
                format!(
                    "0x{}: {} proofs vs {} signatures",
                    m.host, m.proofs, m.signatures
                )
            })
            .collect();
        notifier.alarm(
            "Provenance reconciliation MISMATCH",
            &format!("window {from}..{to}\n{}", lines.join("\n")),
        );
    }
    Ok(mismatches)
}

/// JSON-RPC counter: `ProofSubmitted` logs on the JobMarketplace for the allow-listed hosts over the last
/// `window_blocks` blocks, fetched in `chunk_blocks` ranges.
pub struct RpcCounter {
    pub rpc_url: String,
    pub job_marketplace: String,
    pub window_blocks: u64,
    pub chunk_blocks: u64,
}

impl RpcCounter {
    fn call(&self, method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let resp: String = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .post(&self.rpc_url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())?
            .into_string()?;
        let resp: serde_json::Value = serde_json::from_str(&resp)?;
        if let Some(e) = resp.get("error") {
            anyhow::bail!("{method}: {e}");
        }
        Ok(resp["result"].clone())
    }
    fn hex_u64(v: &serde_json::Value) -> anyhow::Result<u64> {
        let s = v
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("not a hex string"))?;
        Ok(u64::from_str_radix(s.trim_start_matches("0x"), 16)?)
    }
    fn block_time(&self, n: u64) -> anyhow::Result<u64> {
        let b = self.call(
            "eth_getBlockByNumber",
            serde_json::json!([format!("0x{n:x}"), false]),
        )?;
        Self::hex_u64(&b["timestamp"])
    }
}

impl ProofCounter for RpcCounter {
    fn count(&self, hosts: &[String]) -> anyhow::Result<(u64, u64, BTreeMap<String, u64>)> {
        let head = Self::hex_u64(&self.call("eth_blockNumber", serde_json::json!([]))?)?;
        let start = head.saturating_sub(self.window_blocks);
        let (t0, t1) = (self.block_time(start)?, self.block_time(head)?);
        let topic = proof_submitted_topic();
        let host_topics: Vec<String> = hosts
            .iter()
            .map(|h| format!("0x{:0>64}", h.trim_start_matches("0x").to_ascii_lowercase()))
            .collect();
        let mut counts: BTreeMap<String, u64> = hosts
            .iter()
            .map(|h| (h.trim_start_matches("0x").to_ascii_lowercase(), 0))
            .collect();
        let mut from = start;
        while from <= head {
            let to = (from + self.chunk_blocks - 1).min(head);
            let logs = self.call(
                "eth_getLogs",
                serde_json::json!([{"address": self.job_marketplace, "fromBlock": format!("0x{from:x}"), "toBlock": format!("0x{to:x}"), "topics": [topic, null, host_topics]}]),
            )?;
            for l in logs.as_array().cloned().unwrap_or_default() {
                if let Some(t) = l["topics"][2]
                    .as_str()
                    .filter(|t| t.len() == 66 && t.is_ascii())
                {
                    let host = t[26..].to_ascii_lowercase();
                    *counts.entry(host).or_default() += 1;
                }
            }
            from = to + 1;
        }
        Ok((t0, t1 + 1, counts))
    }
}
