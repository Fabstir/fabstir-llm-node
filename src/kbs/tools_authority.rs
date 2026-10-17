// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Policy schema 3 tooling (`docs/development/IMPLEMENTATION-POLICY-V3-HOST-
//! PROFILES.md` D10, D14, D18, D22), offline, never on the request path:
//! `authority sign|assemble|check`, `profile new|pin|check|assemble|sign`,
//! `policy migrate-v2`, the `--authority` cross-check of `policy sign` and
//! `seal`, and the replay of real captures.
//!
//! A capture is never a source of truth for a profile (H2): `profile new`
//! takes PREDICTED, independently obtained values, `profile pin` takes the
//! `app_id`/`key_provider` obtained out of band, and a capture only CONFIRMS.
//! Every tool is read-only towards the broker's state (L1): `authority check
//! --hwm-dir` reads the high-water mark and never writes it.

use crate::kbs::authority::{
    archive_name, decide, root_key, validate_bundle, Authority, Hwm, HwmStore,
};
use crate::kbs::config::GpuEvidenceMode;
use crate::kbs::cpu::{decode_unverified, TdxEvidence, UnverifiedTd};
use crate::kbs::error::KbsError;
use crate::kbs::eventlog::{replay_and_extract, Replayed};
use crate::kbs::gpu::CANNED_LABEL;
use crate::kbs::nras_claims::{GpuFields, GpuOutcome};
use crate::kbs::routes::{release_gate, RequestKeyRequestWire};
use crate::kbs::tools::{create_new_0644, with_output, ToolError};
use crate::kbs::verify::{check_policy, prefilter, Expected, Verified};
use crate::tee::host_profile::{
    canonical_bytes, document_digest, eip191_digest, parse_bundle, parse_signed_document,
    recover_document_signer, verify_manifest, verify_set, AuthorityBundle, AuthorityManifest,
    HostProfile, HostProfileSet, ProfileCvm, ProfileGpu, SignedAuthorityManifest,
    SignedHostProfileSet, MAX_SET_SIGNATURES, SET_DOC, SET_SCHEMA_VERSION,
};
use crate::tee::policy::SignedModelPolicy;
use crate::tee::types::{
    sha256_32, CcMode, CvmPolicy, GpuPolicy, HostAuthority, Policy, POLICY_DOC,
    POLICY_SCHEMA_VERSION,
};
use k256::ecdsa::SigningKey;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The `key-provider` event payload of Phala's KMS: the hex of
/// `{"name":"kms","id":"3059…9d9f"}`, byte-identical in the CPU-round
/// policies, run 1's pinned policy and boot-2 capture, and run 2's attestation
/// JSON (D14). `profile pin` pins it unless `--key-provider-override` is given.
pub const PHALA_KMS_KEY_PROVIDER: &str =
    "7b226e616d65223a226b6d73222c226964223a223330353933303133303630373261383634386365336430323031303630383261383634386365336430333031303730333432303030343838343465623432636364663863353266643466313734663336326663623962626431396334356664343866316564656332643866316361323335333665633161373430323162346365653631306330373466383239346434333162326237666565326333396535333333666461663061343532326434336662313539643966227d";

/// What `profile pin --key-provider-override` prints (D18, B4).
pub const KEY_PROVIDER_STOP: &str = "STOP: --key-provider-override replaces the committed Phala \
KMS key provider. Never take this value from a capture (a capture's differing value is exactly \
what an attacker wants copied): obtain it from Phala's published KMS information, record where \
in --evidence, and have a second person confirm it before the set is signed.";

/// The tools' clock (fail closed: a broken clock refuses, it never passes
/// every window).
pub fn now() -> Result<u64, ToolError> {
    match crate::tee::types::now_unix() {
        u64::MAX => refuse("system clock unavailable"),
        n => Ok(n),
    }
}

fn te(e: impl std::fmt::Display) -> ToolError {
    ToolError(e.to_string())
}

fn refuse<T>(msg: impl Into<String>) -> Result<T, ToolError> {
    Err(ToolError(msg.into()))
}

// ---------- signatures ----------

/// `sk`'s 65-byte recoverable signature (`v` = 27/28) over the EIP-191 digest
/// of `doc`'s canonical bytes: what [`recover_document_signer`] recovers.
pub fn sign_document<T: Serialize>(doc: &T, sk: &SigningKey) -> Result<Vec<u8>, ToolError> {
    let digest = eip191_digest(&canonical_bytes(doc).map_err(te)?);
    let (sig, recid) = sk.sign_prehash_recoverable(&digest).map_err(te)?;
    let mut out = sig.to_bytes().to_vec();
    out.push(recid.to_byte() + 27);
    Ok(out)
}

/// The lowercase `0x` Ethereum address of `sk`: the last 20 bytes of the
/// keccak-256 of its uncompressed public key. What a manifest names as its
/// root or a signer, printed by `key address` for the ceremony (D16).
pub fn address_of(sk: &SigningKey) -> String {
    use tiny_keccak::{Hasher, Keccak};
    let point = sk.verifying_key().to_encoded_point(false);
    let mut k = Keccak::v256();
    k.update(&point.as_bytes()[1..]);
    let mut h = [0u8; 32];
    k.finalize(&mut h);
    format!("0x{}", hex::encode(&h[12..]))
}

/// An externally produced signature (`--signature-file`, D2): 130 hex chars
/// (optional `0x`, surrounding whitespace ignored) or the raw 65 bytes.
pub fn read_signature_file(bytes: &[u8]) -> Result<Vec<u8>, ToolError> {
    if bytes.len() == 65 {
        return Ok(bytes.to_vec());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ToolError("signature file: neither 65 raw bytes nor hex text".into()))?
        .trim();
    let hex_str = text.strip_prefix("0x").unwrap_or(text);
    let sig = hex::decode(hex_str).map_err(|_| ToolError("signature file: not hex".into()))?;
    if sig.len() != 65 {
        return refuse(format!("signature file: {} bytes, want 65", sig.len()));
    }
    Ok(sig)
}

/// Who signs a manifest: a root key file, or an external signature.
pub enum ManifestSigner<'a> {
    Key(&'a SigningKey),
    External(Vec<u8>),
}

// ---------- the counters (D10) ----------

/// `--previous <bundle>` or `--first` (D10, R3-7).
pub enum Prior {
    First,
    Previous(Box<AuthorityBundle>),
}

/// Read a `--previous` bundle and verify it: its manifest's rules and
/// signature against its own root, its set's rules and signatures against that
/// manifest. The window is NOT checked: the previous bundle is usually the
/// archived, accepted one and may have expired since.
pub fn read_previous(bytes: &[u8], what: &str) -> Result<AuthorityBundle, ToolError> {
    let b = parse_bundle(bytes, what).map_err(te)?;
    verify_manifest(&b.manifest, &b.manifest.manifest.root)
        .map_err(|e| ToolError(format!("{what}: {e}")))?;
    verify_set(&b.set, &b.manifest.manifest).map_err(|e| ToolError(format!("{what}: {e}")))?;
    Ok(b)
}

fn hex_digest<T: Serialize>(doc: &T) -> Result<String, ToolError> {
    Ok(hex::encode(document_digest(doc).map_err(te)?))
}

