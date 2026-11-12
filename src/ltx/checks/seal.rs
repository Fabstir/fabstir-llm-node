// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sealing for the strongest signal only (D15): an ephemeral secp256k1 key agreement with `LTX_EVIDENCE_PUBKEY`,
//! HKDF-SHA256, XChaCha20-Poly1305. The node keeps nothing that can open a sealed item; the private key lives
//! offline with the named person and the `open-evidence` tool.
//!
//! File layout: `MAGIC (8) | ephemeral public key (33) | nonce (24) | ciphertext`. The plaintext is
//! `u32 big-endian record length | record JSON | item bytes`; `MAGIC | ephemeral key` is the associated data.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{PublicKey, SecretKey};
use rand::RngCore;
use sha2::Sha256;

pub const MAGIC: &[u8; 8] = b"FLTXEV1\n";
const INFO: &[u8] = b"fabstir-ltx-evidence-v1";
const HEADER: usize = 8 + 33 + 24;

/// Parse `LTX_EVIDENCE_PUBKEY`: hex (optional `0x`) of a compressed (33-byte) or uncompressed (65-byte) key.
pub fn parse_public_key(hex_key: &str) -> Result<[u8; 33], String> {
    let raw = hex::decode(hex_key.trim().trim_start_matches("0x"))
        .map_err(|_| "LTX_EVIDENCE_PUBKEY is not hex".to_string())?;
    let pk = PublicKey::from_sec1_bytes(&raw)
        .map_err(|_| "LTX_EVIDENCE_PUBKEY is not a secp256k1 public key".to_string())?;
    Ok(compress(&pk))
}

fn compress(pk: &PublicKey) -> [u8; 33] {
    let mut out = [0u8; 33];
    out.copy_from_slice(pk.to_encoded_point(true).as_bytes());
    out
}

/// The compressed public key of a private key (for the offline tool).
pub fn public_key_of(private_key: &[u8; 32]) -> Result<[u8; 33], String> {
    let sk = SecretKey::from_slice(private_key).map_err(|_| "not a secp256k1 private key")?;
    Ok(compress(&sk.public_key()))
}

fn derive_key(
    shared: &[u8],
    ephemeral: &[u8; 33],
    recipient: &[u8; 33],
) -> Result<[u8; 32], String> {
    let mut info = INFO.to_vec();
    info.extend_from_slice(ephemeral);
    info.extend_from_slice(recipient);
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, shared)
        .expand(&info, &mut key)
        .map_err(|_| "key derivation failed".to_string())?;
    Ok(key)
}

/// Seal `item` and its `record` (derived figures) to `recipient`.
pub fn seal(
    recipient: &[u8; 33],
    record: &serde_json::Value,
    item: &[u8],
) -> Result<Vec<u8>, String> {
    let recipient_pk =
        PublicKey::from_sec1_bytes(recipient).map_err(|_| "the evidence key is not valid")?;
    let eph = SecretKey::random(&mut rand::thread_rng());
    let eph_pub = compress(&eph.public_key());
    let shared = k256::ecdh::diffie_hellman(eph.to_nonzero_scalar(), recipient_pk.as_affine());
    let key = derive_key(shared.raw_secret_bytes(), &eph_pub, recipient)?;
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let record = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    let mut plain = Vec::with_capacity(4 + record.len() + item.len());
    plain.extend_from_slice(&(record.len() as u32).to_be_bytes());
    plain.extend_from_slice(&record);
    plain.extend_from_slice(item);
    let mut out = Vec::with_capacity(HEADER + plain.len() + 16);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&eph_pub);
    let ct = XChaCha20Poly1305::new((&key).into())
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plain,
                aad: &out,
            },
        )
        .map_err(|_| "sealing failed".to_string())?;
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Open a sealed file with the private key. Returns the record and the item.
pub fn open(private_key: &[u8; 32], sealed: &[u8]) -> Result<(serde_json::Value, Vec<u8>), String> {
    if sealed.len() < HEADER + 16 || &sealed[..8] != MAGIC {
        return Err("not a sealed evidence file".to_string());
    }
    let sk = SecretKey::from_slice(private_key).map_err(|_| "not a secp256k1 private key")?;
    let mut eph_pub = [0u8; 33];
    eph_pub.copy_from_slice(&sealed[8..41]);
    let eph_pk = PublicKey::from_sec1_bytes(&eph_pub).map_err(|_| "the file's key is damaged")?;
    let shared = k256::ecdh::diffie_hellman(sk.to_nonzero_scalar(), eph_pk.as_affine());
    let key = derive_key(
        shared.raw_secret_bytes(),
        &eph_pub,
        &compress(&sk.public_key()),
    )?;
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&sealed[41..HEADER]);
    let plain = XChaCha20Poly1305::new((&key).into())
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &sealed[HEADER..],
                aad: &sealed[..41],
            },
        )
        .map_err(|_| "this key does not open the file (or the file is damaged)".to_string())?;
    if plain.len() < 4 {
        return Err("the sealed record is damaged".to_string());
    }
    let n = u32::from_be_bytes([plain[0], plain[1], plain[2], plain[3]]) as usize;
    if plain.len() < 4 + n {
        return Err("the sealed record is damaged".to_string());
    }
    let record = serde_json::from_slice(&plain[4..4 + n]).map_err(|e| e.to_string())?;
    Ok((record, plain[4 + n..].to_vec()))
}

fn sync_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Write `bytes` as `dir/name`, owner-only and DURABLY: `.part`, fsync, rename, then fsync the directory, so the
/// file survives a power loss once this returns. Blocking.
fn write_durable(
    dir: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dest = dir.join(name);
    let part = dir.join(format!("{name}.part"));
    // evidence is never overwritten: a name already taken (file, folder or link) is an error
    if dest.symlink_metadata().is_ok() {
        return Err("a file of that name already exists".to_string());
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&part)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .and_then(|_| std::fs::rename(&part, &dest))
        .and_then(|_| sync_dir(dir))
        .map_err(|e| {
            let _ = std::fs::remove_file(&part);
            e.to_string()
        })?;
    Ok(dest)
}

/// Write one sealed file into the evidence directory (owner-only, durable). Blocking.
pub fn write_sealed(
    dir: &std::path::Path,
    name: &str,
    sealed: &[u8],
) -> Result<std::path::PathBuf, String> {
    write_durable(dir, name, sealed)
}

/// The fallback when sealing failed: keep the item, in clear, in the owner-only `unsealed/` subfolder of the
/// evidence directory, rather than delete the only copy. Blocking.
pub fn write_unsealed(
    evidence_dir: &std::path::Path,
    name: &str,
    item: &[u8],
) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let dir = evidence_dir.join("unsealed");
    if !dir.is_dir() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| e.to_string())?;
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    write_durable(&dir, name, item)
}
