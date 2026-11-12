// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Node-side C2PA flow of D11(A) (proven in spike AP0.2): build the Builder from the shared definition,
//! archive it BEFORE any hard binding exists, add the hard binding with the shared exclusion set, make the
//! placeholder with the node's chosen reserve, embed it, hash. The node never holds a key or a chain.

use crate::defs::{self, Params};
use c2pa::assertions::BmffHash;
use c2pa::{Builder, Context, Signer, SigningAlg};
use serde_json::{json, Value};
use std::io::Cursor;

pub const FMT_MP4: &str = "video/mp4";
pub const BMFF_LABEL: &str = "c2pa.hash.bmff.v3";
/// Composed BMFF C2PA box header: 8 (size, type) + 16 (uuid) + 4 (version, flags) + 9 ("manifest\0") + 8 (merkle offset).
pub const C2PA_BOX_HDR: usize = 45;
pub const C2PA_UUID: [u8; 16] = [
    0xd8, 0xfe, 0xc3, 0xd6, 0x1b, 0x0e, 0x48, 0x3c, 0x92, 0x97, 0x58, 0x28, 0x87, 0x7e, 0xc4, 0x81,
];

#[derive(Debug, thiserror::Error)]
pub enum FlowError {
    #[error("definition: {0}")]
    Def(#[from] defs::DefError),
    #[error("c2pa: {0}")]
    C2pa(#[from] c2pa::Error),
    #[error("{0}")]
    Other(String),
}

fn other(m: impl Into<String>) -> FlowError {
    FlowError::Other(m.into())
}

/// The hard binding both sides build: SHA-256, BMFF v3, exclusions `/uuid(C2PA)`, `/ftyp`, `/mfra` only
/// (the trimmed set adopted in AP0.2 item 11, avoiding c2pa-rs 0.91's `additionalExclusionsPresent`).
pub fn bmff_hash(hash: Option<Vec<u8>>) -> Result<BmffHash, FlowError> {
    let mut b = BmffHash::new("jumbf manifest", "sha256", None);
    b.set_default_exclusions();
    b.exclusions_mut()
        .retain(|e| e.xpath != "/free" && e.xpath != "/skip");
    b.set_bmff_version(3);
    match hash {
        Some(h) => b.set_hash(h),
        None => b.add_place_holder_hash()?,
    }
    Ok(b)
}

/// The xpaths of the shared exclusion set (the signer's structural check compares against these).
pub fn exclusion_xpaths() -> Vec<String> {
    let b = bmff_hash(Some(vec![0u8; 32])).expect("static construction");
    b.exclusions().iter().map(|e| e.xpath.clone()).collect()
}

/// Node-side stand-in for the c2pa Signer: it can only answer `reserve_size` (placeholder sizing).
pub struct ReserveOnly(pub usize);
impl Signer for ReserveOnly {
    fn sign(&self, _data: &[u8]) -> c2pa::Result<Vec<u8>> {
        Err(c2pa::Error::BadParam("the node cannot sign".into()))
    }
    fn alg(&self) -> SigningAlg {
        SigningAlg::Es256
    }
    fn certs(&self) -> c2pa::Result<Vec<Vec<u8>>> {
        Ok(vec![])
    }
    fn reserve_size(&self) -> usize {
        self.0
    }
}

/// c2pa settings: never fetch anything; optional trust anchors (PEM) for reading inputs' own manifests.
pub fn settings(trust_anchors: Option<&str>, verify_after_sign: bool) -> Value {
    let mut s = json!({"verify": {"verify_after_sign": verify_after_sign, "ocsp_fetch": false, "remote_manifest_fetch": false}});
    if let Some(a) = trust_anchors {
        s["trust"] = json!({"trust_anchors": a});
    }
    s
}

/// Build the Builder from the shared definition and ingredients made from their formats alone (parent first
/// for a Modified output, then the inputs). Node and signer call this same function, so the builder the signer
/// signs is byte-for-byte the one the node prepared, and nothing node-chosen beyond the gated values reaches it.
pub fn build(
    ctx: Context,
    p: &Params,
    digest: Option<&[u8; 32]>,
    ingredient_formats: &[&str],
) -> Result<Builder, FlowError> {
    let n_inputs = match p.class {
        crate::Class::Modified => ingredient_formats
            .len()
            .checked_sub(1)
            .ok_or_else(|| other("modified class needs its parent"))?,
        _ => ingredient_formats.len(),
    };
    let def = defs::definition_json(p, digest, n_inputs)?;
    let mut b = Builder::from_context(ctx).with_definition(def)?;
    for (k, fmt) in ingredient_formats.iter().enumerate() {
        let ing: c2pa::Ingredient = serde_json::from_value(defs::ingredient_json(p, k, fmt))
            .map_err(|e| other(format!("ingredient: {e}")))?;
        b.add_ingredient(ing);
    }
    Ok(b)
}

/// Parse a node's archive for the gate: the JUMBF working store through `Reader` (never `Builder::with_archive`,
/// whose legacy ZIP path deserialises a whole Builder from the node's JSON).
pub fn gate_load(ctx: Context, archive: &[u8]) -> Result<Builder, FlowError> {
    if archive.len() < 8 || &archive[4..8] != b"jumb" {
        return Err(other("archive must be a JUMBF working store"));
    }
    let r = c2pa::Reader::from_context(ctx)
        .with_stream("application/c2pa", Cursor::new(archive.to_vec()))?;
    Ok(r.into_builder()?)
}

/// The mp4 with its placeholder, ready to be patched with the signer's manifest.
pub struct Prepared {
    pub with_placeholder: Vec<u8>,
    pub composed_len: usize,
    pub uuid_offset: usize,
}

/// What the node sends (with the reservation id and the fingerprint) in `POST /v1/sign`.
pub struct Handoff {
    pub archive: Vec<u8>,
    pub bmff_hash: Vec<u8>,
    pub placeholder_len: usize,
    pub reserve: usize,
}

/// Top-level ISO-BMFF boxes: (type, offset, size). Stops at the first malformed box.
pub fn top_boxes(b: &[u8]) -> Vec<(String, usize, usize)> {
    let mut v = vec![];
    let mut i = 0usize;
    while i + 8 <= b.len() {
        let mut sz = u32::from_be_bytes(b[i..i + 4].try_into().unwrap()) as usize;
        let typ = String::from_utf8_lossy(&b[i + 4..i + 8]).to_string();
        if sz == 1 {
            if i + 16 > b.len() {
                break;
            }
            sz = u64::from_be_bytes(b[i + 8..i + 16].try_into().unwrap()) as usize;
        } else if sz == 0 {
            sz = b.len() - i;
        }
        if sz < 8 || i.checked_add(sz).map(|e| e > b.len()).unwrap_or(true) {
            break;
        }
        v.push((typ, i, sz));
        i += sz;
    }
    v
}

pub fn find_c2pa_box(b: &[u8]) -> Option<(usize, usize)> {
    top_boxes(b)
        .into_iter()
        .find(|(t, off, _)| t == "uuid" && b.len() >= off + 24 && b[off + 8..off + 24] == C2PA_UUID)
        .map(|(_, o, s)| (o, s))
}

/// D11(A) node steps. `ctx` must not carry a key (use `ReserveOnly`); `src` = the mp4 as fetched.
pub fn prepare_mp4(
    ctx: Context,
    p: &Params,
    digest: Option<&[u8; 32]>,
    ingredient_formats: &[&str],
    src: &[u8],
    reserve: usize,
) -> Result<(Handoff, Prepared), FlowError> {
    let mut b = build(ctx, p, digest, ingredient_formats)?;
    let mut ar = Cursor::new(Vec::<u8>::new());
    b.to_archive(&mut ar)?;
    b.add_assertion(BMFF_LABEL, &bmff_hash(None)?)?;
    let composed = b.placeholder(FMT_MP4)?;
    if composed.len() < C2PA_BOX_HDR + 16 {
        return Err(other("composed placeholder too short"));
    }
    let mut out = Cursor::new(Vec::<u8>::new());
    c2pa::jumbf_io::save_jumbf_to_stream(
        FMT_MP4,
        &mut Cursor::new(src.to_vec()),
        &mut out,
        &composed[C2PA_BOX_HDR..],
    )?;
    let bytes = out.into_inner();
    let (off, sz) = find_c2pa_box(&bytes).ok_or_else(|| other("no c2pa box after embed"))?;
    if sz != composed.len() || bytes[off..off + sz] != composed[..] {
        return Err(other("embedded box differs from the composed placeholder"));
    }
    b.update_hash_from_stream(FMT_MP4, &mut Cursor::new(bytes.clone()))?;
    let hv = b
        .definition
        .assertions
        .iter()
        .filter_map(|a| serde_json::to_value(a).ok())
        .find(|a| a["label"] == BMFF_LABEL)
        .ok_or_else(|| other("no bmff hash in builder"))?;
    let bmff_hash = hash_bytes(&hv["data"]["hash"]).ok_or_else(|| other("unreadable bmff hash"))?;
    if bmff_hash.len() != 32 {
        return Err(other("bmff hash is not 32 bytes"));
    }
    Ok((
        Handoff {
            archive: ar.into_inner(),
            bmff_hash,
            placeholder_len: composed.len(),
            reserve,
        },
        Prepared {
            with_placeholder: bytes,
            composed_len: composed.len(),
            uuid_offset: off,
        },
    ))
}

fn hash_bytes(v: &Value) -> Option<Vec<u8>> {
    use base64::Engine;
    match v {
        Value::Array(a) => a
            .iter()
            .map(|x| x.as_u64().filter(|n| *n < 256).map(|n| n as u8))
            .collect(),
        Value::String(s) => base64::engine::general_purpose::STANDARD.decode(s).ok(),
        _ => None,
    }
}

/// Patch the signer's composed manifest into the placeholder (lengths must match exactly).
pub fn patch(prep: &Prepared, signed_composed: &[u8]) -> Result<Vec<u8>, FlowError> {
    if signed_composed.len() != prep.composed_len {
        return Err(other(format!(
            "signed manifest length {} != placeholder length {}",
            signed_composed.len(),
            prep.composed_len
        )));
    }
    let mut out = prep.with_placeholder.clone();
    out[prep.uuid_offset..prep.uuid_offset + prep.composed_len].copy_from_slice(signed_composed);
    Ok(out)
}

/// The final length of the mp4 (D9a check before hashing).
pub fn final_len(prep: &Prepared) -> usize {
    prep.with_placeholder.len()
}
