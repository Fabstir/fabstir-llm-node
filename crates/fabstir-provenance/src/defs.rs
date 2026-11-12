// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Deterministic C2PA definitions (plan D6, D7, D11(A); golden files in `tests/golden/`).
//!
//! The node feeds `definition_json` to the c2pa `Builder`; the signer rebuilds the same values from ITS
//! reservation and `check_builder` compares them byte for byte (canonical JSON). Both sides link this one
//! crate, so identical bytes come by construction. Nothing here is a prompt, seed, session, job, wallet,
//! host, transaction, input hash or template version.

use crate::class::{Class, OutputRole, TemplateClass};
use crate::fingerprint;
use base64::Engine;
use c2pa::Builder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const C2PA_CRATE_VERSION: &str = "0.90.22";
pub const SPEC_VERSION: &str = "2.5";
pub const CLAIM_GENERATOR_NAME: &str = "Platformless AI";
pub const SOURCE_TYPE_MEDIA: &str =
    "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia";
pub const SOURCE_TYPE_DATA: &str = "http://c2pa.org/digitalsourcetype/trainedAlgorithmicData";
/// IPTC: a mix of synthetic and non-synthetic elements (the frames index of an AI-modified sequence).
pub const SOURCE_TYPE_COMPOSITE: &str =
    "http://cv.iptc.org/newscodes/digitalsourcetype/compositeWithTrainedAlgorithmicMedia";
pub const MODEL_TYPE: &str = "c2pa.types.model.pytorch";
pub const HUMAN_OVERSIGHT: &str = "prompt_guided";
pub const SEQUENCE_LABEL: &str = "org.platformlessai.sequence";
pub const FORMAT_MP4: &str = "video/mp4";
pub const FORMAT_SEQUENCE: &str = "application/json";
/// Most visual inputs any template takes (ingredients mode: several reference stills).
pub const MAX_VISUAL_INPUTS: usize = 8;
/// Media types an ingredient may have.
pub const INGREDIENT_FORMATS: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "video/mp4",
    "video/quicktime",
    "image/x-exr",
];

/// The reference from an EXR job's mp4 manifest to its standalone sequence manifest (D9; non-binding).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceRef {
    pub sequence_id: String,
    pub manifest_uuid: String,
}

/// Everything the signer needs to rebuild the expected manifest. Issued by the signer in the reservation
/// (class, model, ids, urls come from its class table and config); the node only echoes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    pub class: Class,
    pub node_version: String,
    pub model_name: String,
    pub model_identifier: String,
    /// The manifest label is `urn:c2pa:<uuid>`, the instance id `xmp:iid:<uuid>`.
    pub manifest_uuid: String,
    pub format: String,
    /// `Some(fingerprint::ALG)` for a Picture output, None for Data (D1).
    pub soft_binding_alg: Option<String>,
    pub informational_url: String,
    /// The output must carry at least one visual ingredient (class table `visual_input`).
    pub visual_input: bool,
    /// EXR jobs only.
    pub sequence: Option<SequenceRef>,
}

impl Params {
    /// The mp4 params for one template row (what `POST /v1/reserve` returns).
    pub fn for_mp4(
        row: &TemplateClass,
        node_version: &str,
        manifest_uuid: &str,
        informational_url: &str,
        sequence: Option<SequenceRef>,
    ) -> Params {
        Params {
            class: row.class,
            node_version: node_version.to_string(),
            model_name: row.model.name().to_string(),
            model_identifier: row.model.identifier().to_string(),
            manifest_uuid: manifest_uuid.to_string(),
            format: FORMAT_MP4.to_string(),
            soft_binding_alg: (row.mp4_role == OutputRole::Picture)
                .then(|| fingerprint::ALG.to_string()),
            informational_url: informational_url.to_string(),
            visual_input: row.visual_input,
            sequence,
        }
    }

