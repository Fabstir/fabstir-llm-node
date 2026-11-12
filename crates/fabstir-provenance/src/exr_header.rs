// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! D9 EXR header surgery: insert the string attribute `platformlessai:provenance` into EVERY part's header and
//! shift every offset-table entry by the bytes inserted before the tables. No decode: chunk data is copied
//! verbatim, so pixels are byte-identical. D9a: a result that lands on an exact 256 KiB multiple is redone with
//! the record padded by one space.

use crate::exr_record::{needs_pad, ATTRIBUTE_NAME};

const MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];
const MULTIPART_FLAG: u32 = 0x1000;
const LONG_NAMES_FLAG: u32 = 0x400;
/// Offset tables are read up to this many entries (a 20,000-line scanline file has 20,000).
const MAX_ENTRIES: usize = 1 << 22;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExrError {
    #[error("not an OpenEXR file")]
    NotExr,
    #[error("malformed header: {0}")]
    Header(&'static str),
    #[error("malformed offset tables: {0}")]
    Tables(&'static str),
    #[error("{0} already present")]
    AlreadyMarked(&'static str),
    #[error("deep or unsupported layout: {0}")]
    Unsupported(&'static str),
}

fn le32(b: &[u8], o: usize) -> Result<u32, ExrError> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or(ExrError::Header("truncated"))
}
fn le64(b: &[u8], o: usize) -> Result<u64, ExrError> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).ok_or(ExrError::Tables("truncated"))
}

fn cstr_end(b: &[u8], mut o: usize) -> Result<usize, ExrError> {
    while o < b.len() && b[o] != 0 {
        o += 1;
    }
    if o >= b.len() {
        return Err(ExrError::Header("unterminated string"));
    }
    Ok(o)
}

/// Where each part's header ends (the offset of its 0 terminator) and where the offset tables start.
struct Layout {
    terminators: Vec<usize>,
    tables_start: usize,
    has_attr: bool,
    flags: u32,
}

fn parse(b: &[u8]) -> Result<Layout, ExrError> {
    if b.len() < 8 || b[0..4] != MAGIC {
        return Err(ExrError::NotExr);
    }
    let flags = le32(b, 4)?;
    let multipart = flags & MULTIPART_FLAG != 0;
    let mut o = 8usize;
    let mut terminators = vec![];
    let mut has_attr = false;
    loop {
        if *b.get(o).ok_or(ExrError::Header("truncated"))? == 0 {
            if multipart && !terminators.is_empty() {
                o += 1; // the empty header ending a multipart header list
                break;
            }
            return Err(ExrError::Header("empty header"));
        }
        loop {
            let c = *b.get(o).ok_or(ExrError::Header("truncated"))?;
            if c == 0 {
                terminators.push(o);
                o += 1;
                break;
            }
            let ne = cstr_end(b, o)?;
            if &b[o..ne] == ATTRIBUTE_NAME.as_bytes() {
                has_attr = true;
            }
            let te = cstr_end(b, ne + 1)?;
            let size = le32(b, te + 1)? as usize;
            // deep files: the `type` attribute's VALUE is deepscanline / deeptile (OpenEXR 2 spec)
            if &b[o..ne] == b"type" && b.get(te + 5..te + 5 + size).is_some_and(|v| v.starts_with(b"deep")) {
                return Err(ExrError::Unsupported("deep data"));
            }
            o = (te + 5).checked_add(size).ok_or(ExrError::Header("attribute size"))?;
            if o > b.len() {
                return Err(ExrError::Header("attribute runs past EOF"));
            }
        }
        if !multipart {
            break;
        }
    }
    Ok(Layout { terminators, tables_start: o, has_attr, flags })
}

/// Offset-table entry count: OpenEXR writes the chunks straight after the tables, so the tables end at the
/// smallest chunk offset. Every entry must point inside the file, after the tables.
fn table_entries(b: &[u8], tables_start: usize) -> Result<usize, ExrError> {
    let mut min_off = u64::MAX;
    let mut n = 0usize;
    while ((tables_start + n * 8) as u64) < min_off {
        if n >= MAX_ENTRIES {
            return Err(ExrError::Tables("too many entries"));
        }
        let off = le64(b, tables_start + n * 8)?;
        min_off = min_off.min(off);
        n += 1;
    }
    let tables_end = (tables_start + n * 8) as u64;
    if min_off != tables_end {
        return Err(ExrError::Tables("chunks do not start right after the tables"));
    }
    for k in 0..n {
        let off = le64(b, tables_start + k * 8)?;
        if off < tables_end || off >= b.len() as u64 {
            return Err(ExrError::Tables("entry outside the chunk area"));
        }
    }
    Ok(n)
}

fn attr_bytes(value: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(ATTRIBUTE_NAME.len() + 12 + value.len());
    v.extend_from_slice(ATTRIBUTE_NAME.as_bytes());
    v.push(0);
    v.extend_from_slice(b"string");
    v.push(0);
    v.extend_from_slice(&(value.len() as i32).to_le_bytes());
    v.extend_from_slice(value);
    v
}

fn insert(input: &[u8], value: &[u8]) -> Result<Vec<u8>, ExrError> {
    let lay = parse(input)?;
    if lay.has_attr {
        return Err(ExrError::AlreadyMarked(ATTRIBUTE_NAME));
    }
    if ATTRIBUTE_NAME.len() > 31 && lay.flags & LONG_NAMES_FLAG == 0 {
        return Err(ExrError::Unsupported("long attribute name"));
    }
    let n = table_entries(input, lay.tables_start)?;
    let attr = attr_bytes(value);
    let shift = (attr.len() * lay.terminators.len()) as u64;
    let tables_end = lay.tables_start + n * 8;
    let mut out = Vec::with_capacity(input.len() + shift as usize);
    let mut from = 0usize;
    for &t in &lay.terminators {
        out.extend_from_slice(&input[from..t]);
        out.extend_from_slice(&attr);
        from = t;
    }
    out.extend_from_slice(&input[from..lay.tables_start]);
    for k in 0..n {
        let off = le64(input, lay.tables_start + k * 8)? + shift;
        out.extend_from_slice(&off.to_le_bytes());
    }
    out.extend_from_slice(&input[tables_end..]);
    Ok(out)
}

/// Mark one EXR frame with `record` (the `ExrRecord` JSON bytes). Returns the marked bytes; D9a applied.
pub fn mark(input: &[u8], record: &dyn Fn(usize) -> Vec<u8>) -> Result<Vec<u8>, ExrError> {
    let out = insert(input, &record(0))?;
    if !needs_pad(out.len()) {
        return Ok(out);
    }
    let out = insert(input, &record(1))?;
    debug_assert!(!needs_pad(out.len()));
    Ok(out)
}

/// The record value of every part's header (None if a part has none). For tests and the detector port.
pub fn read_records(b: &[u8]) -> Result<Vec<Option<Vec<u8>>>, ExrError> {
    let lay = parse(b)?;
    let multipart = lay.flags & MULTIPART_FLAG != 0;
    let mut out = vec![];
    let mut o = 8usize;
    for _ in 0..lay.terminators.len() {
        let mut found = None;
        loop {
            if b[o] == 0 {
                o += 1;
                break;
            }
            let ne = cstr_end(b, o)?;
            let te = cstr_end(b, ne + 1)?;
            let size = le32(b, te + 1)? as usize;
            if &b[o..ne] == ATTRIBUTE_NAME.as_bytes() && &b[ne + 1..te] == b"string" {
                found = Some(b[te + 5..te + 5 + size].to_vec());
            }
            o = te + 5 + size;
        }
        out.push(found);
    }
    let _ = multipart;
    Ok(out)
}
