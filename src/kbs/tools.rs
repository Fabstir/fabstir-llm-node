// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Provider tooling (design §13.1), offline, never on the request path:
//! `policy sign`, `seal`, `reseal`, `keyring add|check`.
//!
//! A container binds `policy_hash` into every chunk's AAD, so EVERY policy change
//! is followed by a `reseal` with the same DEK (and, on the paid day, the target is
//! sealed at T−1 under the compose that will be deployed; the refusal cycle runs on
//! the small model). `reseal` streams: the old container is decrypted on a spawned
//! thread into a pipe while the calling thread seals from it, so nothing is held
//! in memory and the tool works for tens-of-GB containers.

use crate::crypto::recover_client_address;
use crate::kbs::keyring::{
    decode_lower_hex, Keyring, KeyringFile, KeyringFileEntry, TEST_ID_PREFIX,
};
use crate::tee::container::{
    decrypt_model_to_writer, encrypt_model_to_writer, ContainerHeader, AEAD_TAG_LEN, HEADER_LEN,
};
use crate::tee::policy::{canonical_policy_bytes, policy_signature_digest, SignedModelPolicy};
use crate::tee::types::Policy;
use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use rand::{rngs::OsRng, RngCore};
use std::io::Write;
use std::path::Path;

/// The chunk size the node's sealer and this tool use (4 MiB).
pub const DEFAULT_CHUNK_SIZE: u32 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(pub String);

fn te(e: impl std::fmt::Display) -> ToolError {
    ToolError(e.to_string())
}

/// Sign `policy` as the provider: EIP-191 over `canonical_policy_bytes`, the
/// recoverable 65-byte signature the node's `verify_signer` recovers. Returns the
/// signed blob and the recovered `0x` address.
pub fn sign_policy(
    policy: &Policy,
    encrypted_ref: &str,
    sk: &SigningKey,
) -> Result<(SignedModelPolicy, String), ToolError> {
    policy.validate().map_err(te)?;
    let canonical = canonical_policy_bytes(policy).map_err(te)?;
    let digest = policy_signature_digest(&canonical);
    let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash_recoverable(&digest).map_err(te)?;
    let mut sig65 = vec![0u8; 65];
    sig65[..64].copy_from_slice(&sig.to_bytes());
    sig65[64] = recid.to_byte() + 27;
    let signer = recover_client_address(&sig65, &digest).map_err(te)?;
    let signed = SignedModelPolicy {
        policy: policy.clone(),
        encrypted_ref: encrypted_ref.to_string(),
        signer: signer.clone(),
        signature: sig65,
    };
    signed.verify_signer(&signer).map_err(te)?;
    Ok((signed, signer))
}

/// A secp256k1 signing key from 64 hex chars (with or without `0x`).
pub fn signing_key_from_hex(s: &str) -> Result<SigningKey, ToolError> {
    let s = s.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes =
        zeroize::Zeroizing::new(hex::decode(s).map_err(|_| ToolError("key is not hex".into()))?);
    SigningKey::from_slice(&bytes).map_err(|e| ToolError(format!("key: {e}")))
}

/// `policy_hash` of a signed policy (what the container binds).
pub fn policy_hash_of(signed: &SignedModelPolicy) -> Result<[u8; 32], ToolError> {
    signed.policy_hash().map_err(te)
}

/// What the sealers require of a signed policy file before binding its hash into a
/// container: schema-valid, NOT already expired, signed by the address it claims
/// and, when the caller names the provider, by THAT address. A file edited after
/// signing, an expired policy or the wrong key would otherwise seal fine (hours,
/// for the target) and only fail on the node after the swap. A policy whose
/// `not_before` is still ahead IS accepted: the T−1 seal of the target is exactly
/// that case, and the node enforces the window itself at load.
pub fn check_signed_policy(
    signed: &SignedModelPolicy,
    expected_signer: Option<&str>,
) -> Result<(), ToolError> {
    signed.policy.validate().map_err(te)?;
    let now = crate::tee::types::now_unix();
    if now > signed.policy.expiry {
        return Err(ToolError(format!(
            "policy expired at {} (now {now}); sign a new one before sealing",
            signed.policy.expiry
        )));
    }
    signed.verify_signer(&signed.signer).map_err(te)?;
    if let Some(want) = expected_signer {
        if !signed.signer.eq_ignore_ascii_case(want) {
            return Err(ToolError(format!(
                "the policy is signed by {}, not the expected provider {want}",
                signed.signer
            )));
        }
    }
    Ok(())
}

