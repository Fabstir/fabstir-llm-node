// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Policy schema 3: host profiles separated from the sealed model binding
//! (`docs/development/IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md`, D1–D13).
//!
//! Three signed documents, one of them bound:
//!
//! * the model release [`Policy`] (provider-signed, bound into every container
//!   chunk's AAD through `policy_hash`) names ONE authority root,
//!   `host_authority.root`;
//! * the [`AuthorityManifest`] (signed by that root) names the signer keys and
//!   the threshold;
//! * the [`HostProfileSet`] (signed by at least `threshold` current signers)
//!   lists the approved host identities, versioned by `epoch`.
//!
//! Only the policy is in any container, so hosts join, change and leave by a
//! new epoch, signers rotate by a new manifest version, and neither touches a
//! model byte. This module holds the feature-free types, their rules, the
//! canonical bytes and digests, the signature checks and the pure matcher; the
//! broker's loader, high-water mark and candidate filter live in `kbs::`.
//!
//! Every document carries a `doc` tag inside its signed body (domain
//! separation: a manifest signature can never verify as a profile set or a
//! policy), and every type refuses unknown fields.

use crate::checkpoint::delta::sort_json_keys;
use crate::crypto::recover_client_address;
use crate::tee::types::{
    check_address, check_hex, check_hex_var, check_token, check_version_floor, sha256_32, Policy,
    TeeError, TeeResult,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

/// `doc` tag of the authority manifest.
pub const MANIFEST_DOC: &str = "fabstir-host-authority-manifest";
/// `doc` tag of the host profile set.
pub const SET_DOC: &str = "fabstir-host-profile-set";
/// The only manifest schema this code reads.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
/// The only profile-set schema this code reads.
pub const SET_SCHEMA_VERSION: u32 = 1;
/// At most this many profiles in one set (D5).
pub const MAX_PROFILES: usize = 64;
/// At most this many signatures on one set (D5): bounds the recovery work an
/// attacker-supplied file can cause.
pub const MAX_SET_SIGNATURES: usize = 16;
/// A profile's window may span at most 400 days (D5): an approval always lapses.
pub const MAX_PROFILE_LIFETIME: u64 = 400 * 86_400;
/// A set's `issued_at` may be at most this far ahead of the broker's clock (D5).
pub const MAX_ISSUED_AT_SKEW: u64 = 300;
/// `evidence` is free text, logged only, bounded (D5).
pub const MAX_EVIDENCE_LEN: usize = 256;

/// Root-signed list of the keys allowed to sign profile sets (D4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityManifest {
    /// Always [`MANIFEST_DOC`].
    pub doc: String,
    /// Always [`MANIFEST_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The authority root, `0x` + 40 lowercase hex. The signature must recover
    /// to exactly this address.
    pub root: String,
    /// Monotonic; the broker's high-water mark refuses a lower one.
    pub manifest_version: u32,
    /// Current signer addresses, `0x` + 40 lowercase hex, distinct; never the root.
    pub signers: Vec<String>,
    /// Distinct current signers a set needs, `1..=signers.len()`.
    pub threshold: u32,
    /// Validity window (unix seconds), inclusive at both ends.
    pub not_before: u64,
    pub expiry: u64,
}

/// An [`AuthorityManifest`] and the root's 65-byte recoverable signature over
/// the EIP-191 digest of its canonical bytes. No claimed-signer field: the
/// recovered address is the fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAuthorityManifest {
    pub manifest: AuthorityManifest,
    pub signature: Vec<u8>,
}

/// The CVM half of an approved host identity. Hex is lowercase without `0x`,
/// exactly as `dstack-mr`, the dashboard and the event log spell it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileCvm {
    /// TD registers, 48 bytes each (96 hex).
    pub mrtd: String,
    pub rtmr0: String,
    pub rtmr1: String,
    pub rtmr2: String,
    /// dstack OS image hash, 32 bytes (64 hex).
    pub os_image_hash: String,
    /// Accepted compose hashes, 32 bytes each (64 hex), at least one.
    pub compose_hashes: Vec<String>,
    /// dstack app id, 20 bytes (40 hex). Host-asserted in KMS mode, so it is
    /// pinned only together with `key_provider` (D18).
    pub app_id: Option<String>,
    /// The `key-provider` event payload, variable-length hex.
    pub key_provider: Option<String>,
}