/// One counter's step (D10): `prev` or `prev + 1`, and a 0 step only with an
/// equal body digest. Returns whether it stepped.
fn counter_step(
    what: &str,
    prev: u64,
    next: u64,
    prev_digest: &str,
    next_digest: &str,
) -> Result<bool, ToolError> {
    if next == prev {
        if next_digest != prev_digest {
            return refuse(format!(
                "{what} {next} equals the previous one but the body differs (digest {} vs {}): \
                 a changed body needs the next number",
                &next_digest[..16],
                &prev_digest[..16]
            ));
        }
        return Ok(false);
    }
    if next == prev.saturating_add(1) {
        return Ok(true);
    }
    refuse(format!(
        "{what} {next} after {prev}: each counter steps by 0 or +1 (D10)"
    ))
}

fn same_root(prev: &str, next: &str) -> Result<(), ToolError> {
    if prev != next {
        return refuse(format!(
            "root {next} differs from the previous bundle's {prev}"
        ));
    }
    Ok(())
}

/// The single-document rule of `authority sign`: `manifest_version` is the
/// previous one (same body) or +1, or 1 with `--first`.
pub fn manifest_counter(prior: &Prior, m: &AuthorityManifest) -> Result<(), ToolError> {
    match prior {
        Prior::First if m.manifest_version == 1 => Ok(()),
        Prior::First => refuse(format!(
            "--first is only for manifest_version 1, this is {}",
            m.manifest_version
        )),
        Prior::Previous(p) => {
            let pm = &p.manifest.manifest;
            same_root(&pm.root, &m.root)?;
            counter_step(
                "manifest_version",
                pm.manifest_version.into(),
                m.manifest_version.into(),
                &hex_digest(pm)?,
                &hex_digest(m)?,
            )
            .map(|_| ())
        }
    }
}

/// The single-document rule of `profile assemble`: `epoch` is the previous
/// one (same body) or +1, or 1 with `--first`.
pub fn set_counter(prior: &Prior, s: &HostProfileSet) -> Result<(), ToolError> {
    match prior {
        Prior::First if s.epoch == 1 => Ok(()),
        Prior::First => refuse(format!("--first is only for epoch 1, this is {}", s.epoch)),
        Prior::Previous(p) => {
            let ps = &p.set.set;
            same_root(&ps.root, &s.root)?;
            counter_step(
                "epoch",
                ps.epoch,
                s.epoch,
                &hex_digest(ps)?,
                &hex_digest(s)?,
            )
            .map(|_| ())
        }
    }
}

/// The D10 step rule where both counters are visible (`authority assemble`,
/// `authority check --previous`): each counter steps by 0 or +1, at least one
/// steps +1, and a 0 step requires an equal body digest (so a manifest-only
/// bump with a co-signed, unchanged set passes; acceptance 6).
pub fn step_rule(prev: &AuthorityBundle, next: &AuthorityBundle) -> Result<(), ToolError> {
    let (pm, nm) = (&prev.manifest.manifest, &next.manifest.manifest);
    let (ps, ns) = (&prev.set.set, &next.set.set);
    same_root(&pm.root, &nm.root)?;
    let m = counter_step(
        "manifest_version",
        pm.manifest_version.into(),
        nm.manifest_version.into(),
        &hex_digest(pm)?,
        &hex_digest(nm)?,
    )?;
    let e = counter_step(
        "epoch",
        ps.epoch,
        ns.epoch,
        &hex_digest(ps)?,
        &hex_digest(ns)?,
    )?;
    if !m && !e {
        return refuse("neither manifest_version nor epoch steps: the bundle is the previous one");
    }
    Ok(())
}

/// The body pair a bundle stands for (the broker's HWM shape).
pub fn bundle_hwm(b: &AuthorityBundle) -> Result<Hwm, ToolError> {
    Ok(Hwm {
        manifest_version: b.manifest.manifest.manifest_version,
        manifest_digest: hex_digest(&b.manifest.manifest)?,
        epoch: b.set.set.epoch,
        set_digest: hex_digest(&b.set.set)?,
    })
}

// ---------- the highest-signed ledger (D10) ----------

/// `~/.fabstir-authority/{root}.json`: the highest epoch and manifest version
/// ever ASSEMBLED into a bundle for a root, with the bodies assembled at those
/// numbers. An assembly below either is refused, and at either only with the
/// body recorded there (which is what `--previous` = that very bundle gives),
/// so abandoned staged epochs can never be re-used with another body (R4-adv 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub root: String,
    pub manifest_version: u32,
    pub manifest_digest: String,
    pub epoch: u64,
    pub set_digest: String,
}

/// The ledger directory: `$HOME/.fabstir-authority`.
pub fn default_ledger_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".fabstir-authority"))
}

fn ledger_path(dir: &Path, root: &str) -> PathBuf {
    dir.join(format!("{}.json", root_key(root)))
}

/// The ledger of `root`: `None` when it has none yet; unreadable or corrupt is
/// an error (fail closed: never treated as "nothing assembled").
pub fn read_ledger(dir: &Path, root: &str) -> Result<Option<Ledger>, ToolError> {
    let path = ledger_path(dir, root);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return refuse(format!("ledger {}: {e}", path.display())),
    };
    let l: Ledger = serde_json::from_slice(&bytes)
        .map_err(|e| ToolError(format!("ledger {} is corrupt: {e}", path.display())))?;
    if l.root != root {
        return refuse(format!("ledger {} is for root {}", path.display(), l.root));
    }
    Ok(Some(l))
}

/// The ledger rule for one assembly (D10).
pub fn ledger_admits(l: &Ledger, next: &Hwm) -> Result<(), ToolError> {
    let at = |what: &str, highest: u64, n: u64, recorded: &str, got: &str| {
        if n < highest {
            return refuse(format!(
                "{what} {n} is below {highest}, the highest ever assembled for this root: \
                 deploy the assembled intermediates in order instead (D10)"
            ));
        }
        if n == highest && recorded != got {
            return refuse(format!(
                "{what} {n} was already assembled with another body (digest {}); a second \
                 body at one number is refused (abandoned staged epochs, D10)",
                &recorded[..16.min(recorded.len())]
            ));
        }
        Ok(())
    };
    at(
        "manifest_version",
        l.manifest_version.into(),
        next.manifest_version.into(),
        &l.manifest_digest,
        &next.manifest_digest,
    )?;
    at(
        "epoch",
        l.epoch,
        next.epoch,
        &l.set_digest,
        &next.set_digest,
    )
}

fn write_ledger(dir: &Path, l: &Ledger) -> Result<(), ToolError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| ToolError(format!("{}: {e}", dir.display())))?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".ledger-")
        .tempfile_in(dir)
        .map_err(|e| ToolError(format!("{}: {e}", dir.display())))?;
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600)).map_err(te)?;
    tmp.write_all(&serde_json::to_vec_pretty(l).map_err(te)?)
        .map_err(te)?;
    tmp.as_file().sync_all().map_err(te)?;
    let path = ledger_path(dir, &l.root);
    tmp.persist(&path)
        .map_err(|e| ToolError(format!("{}: {}", path.display(), e.error)))?;
    Ok(())
}

// ---------- authority sign / assemble ----------

/// `authority sign`: the manifest's rules, its own counter against `prior`,
/// then the root's signature (a key, or an external one), which must recover
/// to `manifest.root`.
pub fn sign_manifest(
    manifest: AuthorityManifest,
    signer: ManifestSigner<'_>,
    prior: &Prior,
) -> Result<SignedAuthorityManifest, ToolError> {
    manifest.validate().map_err(te)?;
    manifest_counter(prior, &manifest)?;
    let signature = match signer {
        ManifestSigner::Key(sk) => sign_document(&manifest, sk)?,
        ManifestSigner::External(sig) => sig,
    };
    let signed = SignedAuthorityManifest {
        manifest,
        signature,
    };
    verify_manifest(&signed, &signed.manifest.root).map_err(te)?;
    Ok(signed)
}

