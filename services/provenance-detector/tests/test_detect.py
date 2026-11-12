# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""AP4 detector tests (IMPLEMENTATION-AI-PROVENANCE.md AP4), each named by the defect it catches."""
import json
import os
import subprocess
import threading

import pytest

from conftest import CRATE_FIX, FIX, SIGNER_ORG, FakeSigner, Deps, SetIndex, fixture, forge_mp4, run_detect, transform
from provdetect import credentials, detect, exr, fingerprint

run = run_detect


def lavfi(ffmpeg, tmp_path, src, secs, name="lavfi.mp4", extra=()):
    out = tmp_path / name
    subprocess.run([ffmpeg.path, "-v", "error", "-y", "-f", "lavfi", "-i", src, "-t", str(secs), *extra, "-c:v", "libx264", "-pix_fmt", "yuv420p", str(out)], check=True)
    return out.read_bytes()


def sig_of(ffmpeg, tmp_path, data, name="reg.mp4"):
    p = tmp_path / name
    p.write_bytes(data)
    return ffmpeg.signature(str(p), str(tmp_path))


# ------------------------------------------------------------------------------------------ format + trust
def test_packing_is_byte_compatible_with_the_rust_crate():
    xml = open(os.path.join(CRATE_FIX, "fixtures", "tiny-signature.xml")).read()
    golden = json.load(open(os.path.join(CRATE_FIX, "golden", "fingerprint-tiny.json")))
    packed = fingerprint.pack(*fingerprint.parse_xml(xml))
    assert len(packed) == golden["packed_len"]
    assert fingerprint.digest(packed).hex() == golden["sha256"] == fingerprint.SELF_TEST_SHA256
    assert fingerprint.FILTER == golden["filter"]


def test_self_test_reproduces_the_golden_signature(ffmpeg, tmp_path, monkeypatch):
    ffmpeg.self_test(str(tmp_path))
    with monkeypatch.context() as m:
        m.setattr(fingerprint, "SELF_TEST_SHA256", "00" * 32)  # an ffmpeg whose signature differs from the pin
        with pytest.raises(fingerprint.FingerprintError):
            ffmpeg.self_test(str(tmp_path))
    broken = fingerprint.Ffmpeg(ffmpeg.path, sandbox=["/nonexistent-sandbox", "--"])
    with pytest.raises(fingerprint.FingerprintError):
        broken.self_test(str(tmp_path))


def test_trust_holds_on_worker_threads(tmp_path):
    # c2pa.load_settings is thread-local: a read on a fresh thread must still use the Fabstir root
    p = tmp_path / "t2v.mp4"
    p.write_bytes(fixture("t2v.mp4"))
    out = []
    t = threading.Thread(target=lambda: out.append(credentials.read_embedded(str(p), "video/mp4")))
    t.start()
    t.join()
    assert credentials.trusted(out[0]), out[0]


def test_anchors_refuse_a_tsa_or_foreign_root():
    with pytest.raises(credentials.TrustError):
        credentials.configure(fixture("anchors.pem").decode() + fixture("tsa-root.pem").decode(), SIGNER_ORG)
    with pytest.raises(credentials.TrustError):
        credentials.configure(fixture("anchors.pem").decode(), "Someone Else Ltd")


def summary_json(issuer, cn, successes=("signingCredential.trusted", "claimSignature.validated"), failures=()):
    return {
        "active_manifest": "urn:c2pa:m",
        "validation_state": "Trusted",
        "manifests": {"urn:c2pa:m": {"claim_generator_info": [{"name": "Platformless AI"}], "signature_info": {"issuer": issuer, "common_name": cn}, "assertions": []}},
        "validation_results": {"activeManifest": {"success": [{"code": c} for c in successes], "failure": [{"code": c} for c in failures]}},
    }