/// The GPU half of an approved host identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileGpu {
    /// Accepted EAT `hwmodel` strings, at least one. A GPU must ALSO be in the
    /// policy's `allowed_hwmodels`.
    pub hwmodels: Vec<String>,
    /// This family's floors, applied independently of the policy's (D7, D15).
    pub min_driver_version: Option<String>,
    pub min_vbios_version: Option<String>,
}

/// One approved host identity (D5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostProfile {
    /// Unique within the set; `[a-z0-9][a-z0-9/.-]{0,127}`.
    pub id: String,
    /// Host class; `[a-z0-9][a-z0-9-]{0,62}`. A policy admits only its classes.
    pub class: String,
    pub cvm: ProfileCvm,
    pub gpu: ProfileGpu,
    /// Validity window (unix seconds), inclusive, at most [`MAX_PROFILE_LIFETIME`].
    pub not_before: u64,
    pub expiry: u64,
    /// Where the values came from; free text, logged only, never parsed.
    pub evidence: String,
}

/// The epoch-versioned list of approved hosts (D5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostProfileSet {
    /// Always [`SET_DOC`].
    pub doc: String,
    /// Always [`SET_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The authority root this set belongs to (must equal the manifest's).
    pub root: String,
    /// Monotonic; the broker's high-water mark refuses a lower one.
    pub epoch: u64,
    /// When the set was assembled (unix seconds); refused if in the future.
    pub issued_at: u64,
    pub profiles: Vec<HostProfile>,
}

/// A [`HostProfileSet`] and its signers' signatures over one body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedHostProfileSet {
    pub set: HostProfileSet,
    pub signatures: Vec<Vec<u8>>,
}

/// What the broker reads per root, in one file, so a request never sees a new
/// manifest with an old set (D6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityBundle {
    pub manifest: SignedAuthorityManifest,
    pub set: SignedHostProfileSet,
}

/// Canonical, byte-stable JSON of any signed document: `serde_json::to_value`
/// → alphabetically sorted object keys → `to_string`. Signer and verifier MUST
/// produce identical bytes here (the `preserve_order` parity of D23).
pub fn canonical_bytes<T: Serialize>(doc: &T) -> TeeResult<Vec<u8>> {
    let value =
        serde_json::to_value(doc).map_err(|e| TeeError::Crypto(format!("to_value: {e}")))?;
    let sorted = sort_json_keys(&value);
    let s =
        serde_json::to_string(&sorted).map_err(|e| TeeError::Crypto(format!("to_string: {e}")))?;
    Ok(s.into_bytes())
}