    /// The params of a job's standalone sequence manifest (built by the signer alone).
    pub fn for_sequence(
        row: &TemplateClass,
        node_version: &str,
        sequence_manifest_uuid: &str,
        informational_url: &str,
    ) -> Params {
        // the frames are what the EXR role says: an alpha job's mattes are data, not pictures (D1)
        let class = if row.exr_role == Some(OutputRole::Data) {
            Class::Data
        } else {
            row.class
        };
        Params {
            class,
            node_version: node_version.to_string(),
            model_name: row.model.name().to_string(),
            model_identifier: row.model.identifier().to_string(),
            manifest_uuid: sequence_manifest_uuid.to_string(),
            format: FORMAT_SEQUENCE.to_string(),
            soft_binding_alg: None,
            informational_url: informational_url.to_string(),
            visual_input: false,
            sequence: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DefError {
    #[error("a Picture output needs its fingerprint digest, a Data output must not have one")]
    FingerprintPresence,
    #[error("ingredient count {0} not allowed for this class")]
    IngredientCount(usize),
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Ingredient labels as the node adds them: Modified: `parent` then `input0..`; Generated: `input0..`.
pub fn input_label(k: usize) -> String {
    format!("input{k}")
}
pub const PARENT_LABEL: &str = "parent";

/// The URL c2pa gives the n-th ingredient assertion of a manifest (0-based, in the order added).
pub fn ingredient_url(n: usize) -> String {
    if n == 0 {
        "self#jumbf=c2pa.assertions/c2pa.ingredient.v3".to_string()
    } else {
        format!("self#jumbf=c2pa.assertions/c2pa.ingredient.v3__{n}")
    }
}

/// `resolved` = the form after c2pa has turned `ingredientIds` into hashed URIs (a reloaded archive),
/// with the salted `hash` removed for comparison.
fn actions(p: &Params, n_inputs: usize, resolved: bool) -> Value {
    let refs = |labels: Vec<String>, first_url: usize| -> Value {
        if resolved {
            json!({"ingredients": (0..labels.len()).map(|i| json!({"url": ingredient_url(first_url + i)})).collect::<Vec<_>>()})
        } else {
            json!({"ingredientIds": labels})
        }
    };
    let inputs: Vec<String> = (0..n_inputs).map(input_label).collect();
    let with_inputs = |mut a: Value, first_url: usize| {
        if n_inputs > 0 {
            a["parameters"] = refs(inputs.clone(), first_url);
        }
        a
    };
    if p.format == FORMAT_SEQUENCE {
        // A standalone frames index is a new asset made by the signer, with no ingredients: it is "created",
        // and its source type says what the frames are (D9).
        let st = match p.class {
            Class::Generated => SOURCE_TYPE_MEDIA,
            Class::Modified => SOURCE_TYPE_COMPOSITE,
            Class::Data => SOURCE_TYPE_DATA,
        };
        return json!({"actions": [{"action": "c2pa.created", "digitalSourceType": st}], "allActionsIncluded": true});
    }
    let acts = match p.class {
        Class::Generated => vec![with_inputs(
            json!({"action":"c2pa.created","digitalSourceType":SOURCE_TYPE_MEDIA}),
            0,
        )],
        Class::Data => vec![json!({"action":"c2pa.created","digitalSourceType":SOURCE_TYPE_DATA})],
        Class::Modified => vec![
            json!({"action":"c2pa.opened","parameters": refs(vec![PARENT_LABEL.to_string()], 0)}),
            with_inputs(
                json!({"action":"c2pa.edited","digitalSourceType":SOURCE_TYPE_MEDIA}),
                1,
            ),
        ],
    };
    json!({"actions": acts, "allActionsIncluded": true})
}

fn ai_disclosure(p: &Params) -> Value {
    json!({
        "modelType": MODEL_TYPE,
        "modelName": p.model_name,
        "modelIdentifier": p.model_identifier,
        "contentProfile": {"humanOversightLevel": HUMAN_OVERSIGHT}
    })
}

fn soft_binding(alg: &str, digest: &[u8; 32], url: &str) -> Value {
    json!({
        "alg": alg,
        "blocks": [{"scope": {}, "value": b64(digest)}],
        "bindingMetadata": {"informationalUrl": url}
    })
}

fn sequence_ref(s: &SequenceRef) -> Value {
    json!({"sequenceId": s.sequence_id, "manifest": format!("urn:c2pa:{}", s.manifest_uuid)})
}

fn check_presence(p: &Params, digest: Option<&[u8; 32]>) -> Result<(), DefError> {
    if p.soft_binding_alg.is_some() != digest.is_some() {
        return Err(DefError::FingerprintPresence);
    }
    Ok(())
}

/// Allowed visual-ingredient counts for a class (inputTo count; Modified additionally has its parent).
pub fn check_input_count(p: &Params, n_inputs: usize) -> Result<(), DefError> {
    let ok = match p.class {
        Class::Data => n_inputs == 0,
        Class::Generated if p.visual_input => (1..=MAX_VISUAL_INPUTS).contains(&n_inputs),
        Class::Generated => n_inputs == 0,
        Class::Modified => n_inputs <= MAX_VISUAL_INPUTS,
    };
    if ok {
        Ok(())
    } else {
        Err(DefError::IngredientCount(n_inputs))
    }
}

/// The ordered (label, created, body) list the signer must find, byte for byte, in the builder it signs.
pub fn expected_assertions(
    p: &Params,
    digest: Option<&[u8; 32]>,
    n_inputs: usize,
    resolved: bool,
) -> Result<Vec<(String, bool, Value)>, DefError> {
    check_presence(p, digest)?;
    check_input_count(p, n_inputs)?;
    let mut v = vec![
        (
            "c2pa.actions.v2".to_string(),
            true,
            actions(p, n_inputs, resolved),
        ),
        ("c2pa.ai-disclosure".to_string(), true, ai_disclosure(p)),
    ];
    if let (Some(alg), Some(d)) = (&p.soft_binding_alg, digest) {
        v.push((
            "c2pa.soft-binding".to_string(),
            true,
            soft_binding(alg, d, &p.informational_url),
        ));
    }
    if let Some(s) = &p.sequence {
        v.push((SEQUENCE_LABEL.to_string(), true, sequence_ref(s)));
    }
    Ok(v)
}

pub fn claim_generator_info(p: &Params) -> Value {
    json!([{"name": CLAIM_GENERATOR_NAME, "version": p.node_version, "specVersion": SPEC_VERSION}])
}

/// The deterministic ManifestDefinition JSON fed to `Builder::with_definition`. Ingredients are added
/// through the Builder API in the order `ingredient_json` documents.
pub fn definition_json(
    p: &Params,
    digest: Option<&[u8; 32]>,
    n_inputs: usize,
) -> Result<Value, DefError> {
    let assertions: Vec<Value> = expected_assertions(p, digest, n_inputs, false)?
        .into_iter()
        .map(|(label, created, data)| json!({"label": label, "data": data, "created": created}))
        .collect();
    Ok(json!({
        "claim_version": 2,
        "claim_generator_info": claim_generator_info(p),
        "format": p.format,
        "label": format!("urn:c2pa:{}", p.manifest_uuid),
        "instance_id": format!("xmp:iid:{}", p.manifest_uuid),
        "assertions": assertions
    }))
}

/// Ingredient JSON: relationship and label only, NO title (D6).
/// The instance id of the ingredient at `position`: derived from the signer-issued manifest uuid, so neither an
/// input's XMP nor the node can put anything there (contract v1.0).
pub fn ingredient_instance_id(manifest_uuid: &str, position: usize) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(
        format!("platformlessai-ingredient-v1\n{manifest_uuid}\n{position}").as_bytes(),
    );
    let x = hex::encode(&h[..16]);
    format!(
        "xmp:iid:{}-{}-{}-{}-{}",
        &x[..8],
        &x[8..12],
        &x[12..16],
        &x[16..20],
        &x[20..32]
    )
}

/// The full ingredient JSON for `position` (0-based; a Modified output's parent is position 0). Built from the
/// format alone, never from the input's bytes: no XMP, no input manifest, no title, no thumbnail (D6).
pub fn ingredient_json(p: &Params, position: usize, format: &str) -> Value {
    let (relationship, label) = match (p.class, position) {
        (Class::Modified, 0) => ("parentOf", PARENT_LABEL.to_string()),
        (Class::Modified, k) => ("inputTo", input_label(k - 1)),
        (_, k) => ("inputTo", input_label(k)),
    };
    json!({"relationship": relationship, "label": label, "format": format, "instance_id": ingredient_instance_id(&p.manifest_uuid, position)})
}

pub fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                out.insert(k.clone(), sort_keys(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sort_keys).collect()),
        o => o.clone(),
    }
}

/// Canonical JSON: sorted keys, compact.
pub fn canon_bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(&sort_keys(v)).expect("JSON value serialises")
}

