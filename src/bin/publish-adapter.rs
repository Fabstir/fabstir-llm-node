// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Publish a serve-back adapter artifact using the node's OWN production path.
//!
//! **Why this exists.** A serve-back `manifestCID` must be a `u`-multibase
//! CAPABILITY CID: base64url of the 0xae envelope that carries the
//! XChaCha20-Poly1305 key, the ciphertext hash and the plaintext CID. It is not
//! a bare content address, so the `blob…` CID a plain bridge PUT returns cannot
//! work and is not merely a different encoding of the same thing.
//!
//! Rather than reimplement that envelope (and the 256 KiB chunked encryption it
//! wraps) in a throwaway script, this calls `training::artifact`, which is what
//! a real training run uses to publish its adapter. Same sharding rule, same
//! encryption, same canonical manifest.
//!
//! Run it where the bridge is reachable, i.e. inside the node's container:
//!
//!   docker cp publish-adapter llm-node-prod:/tmp/
//!   docker cp adapter.gguf   llm-node-prod:/tmp/
//!   docker compose -f docker-compose.prod.yml exec llm-node \
//!     /tmp/publish-adapter /tmp/adapter.gguf

use fabstir_llm_node::storage::{
    enhanced_s5_client::EnhancedS5Client, s5_client::EnhancedS5Backend,
};
use fabstir_llm_node::training::artifact::{upload_artifact_manifest, upload_file_sharded};
use fabstir_llm_node::training::serve::M0_ADAPTER_FILE;

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).ok_or(
        "usage: publish-adapter <adapter.gguf> [s5_base]\n\
         s5_base defaults to $ENHANCED_S5_URL, then http://s5-bridge:5522",
    )?;
    let s5_base = args.get(2).cloned().unwrap_or_else(|| {
        std::env::var("ENHANCED_S5_URL").unwrap_or_else(|_| "http://s5-bridge:5522".to_string())
    });

    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    println!("adapter: {} bytes", bytes.len());
    println!("s5:      {s5_base}");

    let client = EnhancedS5Client::new_legacy(s5_base).map_err(|e| format!("S5 client: {e}"))?;
    let s5 = EnhancedS5Backend::new(client);

    // Sharded + encrypted exactly as a real run publishes it. The file name is
    // checked BY NAME on the serve-back path, so it is not ours to choose.
    let prefix = "home/training/serve-back-test";
    let entry = upload_file_sharded(&s5, prefix, M0_ADAPTER_FILE, &bytes).await?;
    println!(
        "uploaded {} in {} shard(s), sha256 {}",
        entry.name,
        entry.shards.len(),
        entry.sha256
    );

    let manifest = upload_artifact_manifest(&s5, prefix, "adapter", None, &[entry]).await?;
    println!("\nPut this in the seed panel:\n");
    println!("  manifestCID     {}", manifest.manifest_cid);
    println!("  manifestSha256  {}", manifest.manifest_sha256);
    println!("  file            {M0_ADAPTER_FILE}");
    if !manifest.manifest_cid.starts_with('u') {
        return Err(format!(
            "manifestCID {} is not 'u'-multibase; serve-back will refuse it",
            manifest.manifest_cid
        ));
    }
    println!("\n(manifestCID is 'u'-multibase, which is what serve-back requires.)");
    Ok(())
}
