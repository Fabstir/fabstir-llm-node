// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Offline opener for sealed content-check evidence (IMPLEMENTATION-CONTENT-CHECKS-LTX.md D15).
//!
//! The node seals the strongest-signal items to `LTX_EVIDENCE_PUBKEY` and can never open them. This tool opens one
//! with the private key, which is held offline by the named person and must never be placed on a host. Run it on
//! an offline machine. It makes no network call and reads the key from a file, never from the command line.
//!
//!   open-evidence keygen --out-key evidence.key          new key pair; prints the public key for the host setting
//!   open-evidence pubkey --key-file evidence.key         print the public key of an existing private key
//!   open-evidence open --key-file evidence.key FILE --out-dir DIR
//!   open-evidence seal-test --pubkey HEX --out FILE      a synthetic sealed item, to prove a key pair opens (R0)

use clap::{Parser, Subcommand};

// The sealing code itself, compiled into this tool directly: it then links none of the node (no CUDA, no chain
// client), so the binary runs on an offline machine. `seal.rs` has no dependency on the rest of the crate.
#[path = "../ltx/checks/seal.rs"]
#[allow(dead_code)]
mod seal;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "open-evidence",
    about = "Open sealed content-check evidence offline"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a key pair: the private key goes to --out-key (owner-only), the public key is printed.
    Keygen {
        #[arg(long)]
        out_key: PathBuf,
    },
    /// Print the public key (the value of LTX_EVIDENCE_PUBKEY) for a private key file.
    Pubkey {
        #[arg(long)]
        key_file: PathBuf,
    },
    /// Open one sealed file: writes the record (JSON) and the item into --out-dir.
    Open {
        #[arg(long)]
        key_file: PathBuf,
        sealed: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Seal a synthetic item to a public key (contains no customer content).
    SealTest {
        #[arg(long)]
        pubkey: String,
        #[arg(long)]
        out: PathBuf,
    },
}

fn read_key(path: &Path) -> Result<[u8; 32], String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw = hex::decode(text.trim().trim_start_matches("0x"))
        .map_err(|_| "the key file must hold 64 hex characters".to_string())?;
    raw.try_into()
        .map_err(|_| "the key file must hold 64 hex characters".to_string())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Keygen { out_key } => {
            let sk = k256::SecretKey::random(&mut rand::thread_rng());
            let bytes: [u8; 32] = sk.to_bytes().into();
            write_private(&out_key, format!("{}\n", hex::encode(bytes)).as_bytes())?;
            println!(
                "private key written to {} (keep it offline, in two places)",
                out_key.display()
            );
            println!(
                "LTX_EVIDENCE_PUBKEY={}",
                hex::encode(seal::public_key_of(&bytes)?)
            );
            Ok(())
        }
        Cmd::Pubkey { key_file } => {
            println!(
                "LTX_EVIDENCE_PUBKEY={}",
                hex::encode(seal::public_key_of(&read_key(&key_file)?)?)
            );
            Ok(())
        }
        Cmd::Open {
            key_file,
            sealed,
            out_dir,
        } => {
            let key = read_key(&key_file)?;
            let data = std::fs::read(&sealed).map_err(|e| format!("{}: {e}", sealed.display()))?;
            let (record, item) = seal::open(&key, &data)?;
            // the opened item is the most sensitive file there is: its folder is the owner's alone
            {
                use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
                if !out_dir.is_dir() {
                    std::fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(&out_dir)
                        .map_err(|e| e.to_string())?;
                }
                std::fs::set_permissions(&out_dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|e| e.to_string())?;
            }
            let stem = sealed
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("evidence")
                .to_string();
            // the extension comes from the sealed record: keep only letters and digits
            let ext: String = record
                .get("ext")
                .and_then(|v| v.as_str())
                .unwrap_or("bin")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(8)
                .collect();
            let record_path = out_dir.join(format!("{stem}.record.json"));
            let item_path = out_dir.join(format!("{stem}.item.{ext}"));
            let pretty = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
            write_private(&record_path, &pretty)?;
            write_private(&item_path, &item)?;
            println!("record: {}", record_path.display());
            println!("item:   {} ({} bytes)", item_path.display(), item.len());
            Ok(())
        }
        Cmd::SealTest { pubkey, out } => {
            let pk = seal::parse_public_key(&pubkey)?;
            let record = serde_json::json!({"synthetic": true, "ext": "txt", "stage": "c0"});
            let sealed = seal::seal(
                &pk,
                &record,
                b"synthetic evidence item: no customer content\n",
            )?;
            write_private(&out, &sealed)?;
            println!("synthetic sealed item written to {}", out.display());
            Ok(())
        }
    }
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("open-evidence: {e}");
        std::process::exit(1);
    }
}