/// EIP-191 personal-sign digest:
/// `keccak256("\x19Ethereum Signed Message:\n" + len + bytes)`.
pub fn eip191_digest(canonical: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", canonical.len());
    let mut k = Keccak::v256();
    k.update(prefix.as_bytes());
    k.update(canonical);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

/// SHA-256 of a document's canonical bytes: the digest the broker's
/// high-water mark records (D10).
pub fn document_digest<T: Serialize>(doc: &T) -> TeeResult<[u8; 32]> {
    Ok(sha256_32(&canonical_bytes(doc)?))
}

/// Recover the lowercase `0x` address that signed `doc`.
pub fn recover_document_signer<T: Serialize>(doc: &T, signature: &[u8]) -> TeeResult<String> {
    let digest = eip191_digest(&canonical_bytes(doc)?);
    recover_client_address(signature, &digest)
        .map(|a| a.to_lowercase())
        .map_err(|e| TeeError::VerificationFailed(format!("signature recovery: {e}")))
}

/// Parse a signed document, refusing the wrong document type or schema as
/// `VerificationFailed` BEFORE the typed parse (D3, F12). Only bytes that are
/// not JSON at all come back as `Fetch` (transient: a half-copied file). A
/// schema-2 policy, a document of another type, or a well-formed document that
/// does not fit the schema-3 shape is a permanent refusal, never a retried
/// fault.
///
/// `body` is the JSON pointer of the unsigned body inside the file (`/policy`,
/// `/manifest`, `/set`); `doc` and `schema_version` are read from there.
pub fn parse_signed_document<T: DeserializeOwned>(
    bytes: &[u8],
    what: &str,
    body: &str,
    doc: &str,
    schema_version: u32,
) -> TeeResult<T> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| TeeError::Fetch(format!("{what}: not JSON: {e}")))?;
    peek_document(&value, what, body, doc, schema_version)?;
    serde_json::from_value(value).map_err(|e| {
        TeeError::VerificationFailed(format!("{what}: does not fit the {doc} schema: {e}"))
    })
}

/// The `doc` + `schema_version` check of [`parse_signed_document`], on an
/// already parsed value (the bundle carries two bodies).
pub fn peek_document(
    value: &serde_json::Value,
    what: &str,
    body: &str,
    doc: &str,
    schema_version: u32,
) -> TeeResult<()> {
    let b = value.pointer(body).ok_or_else(|| {
        TeeError::VerificationFailed(format!("{what}: no {body} object (not a {doc})"))
    })?;
    let got_schema = b.get("schema_version").and_then(|v| v.as_u64());
    if got_schema != Some(u64::from(schema_version)) {
        return Err(TeeError::VerificationFailed(format!(
            "{what}: {body} schema_version {} is not {schema_version}{}",
            got_schema.map_or_else(|| "absent".to_string(), |v| v.to_string()),
            if doc == crate::tee::types::POLICY_DOC && got_schema == Some(2) {
                " (schema 2 is no longer accepted: migrate with `fabstir-kbs policy migrate-v2` and reseal)"
            } else {
                ""
            }
        )));
    }
    match b.get("doc").and_then(|v| v.as_str()) {
        Some(d) if d == doc => Ok(()),
        Some(d) => Err(TeeError::VerificationFailed(format!(
            "{what}: {body} doc {d:?} is not {doc:?}"
        ))),
        None => Err(TeeError::VerificationFailed(format!(
            "{what}: {body} has no doc tag (not a {doc})"
        ))),
    }
}

/// Parse an [`AuthorityBundle`] file: one JSON parse, then the `doc` +
/// `schema_version` peek of BOTH bodies (`/manifest/manifest`, `/set/set`)
/// before the typed parse, with [`parse_signed_document`]'s error classes.
pub fn parse_bundle(bytes: &[u8], what: &str) -> TeeResult<AuthorityBundle> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| TeeError::Fetch(format!("{what}: not JSON: {e}")))?;
    peek_document(
        &value,
        what,
        "/manifest/manifest",
        MANIFEST_DOC,
        MANIFEST_SCHEMA_VERSION,
    )?;
    peek_document(&value, what, "/set/set", SET_DOC, SET_SCHEMA_VERSION)?;
    serde_json::from_value(value).map_err(|e| {
        TeeError::VerificationFailed(format!(
            "{what}: does not fit the authority bundle schema: {e}"
        ))
    })
}

fn refuse<T>(msg: String) -> TeeResult<T> {
    Err(TeeError::VerificationFailed(msg))
}

fn check_window(what: &str, not_before: u64, expiry: u64) -> TeeResult<()> {
    if not_before > expiry {
        return refuse(format!(
            "{what}: window inverted (not_before {not_before} > expiry {expiry})"
        ));
    }
    Ok(())
}