/// The tool never overwrites: `--out` must not exist (so it can never be the input,
/// a symlink to the input, a hard link of it, or a live container the atomic swap
/// would pick up), and `--out` must not spell the input's path even when absent.
fn refuse_output(input: &Path, out: &Path) -> Result<(), ToolError> {
    if out.symlink_metadata().is_ok() {
        return Err(ToolError(format!(
            "--out {} exists; the tool never overwrites (write to a temp name, then mv)",
            out.display()
        )));
    }
    let canon = |p: &Path| -> Option<std::path::PathBuf> {
        let parent = p
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Some(parent.canonicalize().ok()?.join(p.file_name()?))
    };
    if let (Some(a), Some(b)) = (input.canonicalize().ok(), canon(out)) {
        if a == b {
            return Err(ToolError(format!(
                "--out must differ from the input ({})",
                a.display()
            )));
        }
    }
    Ok(())
}

/// The output file, created exclusively: `refuse_output`'s existence check is not
/// atomic on its own, so the create itself must refuse an existing (or planted) file.
fn create_new_0644(out: &Path) -> Result<std::fs::File, ToolError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(out)
        .map_err(|e| ToolError(format!("{}: {e}", out.display())))?;
    // Explicit, not umask-derived: a `umask 077` shell (sudo unions umasks) would
    // otherwise leave a container nginx cannot serve.
    std::fs::set_permissions(out, std::fs::Permissions::from_mode(0o644)).map_err(te)?;
    Ok(f)
}

/// Run `f` writing `out`, which this invocation CREATED exclusively (so it is ours);
/// on `Err` the partial output is removed so an atomic swap can never pick up a
/// half-written container.
fn with_output<T>(out: &Path, f: impl FnOnce() -> Result<T, ToolError>) -> Result<T, ToolError> {
    match f() {
        Ok(v) => Ok(v),
        Err(e) => {
            let _ = std::fs::remove_file(out);
            Err(e)
        }
    }
}

/// Seal `model` into `out` under `dek` for `model_id`, bound to `signed`'s
/// `policy_hash`; streaming, fresh `nonce_base`.
pub fn seal(
    model: &Path,
    out: &Path,
    dek: &[u8; 32],
    model_id: [u8; 32],
    signed: &SignedModelPolicy,
    expected_signer: Option<&str>,
) -> Result<u64, ToolError> {
    if signed.policy.model_id != model_id {
        return Err(ToolError("the signed policy is for another model".into()));
    }
    check_signed_policy(signed, expected_signer)?;
    refuse_output(model, out)?;
    // Created exclusively BEFORE the cleanup scope: a failure of the create itself
    // (the name appeared in between) must not delete another writer's file.
    let created = create_new_0644(out)?;
    with_output(out, || seal_inner(model, created, dek, model_id, signed))
}

fn seal_inner(
    model: &Path,
    created: std::fs::File,
    dek: &[u8; 32],
    model_id: [u8; 32],
    signed: &SignedModelPolicy,
) -> Result<u64, ToolError> {
    let policy_hash = policy_hash_of(signed)?;
    let f = std::fs::File::open(model).map_err(te)?;
    let len = f.metadata().map_err(te)?.len();
    let reader = std::io::BufReader::with_capacity(1 << 20, f);
    let mut nonce_base = [0u8; 16];
    OsRng.fill_bytes(&mut nonce_base);
    let mut w = std::io::BufWriter::with_capacity(1 << 20, created);
    encrypt_model_to_writer(
        reader,
        len,
        &mut w,
        dek,
        model_id,
        policy_hash,
        DEFAULT_CHUNK_SIZE,
        nonce_base,
    )
    .map_err(te)?;
    w.flush().map_err(te)?;
    // Durable before the success line: a crash after "sealed" must not leave a
    // short container for the operator to mv over the live one.
    w.get_ref().sync_all().map_err(te)?;
    Ok(len)
}