def test_ours_needs_the_pinned_leaf_and_a_trusted_chain():
    from conftest import SIGNER_CN

    assert credentials._summary(summary_json(SIGNER_ORG, SIGNER_CN))["ours"]
    assert not credentials._summary(summary_json("Other Org", SIGNER_CN))["ours"], "organisation pinned"
    assert not credentials._summary(summary_json(SIGNER_ORG, "Another Leaf"))["ours"], "common name pinned"
    assert not credentials._summary(summary_json(SIGNER_ORG, SIGNER_CN, successes=("claimSignature.validated",)))["ours"], "chain must be trusted"
    assert not credentials._summary(summary_json(SIGNER_ORG, SIGNER_CN, failures=("claimSignature.mismatch",)))["ours"]
    assert credentials._summary(summary_json(SIGNER_ORG, SIGNER_CN, failures=("assertion.bmffHash.mismatch",)))["ours"], "ours, content altered"


def test_a_verified_manifest_without_a_class_is_never_called_modified(ffmpeg, tmp_path, monkeypatch):
    real = credentials.read_embedded

    def no_class(path, mime):
        s = real(path, mime)
        return {**s, "class": None}

    monkeypatch.setattr(credentials, "read_embedded", no_class)
    r = run(fixture("t2v.mp4"), ffmpeg, tmp_path, FakeSigner(names=()))
    assert not r["found"] and r["statement"] == detect.STATEMENT_UNCLASSED and r["basis"] == ["content_credentials_unclassed"]


# ------------------------------------------------------------------------------------------ embedded manifests
def test_valid_manifest_gives_definitive_wording(ffmpeg, tmp_path):
    r = run(fixture("t2v.mp4"), ffmpeg, tmp_path)
    assert r["found"] and r["basis"] == ["content_credentials"]
    assert r["statement"] == detect.STATEMENT_GENERATED
    assert r["provenance"]["class"] == "generated" and r["provenance"]["provider"] == "Fabstir Services Limited"
    m = run(fixture("edit.mp4"), ffmpeg, tmp_path)
    assert m["statement"] == detect.STATEMENT_MODIFIED


def test_data_class_manifest_reads_as_image_data(ffmpeg, tmp_path):
    r = run(fixture("passes.mp4"), ffmpeg, tmp_path)
    assert r["found"] and r["basis"] == ["content_credentials"]
    assert r["statement"] == detect.STATEMENT_DATA and r["provenance"]["class"] == "data"


def test_damaged_manifest_is_not_given_the_definitive_wording(ffmpeg, tmp_path):
    b = bytearray(fixture("t2v.mp4"))
    i = b.find(b"mdat") + 200
    b[i] ^= 0xFF  # picture bytes no longer match the hard binding
    r = run(bytes(b), ffmpeg, tmp_path, FakeSigner(names=()))
    assert r["statement"] == detect.STATEMENT_UNVERIFIED
    assert r["basis"] == ["content_credentials_invalid"] and not r["found"]


def test_forged_issuer_is_another_issuer(ffmpeg, tmp_path):
    # every name ours carries, signed by a chain the attacker made: never "Platformless AI credentials"
    forged = forge_mp4(fixture("t2v.mp4"), tmp_path)
    r = run(forged, ffmpeg, tmp_path, FakeSigner(names=()))
    assert r["basis"] == ["content_credentials_other_issuer"] and not r["found"]
    assert r["statement"] == detect.STATEMENT_OTHER_ISSUER


# ------------------------------------------------------------------------------------------ fingerprints
def test_detects_by_fingerprint_after_strip(ffmpeg, tmp_path):
    stripped = transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", "-c", "copy")
    assert b"c2pa" not in stripped, "ffmpeg drops the manifest box (G24)"
    r = run(stripped, ffmpeg, tmp_path, FakeSigner(names=("t2v",)))
    assert r["found"] and r["basis"] == ["fingerprint"]


def test_a_fingerprint_match_is_never_a_definitive_claim(ffmpeg, tmp_path):
    stripped = transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", "-c", "copy")
    for names in [("t2v",), ("edit",), ("i2v",), ("t2v", "i2v")]:
        r = run(stripped, ffmpeg, tmp_path, FakeSigner(names=names))
        assert r["statement"] == detect.STATEMENT_FP_POSSIBLE, names
        assert r["provenance"]["class"] is None
        s = r["statement"].lower()
        assert "ai-generated" not in s and "ai-modified" not in s and "generated by ai" not in s


