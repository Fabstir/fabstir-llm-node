# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""MPEG-7 video-signature fingerprints, byte-compatible with `crates/fabstir-provenance/src/fingerprint.rs`
(`PAIFP` v1). Same pinned ffmpeg and filter as the node; the detector adds query VARIANTS (mirrored,
black-border trimmed, centre crops) because MPEG-7 signatures are not mirror- or pad-invariant (AP0.1b)."""
import hashlib
import os
import re
import struct
import subprocess
import time

FILTER = "scale='min(1280,iw)':-2,signature=format=xml:filename={out}"
MAGIC = b"PAIFP"
VERSION = 1
TRITS = 380
MAX_FRAMES = 72_000
MAX_MEDIA_SECS = 86_400
HEADER = 14
FRAME_BYTES = 85
# the signer refuses an index query of more than MAX_QUERY_FRAMES frames (service.rs); longer queries go in
# overlapping windows so a registered clip that straddles a window edge is still seen whole by one window
MAX_QUERY_FRAMES = 1_500
WINDOW_OVERLAP = 300
# what a detection decodes: at most this much media, demuxers on this list only, local file protocol only
MAX_QUERY_SECS = 300
FORMATS = "mov,matroska,png_pipe,jpeg_pipe,webp_pipe,gif,mpegts,avi"
MAX_PIXELS = 7680 * 4320
# ffmpeg's signature filter dies (SIGFPE) on pictures under ~32 px a side; a crop that small matches nothing
MIN_CROP_PX = 32
SELF_TEST_MP4 = os.path.join(os.path.dirname(__file__), "selftest.mp4")
# = crates/fabstir-provenance/tests/golden/fingerprint-tiny.json "sha256"
SELF_TEST_SHA256 = "dffb0575bb936cc81d50650a4a89ea0a7821b5301dc37e59cf848f0931befa0c"

_FRAME = re.compile(
    r"<VideoFrame>\s*<MediaTimeOfFrame>(\d+)</MediaTimeOfFrame>\s*<FrameConfidence>(\d+)</FrameConfidence>"
    r"\s*<Word>[^<]*</Word>\s*<FrameSignature>([^<]*)</FrameSignature>\s*</VideoFrame>"
)
_UNIT = re.compile(r"<MediaTimeUnit>(\d+)</MediaTimeUnit>")


class FingerprintError(Exception):
    pass


def windows(packed, size=MAX_QUERY_FRAMES, overlap=WINDOW_OVERLAP):
    """Split a packed set into packed windows of at most `size` frames, consecutive windows overlapping."""
    if packed[:6] != MAGIC + bytes([VERSION]) or len(packed) < HEADER:
        raise FingerprintError("not a PAIFP v1 set")
    unit, n = struct.unpack_from(">II", packed, 6)
    if len(packed) != HEADER + n * FRAME_BYTES:
        raise FingerprintError("PAIFP length mismatch")
    if n <= size:
        return [packed]
    out, step = [], size - overlap
    for start in range(0, n, step):
        k = min(size, n - start)
        body = packed[HEADER + start * FRAME_BYTES : HEADER + (start + k) * FRAME_BYTES]
        out.append(MAGIC + bytes([VERSION]) + struct.pack(">II", unit, k) + body)
        if start + k >= n:
            break
    return out


def parse_xml(text):
    """(unit, [(time, confidence, [380 trits])]) from ffmpeg's signature XML; strict like the Rust parser."""
    m = _UNIT.search(text)
    if not m or int(m.group(1)) == 0:
        raise FingerprintError("no MediaTimeUnit")
    unit = int(m.group(1))
    frames = []
    prev = -1
    for t, c, sig in _FRAME.findall(text):
        t, c = int(t), int(c)
        vals = sig.split()
        if len(vals) != TRITS or any(v not in ("0", "1", "2") for v in vals):
            raise FingerprintError("frame signature is not 380 values in {0,1,2}")
        if t < prev or t // unit > MAX_MEDIA_SECS or c > 255:
            raise FingerprintError("bad frame time or confidence")
        prev = t
        frames.append((t, c, [int(v) for v in vals]))
        if len(frames) > MAX_FRAMES:
            raise FingerprintError("too many frames")
    if not frames:
        raise FingerprintError("empty signature")
    return unit, frames


def pack(unit, frames):
    out = bytearray(MAGIC + bytes([VERSION]) + struct.pack(">II", unit, len(frames)))
    for t, c, trits in frames:
        out += struct.pack(">QB", t, c)
        for i in range(0, TRITS, 5):
            b, w = 0, 1
            for v in trits[i : i + 5]:
                b += v * w
                w *= 3
            out.append(b)
    return bytes(out)


def digest(packed):
    return hashlib.sha256(packed).digest()