/// Reseal `input` (an existing container) into `out` under the NEW signed policy
/// with the SAME DEK: the old header is read from the container, the plaintext
/// length derived as `file_size − HEADER_LEN − num_chunks × AEAD_TAG_LEN`, the
/// decrypt runs on a spawned thread into a pipe, the seal reads from it.
pub fn reseal(
    input: &Path,
    out: &Path,
    dek: &[u8; 32],
    signed: &SignedModelPolicy,
    expected_signer: Option<&str>,
) -> Result<u64, ToolError> {
    check_signed_policy(signed, expected_signer)?;
    refuse_output(input, out)?;
    let created = create_new_0644(out)?;
    with_output(out, || reseal_inner(input, created, dek, signed))
}

fn reseal_inner(
    input: &Path,
    created: std::fs::File,
    dek: &[u8; 32],
    signed: &SignedModelPolicy,
) -> Result<u64, ToolError> {
    let f = std::fs::File::open(input).map_err(te)?;
    let size = f.metadata().map_err(te)?.len();
    let mut head = [0u8; HEADER_LEN];
    {
        use std::io::Read;
        (&f).read_exact(&mut head).map_err(te)?;
    }
    let header = ContainerHeader::decode(&head).map_err(te)?;
    if header.model_id != signed.policy.model_id {
        return Err(ToolError(
            "the new signed policy is for another model".into(),
        ));
    }
    let overhead = HEADER_LEN as u64 + header.num_chunks as u64 * AEAD_TAG_LEN as u64;
    let plaintext_len = size
        .checked_sub(overhead)
        .ok_or_else(|| ToolError("container shorter than its header declares".into()))?;
    let new_hash = policy_hash_of(signed)?;
    let old_hash = header.policy_hash;
    let model_id = header.model_id;

    let (reader, writer) = std::io::pipe().map_err(te)?;
    let dek_copy = zeroize::Zeroizing::new(*dek);
    let input_path = input.to_path_buf();
    // The thread reports the io::ErrorKind alongside the message: attribution below
    // keys on `BrokenPipe`, never on the (locale-dependent) message text.
    let decrypt = std::thread::spawn(
        move || -> Result<(), (ToolError, Option<std::io::ErrorKind>)> {
            let io_kind = |e: &crate::tee::types::TeeError| match e {
                crate::tee::types::TeeError::Io(io) => Some(io.kind()),
                _ => None,
            };
            let f = std::fs::File::open(&input_path).map_err(|e| (te(&e), Some(e.kind())))?;
            let r = std::io::BufReader::with_capacity(1 << 20, f);
            let mut w = std::io::BufWriter::with_capacity(1 << 20, writer);
            decrypt_model_to_writer(r, &mut w, &dek_copy, &model_id, &old_hash)
                .map_err(|e| (te(&e), io_kind(&e)))?;
            w.flush().map_err(|e| (te(&e), Some(e.kind())))?;
            Ok(())
        },
    );
    let mut nonce_base = [0u8; 16];
    OsRng.fill_bytes(&mut nonce_base);
    let mut w = std::io::BufWriter::with_capacity(1 << 20, created);
    let sealed = encrypt_model_to_writer(
        std::io::BufReader::with_capacity(1 << 20, reader),
        plaintext_len,
        &mut w,
        dek,
        model_id,
        new_hash,
        DEFAULT_CHUNK_SIZE,
        nonce_base,
    );
    let decrypted = decrypt
        .join()
        .map_err(|_| ToolError("decrypt thread panicked".into()))?;
    match (decrypted, sealed) {
        (Ok(()), Ok(())) => {}
        // The seal side failed first (ENOSPC, a write error): the decrypt thread then
        // sees a broken pipe, which is a symptom, not the cause.
        (Err((_, Some(std::io::ErrorKind::BrokenPipe))), Err(s)) => return Err(te(s)),
        (Err((d, _)), _) => return Err(d),
        (Ok(()), Err(s)) => return Err(te(s)),
    }
    w.flush().map_err(te)?;
    w.get_ref().sync_all().map_err(te)?;
    Ok(plaintext_len)
}

/// Generate a fresh 32-byte DEK.
pub fn generate_dek() -> [u8; 32] {
    let mut d = [0u8; 32];
    OsRng.fill_bytes(&mut d);
    d
}

