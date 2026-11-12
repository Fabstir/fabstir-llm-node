// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The signer's operations, independent of HTTP: authentication, reservation (D10), the D11(A) mp4 sign,
//! the sequence sign (D9), the public manifest read and the detector's index match (D12).

use crate::claim_signer::ClaimSigner;
use crate::keystore::KeyStore;
use crate::repo::{self, Kind, Meta, Record, Repo, RepoError};
use crate::store::{NonceOutcome, Reservation, Store};
use crate::tsa::TsaConfig;
use base64::Engine;
use c2pa::{Builder, Context, Reader};
use fabstir_provenance::contract::{
    self, ErrorCode, Limits, ReserveRequest, ReserveResponse, SequenceGrant, SignRequest,
    SignSequenceRequest,
};
use fabstir_provenance::defs::{self, check_builder, Params, SequenceRef};
use fabstir_provenance::fingerprint::{self, SignatureSet};
use fabstir_provenance::{class, flow, frames_index, matcher};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::{Arc, Mutex};

/// A refusal: contract error code + a message that never echoes request bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fail(pub ErrorCode, pub String);

impl Fail {
    pub fn new(c: ErrorCode, m: impl Into<String>) -> Fail {
        Fail(c, m.into())
    }
}

fn f(c: ErrorCode, m: impl Into<String>) -> Fail {
    Fail::new(c, m)
}

pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after 1970")
            .as_secs()
    })
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub allowlist: HashSet<[u8; 20]>,
    pub informational_url: String,
    pub reserve_min: usize,
    pub reserve_max: usize,
    pub reserve_recommended: usize,
    pub min_bundle_version: u32,
    pub reservations_per_minute: u64,
    pub chain_epoch: u64,
    pub trust_anchors: Option<String>,
    /// Exact node versions allowed into `claim_generator_info.version` (never free text: D6).
    pub node_versions: HashSet<String>,
    /// Authenticated requests per host per minute, all routes (T9).
    pub requests_per_minute: u64,
}