class Ffmpeg:
    """The pinned ffmpeg, optionally behind a sandbox prefix (nsjail in production). `{work}` in the prefix is
    replaced by the request's own directory, so the sandbox binds that directory and nothing else of the spool."""

    def __init__(self, path, sandbox=(), timeout=300, threads=2):
        self.path = path
        self.sandbox = list(sandbox)
        self.timeout = timeout
        self.threads = threads

    def argv(self, args, work_dir):
        prefix = [a.replace("{work}", work_dir) for a in self.sandbox]
        return prefix + [self.path, "-nostdin", "-hide_banner", "-threads", str(self.threads)] + args

    def input_args(self, media_path):
        """Input options of every decode: local file protocol only, listed demuxers only, a pixel cap, and no
        more than MAX_QUERY_SECS (+1, so a longer clip is recognised as too long)."""
        return [
            "-protocol_whitelist", "file", "-format_whitelist", FORMATS, "-max_pixels", str(MAX_PIXELS),
            "-t", str(MAX_QUERY_SECS + 1), "-i", media_path,
        ]

    def _run(self, args, cwd, deadline=None):
        timeout = self.timeout
        if deadline is not None:
            timeout = min(timeout, deadline - time.monotonic())
            if timeout <= 0:
                raise FingerprintError("decoding took too long")
        try:
            return subprocess.run(
                self.argv(args, cwd),
                cwd=cwd,
                env={},
                stdin=subprocess.DEVNULL,
                capture_output=True,
                timeout=timeout,
                check=False,
            )
        except subprocess.TimeoutExpired as e:
            raise FingerprintError("decoding took too long") from e
        except OSError as e:
            raise FingerprintError("the decoder could not start") from e

    def signature(self, media_path, work_dir, prefilter="", deadline=None):
        xml = os.path.join(work_dir, "sig.xml")
        if os.path.exists(xml):
            os.remove(xml)
        vf = (prefilter + "," if prefilter else "") + FILTER.format(out=xml)
        r = self._run(["-v", "error", *self.input_args(media_path), "-vf", vf, "-f", "null", "-"], work_dir, deadline)
        if r.returncode != 0 or not os.path.exists(xml):
            raise FingerprintError("ffmpeg could not compute a signature")
        with open(xml, encoding="ascii", errors="replace") as f:
            text = f.read()
        os.remove(xml)
        unit, frames = parse_xml(text)
        if frames[-1][0] / unit > MAX_QUERY_SECS:
            raise FingerprintError(f"longer than {MAX_QUERY_SECS // 60} minutes; send a shorter clip")
        return pack(unit, frames)

    def scan(self, media_path, work_dir, deadline=None):
        """((width, height), crop) where crop is `w:h:x:y` of the picture inside black borders (pillarbox,
        letterbox, a 4:3 pad) or None. skip=0 so a single still is scanned too."""
        r = self._run(["-v", "info", *self.input_args(media_path), "-vf", "cropdetect=limit=24:round=2:reset=0:skip=0", "-f", "null", "-"], work_dir, deadline)
        if r.returncode != 0:
            raise FingerprintError("ffmpeg could not scan the borders")
        err = r.stderr.decode("ascii", "replace")
        dims = re.search(r"Stream #0:\d+\S*: Video: .*?, (\d{1,5})x(\d{1,5})", err)
        if not dims:
            raise FingerprintError("no video stream")
        size = (int(dims.group(1)), int(dims.group(2)))
        found = re.findall(r"crop=(\d+):(\d+):(\d+):(\d+)", err)
        return size, (found[-1] if found else None)

    def variants(self, media_path, work_dir, deadline=None):
        """[(name, packed)] for the query: as is, mirrored, border-trimmed, centre crops (screen recordings).
        Every variant must compute: a "no mark found" is only ever signed after the WHOLE search ran, so any
        failure (a sandbox refusal, a timeout, an undecodable file) raises instead of shrinking the search.
        A crop variant whose picture would be under MIN_CROP_PX a side is not run (it can match nothing, and the
        signature filter cannot compute it); a border crop equal to the whole frame is the as-is query again."""
        dl = deadline  # per call: one Ffmpeg serves concurrent detections
        out = [(name, self.signature(media_path, work_dir, pre, dl)) for name, pre in [("as_is", ""), ("mirrored", "hflip")]]
        (w, h), crop = self.scan(media_path, work_dir, dl)
        if crop:
            cw, ch = int(crop[0]), int(crop[1])
            if min(cw, ch) >= MIN_CROP_PX and (cw, ch) != (w, h):
                c = ":".join(crop)
                for name, pre in [("border_trimmed", f"crop={c}"), ("border_trimmed_mirrored", f"crop={c},hflip")]:
                    out.append((name, self.signature(media_path, work_dir, pre, dl)))
        for frac in (0.8, 0.6):
            if min(w, h) * frac >= MIN_CROP_PX:
                out.append((f"centre_{frac}", self.signature(media_path, work_dir, f"crop=iw*{frac}:ih*{frac}", dl)))
        return out

    def self_test(self, work_dir):
        """Startup check (fail closed): through the sandbox, inside a request-like directory, the pinned ffmpeg
        reproduces the crate's golden signature of `selftest.mp4` (the crate's tiny.mp4)."""
        path = os.path.join(work_dir, "selftest.mp4")
        with open(SELF_TEST_MP4, "rb") as src, open(path, "wb") as dst:
            dst.write(src.read())
        got = digest(self.signature(path, work_dir)).hex()
        if got != SELF_TEST_SHA256:
            raise FingerprintError(f"ffmpeg self-test digest {got} != {SELF_TEST_SHA256}")