/// Add an entry to the keyring file (created if absent), validating the result
/// with the same rules the broker applies at load. The caller (the CLI) runs as
/// the service user so the file stays owned by it.
pub fn keyring_add(
    path: &Path,
    model_id_hex: &str,
    provider: &str,
    dek: &[u8; 32],
    test: bool,
    min_policy_version: u32,
    note: &str,
) -> Result<usize, ToolError> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        // A root-owned keyring is refused at the next broker start (exit 78).
        return Err(ToolError(
            "refusing to write the keyring as root: run as the service user (sudo -u fabstir-kbs)"
                .into(),
        ));
    }
    let mut file: KeyringFile = if let Ok(link_meta) = std::fs::symlink_metadata(path) {
        if link_meta.file_type().is_symlink() {
            // The rewrite is a rename: it would replace the link, not the file behind it.
            return Err(ToolError(format!(
                "{} is a symlink; point --file at the real file",
                path.display()
            )));
        }
        // The new inode belongs to whoever runs this: a root run over the service
        // user's keyring would make the next broker start refuse.
        use std::os::unix::fs::MetadataExt;
        let owner = link_meta.uid();
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        if owner != me {
            return Err(ToolError(format!(
                "{} is owned by uid {owner}, you are uid {me}: run as that user (sudo -u fabstir-kbs)",
                path.display()
            )));
        }
        let bytes = zeroize::Zeroizing::new(std::fs::read(path).map_err(te)?);
        serde_json::from_slice(&bytes).map_err(te)?
    } else {
        KeyringFile {
            schema: 1,
            keys: Vec::new(),
        }
    };
    let id = decode_lower_hex(model_id_hex, 32)
        .ok_or_else(|| ToolError("model_id must be 64 lowercase hex".into()))?;
    let is_test_id = id.starts_with(TEST_ID_PREFIX);
    if test != is_test_id {
        return Err(ToolError(format!(
            "test={test} but the id {} the t5t: prefix",
            if is_test_id {
                "carries"
            } else {
                "does not carry"
            }
        )));
    }
    file.keys.push(KeyringFileEntry {
        model_id: model_id_hex.to_string(),
        provider: provider.to_string(),
        dek: zeroize::Zeroizing::new(hex::encode(dek)),
        test,
        min_policy_version,
        note: note.to_string(),
    });
    let json = zeroize::Zeroizing::new(serde_json::to_vec_pretty(&file).map_err(te)?);
    Keyring::parse(&json).map_err(te)?;
    write_0600(path, &json)?;
    Ok(file.keys.len())
}

/// Validate a keyring file with the broker's rules (no mode coupling: that needs
/// the config).
pub fn keyring_check(path: &Path) -> Result<usize, ToolError> {
    let bytes = zeroize::Zeroizing::new(std::fs::read(path).map_err(te)?);
    Ok(Keyring::parse(&bytes).map_err(te)?.len())
}

/// Replace `path` atomically with `bytes` at mode 0600: a temp file in the same
/// directory, then rename. The keyring holds every provider's DEK and the DEK files
/// are shredded after `seal`, so a truncate-then-write that dies halfway would be
/// unrecoverable. `write_0600_new` is for a DEK file, which is never overwritten.
pub fn write_0600(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    use std::os::unix::fs::PermissionsExt;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::Builder::new()
        .prefix(".keyring-")
        .tempfile_in(dir)
        .map_err(|e| ToolError(format!("{}: {e}", dir.display())))?;
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600)).map_err(te)?;
    tmp.write_all(bytes).map_err(te)?;
    tmp.as_file().sync_all().map_err(te)?;
    tmp.persist(path)
        .map_err(|e| ToolError(format!("{}: {}", path.display(), e.error)))?;
    Ok(())
}

/// Write a NEW plain file (mode 0644), refusing an existing one: the tools never
/// overwrite (a signed policy written over its own unsigned source would break the
/// next run).
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    let mut f = create_new_0644(path)?;
    f.write_all(bytes).map_err(te)?;
    f.sync_all().map_err(te)?;
    Ok(())
}

/// Write a NEW file at mode 0600, refusing an existing one (`create_new`): a DEK
/// file is never truncated.
pub fn write_0600_new(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| ToolError(format!("{}: {e}", path.display())))?;
    f.write_all(bytes).map_err(te)?;
    f.sync_all().map_err(te)?;
    Ok(())
}