impl AuthorityManifest {
    /// Form rules (D4). The signature and the window at `now` are checked by
    /// [`verify_manifest`] and the broker's loader.
    pub fn validate(&self) -> TeeResult<()> {
        if self.doc != MANIFEST_DOC {
            return refuse(format!(
                "manifest doc {:?} is not {MANIFEST_DOC:?}",
                self.doc
            ));
        }
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return refuse(format!(
                "manifest schema_version {} is not {MANIFEST_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        check_address("manifest root", &self.root)?;
        if self.signers.is_empty() {
            return refuse("manifest signers is empty: no set could ever verify".into());
        }
        for (i, s) in self.signers.iter().enumerate() {
            check_address(&format!("manifest signers[{i}]"), s)?;
            if self.signers[..i].contains(s) {
                return refuse(format!("manifest signers: {s} is listed twice"));
            }
            if s == &self.root {
                return refuse(format!(
                    "manifest signers: the root {s} may not also be a signer (the root stays offline)"
                ));
            }
        }
        if self.threshold == 0 || self.threshold as usize > self.signers.len() {
            return refuse(format!(
                "manifest threshold {} outside 1..={}",
                self.threshold,
                self.signers.len()
            ));
        }
        check_window("manifest", self.not_before, self.expiry)
    }
}

/// Validate the manifest's form, recover its signer and require it to equal
/// `manifest.root`, and require `manifest.root == expected_root` (the policy's
/// `host_authority.root`; D6's root equalities). The window at `now` is the
/// caller's (the broker reads `now` once per request).
pub fn verify_manifest(signed: &SignedAuthorityManifest, expected_root: &str) -> TeeResult<()> {
    let m = &signed.manifest;
    m.validate()?;
    let recovered = recover_document_signer(m, &signed.signature)
        .map_err(|e| TeeError::VerificationFailed(format!("manifest {e}")))?;
    if recovered != m.root {
        return refuse(format!(
            "manifest signer {recovered} != manifest root {}",
            m.root
        ));
    }
    if m.root != expected_root {
        return refuse(format!(
            "manifest root {} != the policy's host_authority.root {expected_root}",
            m.root
        ));
    }
    Ok(())
}

impl HostProfile {
    /// Per-profile form rules (D5). Set-level rules (unique ids, count) are
    /// [`HostProfileSet::validate`]'s.
    pub fn validate(&self) -> TeeResult<()> {
        if !id_ok(&self.id) {
            return refuse(format!(
                "profile id {:?}: expected [a-z0-9][a-z0-9/.-]{{0,127}}",
                self.id
            ));
        }
        let at = |field: &str| format!("profile {} {field}", self.id);
        check_token(&at("class"), &self.class)?;
        let c = &self.cvm;
        check_hex(&at("cvm.mrtd"), &c.mrtd, 48)?;
        check_hex(&at("cvm.rtmr0"), &c.rtmr0, 48)?;
        check_hex(&at("cvm.rtmr1"), &c.rtmr1, 48)?;
        check_hex(&at("cvm.rtmr2"), &c.rtmr2, 48)?;
        check_hex(&at("cvm.os_image_hash"), &c.os_image_hash, 32)?;
        if c.compose_hashes.is_empty() {
            return refuse(format!(
                "{}: empty, no compose could ever match",
                at("cvm.compose_hashes")
            ));
        }
        for (i, h) in c.compose_hashes.iter().enumerate() {
            check_hex(&at(&format!("cvm.compose_hashes[{i}]")), h, 32)?;
        }
        if let Some(a) = &c.app_id {
            check_hex(&at("cvm.app_id"), a, 20)?;
            if c.key_provider.is_none() {
                return refuse(format!(
                    "{}: pinned without cvm.key_provider (an app id is host-asserted unless the key provider is pinned too)",
                    at("cvm.app_id")
                ));
            }
        }
        if let Some(k) = &c.key_provider {
            check_hex_var(&at("cvm.key_provider"), k, 1, 512)?;
        }
        if self.gpu.hwmodels.is_empty() {
            return refuse(format!(
                "{}: empty, no GPU could ever match",
                at("gpu.hwmodels")
            ));
        }
        check_version_floor(&at("gpu.min_driver_version"), &self.gpu.min_driver_version)?;
        check_version_floor(&at("gpu.min_vbios_version"), &self.gpu.min_vbios_version)?;
        check_window(
            &format!("profile {}", self.id),
            self.not_before,
            self.expiry,
        )?;
        if self.expiry - self.not_before > MAX_PROFILE_LIFETIME {
            return refuse(format!(
                "profile {}: window spans {} s, more than the {} s (400 day) maximum",
                self.id,
                self.expiry - self.not_before,
                MAX_PROFILE_LIFETIME
            ));
        }
        if self.evidence.len() > MAX_EVIDENCE_LEN
            || !self.evidence.bytes().all(|b| (0x20..=0x7e).contains(&b))
        {
            return refuse(format!(
                "{}: at most {MAX_EVIDENCE_LEN} printable ASCII characters",
                at("evidence")
            ));
        }
        Ok(())
    }

    /// `not_before ≤ now ≤ expiry`, inclusive.
    pub fn in_window(&self, now: u64) -> bool {
        self.not_before <= now && now <= self.expiry
    }
}

fn id_ok(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && matches!(b[0], b'a'..=b'z' | b'0'..=b'9')
        && b[1..]
            .iter()
            .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-'))
}

impl HostProfileSet {
    /// Form rules (D5): `doc`, schema, root form, `1..=64` profiles, unique
    /// ids, every profile's own rules. `issued_at` against the clock is the
    /// broker's loader's (it reads `now`).
    pub fn validate(&self) -> TeeResult<()> {
        if self.doc != SET_DOC {
            return refuse(format!("profile set doc {:?} is not {SET_DOC:?}", self.doc));
        }
        if self.schema_version != SET_SCHEMA_VERSION {
            return refuse(format!(
                "profile set schema_version {} is not {SET_SCHEMA_VERSION}",
                self.schema_version
            ));
        }
        check_address("profile set root", &self.root)?;
        if self.profiles.is_empty() || self.profiles.len() > MAX_PROFILES {
            return refuse(format!(
                "profile set holds {} profiles, expected 1..={MAX_PROFILES}",
                self.profiles.len()
            ));
        }
        for (i, p) in self.profiles.iter().enumerate() {
            p.validate()?;
            if self.profiles[..i].iter().any(|q| q.id == p.id) {
                return refuse(format!("profile id {} appears twice in the set", p.id));
            }
        }
        Ok(())
    }
}

/// Which set signatures counted and which were ignored (D5, L4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Counted {
    /// Distinct CURRENT signers (in the manifest), in order of first appearance.
    pub signers: Vec<String>,
    /// Distinct recovered addresses NOT in the manifest: ignored and logged,
    /// never refused, so a transition set co-signed by an incoming signer
    /// verifies under the old manifest and the new one (F1).
    pub ignored: Vec<String>,
}

/// Validate the set's form, require `set.root == manifest.root`, and count its
/// signatures against `manifest` (which the caller has ALREADY verified with
/// [`verify_manifest`]): `1..=16` signatures; one that does not recover
/// refuses the set; a recovered address outside `manifest.signers` is ignored
/// and logged; duplicates count once; distinct current signers must reach
/// `manifest.threshold`.
pub fn verify_set(
    signed: &SignedHostProfileSet,
    manifest: &AuthorityManifest,
) -> TeeResult<Counted> {
    let set = &signed.set;
    set.validate()?;
    if set.root != manifest.root {
        return refuse(format!(
            "profile set root {} != manifest root {}",
            set.root, manifest.root
        ));
    }
    let n = signed.signatures.len();
    if n == 0 || n > MAX_SET_SIGNATURES {
        return refuse(format!(
            "profile set carries {n} signatures, expected 1..={MAX_SET_SIGNATURES}"
        ));
    }
    let digest = eip191_digest(&canonical_bytes(set)?);
    let mut counted = Counted::default();
    for (i, sig) in signed.signatures.iter().enumerate() {
        let who = recover_client_address(sig, &digest)
            .map(|a| a.to_lowercase())
            .map_err(|e| {
                TeeError::VerificationFailed(format!(
                    "profile set signature {i} does not recover: {e}"
                ))
            })?;
        if manifest.signers.contains(&who) {
            if !counted.signers.contains(&who) {
                counted.signers.push(who);
            }
        } else if !counted.ignored.contains(&who) {
            tracing::warn!(
                signer = %who,
                epoch = set.epoch,
                manifest_version = manifest.manifest_version,
                "profile set signature by an address outside the current manifest: ignored"
            );
            counted.ignored.push(who);
        }
    }
    if (counted.signers.len() as u64) < u64::from(manifest.threshold) {
        return refuse(format!(
            "profile set has {} distinct current signer(s) {:?}, manifest v{} threshold is {} (ignored, not in the manifest: {:?})",
            counted.signers.len(),
            counted.signers,
            manifest.manifest_version,
            manifest.threshold,
            counted.ignored
        ));
    }
    Ok(counted)
}

/// The GPU model as far as the caller knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HwModel {
    /// The verified EAT `hwmodel`.
    Known(String),
    /// The broker's prefilter runs before NRAS: the row is deferred to step 7.
    /// PREFILTER ONLY: step 7 maps `GpuOutcome::Real` to `Known` and canned
    /// evidence to `CannedTestEntry`; passing this there would skip the row.
    NotYetKnown,
    /// Canned GPU evidence on a `test: true` keyring entry: there is no GPU
    /// data, so every GPU row is skipped (D7), this one included.
    CannedTestEntry,
}

