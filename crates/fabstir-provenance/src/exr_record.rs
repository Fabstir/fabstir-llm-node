// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The EXR header record (D9): the JSON value of the string attribute `platformlessai:provenance` that the
//! node inserts into every EXR header. It points at the job's manifests; it is not itself signed (the chain
//! of proof is record -> sequence manifest -> the frame's hash in the signed index).

use crate::class::Class;
use crate::defs::{canon_bytes, CLAIM_GENERATOR_NAME};
use serde_json::json;

pub const ATTRIBUTE_NAME: &str = "platformlessai:provenance";
pub const PROVIDER: &str = "Fabstir Services Limited";
pub const SCHEMA: &str = "platformlessai.exr-provenance.v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExrRecord {
    pub class: Class,
    pub node_version: String,
    /// RFC 3339 UTC, seconds precision (the job's delivery time).
    pub time: String,
    pub manifest_uuid: String,
    pub sequence_id: String,
    pub sequence_manifest_uuid: String,
    pub resolver_url: String,
}

impl ExrRecord {
    /// Canonical JSON bytes; `pad` trailing spaces are appended after the object (D9a: one space moves a
    /// frame off a 256 KiB multiple). Readers trim.
    pub fn to_bytes(&self, pad: usize) -> Vec<u8> {
        let class = match self.class {
            Class::Generated => "generated",
            Class::Modified => "modified",
            Class::Data => "data",
        };
        let mut b = canon_bytes(&json!({
            "schema": SCHEMA,
            "class": class,
            "provider": PROVIDER,
            "system": CLAIM_GENERATOR_NAME,
            "version": self.node_version,
            "time": self.time,
            "manifest": format!("urn:c2pa:{}", self.manifest_uuid),
            "sequenceId": self.sequence_id,
            "sequenceManifest": format!("urn:c2pa:{}", self.sequence_manifest_uuid),
            "resolver": self.resolver_url,
        }));
        b.extend(std::iter::repeat_n(b' ', pad));
        b
    }
}

/// D9a: true when a length lands on an exact multiple of 256 KiB (`exr::encrypt_frame` refuses those).
pub fn needs_pad(len: usize) -> bool {
    len > 0 && len.is_multiple_of(262_144)
}
