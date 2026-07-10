//! Minimal, panic-safe ISO-BMFF (mp4) inspection for the BL3 control-video gate.
//!
//! The pinned iclora graph derives its rendered frame count from the CONTROL
//! CLIP (`Video Slice` → MoGe → latent length), while billing uses the job's
//! `frames` — so a client speaking the WS directly could upload a higher-fps
//! clip and make the GPU render several times the billed frames (the TS helper
//! enforces conformance client-side, which a direct client can skip). The node
//! therefore reads the video track's own sample count from the container index
//! (`moov/trak/mdia/minf/stbl/stsz`) — no decode — and the handler rejects the
//! job before any GPU work when it falls outside the billed range. Bounding the
//! SAMPLE COUNT alone caps the render: `Video Slice` can never emit more frames
//! than the clip contains (`strict_duration` is false in the pinned graph).
//!
//! Every read is bounds-checked; malformed input yields `Err`, never a panic.
//! Fragmented mp4 (moof-based, empty `stsz`) is rejected fail-closed — every
//! encoder in this pipeline (Blender FFMPEG, ffmpeg CLI) writes a plain moov.

/// The number of samples (frames) in the FIRST video (`hdlr` = `vide`) track.
pub fn video_sample_count(bytes: &[u8]) -> Result<u64, String> {
    let moov = find_box(bytes, 0, bytes.len(), b"moov")?
        .ok_or_else(|| "no moov box (fragmented or truncated mp4?)".to_string())?;
    let mut off = moov.0;
    while off + 8 <= moov.1 {
        let (typ, payload_start, box_end) = read_box_header(bytes, off, moov.1)?;
        if typ == *b"trak" {
            if let Some(hdlr) = find_box_path(bytes, payload_start, box_end, &[b"mdia", b"hdlr"])? {
                // hdlr payload: version/flags (4) + pre_defined (4) + handler_type (4)
                let handler = bytes
                    .get(hdlr.0 + 8..hdlr.0 + 12)
                    .ok_or_else(|| "truncated hdlr box".to_string())?;
                if handler == b"vide" {
                    let stsz = find_box_path(
                        bytes,
                        payload_start,
                        box_end,
                        &[b"mdia", b"minf", b"stbl", b"stsz"],
                    )?
                    .ok_or_else(|| "video track has no stsz box (fragmented mp4?)".to_string())?;
                    // stsz payload: version/flags (4) + sample_size (4) + sample_count (4)
                    let count = bytes
                        .get(stsz.0 + 8..stsz.0 + 12)
                        .ok_or_else(|| "truncated stsz box".to_string())?;
                    let count = u32::from_be_bytes([count[0], count[1], count[2], count[3]]);
                    if count == 0 {
                        return Err(
                            "video track stsz reports 0 samples (fragmented mp4?)".to_string()
                        );
                    }
                    return Ok(u64::from(count));
                }
            }
        }
        off = box_end;
    }
    Err("no video (vide) track found".to_string())
}

/// Header of the box at `off`: (type, payload_start, box_end). Handles 64-bit
/// `largesize` (size == 1) and to-end (size == 0) forms.
fn read_box_header(
    bytes: &[u8],
    off: usize,
    end: usize,
) -> Result<([u8; 4], usize, usize), String> {
    let head = bytes
        .get(off..off + 8)
        .ok_or_else(|| "truncated box header".to_string())?;
    let size32 = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
    let typ = [head[4], head[5], head[6], head[7]];
    let (size, header_len) = match size32 {
        0 => ((end - off) as u64, 8usize),
        1 => {
            let large = bytes
                .get(off + 8..off + 16)
                .ok_or_else(|| "truncated largesize".to_string())?;
            let mut buf = [0u8; 8];
            buf.copy_from_slice(large);
            (u64::from_be_bytes(buf), 16usize)
        }
        n => (n, 8usize),
    };
    if size < header_len as u64 {
        return Err(format!("box size {size} smaller than its header"));
    }
    let box_end = (off as u64)
        .checked_add(size)
        .filter(|&e| e <= end as u64)
        .ok_or_else(|| "box overruns file".to_string())? as usize;
    Ok((typ, off + header_len, box_end))
}

/// First box of `typ` directly inside [start, end); (payload_start, box_end).
fn find_box(
    bytes: &[u8],
    start: usize,
    end: usize,
    typ: &[u8; 4],
) -> Result<Option<(usize, usize)>, String> {
    let mut off = start;
    while off + 8 <= end {
        let (t, payload_start, box_end) = read_box_header(bytes, off, end)?;
        if t == *typ {
            return Ok(Some((payload_start, box_end)));
        }
        off = box_end;
    }
    Ok(None)
}

/// Descend a container path (each element directly inside the previous).
fn find_box_path(
    bytes: &[u8],
    start: usize,
    end: usize,
    path: &[&[u8; 4]],
) -> Result<Option<(usize, usize)>, String> {
    let (mut s, mut e) = (start, end);
    for typ in path {
        match find_box(bytes, s, e, typ)? {
            Some((ps, be)) => {
                s = ps;
                e = be;
            }
            None => return Ok(None),
        }
    }
    Ok(Some((s, e)))
}