/// `authority assemble`, pure half: both documents verify (manifest against
/// its root, set against that manifest at its threshold), the step rule
/// against `prior` (or `--first` = 1 and 1), and the ledger. Returns the
/// bundle and the ledger entry to record.
pub fn assemble(
    manifest: SignedAuthorityManifest,
    set: SignedHostProfileSet,
    prior: &Prior,
    ledger: Option<&Ledger>,
) -> Result<(AuthorityBundle, Ledger), ToolError> {
    verify_manifest(&manifest, &manifest.manifest.root).map_err(te)?;
    verify_set(&set, &manifest.manifest).map_err(te)?;
    let bundle = AuthorityBundle { manifest, set };
    let (m, s) = (&bundle.manifest.manifest, &bundle.set.set);
    match prior {
        Prior::First if m.manifest_version == 1 && s.epoch == 1 => {}
        Prior::First => {
            return refuse(format!(
                "--first is only for manifest_version 1 and epoch 1, this is {} and {}",
                m.manifest_version, s.epoch
            ))
        }
        Prior::Previous(p) => step_rule(p, &bundle)?,
    }
    let next = bundle_hwm(&bundle)?;
    if let Some(l) = ledger {
        ledger_admits(l, &next)?;
    }
    let entry = Ledger {
        root: m.root.clone(),
        manifest_version: next.manifest_version,
        manifest_digest: next.manifest_digest,
        epoch: next.epoch,
        set_digest: next.set_digest,
    };
    Ok((bundle, entry))
}

/// `authority assemble`: [`assemble`], then the bundle written to `out`
/// (never overwriting), then the ledger. A failed ledger write removes the
/// bundle, so no assembled bundle is ever left unrecorded.
pub fn assemble_to(
    out: &Path,
    manifest: SignedAuthorityManifest,
    set: SignedHostProfileSet,
    prior: &Prior,
    ledger_dir: &Path,
) -> Result<(AuthorityBundle, Ledger), ToolError> {
    let root = manifest.manifest.root.clone();
    let ledger = read_ledger(ledger_dir, &root)?;
    let (bundle, entry) = assemble(manifest, set, prior, ledger.as_ref())?;
    let body = serde_json::to_vec_pretty(&bundle).map_err(te)?;
    let mut f = create_new_0644(out)?;
    with_output(out, || {
        f.write_all(&body).map_err(te)?;
        f.sync_all().map_err(te)?;
        write_ledger(ledger_dir, &entry)
    })?;
    Ok((bundle, entry))
}

// ---------- authority check ----------

/// What `authority check` found. Failures exit 1, warnings alone exit 3, so
/// the daily timer alerts on both (D17).
#[derive(Debug, Default, Clone)]
pub struct CheckReport {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub failures: Vec<String>,
}

/// The inputs of `authority check` (all read by the caller).
pub struct CheckInputs<'a> {
    /// The bundle and its file name (a served file is named `{root}.json`).
    pub bundle: &'a [u8],
    pub bundle_name: &'a str,
    /// `--policy <signed v3>…`, with their paths for the report.
    pub policies: Vec<(String, Vec<u8>)>,
    pub previous: Option<&'a [u8]>,
    pub warn_days: Option<u64>,
    /// `--replay <capture dir>…`.
    pub replays: Vec<PathBuf>,
    /// `--hwm-dir`: the broker's HWM directory (the archive is its sibling).
    pub hwm_dir: Option<PathBuf>,
    pub now: u64,
}

fn days(secs: u64) -> u64 {
    secs / 86_400
}

/// `authority check`: the broker's own pure loader (`validate_bundle`,
/// `Authority::from_bundle`) and matcher, offline and READ-ONLY (L1). Every
/// finding is collected (none stops the others).
pub fn authority_check(i: &CheckInputs<'_>) -> CheckReport {
    let mut r = CheckReport::default();
    let bundle = match parse_bundle(i.bundle, i.bundle_name) {
        Ok(b) => b,
        Err(e) => {
            r.failures.push(e.to_string());
            return r;
        }
    };
    let m = &bundle.manifest.manifest;
    let s = &bundle.set.set;
    let root = m.root.clone();
    let stem = i.bundle_name.strip_suffix(".json").unwrap_or(i.bundle_name);
    if stem.len() == 40 && stem.bytes().all(|b| b.is_ascii_hexdigit()) && stem != root_key(&root) {
        r.failures.push(format!(
            "the file is named for root 0x{stem} but holds root {root} (D6)"
        ));
    }
    match validate_bundle(&bundle, &root, i.now) {
        Ok((md, sd, counted)) => r.lines.push(format!(
            "root={root} manifest_version={} epoch={} manifest_digest={} set_digest={} \
             signers={} ignored={} threshold={} profiles={}",
            m.manifest_version,
            s.epoch,
            hex::encode(md),
            hex::encode(sd),
            counted.signers.join(","),
            counted.ignored.join(","),
            m.threshold,
            s.profiles.len()
        )),
        Err(e) => r.failures.push(format!("bundle: {}", e.detail)),
    }
    if let Some(prev) = i.previous {
        match read_previous(prev, "--previous").and_then(|p| step_rule(&p, &bundle)) {
            Ok(()) => r.lines.push("step_rule=ok".into()),
            Err(e) => r.failures.push(format!("--previous: {e}")),
        }
    }
    for p in &s.profiles {
        if p.expiry < i.now {
            r.warnings.push(format!(
                "profile {} expired at {}: hosts on it are refused; remove or renew it in the \
                 next epoch",
                p.id, p.expiry
            ));
        }
    }
    if let Some(n) = i.warn_days {
        warn_expiries(&bundle, n, i.now, &mut r);
    }
    let mut policies: Vec<Policy> = Vec::new();
    for (path, bytes) in &i.policies {
        check_one_policy(
            path,
            bytes,
            &bundle,
            i.warn_days,
            i.now,
            &mut r,
            &mut policies,
        );
    }
    for dir in &i.replays {
        replay_one(dir, &policies, &bundle, i.now, &mut r);
    }
    if let Some(dir) = &i.hwm_dir {
        compare_hwm(dir, &bundle, &mut r);
    }
    r
}

fn warn_expiries(b: &AuthorityBundle, n: u64, now: u64, r: &mut CheckReport) {
    let horizon = now.saturating_add(n.saturating_mul(86_400));
    let m = &b.manifest.manifest;
    if m.expiry >= now && m.expiry <= horizon {
        r.warnings.push(format!(
            "manifest v{} expires at {} (in {} days): sign the next manifest",
            m.manifest_version,
            m.expiry,
            days(m.expiry - now)
        ));
    }
    for p in &b.set.set.profiles {
        if p.in_window(now) && p.expiry <= horizon {
            r.warnings.push(format!(
                "profile {} expires at {} (in {} days): renew it in the next epoch",
                p.id,
                p.expiry,
                days(p.expiry - now)
            ));
        }
    }
}