/// Why the signer refuses a builder (maps to the contract's 422 codes).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Refusal {
    /// `assertion_mismatch`: names the first label that differs, never echoes bodies.
    #[error("assertion_mismatch: {0}")]
    AssertionMismatch(String),
    /// `forbidden_structure`: hard-binding count, extra label, ingredient shape.
    #[error("forbidden_structure: {0}")]
    ForbiddenStructure(String),
}

impl From<DefError> for Refusal {
    fn from(e: DefError) -> Self {
        Refusal::ForbiddenStructure(e.to_string())
    }
}

/// What the gate saw (for the audit log; carries no bodies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub compared: Vec<String>,
    pub n_inputs: usize,
    pub has_visual_input: bool,
    /// Ingredient formats in order (the signer rebuilds the ingredients from these alone).
    pub ingredient_formats: Vec<String>,
}

fn mismatch(what: &str) -> Refusal {
    Refusal::AssertionMismatch(what.to_string())
}
fn forbidden(what: impl Into<String>) -> Refusal {
    Refusal::ForbiddenStructure(what.into())
}

/// D11(A) signer-side gate. `p` is what the signer's OWN reservation says; `digest` is SHA-256 of the
/// fingerprint set sent in the same request (None for Data). The builder must already hold exactly one
/// hard binding (`c2pa.hash.bmff.v3`, added by the signer). Compares every non-binding assertion body
/// byte for byte, checks the ingredients structurally, and returns what it found.
pub fn check_builder(
    b: &Builder,
    p: &Params,
    digest: Option<&[u8; 32]>,
) -> Result<CheckReport, Refusal> {
    let d = &b.definition;
    let mut compared = vec![];

    let got_cgi = serde_json::to_value(&d.claim_generator_info)
        .map_err(|_| mismatch("claim_generator_info"))?;
    let mut want_cgi = claim_generator_info(p);
    // c2pa-rs injects its own marker into entry 0 (same crate version on both sides).
    want_cgi[0]["org.contentauth.c2pa_rs"] = json!(C2PA_CRATE_VERSION);
    if canon_bytes(&got_cgi) != canon_bytes(&want_cgi) {
        return Err(mismatch("claim_generator_info"));
    }
    compared.push("claim_generator_info".to_string());

    if d.label.as_deref() != Some(format!("urn:c2pa:{}", p.manifest_uuid).as_str()) {
        return Err(mismatch("label"));
    }
    if d.instance_id != format!("xmp:iid:{}", p.manifest_uuid) {
        return Err(mismatch("instance_id"));
    }
    // the archive round trip drops `format`; the signer sets it itself
    if !d.format.is_empty() && d.format != p.format {
        return Err(mismatch("format"));
    }
    // builder-level fields the claim would inherit (the legacy ZIP archive form can set them all)
    if d.vendor.is_some()
        || !matches!(d.claim_version, None | Some(2))
        || d.hash_alg.as_deref().is_some_and(|h| h != "sha256")
    {
        return Err(forbidden("vendor, claim version or hash algorithm"));
    }
    let bv = serde_json::to_value(b).map_err(|_| forbidden("unserialisable builder"))?;
    let empty = |k: &str| match &bv[k] {
        Value::Null => true,
        Value::Bool(x) => !x,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::String(s) => s.is_empty(),
        _ => false,
    };
    if !empty("remote_url")
        || !empty("no_embed")
        || !empty("intent")
        || !empty("timestamp_manifest_labels")
    {
        return Err(forbidden(
            "remote_url, no_embed, intent or timestamp labels",
        ));
    }
    if d.title.is_some() || d.thumbnail.is_some() || d.redactions.is_some() || d.metadata.is_some()
    {
        return Err(forbidden(
            "title, thumbnail, redactions or metadata in the definition",
        ));
    }

    // ingredients, structurally (first: they fix how many inputs the actions must reference)
    let ings: Vec<Value> = d
        .ingredients
        .iter()
        .map(|i| serde_json::to_value(i).unwrap_or(Value::Null))
        .collect();
    let mut idx = 0;
    if p.class == Class::Modified {
        let first = ings
            .first()
            .ok_or_else(|| forbidden("modified class needs a parentOf ingredient"))?;
        check_ingredient(first, "parentOf", 0)?;
        idx = 1;
    }
    for (k, ing) in ings[idx..].iter().enumerate() {
        check_ingredient(ing, "inputTo", idx + k)?;
    }
    let n_inputs = ings.len() - idx;
    check_input_count(p, n_inputs)?;
    let has_visual_input = !ings.is_empty();
    if p.visual_input != has_visual_input && p.class != Class::Data {
        return Err(forbidden(
            "visual ingredient presence differs from the class table",
        ));
    }

    // assertions: hard bindings apart, the rest compared
    let mut hard = vec![];
    let mut rest: Vec<(String, bool, Value)> = vec![];
    for a in &d.assertions {
        let av = serde_json::to_value(a).map_err(|_| forbidden("unserialisable assertion"))?;
        let label = av["label"].as_str().unwrap_or("").to_string();
        let created = av["created"].as_bool().unwrap_or(false);
        if label.starts_with("c2pa.hash.") {
            hard.push(label);
            continue;
        }
        rest.push((label, created, av["data"].clone()));
    }
    for (label, _, body) in rest.iter_mut() {
        if label == "c2pa.actions.v2" {
            strip_ingredient_hashes(body);
        }
    }
    let want = expected_assertions(p, digest, n_inputs, true)?;
    let labels = |v: &[(String, bool, Value)]| {
        let mut l: Vec<String> = v.iter().map(|x| x.0.clone()).collect();
        l.sort();
        l
    };
    if labels(&rest) != labels(&want) {
        return Err(forbidden("assertion labels differ from the expected set"));
    }
    let key = |x: &(String, bool, Value)| (x.0.clone(), canon_bytes(&x.2));
    let mut got_sorted = rest.clone();
    let mut want_sorted = want.clone();
    got_sorted.sort_by_key(key);
    want_sorted.sort_by_key(key);
    for (g, w) in got_sorted.iter().zip(want_sorted.iter()) {
        if g.1 != w.1 || canon_bytes(&g.2) != canon_bytes(&w.2) {
            return Err(mismatch(&w.0));
        }
        compared.push(w.0.clone());
    }
    if hard.len() != 1 || hard[0] != "c2pa.hash.bmff.v3" {
        return Err(forbidden(format!(
            "exactly one c2pa.hash.bmff.v3 hard binding required, found {}",
            hard.len()
        )));
    }
    let ingredient_formats = ings
        .iter()
        .map(|i| i["format"].as_str().unwrap_or("").to_string())
        .collect();
    Ok(CheckReport {
        compared,
        n_inputs,
        has_visual_input,
        ingredient_formats,
    })
}