pub struct Service {
    pub settings: Settings,
    pub store: Store,
    pub repo: Repo,
    pub key: Arc<KeyStore>,
    pub chain: Arc<Vec<Vec<u8>>>,
    pub chain_sha256: String,
    pub tsa: TsaConfig,
    pub clock: Clock,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// A signed manifest as returned to the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub manifest: Vec<u8>,
    pub manifest_uuid: String,
    pub sha256: String,
    pub tsa: Option<String>,
    pub replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchOut {
    pub manifest: String,
    pub class: class::Class,
    pub has_visual_input: bool,
    pub time: u64,
    /// false: the manifest is not served publicly (it carries a third-party manifest, D6).
    pub public: bool,
}

/// Most frames one index query may carry (about a minute of video; the detector sends windows).
pub const MAX_QUERY_FRAMES: usize = 1_500;

fn sha_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// `rsv_` + 24 lower-case hex (signer-issued).
pub fn valid_reservation_id(id: &str) -> bool {
    id.strip_prefix("rsv_").is_some_and(|h| {
        h.len() == 24
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// JUMBF superbox: bytes 4..8 are `jumb`. The legacy ZIP archive form (`PK..`) would let a host set
/// builder fields the gate never sees, and is an unbounded decompression; only the JUMBF form is accepted.
pub fn is_jumbf_archive(a: &[u8]) -> bool {
    a.len() >= 8 && &a[4..8] == b"jumb"
}

impl Service {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: Settings,
        store: Store,
        repo: Repo,
        key: Arc<KeyStore>,
        chain: Vec<Vec<u8>>,
        tsa: TsaConfig,
        clock: Clock,
    ) -> anyhow::Result<Service> {
        anyhow::ensure!(!chain.is_empty(), "empty chain");
        let chain_sha256 = sha_hex(&chain[0]);
        Ok(Service {
            settings,
            store,
            repo,
            key,
            chain: Arc::new(chain),
            chain_sha256,
            tsa,
            clock,
            locks: Mutex::new(HashMap::new()),
        })
    }

    fn now(&self) -> u64 {
        (self.clock)()
    }

    pub fn limits(&self) -> Limits {
        Limits {
            api: contract::API_VERSION,
            c2pa_crate: defs::C2PA_CRATE_VERSION.to_string(),
            provenance_crate: fabstir_provenance::CRATE_VERSION.to_string(),
            spec_version: defs::SPEC_VERSION.to_string(),
            time: self.now(),
            reserve_min: self.settings.reserve_min,
            reserve_max: self.settings.reserve_max,
            reserve_recommended: self.settings.reserve_recommended,
            chain_epoch: self.settings.chain_epoch,
            chain_sha256: self.chain_sha256.clone(),
            max_body_bytes: contract::MAX_SIGN_BODY,
            max_sequence_body_bytes: contract::MAX_SEQUENCE_BODY,
        }
    }

    // ------------------------------------------------------------------------------------- auth

    /// Order (contract 1): format, window, recover, allow-list, THEN consume the nonce (so nobody can burn a
    /// host's nonces). Returns the host address (lower-case hex, no 0x).
    pub fn authenticate(
        &self,
        method: &str,
        target: &str,
        ts: &str,
        nonce: &str,
        sig: &str,
        body: &[u8],
    ) -> Result<String, Fail> {
        let now = self.now();
        let addr = match contract::recover_caller(now, method, target, ts, nonce, sig, body) {
            Ok(a) => a,
            Err(contract::AuthError::Stale) => {
                return Err(f(ErrorCode::StaleRequest, "timestamp outside the window"))
            }
            Err(_) => return Err(f(ErrorCode::Unauthorized, "unauthorized")),
        };
        if !self.settings.allowlist.contains(&addr) {
            return Err(f(ErrorCode::Unauthorized, "unauthorized"));
        }
        let host = hex::encode(addr);
        match self
            .store
            .use_nonce(&host, nonce, now, contract::NONCE_MEMORY_SECS)
        {
            Ok(NonceOutcome::Fresh) => {
                let n = self
                    .store
                    .recent_nonces(&host, now, 60)
                    .map_err(|_| f(ErrorCode::Internal, "state"))?;
                if n > self.settings.requests_per_minute {
                    return Err(f(ErrorCode::RateLimited, "too many requests"));
                }
                Ok(host)
            }
            Ok(NonceOutcome::Replayed) => Err(f(ErrorCode::Unauthorized, "unauthorized")),
            Err(_) => Err(f(ErrorCode::Internal, "state")),
        }
    }

    // ------------------------------------------------------------------------------------- reserve

    pub fn reserve(&self, host: &str, req: &ReserveRequest) -> Result<ReserveResponse, Fail> {
        let now = self.now();
        let recent = self
            .store
            .recent_reservations(host, now, 60)
            .map_err(|_| f(ErrorCode::Internal, "state"))?;
        if recent >= self.settings.reservations_per_minute {
            return Err(f(ErrorCode::RateLimited, "too many reservations"));
        }
        let row = class::lookup(&req.template_id)
            .ok_or_else(|| f(ErrorCode::ClassMismatch, "template is not classed"))?;
        if req.bundle_version < self.settings.min_bundle_version {
            return Err(f(ErrorCode::VersionMismatch, "bundle version too old"));
        }
        if !self.settings.node_versions.contains(&req.node_version) {
            return Err(f(ErrorCode::VersionMismatch, "node version not allowed"));
        }
        if req.slots.mp4 != 1 || req.slots.sequence > 1 {
            return Err(f(ErrorCode::MalformedRequest, "slots"));
        }
        if req.slots.sequence == 1 && row.exr_role.is_none() {
            return Err(f(ErrorCode::ClassMismatch, "template delivers no EXR"));
        }
        if req.deadline_secs == 0 || req.deadline_secs > contract::MAX_DEADLINE_SECS {
            return Err(f(ErrorCode::MalformedRequest, "deadline_secs"));
        }
        let manifest_uuid = uuid::Uuid::new_v4().to_string();
        let grant = (req.slots.sequence == 1).then(|| SequenceGrant {
            id: uuid::Uuid::new_v4().to_string(),
            manifest_uuid: uuid::Uuid::new_v4().to_string(),
        });
        let seq_ref = grant.as_ref().map(|g| SequenceRef {
            sequence_id: g.id.clone(),
            manifest_uuid: g.manifest_uuid.clone(),
        });
        let params = Params::for_mp4(
            row,
            &req.node_version,
            &manifest_uuid,
            &self.settings.informational_url,
            seq_ref,
        );
        let seq_params = grant.as_ref().map(|g| {
            Params::for_sequence(
                row,
                &req.node_version,
                &g.manifest_uuid,
                &self.settings.informational_url,
            )
        });
        let id = format!("rsv_{}", hex::encode(rand::random::<[u8; 12]>()));
        let expires_at = now + req.deadline_secs + contract::RESERVATION_GRACE_SECS;
        let r = Reservation {
            id: id.clone(),
            host: host.to_string(),
            template_id: req.template_id.clone(),
            bundle_version: req.bundle_version,
            params_json: serde_json::to_string(&params).expect("params serialise"),
            sequence_params_json: seq_params
                .map(|p| serde_json::to_string(&p).expect("params serialise")),
            sequence_json: grant
                .as_ref()
                .map(|g| serde_json::to_string(g).expect("grant serialises")),
            created_at: now,
            expires_at,
        };
        self.store
            .insert_reservation(&r)
            .map_err(|_| f(ErrorCode::Internal, "state"))?;
        self.store.audit(
            now,
            Some(host),
            "/v1/reserve",
            Some(&id),
            None,
            Some(&manifest_uuid),
            "reserved",
            Some(&req.template_id),
        );
        Ok(ReserveResponse {
            reservation_id: id,
            expires_at,
            params,
            sequence: grant,
        })
    }

    /// Per-reservation lock. Ids are format-checked first so the map holds only real-looking ids, and the
    /// entry is dropped again by [`Service::unlock`] once nobody else holds it.
    fn lock(&self, id: &str) -> Result<Arc<Mutex<()>>, Fail> {
        if !valid_reservation_id(id) {
            return Err(f(ErrorCode::ReservationUnknown, "reservation unknown"));
        }
        let mut m = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        Ok(m.entry(id.to_string()).or_default().clone())
    }

    /// Live per-reservation locks (tests: the map must not grow with junk ids or finished requests).
    pub fn lock_count(&self) -> usize {
        self.locks.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    fn unlock(&self, id: &str, l: Arc<Mutex<()>>) {
        let mut m = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        // map + ours = 2: nobody else is waiting
        if Arc::strong_count(&l) <= 2 {
            m.remove(id);
        }
    }

    /// Reservation checks shared by both sign routes.
    fn owned_live(&self, host: &str, id: &str) -> Result<Reservation, Fail> {
        let r = self
            .store
            .reservation(id)
            .map_err(|_| f(ErrorCode::Internal, "state"))?
            .ok_or_else(|| f(ErrorCode::ReservationUnknown, "reservation unknown"))?;
        if r.host != host {
            return Err(f(
                ErrorCode::ReservationWrongHost,
                "reservation belongs to another host",
            ));
        }
        if self.now() > r.expires_at {
            return Err(f(ErrorCode::ReservationExpired, "reservation expired"));
        }
        Ok(r)
    }

    /// A consumed slot (or a record written just before a crash) answers an identical retry and refuses
    /// anything else. Returns Some(replay) when the request is already done.
    fn already_done(
        &self,
        r: &Reservation,
        slot: &str,
        manifest_uuid: &str,
        body_sha: &str,
    ) -> Result<Option<Signed>, Fail> {
        let used = self
            .store
            .slot(&r.id, slot)
            .map_err(|_| f(ErrorCode::Internal, "state"))?;
        if let Some(u) = used {
            if u.body_sha256 != body_sha {
                return Err(f(
                    ErrorCode::SlotConsumed,
                    "slot already signed for a different request",
                ));
            }
            let rec = self
                .repo
                .get(&u.manifest_uuid)
                .ok_or_else(|| f(ErrorCode::Internal, "record missing"))?;
            return Ok(Some(Signed {
                sha256: sha_hex(&rec.manifest),
                manifest: rec.manifest,
                manifest_uuid: u.manifest_uuid,
                tsa: None,
                replay: true,
            }));
        }
        if let Some(rec) = self.repo.get(manifest_uuid) {
            if rec.meta.request_sha256 != body_sha {
                return Err(f(
                    ErrorCode::SlotConsumed,
                    "slot already signed for a different request",
                ));
            }
            self.store
                .consume_slot(&r.id, slot, body_sha, manifest_uuid, self.now())
                .map_err(|_| f(ErrorCode::Internal, "state"))?;
            return Ok(Some(Signed {
                sha256: sha_hex(&rec.manifest),
                manifest: rec.manifest,
                manifest_uuid: manifest_uuid.to_string(),
                tsa: None,
                replay: true,
            }));
        }
        Ok(None)
    }

    fn signing_context(&self, reserve: usize) -> Result<(Context, ClaimSigner), Fail> {
        let s = ClaimSigner::new(
            self.key.clone(),
            self.chain.clone(),
            self.tsa.clone(),
            reserve,
        );
        let probe = ClaimSigner {
            key: s.key.clone(),
            chain: s.chain.clone(),
            tsa: s.tsa.clone(),
            reserve,
            tsa_used: s.tsa_used.clone(),
            tsa_error: s.tsa_error.clone(),
        };
        let ctx = Context::new()
            .with_settings(flow::settings(self.settings.trust_anchors.as_deref(), true))
            .map_err(|_| f(ErrorCode::Internal, "c2pa settings"))?
            .with_signer(Box::new(s));
        Ok((ctx, probe))
    }

    fn sign_failure(&self, probe: &ClaimSigner, e: &c2pa::Error) -> Fail {
        if probe.tsa_error.lock().unwrap().is_some() {
            return f(ErrorCode::TsaUnavailable, "all TSAs failed");
        }
        let m = e.to_string();
        if m.contains("key:") {
            return f(ErrorCode::KeyUnavailable, "key unavailable");
        }
        // the node's reserve cannot hold the chain + TSA token: tell it to redo the placeholder (contract 4, 8)
        if m.contains("too big for JUMBF box") {
            return f(
                ErrorCode::ReserveTooSmall,
                "reserve_size too small for the signature and timestamp",
            );
        }
        tracing::error!("sign failed: {m}");
        f(ErrorCode::Internal, "signing failed")
    }

    // ------------------------------------------------------------------------------------- mp4 sign

    pub fn sign_mp4(&self, host: &str, body: &[u8], req: &SignRequest) -> Result<Signed, Fail> {
        let lock = self.lock(&req.reservation_id)?;
        let out = {
            let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
            self.sign_mp4_locked(host, body, req)
        };
        self.unlock(&req.reservation_id, lock);
        out
    }

    fn sign_mp4_locked(&self, host: &str, body: &[u8], req: &SignRequest) -> Result<Signed, Fail> {
        let r = self.owned_live(host, &req.reservation_id)?;
        if req.slot != "mp4" {
            return Err(f(ErrorCode::MalformedRequest, "slot"));
        }
        let p: Params =
            serde_json::from_str(&r.params_json).map_err(|_| f(ErrorCode::Internal, "state"))?;
        let body_sha = sha_hex(body);
        if let Some(done) = self.already_done(&r, "mp4", &p.manifest_uuid, &body_sha)? {
            self.store.audit(
                self.now(),
                Some(host),
                "/v1/sign",
                Some(&r.id),
                Some("mp4"),
                Some(&done.manifest_uuid),
                "replay",
                None,
            );
            return Ok(done);
        }
        let out = self.sign_mp4_inner(&r, &p, &body_sha, req);
        let now = self.now();
        match &out {
            Ok(s) => self.store.audit(
                now,
                Some(host),
                "/v1/sign",
                Some(&r.id),
                Some("mp4"),
                Some(&s.manifest_uuid),
                "signed",
                s.tsa.as_deref(),
            ),
            Err(e) => self.store.audit(
                now,
                Some(host),
                "/v1/sign",
                Some(&r.id),
                Some("mp4"),
                Some(&p.manifest_uuid),
                "refused",
                Some(&format!("{:?}", e.0)),
            ),
        }
        out
    }

    fn sign_mp4_inner(
        &self,
        r: &Reservation,
        p: &Params,
        body_sha: &str,
        req: &SignRequest,
    ) -> Result<Signed, Fail> {
        if !(self.settings.reserve_min..=self.settings.reserve_max).contains(&req.reserve_size) {
            return Err(f(
                ErrorCode::ReserveOutOfRange,
                "reserve_size outside the published range",
            ));
        }
        let hash = hex::decode(&req.bmff_hash_sha256_hex)
            .ok()
            .filter(|h| h.len() == 32 && req.bmff_hash_sha256_hex.len() == 64)
            .ok_or_else(|| f(ErrorCode::MalformedRequest, "bmff hash"))?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let (packed, digest) = match (&p.soft_binding_alg, &req.fingerprint_b64) {
            (Some(_), Some(fp)) => {
                let packed = b64
                    .decode(fp)
                    .map_err(|_| f(ErrorCode::MalformedRequest, "fingerprint base64"))?;
                SignatureSet::unpack(&packed)
                    .map_err(|_| f(ErrorCode::MalformedRequest, "fingerprint set"))?;
                let d = fingerprint::digest(&packed);
                (Some(packed), Some(d))
            }
            (Some(_), None) => {
                return Err(f(
                    ErrorCode::MalformedRequest,
                    "a picture output needs its fingerprint",
                ))
            }
            (None, Some(_)) => {
                return Err(f(
                    ErrorCode::ForbiddenStructure,
                    "a data output carries no fingerprint",
                ))
            }
            (None, None) => (None, None),
        };
        let archive = b64
            .decode(&req.builder_archive_b64)
            .map_err(|_| f(ErrorCode::MalformedRequest, "archive base64"))?;
        if !is_jumbf_archive(&archive) {
            return Err(f(
                ErrorCode::ArchiveUnreadable,
                "archive must be a JUMBF working store",
            ));
        }
        // 1. gate the node's archive (parsed through Reader: no legacy ZIP path)
        let gate_ctx = Context::new()
            .with_settings(flow::settings(None, false))
            .map_err(|_| f(ErrorCode::Internal, "c2pa settings"))?;
        let mut g = flow::gate_load(gate_ctx, &archive)
            .map_err(|_| f(ErrorCode::ArchiveUnreadable, "archive unreadable"))?;
        g.set_format(flow::FMT_MP4);
        let bmff =
            flow::bmff_hash(Some(hash.clone())).map_err(|_| f(ErrorCode::Internal, "bmff"))?;
        g.add_assertion(flow::BMFF_LABEL, &bmff)
            .map_err(|_| f(ErrorCode::Internal, "bmff"))?;
        let report = check_builder(&g, p, digest.as_ref()).map_err(refusal)?;
        // 2. sign a builder the SIGNER makes from its own reservation and the gated ingredient formats alone:
        //    nothing of the node's archive (salted hashes, ingredient ids, encodings) reaches the claim
        let (ctx, probe) = self.signing_context(req.reserve_size)?;
        let formats: Vec<&str> = report
            .ingredient_formats
            .iter()
            .map(String::as_str)
            .collect();
        let mut b = flow::build(ctx, p, digest.as_ref(), &formats)
            .map_err(|_| f(ErrorCode::ForbiddenStructure, "rebuild"))?;
        b.set_format(flow::FMT_MP4);
        let bmff = flow::bmff_hash(Some(hash)).map_err(|_| f(ErrorCode::Internal, "bmff"))?;
        b.add_assertion(flow::BMFF_LABEL, &bmff)
            .map_err(|_| f(ErrorCode::Internal, "bmff"))?;
        let ph = b
            .placeholder(flow::FMT_MP4)
            .map_err(|_| f(ErrorCode::ForbiddenStructure, "placeholder"))?;
        if ph.len() != req.placeholder_len {
            return Err(f(
                ErrorCode::PlaceholderLengthMismatch,
                "placeholder length differs",
            ));
        }
        let signed = b
            .sign_embeddable(flow::FMT_MP4)
            .map_err(|e| self.sign_failure(&probe, &e))?;
        if signed.len() != req.placeholder_len {
            return Err(f(
                ErrorCode::Internal,
                "signed manifest length differs from the placeholder",
            ));
        }
        // re-read what we produced and run the same gate on it
        let public = self.post_check(&signed, p, digest.as_ref())?;
        let meta = Meta {
            kind: Kind::Mp4,
            class: p.class,
            has_visual_input: report.has_visual_input,
            time: self.now(),
            public,
            request_sha256: body_sha.to_string(),
        };
        let rec = Record {
            manifest: signed.clone(),
            meta,
            fingerprint: packed,
            frames_index: None,
        };
        self.store_record(r, "mp4", &p.manifest_uuid, body_sha, rec)?;
        Ok(Signed {
            sha256: sha_hex(&signed),
            manifest: signed,
            manifest_uuid: p.manifest_uuid.clone(),
            tsa: tsa_used(&probe),
            replay: false,
        })
    }

    /// Store first (first write wins), then consume the slot: a returned manifest always has its record.
    fn store_record(
        &self,
        r: &Reservation,
        slot: &str,
        uuid: &str,
        body_sha: &str,
        rec: Record,
    ) -> Result<(), Fail> {
        match self.repo.put(uuid, &rec) {
            Ok(()) => {}
            Err(RepoError::Exists) => return Err(f(ErrorCode::SlotConsumed, "record exists")),
            Err(e) => {
                tracing::error!("repository write failed: {e}");
                return Err(f(ErrorCode::Internal, "repository"));
            }
        }
        if !self
            .store
            .consume_slot(&r.id, slot, body_sha, uuid, self.now())
            .map_err(|_| f(ErrorCode::Internal, "state"))?
        {
            return Err(f(ErrorCode::SlotConsumed, "slot consumed"));
        }
        Ok(())
    }

    /// Parse the composed manifest, rebuild a Builder from it and gate it again. Returns whether the
    /// manifest store holds only manifests signed by our chain (else not served publicly, D6).
    fn post_check(
        &self,
        composed: &[u8],
        p: &Params,
        digest: Option<&[u8; 32]>,
    ) -> Result<bool, Fail> {
        if composed.len() <= flow::C2PA_BOX_HDR {
            return Err(f(ErrorCode::Internal, "composed manifest too short"));
        }
        let ctx = Context::new()
            .with_settings(serde_json::json!({"verify": {"verify_after_reading": false, "ocsp_fetch": false, "remote_manifest_fetch": false}}))
            .map_err(|_| f(ErrorCode::Internal, "c2pa settings"))?;
        let reader = Reader::from_context(ctx)
            .with_stream(
                "application/c2pa",
                Cursor::new(composed[flow::C2PA_BOX_HDR..].to_vec()),
            )
            .map_err(|_| f(ErrorCode::Internal, "post-check read"))?;
        // the gate refuses ingredients carrying manifests, so our store holds exactly our one manifest
        if reader.manifests().len() != 1 {
            return Err(f(
                ErrorCode::Internal,
                "post-check: unexpected manifests in the store",
            ));
        }
        let leaf_pem = reader
            .active_manifest()
            .and_then(|m| m.signature_info())
            .map(|s| s.cert_chain().to_string())
            .unwrap_or_default();
        let leaf = pem::parse_many(leaf_pem.as_bytes())
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|p| p.into_contents());
        if leaf.as_deref() != Some(&self.chain[0][..]) {
            return Err(f(
                ErrorCode::Internal,
                "post-check: not signed by our certificate",
            ));
        }
        let public = true;
        let b = reader
            .into_builder()
            .map_err(|_| f(ErrorCode::Internal, "post-check builder"))?;
        check_builder(&b, p, digest).map_err(|e| {
            tracing::error!("post-check refused our own output: {e}");
            f(ErrorCode::Internal, "post-check")
        })?;
        Ok(public)
    }

    // ------------------------------------------------------------------------------------- sequence

    pub fn sign_sequence(
        &self,
        host: &str,
        body: &[u8],
        req: &SignSequenceRequest,
    ) -> Result<Signed, Fail> {
        let lock = self.lock(&req.reservation_id)?;
        let out = {
            let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
            self.sign_sequence_locked(host, body, req)
        };
        self.unlock(&req.reservation_id, lock);
        out
    }

    fn sign_sequence_locked(
        &self,
        host: &str,
        body: &[u8],
        req: &SignSequenceRequest,
    ) -> Result<Signed, Fail> {
        let r = self.owned_live(host, &req.reservation_id)?;
        if req.slot != "sequence" {
            return Err(f(ErrorCode::MalformedRequest, "slot"));
        }
        let sp: Params =
            serde_json::from_str(r.sequence_params_json.as_deref().ok_or_else(|| {
                f(
                    ErrorCode::MalformedRequest,
                    "reservation has no sequence slot",
                )
            })?)
            .map_err(|_| f(ErrorCode::Internal, "state"))?;
        let body_sha = sha_hex(body);
        if let Some(done) = self.already_done(&r, "sequence", &sp.manifest_uuid, &body_sha)? {
            self.store.audit(
                self.now(),
                Some(host),
                "/v1/sign-sequence",
                Some(&r.id),
                Some("sequence"),
                Some(&done.manifest_uuid),
                "replay",
                None,
            );
            return Ok(done);
        }
        let out = self.sign_sequence_inner(&r, &sp, &body_sha, req);
        let now = self.now();
        match &out {
            Ok(s) => self.store.audit(
                now,
                Some(host),
                "/v1/sign-sequence",
                Some(&r.id),
                Some("sequence"),
                Some(&s.manifest_uuid),
                "signed",
                s.tsa.as_deref(),
            ),
            Err(e) => self.store.audit(
                now,
                Some(host),
                "/v1/sign-sequence",
                Some(&r.id),
                Some("sequence"),
                Some(&sp.manifest_uuid),
                "refused",
                Some(&format!("{:?}", e.0)),
            ),
        }
        out
    }

    fn sign_sequence_inner(
        &self,
        r: &Reservation,
        sp: &Params,
        body_sha: &str,
        req: &SignSequenceRequest,
    ) -> Result<Signed, Fail> {
        let index = base64::engine::general_purpose::STANDARD
            .decode(&req.frames_index_b64)
            .map_err(|_| f(ErrorCode::MalformedRequest, "index base64"))?;
        frames_index::validate(&index, sp.class)
            .map_err(|e| f(ErrorCode::MalformedRequest, e.to_string()))?;
        let (ctx, probe) = self.signing_context(self.settings.reserve_recommended)?;
        let def =
            defs::definition_json(sp, None, 0).map_err(|_| f(ErrorCode::Internal, "definition"))?;
        let mut b = Builder::from_context(ctx)
            .with_definition(def)
            .map_err(|_| f(ErrorCode::Internal, "definition"))?;
        b.update_hash_from_stream(defs::FORMAT_SEQUENCE, &mut Cursor::new(index.clone()))
            .map_err(|_| f(ErrorCode::Internal, "data hash"))?;
        let signed = b
            .sign_embeddable("application/c2pa")
            .map_err(|e| self.sign_failure(&probe, &e))?;
        // validate against the index bytes
        let rctx = Context::new()
            .with_settings(flow::settings(
                self.settings.trust_anchors.as_deref(),
                false,
            ))
            .map_err(|_| f(ErrorCode::Internal, "c2pa settings"))?;
        let reader = Reader::from_context(rctx)
            .with_manifest_data_and_stream(
                &signed,
                defs::FORMAT_SEQUENCE,
                Cursor::new(index.clone()),
            )
            .map_err(|_| f(ErrorCode::Internal, "sequence post-check"))?;
        if reader.validation_state() == c2pa::ValidationState::Invalid {
            return Err(f(
                ErrorCode::Internal,
                "sequence manifest does not validate",
            ));
        }
        let meta = Meta {
            kind: Kind::Sequence,
            class: sp.class,
            has_visual_input: false,
            time: self.now(),
            public: true,
            request_sha256: body_sha.to_string(),
        };
        let rec = Record {
            manifest: signed.clone(),
            meta,
            fingerprint: None,
            frames_index: Some(index),
        };
        self.store_record(r, "sequence", &sp.manifest_uuid, body_sha, rec)?;
        Ok(Signed {
            sha256: sha_hex(&signed),
            manifest: signed,
            manifest_uuid: sp.manifest_uuid.clone(),
            tsa: tsa_used(&probe),
            replay: false,
        })
    }

    // ------------------------------------------------------------------------------------- reads

    /// Public read: the manifest bytes, or None (uniform 404) for unknown, malformed or non-public ids.
    pub fn public_manifest(&self, id: &str) -> Option<(Vec<u8>, Kind)> {
        let id = id.strip_prefix("urn:c2pa:").unwrap_or(id);
        if !repo::valid_uuid(id) {
            return None;
        }
        // an mp4 record holds the composed uuid box; the public read serves the bare manifest store
        self.repo.get_public(id).map(|(m, meta)| match meta.kind {
            Kind::Mp4 => (
                m.get(flow::C2PA_BOX_HDR..).unwrap_or_default().to_vec(),
                meta.kind,
            ),
            Kind::Sequence => (m, meta.kind),
        })
    }

    /// Public read of a sequence manifest's frames index (the detector checks a frame's hash in it).
    pub fn public_frames_index(&self, id: &str) -> Option<Vec<u8>> {
        let id = id.strip_prefix("urn:c2pa:").unwrap_or(id);
        if !repo::valid_uuid(id) {
            return None;
        }
        self.repo.get_frames_index(id)
    }

    /// The detector's query: every registered output whose fingerprint matches (never raw fingerprints).
    pub fn index_match(&self, query_packed: &[u8]) -> Result<Vec<MatchOut>, Fail> {
        // frame count from the header before unpacking (a 72,000-frame set is about 28 MB in memory)
        if query_packed.len() >= 14 {
            let n = u32::from_be_bytes(query_packed[10..14].try_into().expect("4 bytes")) as usize;
            if n > MAX_QUERY_FRAMES {
                return Err(f(ErrorCode::BodyTooLarge, "query has too many frames"));
            }
        }
        let q = SignatureSet::unpack(query_packed)
            .map_err(|_| f(ErrorCode::MalformedRequest, "query set"))?;
        if q.frames.len() > MAX_QUERY_FRAMES {
            return Err(f(ErrorCode::BodyTooLarge, "query has too many frames"));
        }
        Ok(self.repo.with_index(|entries| {
            let mut out: Vec<MatchOut> = entries
                .iter()
                .filter(|e| matcher::is_match(&q, &e.set, matcher::AP01B))
                .map(|e| MatchOut {
                    manifest: format!("urn:c2pa:{}", e.uuid),
                    class: e.meta.class,
                    has_visual_input: e.meta.has_visual_input,
                    time: e.meta.time,
                    public: e.meta.public,
                })
                .collect();
            out.sort_by(|a, b| a.time.cmp(&b.time).then(a.manifest.cmp(&b.manifest)));
            out
        }))
    }
}

fn tsa_used(probe: &ClaimSigner) -> Option<String> {
    let g = probe.tsa_used.lock().unwrap();
    g.clone()
}

fn refusal(r: defs::Refusal) -> Fail {
    match r {
        defs::Refusal::AssertionMismatch(l) if l == "c2pa.soft-binding" => {
            f(ErrorCode::FingerprintDigestMismatch, l)
        }
        defs::Refusal::AssertionMismatch(l) => f(ErrorCode::AssertionMismatch, l),
        defs::Refusal::ForbiddenStructure(m) => f(ErrorCode::ForbiddenStructure, m),
    }
}