fn check_one_policy(
    path: &str,
    bytes: &[u8],
    bundle: &AuthorityBundle,
    warn_days: Option<u64>,
    now: u64,
    r: &mut CheckReport,
    out: &mut Vec<Policy>,
) {
    let signed: SignedModelPolicy =
        match parse_signed_document(bytes, path, "/policy", POLICY_DOC, POLICY_SCHEMA_VERSION) {
            Ok(s) => s,
            Err(e) => {
                r.failures.push(format!("policy {path}: {e}"));
                return;
            }
        };
    let p = &signed.policy;
    if let Err(e) = p
        .validate()
        .and_then(|()| signed.verify_signer(&signed.signer))
    {
        r.failures.push(format!("policy {path}: {e}"));
        return;
    }
    if now > p.expiry {
        r.failures.push(format!(
            "policy {path}: expired at {} (now {now})",
            p.expiry
        ));
    } else if now < p.not_before {
        // A policy signed ahead (the T-1 seal): the node refuses it until then.
        r.warnings.push(format!(
            "policy {path}: not valid before {} (now {now})",
            p.not_before
        ));
    } else if let Some(n) = warn_days {
        if p.expiry <= now.saturating_add(n.saturating_mul(86_400)) {
            r.warnings.push(format!(
                "policy {path} (model {}) expires at {} (in {} days): sign and seal its successor",
                hex::encode(p.model_id),
                p.expiry,
                days(p.expiry - now)
            ));
        }
    }
    match Authority::from_bundle(bundle.clone(), p, now) {
        Ok(a) => {
            let c = &a.candidates;
            let line = format!(
                "policy={path} model_id={} signer={} candidates={} of {} (expired={}, \
                 class_filtered={}, unpinned={})",
                hex::encode(p.model_id),
                signed.signer,
                c.list.len(),
                a.profiles().len(),
                c.expired.len(),
                c.class_filtered.len(),
                c.unpinned.len()
            );
            if c.list.is_empty() && !c.unpinned.is_empty() {
                r.warnings.push(format!(
                    "no candidate profile until one is pinned (the two-boot first epoch, \
                     D18: boot 1 is refused by design): {line}"
                ));
            } else if c.list.is_empty() && now < p.not_before {
                // A staged epoch checked ahead (the T-1 seal): its profiles may
                // open with the policy; check again at deploy time.
                r.warnings.push(format!(
                    "no candidate profile yet; the policy itself opens at {}: {line}",
                    p.not_before
                ));
            } else if c.list.is_empty() {
                r.failures
                    .push(format!("no candidate profile, nothing releases: {line}"));
            } else {
                r.lines.push(line);
            }
        }
        Err(e) => r.failures.push(format!("policy {path}: {}", e.detail)),
    }
    out.push(signed.policy);
}

fn replay_one(
    dir: &Path,
    policies: &[Policy],
    bundle: &AuthorityBundle,
    now: u64,
    r: &mut CheckReport,
) {
    let d = dir.display();
    let c = match load_capture(dir) {
        Ok(c) => c,
        Err(e) => {
            r.failures.push(format!("replay {d}: {e}"));
            return;
        }
    };
    let mine: Vec<&Policy> = policies
        .iter()
        .filter(|p| p.model_id == c.expected.model_id)
        .collect();
    if mine.is_empty() {
        r.failures.push(format!(
            "replay {d}: no --policy for its model {}",
            hex::encode(c.expected.model_id)
        ));
    }
    for policy in mine {
        let judged = Authority::from_bundle(bundle.clone(), policy, now)
            .and_then(|a| judge_capture(&c, policy, &a, false, now));
        match judged {
            // The replay re-checks the quote's registers and report_data, the
            // event log, the GPU wrapper and every policy/profile row; it
            // TRUSTS the TCB status, advisories and NRAS claims recorded in
            // verified.json (no collateral or token is replayed).
            Ok(v) => r.lines.push(format!(
                "replay={d} policy_version={} outcome=release profile={} matched={} \
                 trusted=tcb,advisories,nras-claims(verified.json)",
                policy.policy_version,
                v.profile.headline,
                v.profile.all.join(",")
            )),
            Err(e) => r.failures.push(format!(
                "replay {d} (policy_version {}): refused: {}",
                policy.policy_version, e.detail
            )),
        }
    }
}

/// `--hwm-dir`: the served bundle against the broker's HWM, read-only (M4).
/// Any difference is a failure: above the HWM is a deploy not yet loaded (or
/// withheld), below or a different body at the HWM's numbers is a rollback.
/// No HWM beside a non-empty archive is a lost HWM (the store's own rule).
fn compare_hwm(dir: &Path, bundle: &AuthorityBundle, r: &mut CheckReport) {
    let root = &bundle.manifest.manifest.root;
    let served = match bundle_hwm(bundle) {
        Ok(h) => h,
        Err(e) => {
            r.failures.push(format!("hwm: {e}"));
            return;
        }
    };
    // A typo or an unmounted path must not read as "never loaded": the broker
    // creates both hwm/ and archive/ (0700) at start.
    let dir = match dir.canonicalize() {
        Ok(d) if d.is_dir() => d,
        _ => {
            r.failures.push(format!(
                "--hwm-dir {} is not a directory (the broker creates it at start)",
                dir.display()
            ));
            return;
        }
    };
    let archive_root = dir.parent().unwrap_or(Path::new("/")).join("archive");
    if !archive_root.is_dir() {
        r.failures.push(format!(
            "--hwm-dir {} has no archive/ beside it: not a broker's hwm/",
            dir.display()
        ));
        return;
    }
    let dir = dir.as_path();
    let store = HwmStore::new(dir, true);
    match store.read(root) {
        Ok(Some(h)) if h == served => r.lines.push(format!(
            "hwm=served epoch={} manifest_version={}",
            h.epoch, h.manifest_version
        )),
        Ok(Some(h)) => {
            let verdict = match decide(root, Some(&h), &served) {
                Ok(_) => "the broker has not loaded it yet (load it with one challenge)".into(),
                Err(e) => format!("the broker refuses it: {}", e.detail),
            };
            r.failures.push(format!(
                "served != hwm: served {} vs hwm {}; {verdict}",
                archive_name(&served),
                archive_name(&h)
            ));
        }
        Ok(None) => {
            let archive = archive_root.join(root_key(root));
            match std::fs::read_dir(&archive) {
                Ok(rd) => {
                    let entries: Vec<String> = rd
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .filter(|n| n.ends_with(".json"))
                        .collect();
                    if entries == [archive_name(&served)] {
                        // A first load interrupted between its archive write and
                        // its HWM write: the load of this very bundle tolerates
                        // it, and `serve` completes it at start.
                        r.warnings.push(format!(
                            "no hwm for {root} yet, only the archived served bundle: an \
                             interrupted first load; the next load or start completes it"
                        ));
                    } else if !entries.is_empty() {
                        r.failures.push(format!(
                            "no hwm for {root} but its archive holds {} bundle(s): the HWM was \
                             lost; every request for the root faults until `serve` restarts and \
                             walks it forward through the archive (D10; there is no HWM reset)",
                            entries.len()
                        ));
                    } else {
                        r.warnings.push(format!("no hwm for {root}: never loaded"));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    r.warnings.push(format!("no hwm for {root}: never loaded"))
                }
                Err(e) => r.failures.push(format!(
                    "no hwm for {root} and its archive {} cannot be listed ({e})",
                    archive.display()
                )),
            }
        }
        Err(e) => r.failures.push(format!("hwm: {}", e.detail)),
    }
}

// ---------- captures (D22) ----------

/// A pre-verify or verified capture directory, decoded for a replay. The
/// prefilter runs on the raw bytes; step 7 and the release gate run on the
/// decoded values (`cpu`, `replayed`, `gpu`), which equal the prefilter's for
/// an unmodified capture, so a test can flip one decoded value in memory.
#[derive(Debug, Clone)]
pub struct Capture {
    pub expected: Expected,
    pub quote: Vec<u8>,
    pub event_log: Vec<u8>,
    pub gpu_report: Vec<u8>,
    /// The quote's registers (decoded here) with the TCB status and advisories
    /// the broker verified (from `verified.json`; collateral is not replayed).
    pub cpu: TdxEvidence,
    pub replayed: Replayed,
    /// The NRAS-verified claims from `verified.json` (tokens are not replayed).
    pub gpu: GpuOutcome,
    pub mode: GpuEvidenceMode,
}

/// The identity rows `profile pin` confirms: the UNVERIFIED quote and the
/// replayed event log of a capture's `request.json`.
#[derive(Debug, Clone)]
pub struct CaptureIdentity {
    pub td: UnverifiedTd,
    pub replayed: Replayed,
}

struct Decoded {
    expected: Expected,
    quote: Vec<u8>,
    event_log: Vec<u8>,
    gpu_report: Vec<u8>,
    td: UnverifiedTd,
    replayed: Replayed,
}

fn decode_request(bytes: &[u8]) -> Result<Decoded, ToolError> {
    let req: RequestKeyRequestWire =
        serde_json::from_slice(bytes).map_err(|e| ToolError(format!("request.json: {e}")))?;
    let model_id: [u8; 32] = hex::decode(&req.model_id)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ToolError("request.json: model_id is not 32 hex bytes".into()))?;
    let ev = req
        .into_node()
        .evidence
        .decode()
        .map_err(|e| ToolError(format!("request.json evidence: {e}")))?;
    let pk_att: [u8; 33] = ev
        .pk_att
        .as_slice()
        .try_into()
        .map_err(|_| ToolError("request.json: pk_att is not 33 bytes".into()))?;
    let td = decode_unverified(&ev.cpu_quote).map_err(|e| ToolError(e.detail))?;
    let replayed = replay_and_extract(&ev.event_log)
        .map_err(|e| ToolError(format!("request.json event log: {}", e.0)))?;
    Ok(Decoded {
        expected: Expected {
            model_id,
            pk_att,
            nonce: ev.nonce,
        },
        quote: ev.cpu_quote,
        event_log: ev.event_log,
        gpu_report: ev.gpu_report,
        td,
        replayed,
    })
}