/// The host identity the evidence shows, decoded (registers from the quote,
/// the rest from the replayed RTMR3 event log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    pub mr_td: [u8; 48],
    pub rt_mr0: [u8; 48],
    pub rt_mr1: [u8; 48],
    pub rt_mr2: [u8; 48],
    pub compose_hash: [u8; 32],
    pub os_image_hash: [u8; 32],
    pub app_id: Option<Vec<u8>>,
    pub key_provider: Option<Vec<u8>>,
    pub hwmodel: HwModel,
}

fn hex_eq(bytes: &[u8], hex_str: &str) -> bool {
    hex::decode(hex_str).map(|h| h == bytes).unwrap_or(false)
}

fn register_row(id: &str, name: &str, got: &[u8], want: &str, rows: &mut Vec<String>) {
    if !hex_eq(got, want) {
        rows.push(format!(
            "{name}: {} != profile {id} {want}",
            hex::encode(got)
        ));
    }
}

fn hwmodel_rows(p: &HostProfile, got: &str, policy_hwmodels: &[String], rows: &mut Vec<String>) {
    if !p.gpu.hwmodels.iter().any(|h| h == got) {
        rows.push(format!(
            "hwmodel: {got:?} not in profile {} {:?}",
            p.id, p.gpu.hwmodels
        ));
    }
    if !policy_hwmodels.iter().any(|h| h == got) {
        rows.push(format!(
            "hwmodel: {got:?} not in policy {policy_hwmodels:?}"
        ));
    }
}