def test_reports_every_matching_manifest(ffmpeg, tmp_path):
    stripped = transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", "-c", "copy")
    r = run(stripped, ffmpeg, tmp_path)
    reg = {"urn:c2pa:" + e["uuid"] for e in json.load(open(os.path.join(FIX, "registry.json"))) if e["kind"] == "mp4" and os.path.exists(os.path.join(FIX, e["name"] + ".pfp"))}
    assert len(reg) == 4 and set(r["provenance"]["manifests"]) == reg, "all four sibling outputs (same source picture)"


def test_finds_mirrored_and_padded_copies(ffmpeg, tmp_path):
    for args in (["-vf", "hflip", "-c:v", "libx264", "-crf", "23"], ["-vf", "pad=ih*4/3*1.5:ih:(ow-iw)/2:0", "-c:v", "libx264", "-crf", "23"]):
        copy = transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", *args)
        r = run(copy, ffmpeg, tmp_path, FakeSigner(names=("t2v",)))
        assert r["found"], args


def test_no_mark_only_after_the_whole_search(ffmpeg, tmp_path):
    # an unrelated clip against a registered one: no match, and "no mark" is backed by every variant's query
    reg = SetIndex([sig_of(ffmpeg, tmp_path, lavfi(ffmpeg, tmp_path, "mandelbrot=size=320x180:rate=24", 3, "a.mp4"))])
    for src in ("cellauto=size=320x180:rate=24", "life=size=320x180:rate=24:mold=10", "mandelbrot=size=320x180:rate=24:start_scale=1.5"):
        reg.queries.clear()
        r = run(lavfi(ffmpeg, tmp_path, src, 3, "neg.mp4"), ffmpeg, tmp_path, deps=Deps(ffmpeg, reg))
        assert not r["found"] and r["statement"] == detect.STATEMENT_NONE, src
        assert len(reg.queries) >= 4, "as_is, mirrored and both centre crops were all queried"


def test_a_failing_variant_fails_the_detection(ffmpeg, tmp_path):
    class Flaky(fingerprint.Ffmpeg):
        def signature(self, media_path, work_dir, prefilter="", deadline=None):
            if prefilter == "hflip":
                raise fingerprint.FingerprintError("sandbox refused")
            return super().signature(media_path, work_dir, prefilter, deadline)

    with pytest.raises(detect.Unanalysable):
        run(lavfi(ffmpeg, tmp_path, "cellauto=size=320x180:rate=24", 2), ffmpeg, tmp_path, deps=Deps(Flaky(ffmpeg.path), FakeSigner()))


def test_long_clips_are_queried_in_signer_sized_windows(ffmpeg, tmp_path):
    # 1,650 frames; the registered clip sits after frame 1,500 (the signer refuses longer queries)
    long_clip = lavfi(ffmpeg, tmp_path, "mandelbrot=size=192x108:rate=30", 55, "long.mp4")
    tail = transform(long_clip, tmp_path, "-ss", "51", "-t", "3", "-c:v", "libx264")
    reg = SetIndex([sig_of(ffmpeg, tmp_path, tail, "tail.mp4")])
    r = run(long_clip, ffmpeg, tmp_path, deps=Deps(ffmpeg, reg))
    assert r["found"]
    assert max(reg.queries) <= fingerprint.MAX_QUERY_FRAMES and len(reg.queries) > 6


def test_windows_overlap_and_cover_every_frame():
    frames = [(i, 0, [i % 3] * 380) for i in range(3200)]
    ws = fingerprint.windows(fingerprint.pack(1, frames))
    import struct

    spans, first = [], 0
    for w in ws:
        n = struct.unpack_from(">II", w, 6)[1]
        assert n <= 1500
        t0 = struct.unpack_from(">Q", w, 14)[0]
        spans.append((t0, t0 + n))
    assert spans[0][0] == 0 and spans[-1][1] == 3200
    for (a0, a1), (b0, b1) in zip(spans, spans[1:]):
        assert b0 == a1 - fingerprint.WINDOW_OVERLAP