/// The identity of a capture's `request.json`, for `profile pin`.
pub fn capture_identity(request_json: &[u8]) -> Result<CaptureIdentity, ToolError> {
    let d = decode_request(request_json)?;
    Ok(CaptureIdentity {
        td: d.td,
        replayed: d.replayed,
    })
}

fn str_at<'a>(v: &'a serde_json::Value, ptr: &str) -> Result<&'a str, ToolError> {
    v.pointer(ptr)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ToolError(format!("verified.json: {ptr} missing")))
}

fn bool_at(v: &serde_json::Value, ptr: &str) -> Result<bool, ToolError> {
    v.pointer(ptr)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| ToolError(format!("verified.json: {ptr} missing")))
}

/// Load a capture directory (`request.json` + `verified.json`).
pub fn load_capture(dir: &Path) -> Result<Capture, ToolError> {
    let read =
        |name: &str| std::fs::read(dir.join(name)).map_err(|e| ToolError(format!("{name}: {e}")));
    capture_from(&read("request.json")?, &read("verified.json")?)
}

/// A capture from its two files' bytes. The quote's registers, `report_data`
/// and debug bit must equal what `verified.json` records, or the capture is
/// refused as inconsistent.
pub fn capture_from(request_json: &[u8], verified_json: &[u8]) -> Result<Capture, ToolError> {
    let d = decode_request(request_json)?;
    let v: serde_json::Value = serde_json::from_slice(verified_json)
        .map_err(|e| ToolError(format!("verified.json: {e}")))?;
    let mut differ = Vec::new();
    for (name, got) in [
        ("mr_td", &d.td.mr_td[..]),
        ("rt_mr0", &d.td.rt_mr0[..]),
        ("rt_mr1", &d.td.rt_mr1[..]),
        ("rt_mr2", &d.td.rt_mr2[..]),
        ("rt_mr3", &d.td.rt_mr3[..]),
        ("report_data", &d.td.report_data[..]),
    ] {
        if str_at(&v, &format!("/tdx/{name}"))? != hex::encode(got) {
            differ.push(name);
        }
    }
    if bool_at(&v, "/tdx/td_debug")? != d.td.td_debug {
        differ.push("td_debug");
    }
    if !differ.is_empty() {
        return refuse(format!(
            "verified.json does not describe request.json's quote ({differ:?})"
        ));
    }
    let advisory_ids = v
        .pointer("/tdx/advisory_ids")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| ToolError("verified.json: /tdx/advisory_ids missing".into()))?
        .iter()
        .map(|a| a.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| ToolError("verified.json: an advisory id is not a string".into()))?;
    let cpu = TdxEvidence {
        mr_td: d.td.mr_td,
        rt_mr0: d.td.rt_mr0,
        rt_mr1: d.td.rt_mr1,
        rt_mr2: d.td.rt_mr2,
        rt_mr3: d.td.rt_mr3,
        report_data: d.td.report_data,
        td_debug: d.td.td_debug,
        tcb_status: str_at(&v, "/tdx/tcb_status")?.to_string(),
        advisory_ids,
    };
    let canned = serde_json::from_slice::<serde_json::Value>(&d.gpu_report)
        .ok()
        .and_then(|g| g.get(CANNED_LABEL).and_then(serde_json::Value::as_bool))
        == Some(true);
    let (gpu, mode) = if canned {
        (GpuOutcome::CannedTolerated, GpuEvidenceMode::Canned)
    } else {
        (
            GpuOutcome::Real(GpuFields {
                nonce: d.expected.nonce,
                hwmodel: str_at(&v, "/gpu/hwmodel")?.to_string(),
                secure_boot: bool_at(&v, "/gpu/secure_boot")?,
                debug_disabled: bool_at(&v, "/gpu/debug_disabled")?,
                driver_version: str_at(&v, "/gpu/driver_version")?.to_string(),
                vbios_version: str_at(&v, "/gpu/vbios_version")?.to_string(),
            }),
            GpuEvidenceMode::Real,
        )
    };
    Ok(Capture {
        expected: d.expected,
        quote: d.quote,
        event_log: d.event_log,
        gpu_report: d.gpu_report,
        cpu,
        replayed: d.replayed,
        gpu,
        mode,
    })
}

/// Judge a capture as the broker would at `now`: the prefilter (step 4) on
/// the raw bytes, `check_policy` (step 7) and the independent `release_gate`
/// (D21) on the decoded values.
pub fn judge_capture(
    c: &Capture,
    policy: &Policy,
    auth: &Authority,
    entry_test: bool,
    now: u64,
) -> Result<Verified, KbsError> {
    prefilter(
        policy,
        auth,
        &c.expected,
        &c.quote,
        &c.event_log,
        &c.gpu_report,
        c.mode,
        now,
    )?;
    let v = check_policy(
        policy,
        auth,
        &c.expected,
        &c.cpu,
        &c.replayed,
        &c.gpu,
        entry_test,
        now,
    )?;
    release_gate(
        &auth.manifest,
        &auth.set,
        now,
        &c.cpu,
        &c.replayed,
        &c.gpu,
        policy,
        &v,
        entry_test,
    )?;
    Ok(v)
}