fn pinned_row(
    id: &str,
    label: &str,
    got: &Option<Vec<u8>>,
    want: &Option<String>,
    rows: &mut Vec<String>,
) {
    if let Some(want) = want {
        match got {
            Some(got) if hex_eq(got, want) => {}
            Some(got) => rows.push(format!(
                "{label}: {} != profile {id} {want}",
                hex::encode(got)
            )),
            None => rows.push(format!("{label}: event absent, profile {id} pins one")),
        }
    }
}

/// Every failing IDENTITY row of profile `p` against `o` (D7, D8); empty =
/// match. Never returns on the first failure: a refusal names every row.
/// Profile-independent rows (report data, RTMR3 replay, model id, TD debug,
/// TCB, advisories, GPU floors and CC) are the caller's, evaluated once.
pub fn match_profile(p: &HostProfile, o: &Observed, policy_hwmodels: &[String]) -> Vec<String> {
    let mut rows = Vec::new();
    let c = &p.cvm;
    register_row(&p.id, "mrtd", &o.mr_td, &c.mrtd, &mut rows);
    register_row(&p.id, "rtmr0", &o.rt_mr0, &c.rtmr0, &mut rows);
    register_row(&p.id, "rtmr1", &o.rt_mr1, &c.rtmr1, &mut rows);
    register_row(&p.id, "rtmr2", &o.rt_mr2, &c.rtmr2, &mut rows);
    if !c.compose_hashes.iter().any(|h| hex_eq(&o.compose_hash, h)) {
        rows.push(format!(
            "compose hash: {} not in profile {} {:?}",
            hex::encode(o.compose_hash),
            p.id,
            c.compose_hashes
        ));
    }
    if !hex_eq(&o.os_image_hash, &c.os_image_hash) {
        rows.push(format!(
            "os image: {} != profile {} {}",
            hex::encode(o.os_image_hash),
            p.id,
            c.os_image_hash
        ));
    }
    pinned_row(&p.id, "app id", &o.app_id, &c.app_id, &mut rows);
    pinned_row(
        &p.id,
        "key provider",
        &o.key_provider,
        &c.key_provider,
        &mut rows,
    );
    match &o.hwmodel {
        HwModel::Known(h) => hwmodel_rows(p, h, policy_hwmodels, &mut rows),
        HwModel::NotYetKnown | HwModel::CannedTestEntry => {}
    }
    rows
}