def test_clips_over_the_duration_cap_are_unanalysable(ffmpeg, tmp_path):
    clip = lavfi(ffmpeg, tmp_path, "testsrc2=size=192x108:rate=2", fingerprint.MAX_QUERY_SECS + 5, "over.mp4")
    with pytest.raises(detect.Unanalysable, match="longer than"):
        run(clip, ffmpeg, tmp_path)
    ok = lavfi(ffmpeg, tmp_path, "testsrc2=size=192x108:rate=2", fingerprint.MAX_QUERY_SECS - 5, "under.mp4")
    assert not run(ok, ffmpeg, tmp_path, FakeSigner(names=()))["found"], "the control: a clip under the cap is searched"


def test_tiny_crops_are_skipped_not_fatal(ffmpeg, tmp_path):
    # a 300x20 strip on black (cropdetect finds 300:20) and a 48 px still (a 0.6 crop is 28 px): the signature
    # filter cannot compute pictures that small, so those variants are skipped and the rest of the search runs
    strip = lavfi(ffmpeg, tmp_path, "color=black:size=640x360:rate=24,drawbox=x=170:y=170:w=300:h=20:color=white:t=fill", 2, "strip.mp4")
    reg = SetIndex([])
    assert not run(strip, ffmpeg, tmp_path, deps=Deps(ffmpeg, reg))["found"]
    assert len(reg.queries) == 4, "as_is, mirrored, centre 0.8 and 0.6; no border variant"
    still = tmp_path / "small.png"
    subprocess.run([ffmpeg.path, "-v", "error", "-y", "-f", "lavfi", "-i", "mandelbrot=size=48x48", "-frames:v", "1", str(still)], check=True)
    reg.queries.clear()
    assert not run(still.read_bytes(), ffmpeg, tmp_path, deps=Deps(ffmpeg, reg))["found"]
    assert len(reg.queries) == 3, "as_is, mirrored, centre 0.8 (38 px); not 0.6 (28 px)"


def test_a_spent_deadline_is_unavailable_not_no_mark(ffmpeg, tmp_path):
    import time

    clip = lavfi(ffmpeg, tmp_path, "cellauto=size=320x180:rate=24", 2)
    with pytest.raises(detect.Unavailable):
        run(clip, ffmpeg, tmp_path, deadline=time.monotonic() - 1)
    assert not run(clip, ffmpeg, tmp_path, FakeSigner(names=()), deadline=time.monotonic() + 120)["found"]


def test_the_deadline_stops_a_running_decode(ffmpeg, tmp_path):
    import time

    clip = lavfi(ffmpeg, tmp_path, "testsrc2=size=1280x720:rate=30", 40, "slow.mp4")
    t0 = time.monotonic()
    run(clip, ffmpeg, tmp_path, FakeSigner(names=()))
    full = time.monotonic() - t0
    assert full > 2, "precondition: the whole search takes a while"
    t0 = time.monotonic()
    with pytest.raises(detect.Unavailable):
        run(clip, ffmpeg, tmp_path, FakeSigner(names=()), deadline=time.monotonic() + 0.3)
    assert time.monotonic() - t0 < full / 3, "the decoder was cut at the deadline, not after the search"


def test_garbage_is_unanalysable_not_no_mark(ffmpeg, tmp_path):
    for data in (os.urandom(4096), b"\x00\x00\x00\x18ftypisom" + os.urandom(2000), b"\x89PNG\r\n\x1a\n" + os.urandom(300)):
        with pytest.raises(detect.Unanalysable):
            run(data, ffmpeg, tmp_path)