// ---------- profiles (D14) ----------

/// `dstack-mr`'s output (A-24): the four predicted TD registers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurements {
    pub mrtd: String,
    pub rtmr0: String,
    pub rtmr1: String,
    pub rtmr2: String,
}

/// The env NAMES of an `--env-names` file, in order: one per line, a
/// `NAME=VALUE` paste is cut at the `=` (values are never kept), blank lines
/// skipped (`predict_compose_hash.py`'s rule). A line whose name is not
/// `[A-Za-z_][A-Za-z0-9_]*` (an `export`, a secret's continuation line) is
/// refused by its NUMBER only: the line itself may hold a secret.
pub fn env_names(text: &str) -> Result<Vec<String>, ToolError> {
    let ok = |n: &str| {
        let b = n.as_bytes();
        !b.is_empty()
            && (b[0].is_ascii_alphabetic() || b[0] == b'_')
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
    };
    let mut names = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.trim().is_empty() {
            continue;
        }
        let name = l.split_once('=').map_or(l, |(k, _)| k);
        if !ok(name) {
            return refuse(format!(
                "--env-names line {} is not NAME or NAME=VALUE (the line is not shown: it may \
                 hold a secret)",
                i + 1
            ));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

fn compose_digest(v: &serde_json::Value) -> Result<String, ToolError> {
    Ok(hex::encode(sha256_32(&canonical_bytes(v).map_err(te)?)))
}

/// Phala's compose hash rule (`predict_compose_hash.py`, proven 2026-09-23):
/// sha256 of the app-compose JSON, compact, keys sorted, UTF-8 unescaped.
/// `reference` is a served attestation JSON (its `instances[0].compose_file`)
/// or a served app-compose JSON itself; the rule must reproduce the served
/// hash before a prediction is trusted. `docker_compose_file` becomes the
/// compose file's exact text and `allowed_envs` the env names. Returns
/// (served, predicted).
pub fn predict_compose_hash(
    reference: &[u8],
    compose_text: &str,
    names: &[String],
) -> Result<(String, String), ToolError> {
    let outer: serde_json::Value = serde_json::from_slice(reference)
        .map_err(|e| ToolError(format!("--compose-reference: {e}")))?;
    let served: String = match outer.pointer("/instances/0/compose_file") {
        Some(s) => s
            .as_str()
            .ok_or_else(|| ToolError("--compose-reference: compose_file is not a string".into()))?
            .to_string(),
        None => std::str::from_utf8(reference)
            .map_err(te)?
            .trim_end()
            .to_string(),
    };
    let mut base: serde_json::Value = serde_json::from_str(&served)
        .map_err(|e| ToolError(format!("--compose-reference: the app-compose: {e}")))?;
    let served_hash = hex::encode(sha256_32(served.as_bytes()));
    if compose_digest(&base)? != served_hash {
        return refuse(
            "the serialisation rule no longer reproduces the served compose hash: do not trust \
             a prediction",
        );
    }
    let obj = base
        .as_object_mut()
        .ok_or_else(|| ToolError("--compose-reference: the app-compose is not an object".into()))?;
    obj.insert("docker_compose_file".into(), compose_text.into());
    obj.insert("allowed_envs".into(), names.into());
    Ok((served_hash, compose_digest(&base)?))
}

/// Every field of the reference app-compose OTHER than the two a prediction
/// replaces, one `key=value` per field (a long string as its sha256): the
/// platform template the predicted hash inherits (`pre_launch_script`,
/// `runner`, `kms_enabled`, ...). The reference must come from the
/// authority's own known-good deployment, never from the host being
/// approved; printing it lets the operator see what is being pinned.
pub fn reference_platform_fields(reference: &[u8]) -> Result<Vec<String>, ToolError> {
    let outer: serde_json::Value = serde_json::from_slice(reference)
        .map_err(|e| ToolError(format!("--compose-reference: {e}")))?;
    let base: serde_json::Value = match outer.pointer("/instances/0/compose_file") {
        Some(s) => serde_json::from_str(s.as_str().unwrap_or_default())
            .map_err(|e| ToolError(format!("--compose-reference: the app-compose: {e}")))?,
        None => outer,
    };
    let obj = base
        .as_object()
        .ok_or_else(|| ToolError("--compose-reference: the app-compose is not an object".into()))?;
    let mut keys: Vec<&String> = obj
        .keys()
        .filter(|k| *k != "docker_compose_file" && *k != "allowed_envs")
        .collect();
    keys.sort();
    Ok(keys
        .into_iter()
        .map(|k| match &obj[k] {
            serde_json::Value::String(v) if v.len() > 64 => {
                format!("{k}.sha256={}", hex::encode(sha256_32(v.as_bytes())))
            }
            v => format!("{k}={v}"),
        })
        .collect())
}

/// `profile new`'s inputs: identity, window, floors and the PREDICTED values.
pub struct NewProfile {
    pub id: String,
    pub class: String,
    pub not_before: u64,
    pub expiry: u64,
    pub evidence: String,
    pub hwmodels: Vec<String>,
    pub min_driver_version: Option<String>,
    pub min_vbios_version: Option<String>,
    pub measurements: Measurements,
    pub os_image_digest: String,
    pub compose_hash: String,
}

/// `profile new`: an UNPINNED profile (no `app_id`/`key_provider`, D18) from
/// predicted values only; validated.
pub fn new_profile(n: NewProfile) -> Result<HostProfile, ToolError> {
    let p = HostProfile {
        id: n.id,
        class: n.class,
        cvm: ProfileCvm {
            mrtd: n.measurements.mrtd,
            rtmr0: n.measurements.rtmr0,
            rtmr1: n.measurements.rtmr1,
            rtmr2: n.measurements.rtmr2,
            os_image_hash: n.os_image_digest,
            compose_hashes: vec![n.compose_hash],
            app_id: None,
            key_provider: None,
        },
        gpu: ProfileGpu {
            hwmodels: n.hwmodels,
            min_driver_version: n.min_driver_version,
            min_vbios_version: n.min_vbios_version,
        },
        not_before: n.not_before,
        expiry: n.expiry,
        evidence: n.evidence,
    };
    p.validate().map_err(te)?;
    Ok(p)
}

fn hex_same(a: &str, b: &[u8]) -> bool {
    hex::decode(a).map(|v| v == b).unwrap_or(false)
}

/// `profile check`: all four registers, the OS image digest and the
/// predicted compose hash byte-compared with the profile (ALL mandatory; F18,
/// H2). Returns every differing row; empty = the profile is the prediction.
pub fn check_profile(
    p: &HostProfile,
    m: &Measurements,
    os_image_digest: &str,
    predicted_compose: &str,
) -> Vec<String> {
    let c = &p.cvm;
    let mut rows = Vec::new();
    let mut same = |name: &str, profile: &str, predicted: &str| {
        let ok = hex::decode(predicted)
            .map(|b| hex_same(profile, &b))
            .unwrap_or(false);
        if !ok {
            rows.push(format!(
                "{name}: profile {profile} != predicted {predicted}"
            ));
        }
    };
    same("mrtd", &c.mrtd, &m.mrtd);
    same("rtmr0", &c.rtmr0, &m.rtmr0);
    same("rtmr1", &c.rtmr1, &m.rtmr1);
    same("rtmr2", &c.rtmr2, &m.rtmr2);
    same("os image", &c.os_image_hash, os_image_digest);
    let predicted = hex::decode(predicted_compose).unwrap_or_default();
    if !c.compose_hashes.iter().any(|h| hex_same(h, &predicted)) {
        rows.push(format!(
            "compose hash: predicted {predicted_compose} not in profile {:?}",
            c.compose_hashes
        ));
    }
    rows
}

/// What `profile pin` produced: the pinned profile and, when the override
/// was used, the STOP notice to print.
#[derive(Debug, Clone)]
pub struct Pinned {
    pub profile: HostProfile,
    pub notice: Option<&'static str>,
}

/// `profile pin`: fill ONLY `app_id` and `key_provider`, from values obtained
/// OUT OF BAND (`app_id` from the Phala dashboard for OUR CVM; `key_provider`
/// = [`PHALA_KMS_KEY_PROVIDER`] unless overridden, which prints STOP), and
/// only after every identity row of the capture equals the profile AND the
/// capture's `app-id`/`key-provider` events equal the given values. Otherwise
/// it refuses with every differing row (the diagnostic use of a capture).
/// The GPU model is not in `request.json` (it needs NRAS), so it is not
/// confirmed here.
pub fn pin_profile(
    mut p: HostProfile,
    c: &CaptureIdentity,
    app_id: &str,
    key_provider_override: Option<&str>,
) -> Result<Pinned, ToolError> {
    if p.cvm.app_id.is_some() || p.cvm.key_provider.is_some() {
        return refuse(format!(
            "profile {} is already pinned: pin the unpinned profile `profile new` wrote",
            p.id
        ));
    }
    let key_provider = key_provider_override.unwrap_or(PHALA_KMS_KEY_PROVIDER);
    let (td, r) = (&c.td, &c.replayed);
    let mut rows = Vec::new();
    if r.rtmr3 != td.rt_mr3 {
        rows.push("event log: replayed rtmr3 != the quote's (the events cannot be trusted)".into());
    }
    let cv = &p.cvm;
    for (name, got, want) in [
        ("mrtd", &td.mr_td[..], &cv.mrtd),
        ("rtmr0", &td.rt_mr0[..], &cv.rtmr0),
        ("rtmr1", &td.rt_mr1[..], &cv.rtmr1),
        ("rtmr2", &td.rt_mr2[..], &cv.rtmr2),
        ("os image", &r.os_image_hash[..], &cv.os_image_hash),
    ] {
        if !hex_same(want, got) {
            rows.push(format!(
                "{name}: capture {} != profile {want}",
                hex::encode(got)
            ));
        }
    }
    if !cv
        .compose_hashes
        .iter()
        .any(|h| hex_same(h, &r.compose_hash))
    {
        rows.push(format!(
            "compose hash: capture {} not in profile {:?}",
            hex::encode(r.compose_hash),
            cv.compose_hashes
        ));
    }
    for (name, got, want) in [
        ("app id", &r.app_id, app_id),
        ("key provider", &r.key_provider, key_provider),
    ] {
        match got {
            Some(g) if hex_same(want, g) => {}
            Some(g) => rows.push(format!(
                "{name}: capture event {} != the out-of-band value {want} (STOP: never take \
                 it from the capture)",
                hex::encode(g)
            )),
            None => rows.push(format!("{name}: the capture has no {name} event")),
        }
    }
    if !rows.is_empty() {
        return refuse(format!(
            "profile {}: the capture does not confirm it: {}",
            p.id,
            rows.join("; ")
        ));
    }
    p.cvm.app_id = Some(app_id.to_string());
    p.cvm.key_provider = Some(key_provider.to_string());
    p.validate().map_err(te)?;
    Ok(Pinned {
        profile: p,
        notice: key_provider_override.map(|_| KEY_PROVIDER_STOP),
    })
}

/// `profile assemble`: an unsigned set; its own counter against `prior`.
pub fn assemble_set(
    root: &str,
    epoch: u64,
    issued_at: u64,
    prior: &Prior,
    profiles: Vec<HostProfile>,
) -> Result<HostProfileSet, ToolError> {
    let s = HostProfileSet {
        doc: SET_DOC.into(),
        schema_version: SET_SCHEMA_VERSION,
        root: root.into(),
        epoch,
        issued_at,
        profiles,
    };
    s.validate().map_err(te)?;
    set_counter(prior, &s)?;
    Ok(s)
}

/// `profile sign`: the signer's key must be a CURRENT signer of `manifest`
/// (verified against its own root); `append` adds this co-signature to a
/// signed set whose body is byte-identical, keeping its body and signatures;
/// at most 16 signatures, one per address. The threshold is not required here
/// (a 2-of-n set is signed one key at a time); `authority assemble` checks it.
pub fn sign_set(
    set: HostProfileSet,
    sk: &SigningKey,
    manifest: &SignedAuthorityManifest,
    append: Option<SignedHostProfileSet>,
) -> Result<SignedHostProfileSet, ToolError> {
    let m = &manifest.manifest;
    verify_manifest(manifest, &m.root).map_err(te)?;
    set.validate().map_err(te)?;
    if set.root != m.root {
        return refuse(format!("set root {} != manifest root {}", set.root, m.root));
    }
    let sig = sign_document(&set, sk)?;
    let who = recover_document_signer(&set, &sig).map_err(te)?;
    if !m.signers.contains(&who) {
        return refuse(format!(
            "{who} is not a signer of manifest v{} ({:?})",
            m.manifest_version, m.signers
        ));
    }
    let Some(mut prev) = append else {
        return Ok(SignedHostProfileSet {
            set,
            signatures: vec![sig],
        });
    };
    if canonical_bytes(&prev.set).map_err(te)? != canonical_bytes(&set).map_err(te)? {
        return refuse("--append: the signed set's body differs from --set");
    }
    for (i, s) in prev.signatures.iter().enumerate() {
        let by = recover_document_signer(&prev.set, s)
            .map_err(|e| ToolError(format!("--append: signature {i}: {e}")))?;
        if by == who {
            return refuse(format!("--append: {who} has already signed this set"));
        }
    }
    if prev.signatures.len() >= MAX_SET_SIGNATURES {
        return refuse(format!(
            "--append: the set already carries {MAX_SET_SIGNATURES} signatures (the maximum)"
        ));
    }
    prev.signatures.push(sig);
    Ok(prev)
}

// ---------- policy migrate-v2 (D14) ----------

/// The schema-2 shapes, private to the migration (the only dual-schema reader).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V2Policy {
    schema_version: u32,
    #[allow(dead_code)]
    policy_version: u32,
    model_id: [u8; 32],
    #[allow(dead_code)]
    not_before: u64,
    #[allow(dead_code)]
    expiry: u64,
    cvm: V2Cvm,
    gpu: V2Gpu,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V2Cvm {
    mrtd: String,
    rtmr0: String,
    rtmr1: String,
    rtmr2: String,
    os_image_hash: String,
    compose_hash: String,
    app_id: Option<String>,
    key_provider: Option<String>,
    require_td_debug_off: bool,
    allowed_tcb_status: Vec<String>,
    allowed_advisory_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V2Gpu {
    allowed_hwmodels: Vec<String>,
    require_cc_mode: Option<CcMode>,
    require_secure_boot: bool,
    require_debug_disabled: bool,
    min_driver_version: Option<String>,
    min_vbios_version: Option<String>,
}

/// The v3 fields `migrate-v2` cannot take from the v2 policy.
pub struct MigrateArgs {
    pub expected_provider: String,
    pub root: String,
    pub class: String,
    pub id: String,
    pub policy_not_before: u64,
    pub policy_expiry: u64,
    pub profile_not_before: u64,
    pub profile_expiry: u64,
    pub policy_version: u32,
    pub min_manifest_version: u32,
    pub min_threshold: u32,
    pub min_profile_epoch: u64,
    pub require_pins: bool,
    pub evidence: String,
}

/// `policy migrate-v2`: the v2 signature is verified against
/// `expected_provider` BEFORE any field is trusted (F12); then the host
/// measurements move to a profile (`compose_hash` → a one-element
/// `compose_hashes`, `allowed_hwmodels` → both lists, the v2 driver/VBIOS
/// floors → the PROFILE, the policy's floors `None`, D15), pins as present.
/// A v2 policy pinning `app_id` without `key_provider` is refused (L2). The v2
/// window is never checked; the two windows are the caller's (R3-5). Returns
/// the UNSIGNED v3 policy and the unsigned profile, both validated.
pub fn migrate_v2(v2_signed: &[u8], a: &MigrateArgs) -> Result<(Policy, HostProfile), ToolError> {
    let v: serde_json::Value =
        serde_json::from_slice(v2_signed).map_err(|e| ToolError(format!("--in: {e}")))?;
    let body = v
        .get("policy")
        .ok_or_else(|| ToolError("--in: no policy object".into()))?;
    if body
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(2)
    {
        return refuse("--in: not a schema-2 policy");
    }
    let signature: Vec<u8> = serde_json::from_value(
        v.get("signature")
            .cloned()
            .ok_or_else(|| ToolError("--in: no signature".into()))?,
    )
    .map_err(|e| ToolError(format!("--in: signature: {e}")))?;
    let recovered = recover_document_signer(body, &signature).map_err(te)?;
    if !recovered.eq_ignore_ascii_case(&a.expected_provider) {
        return refuse(format!(
            "--in is signed by {recovered}, not the expected provider {}",
            a.expected_provider
        ));
    }
    let p: V2Policy = serde_json::from_value(body.clone())
        .map_err(|e| ToolError(format!("--in: does not fit schema 2: {e}")))?;
    debug_assert_eq!(p.schema_version, 2);
    if p.cvm.app_id.is_some() && p.cvm.key_provider.is_none() {
        return refuse(
            "the v2 policy pins app_id without key_provider: an app id is host-asserted unless \
             the key provider is pinned too (L2); pin both or neither",
        );
    }
    if p.gpu.allowed_hwmodels.is_empty() {
        return refuse("the v2 policy lists no allowed_hwmodels; v3 needs at least one");
    }
    let policy = Policy {
        doc: POLICY_DOC.into(),
        schema_version: POLICY_SCHEMA_VERSION,
        policy_version: a.policy_version,
        model_id: p.model_id,
        not_before: a.policy_not_before,
        expiry: a.policy_expiry,
        cvm: CvmPolicy {
            require_td_debug_off: p.cvm.require_td_debug_off,
            allowed_tcb_status: p.cvm.allowed_tcb_status,
            allowed_advisory_ids: p.cvm.allowed_advisory_ids,
            require_app_id_pinned: a.require_pins,
            require_key_provider_pinned: a.require_pins,
        },
        gpu: GpuPolicy {
            require_cc_mode: p.gpu.require_cc_mode,
            require_secure_boot: p.gpu.require_secure_boot,
            require_debug_disabled: p.gpu.require_debug_disabled,
            min_driver_version: None,
            min_vbios_version: None,
            allowed_hwmodels: p.gpu.allowed_hwmodels.clone(),
        },
        host_authority: HostAuthority {
            root: a.root.clone(),
            min_manifest_version: a.min_manifest_version,
            min_threshold: a.min_threshold,
            min_profile_epoch: a.min_profile_epoch,
            allowed_classes: vec![a.class.clone()],
        },
    };
    policy.validate().map_err(te)?;
    let profile = HostProfile {
        id: a.id.clone(),
        class: a.class.clone(),
        cvm: ProfileCvm {
            mrtd: p.cvm.mrtd,
            rtmr0: p.cvm.rtmr0,
            rtmr1: p.cvm.rtmr1,
            rtmr2: p.cvm.rtmr2,
            os_image_hash: p.cvm.os_image_hash,
            compose_hashes: vec![p.cvm.compose_hash],
            app_id: p.cvm.app_id,
            key_provider: p.cvm.key_provider,
        },
        gpu: ProfileGpu {
            hwmodels: p.gpu.allowed_hwmodels,
            min_driver_version: p.gpu.min_driver_version,
            min_vbios_version: p.gpu.min_vbios_version,
        },
        not_before: a.profile_not_before,
        expiry: a.profile_expiry,
        evidence: a.evidence.clone(),
    };
    profile.validate().map_err(te)?;
    Ok((policy, profile))
}

/// Write two NEW files (never overwriting); if the second fails, the first is
/// removed, so a migration never leaves half its output.
pub fn write_pair(a: &Path, a_bytes: &[u8], b: &Path, b_bytes: &[u8]) -> Result<(), ToolError> {
    if a == b {
        return refuse(format!("the two outputs are one path ({})", a.display()));
    }
    let mut fa = create_new_0644(a)?;
    with_output(a, || {
        fa.write_all(a_bytes).map_err(te)?;
        fa.sync_all().map_err(te)?;
        crate::kbs::tools::write_new(b, b_bytes)
    })
}

// ---------- policy sign / seal --authority (F10) ----------

/// `policy sign --authority` / `seal --authority`: the bundle verifies for
/// the policy's root (signature, rules, threshold), and the policy's minimums
/// and classes can be met by it: a `root` that does not match, minimums above
/// the bundle's values, `min_threshold` above the manifest's, or
/// `allowed_classes` disjoint from the set are refused, every row named
/// (typos that would otherwise be bound into a 29 GB container).
pub fn check_policy_against_bundle(policy: &Policy, b: &AuthorityBundle) -> Result<(), ToolError> {
    let a = &policy.host_authority;
    let m = &b.manifest.manifest;
    let s = &b.set.set;
    let mut rows = Vec::new();
    if let Err(e) = verify_manifest(&b.manifest, &a.root) {
        rows.push(e.to_string());
    }
    if let Err(e) = verify_set(&b.set, m) {
        rows.push(e.to_string());
    }
    if a.min_manifest_version > m.manifest_version {
        rows.push(format!(
            "min_manifest_version {} is above the bundle's manifest v{}",
            a.min_manifest_version, m.manifest_version
        ));
    }
    if a.min_threshold > m.threshold {
        rows.push(format!(
            "min_threshold {} is above the manifest's threshold {}",
            a.min_threshold, m.threshold
        ));
    }
    if a.min_profile_epoch > s.epoch {
        rows.push(format!(
            "min_profile_epoch {} is above the bundle's epoch {}",
            a.min_profile_epoch, s.epoch
        ));
    }
    if !s
        .profiles
        .iter()
        .any(|p| a.allowed_classes.contains(&p.class))
    {
        rows.push(format!(
            "allowed_classes {:?} share no class with the set's profiles",
            a.allowed_classes
        ));
    }
    if rows.is_empty() {
        Ok(())
    } else {
        refuse(format!("--authority: {}", rows.join("; ")))
    }
}
