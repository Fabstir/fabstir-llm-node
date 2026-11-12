# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""The EXR header record `platformlessai:provenance` (D9): a port of `exr_header::read_records`."""
import json
import struct

ATTRIBUTE = b"platformlessai:provenance"
MAGIC = b"\x76\x2f\x31\x01"
MULTIPART = 0x1000


class ExrError(Exception):
    pass


def is_exr(b):
    return b[:4] == MAGIC


def read_records(b):
    """The record of every part's header (None where a part has none). Any malformed header raises ExrError."""
    try:
        return _read_records(b)
    except (ValueError, IndexError, struct.error) as e:  # b.index misses, a truncated size field
        raise ExrError("malformed header") from e


def _read_records(b):
    if len(b) < 8 or not is_exr(b):
        raise ExrError("not an OpenEXR file")
    flags = struct.unpack_from("<I", b, 4)[0]
    multipart = bool(flags & MULTIPART)
    o, out = 8, []
    while True:
        if o >= len(b):
            raise ExrError("truncated header")
        if b[o] == 0:
            if multipart and out:
                break
            raise ExrError("empty header")
        found = None
        while True:
            if o >= len(b):
                raise ExrError("truncated header")
            if b[o] == 0:
                o += 1
                break
            ne = b.index(b"\0", o)
            te = b.index(b"\0", ne + 1)
            size = struct.unpack_from("<i", b, te + 1)[0]
            if size < 0 or te + 5 + size > len(b):
                raise ExrError("attribute runs past EOF")
            if b[o:ne] == ATTRIBUTE and b[ne + 1 : te] == b"string":
                found = b[te + 5 : te + 5 + size]
            o = te + 5 + size
        out.append(found)
        if not multipart:
            break
    return out


def record(b):
    """The parsed record of part 0, or None."""
    recs = read_records(b)
    if not recs or recs[0] is None:
        return None
    try:
        rec = json.loads(recs[0].decode("utf-8").strip())
    except (ValueError, RecursionError) as e:  # UnicodeDecodeError, JSONDecodeError, int-digit limit
        raise ExrError("unreadable provenance record") from e
    if not isinstance(rec, dict):
        raise ExrError("provenance record is not an object")
    return rec