def test_playlists_cannot_pull_in_other_files(ffmpeg, tmp_path):
    # a concat playlist naming a second file: the demuxer whitelist refuses it before anything is decoded
    work = tmp_path / "w"
    work.mkdir()
    (work / "sub.mp4").write_bytes(transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", "-c", "copy"))
    (work / "upload").write_bytes(b"ffconcat version 1.0\nfile 'sub.mp4'\n")
    with pytest.raises(detect.Unanalysable):
        detect.detect(str(work / "upload"), Deps(ffmpeg, FakeSigner(names=("t2v",))), str(work))


def test_sandbox_prefix_binds_only_the_request_directory():
    ff = fingerprint.Ffmpeg("/opt/provenance/ffmpeg", sandbox=["nsjail", "--bindmount", "{work}", "--cwd", "{work}", "--"])
    argv = ff.argv(["-i", "/run/x/req-1/upload"], "/run/x/req-1")
    assert argv[:6] == ["nsjail", "--bindmount", "/run/x/req-1", "--cwd", "/run/x/req-1", "--"]
    i = argv.index("/opt/provenance/ffmpeg")
    assert "-protocol_whitelist" in ff.input_args("u") and ff.input_args("u")[ff.input_args("u").index("-protocol_whitelist") + 1] == "file"
    assert argv[i + 1 : i + 5] == ["-nostdin", "-hide_banner", "-threads", "2"]


def test_decoder_timeout_is_unanalysable(ffmpeg, tmp_path):
    slow = fingerprint.Ffmpeg(ffmpeg.path, timeout=0.01)
    with pytest.raises(detect.Unanalysable):
        run(lavfi(ffmpeg, tmp_path, "cellauto=size=320x180:rate=24", 2), ffmpeg, tmp_path, deps=Deps(slow, FakeSigner()))


def test_signer_outage_is_never_no_mark(ffmpeg, tmp_path):
    class Down(FakeSigner):
        def index(self, packed):
            raise detect.Unavailable("index unreachable")

    with pytest.raises(detect.Unavailable):
        run(lavfi(ffmpeg, tmp_path, "cellauto=size=320x180:rate=24", 2), ffmpeg, tmp_path, Down())


# ------------------------------------------------------------------------------------------ EXR
def seq_entry():
    return next(e for e in json.load(open(os.path.join(FIX, "registry.json"))) if e["kind"] == "sequence")


def test_reads_exr_header_record(ffmpeg, tmp_path):
    frame = fixture("sdr2hdr-frame_000001.exr")
    rec = exr.record(frame)
    assert rec["provider"] == "Fabstir Services Limited" and rec["system"] == "Platformless AI"
    r = run(frame, ffmpeg, tmp_path)
    assert r["found"] and r["basis"] == ["exr_header_record", "sequence_manifest"]
    assert r["statement"] == detect.STATEMENT_MODIFIED and r["provenance"]["class"] == "modified"
    assert r["provenance"]["manifests"] == ["urn:c2pa:" + seq_entry()["uuid"]]


def test_tampered_exr_frame_reported_mismatch(ffmpeg, tmp_path):
    b = bytearray(fixture("sdr2hdr-frame_000001.exr"))
    b[-10] ^= 0xFF  # a pixel byte: the record survives, the hash no longer is in the signed index
    r = run(bytes(b), ffmpeg, tmp_path)
    assert r["statement"] == detect.STATEMENT_EXR_MISMATCH
    assert not r["found"] and r["provenance"] is None, "nothing from the record is signed"


class Listener(FakeSigner):
    def __init__(self, manifest=None, index=None, any_uuid=False):
        super().__init__()
        self.m, self.i, self.any = manifest, index, any_uuid
        self.paths = []

    def public(self, path):
        self.paths.append(path)
        if self.any:
            return fixture("sdr2hdr-frames-index.json") if path.endswith("/frames-index") else fixture("sdr2hdr-sequence.c2pa")
        if path.endswith("/frames-index"):
            return self.i
        return self.m


def test_tampered_index_is_unverified(ffmpeg, tmp_path):
    idx = json.loads(fixture("sdr2hdr-frames-index.json"))
    frame = fixture("sdr2hdr-frame_000001.exr")
    import hashlib

    idx["frames"]["frame_000002.exr"] = hashlib.sha256(frame).hexdigest()  # an index the signature does not cover
    r = run(frame, ffmpeg, tmp_path, Listener(fixture("sdr2hdr-sequence.c2pa"), json.dumps(idx).encode()))
    assert r["statement"] == detect.STATEMENT_EXR_UNVERIFIED and not r["found"] and r["provenance"] is None


def test_unknown_sequence_is_unverified(ffmpeg, tmp_path):
    r = run(fixture("sdr2hdr-frame_000001.exr"), ffmpeg, tmp_path, Listener(None, None))
    assert r["statement"] == detect.STATEMENT_EXR_UNVERIFIED and not r["found"] and r["provenance"] is None


def test_record_pointing_at_another_sequence_is_unverified(ffmpeg, tmp_path):
    # the record names a sequence the listener answers with a DIFFERENT (genuine) sequence manifest
    frame = fixture("sdr2hdr-frame_000001.exr")
    real = seq_entry()["uuid"]
    other = "00000000-0000-4000-8000-000000000000"
    forged = frame.replace(real.encode(), other.encode())
    assert forged != frame
    r = run(forged, ffmpeg, tmp_path, Listener(any_uuid=True))
    assert r["statement"] == detect.STATEMENT_EXR_UNVERIFIED and not r["found"]


def exr_with_record(value):
    # a minimal single-part header carrying only the provenance record
    import struct

    attr = b"platformlessai:provenance\0string\0" + struct.pack("<i", len(value)) + value
    return b"\x76\x2f\x31\x01" + struct.pack("<I", 0x400) + attr + b"\0" + b"\0" * 16


@pytest.mark.parametrize(
    "value",
    [b"[" * 100_000 + b"]" * 100_000, b"[1, 2]", b'"just a string"', b"\xff\xfe not utf-8", b'{"sequenceManifest": ' + b"9" * 5000 + b"}"],
)
def test_hostile_exr_records_are_unanalysable(ffmpeg, tmp_path, value):
    with pytest.raises(detect.Unanalysable):
        run(exr_with_record(value), ffmpeg, tmp_path)


@pytest.mark.parametrize("seq", ["../../v1/index", "urn:c2pa:not-a-uuid", "urn:c2pa:00000000-0000-4000-8000-00000000000G", 7, None])
def test_record_sequence_must_be_a_uuid_urn(ffmpeg, tmp_path, seq):
    listener = Listener(any_uuid=True)
    r = run(exr_with_record(json.dumps({"sequenceManifest": seq}).encode()), ffmpeg, tmp_path, listener)
    assert r["statement"] == detect.STATEMENT_EXR_UNVERIFIED and listener.paths == []


def test_truncated_exr_headers_are_unanalysable(ffmpeg, tmp_path):
    good = exr_with_record(b'{"sequenceManifest": "x"}')
    for cut in (12, 40, 60):
        with pytest.raises(detect.Unanalysable):
            run(good[:cut], ffmpeg, tmp_path)


# ------------------------------------------------------------------------------------------ result shape
def test_result_is_coarse(ffmpeg, tmp_path):
    stripped = transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", "-c", "copy")
    text = json.dumps(run(stripped, ffmpeg, tmp_path)).lower()
    for word in ("score", "distance", "votes", "confidence", "probability", "theta"):
        assert word not in text, word


@pytest.mark.parametrize(
    "attack,needs",
    [
        (["-vf", "hflip"], "mirrored"),
        (["-vf", "pad=ih*16/9*1.4:ih:(ow-iw)/2:0:black"], "border_trimmed"),
    ],
)
def test_query_variants_recover_mirror_and_pad(ffmpeg, tmp_path, attack, needs):
    src = lavfi(ffmpeg, tmp_path, "mandelbrot=size=480x270:rate=24", 3, "asym.mp4")
    reg = SetIndex([sig_of(ffmpeg, tmp_path, src)])
    copy = transform(src, tmp_path, *attack, "-c:v", "libx264", "-crf", "23")
    q = tmp_path / "q.mp4"
    q.write_bytes(copy)
    by_name = dict(ffmpeg.variants(str(q), str(tmp_path)))
    assert not reg.index(by_name["as_is"]), "the attack defeats the plain query (else this test proves nothing)"
    assert reg.index(by_name[needs]), f"the {needs} variant recovers it"
    assert run(copy, ffmpeg, tmp_path, deps=Deps(ffmpeg, reg))["found"]