/// The node's MOCK path only (D13): mock evidence has no registers or event
/// log, so the mock compares exactly the two rows it can observe, `mrtd` (the
/// mock's `image_measurement`) and `hwmodel`. The broker never calls this.
pub fn match_profile_mock(
    p: &HostProfile,
    mrtd: &[u8; 48],
    hwmodel: &str,
    policy_hwmodels: &[String],
) -> Vec<String> {
    let mut rows = Vec::new();
    register_row(&p.id, "mrtd", mrtd, &p.cvm.mrtd, &mut rows);
    hwmodel_rows(p, hwmodel, policy_hwmodels, &mut rows);
    rows
}

/// The ids of the profiles in `candidates` whose identity rows all hold,
/// sorted: several matches are all recorded and the FIRST is the headline
/// (D7).
pub fn matching_ids(
    candidates: &[&HostProfile],
    o: &Observed,
    policy_hwmodels: &[String],
) -> Vec<String> {
    let mut ids: Vec<String> = candidates
        .iter()
        .filter(|p| match_profile(p, o, policy_hwmodels).is_empty())
        .map(|p| p.id.clone())
        .collect();
    ids.sort();
    ids
}

/// The policy-side rules a profile must meet before it is even a candidate
/// (D7): its class is allowed and it is pinned where the policy requires it.
/// The window at `now` is checked separately ([`HostProfile::in_window`]).
pub fn admitted_by_policy(p: &HostProfile, policy: &Policy) -> Result<(), String> {
    if !policy
        .host_authority
        .allowed_classes
        .iter()
        .any(|c| c == &p.class)
    {
        return Err(format!(
            "profile {} class {:?} is not allowed by the policy {:?}",
            p.id, p.class, policy.host_authority.allowed_classes
        ));
    }
    if policy.cvm.require_app_id_pinned && p.cvm.app_id.is_none() {
        return Err(format!(
            "profile {}: the policy requires a pinned app id",
            p.id
        ));
    }
    if policy.cvm.require_key_provider_pinned && p.cvm.key_provider.is_none() {
        return Err(format!(
            "profile {}: the policy requires a pinned key provider",
            p.id
        ));
    }
    Ok(())
}