fn strip_ingredient_hashes(body: &mut Value) {
    if let Some(acts) = body.get_mut("actions").and_then(|a| a.as_array_mut()) {
        for a in acts {
            if let Some(ings) = a
                .get_mut("parameters")
                .and_then(|p| p.get_mut("ingredients"))
                .and_then(|i| i.as_array_mut())
            {
                for i in ings {
                    if let Some(o) = i.as_object_mut() {
                        o.remove("hash");
                    }
                }
            }
        }
    }
}

/// The only ingredient fields Fabstir signs. Everything else c2pa would copy into the signed ingredient
/// assertion (description, metadata, informational URI, document id, data, data types, hash, OCSP
/// responses, validation results, an active manifest and its store) is refused: those are node-written and
/// would let a host put a prompt or an identity under Fabstir's signature. In v1 that includes an input's
/// own C2PA manifest (its validation results are node-written text); retention of input credentials is LATER.
pub const INGREDIENT_KEYS: &[&str] = &["relationship", "format", "label", "instance_id"];

fn is_iid(s: &str) -> bool {
    s.strip_prefix("xmp:iid:").is_some_and(|u| {
        u.len() == 36
            && u.bytes().enumerate().all(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
    })
}

/// c2pa's label for the n-th ingredient assertion.
pub fn ingredient_label(n: usize) -> String {
    if n == 0 {
        "c2pa.ingredient.v3".to_string()
    } else {
        format!("c2pa.ingredient.v3__{n}")
    }
}

fn check_ingredient(ing: &Value, relationship: &str, position: usize) -> Result<(), Refusal> {
    let o = ing
        .as_object()
        .ok_or_else(|| forbidden("unserialisable ingredient"))?;
    for (k, v) in o {
        if !v.is_null() && !INGREDIENT_KEYS.contains(&k.as_str()) {
            return Err(forbidden(format!("ingredient field {k} not allowed")));
        }
    }
    if ing["relationship"] != relationship {
        return Err(forbidden(format!(
            "ingredient relationship must be {relationship}"
        )));
    }
    let fmt = ing["format"].as_str().unwrap_or("");
    if !INGREDIENT_FORMATS.contains(&fmt) {
        return Err(forbidden("ingredient format not allowed"));
    }
    if !ing["label"].is_null() && ing["label"].as_str() != Some(ingredient_label(position).as_str())
    {
        return Err(forbidden("ingredient label"));
    }
    if !ing["instance_id"].is_null() && !ing["instance_id"].as_str().is_some_and(is_iid) {
        return Err(forbidden("ingredient instance id must be xmp:iid:<uuid>"));
    }
    Ok(())
}
