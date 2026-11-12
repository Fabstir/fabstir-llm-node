# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""Detector test harness. Fixtures (tests/fixtures) come from the node's `export_detector_fixtures` test: real
outputs signed by a staging chain + local TSA, their registered fingerprint sets, an EXR frame with its signed
sequence manifest. The pinned ffmpeg is REQUIRED (PROVENANCE_FFMPEG): tests fail, never skip, without it."""
import datetime
import json
import os
import struct
import subprocess

import pytest

FIX = os.path.join(os.path.dirname(__file__), "fixtures")
CRATE_FIX = os.path.join(os.path.dirname(__file__), "..", "..", "..", "crates", "fabstir-provenance", "tests")
FFMPEG = os.environ.get("PROVENANCE_FFMPEG")
SIGNER_ORG = "Fabstir Services Limited (STAGING)"
SIGNER_CN = "Platformless AI Staging Signer"


def fixture(name):
    with open(os.path.join(FIX, name), "rb") as f:
        return f.read()


def registry():
    return json.load(open(os.path.join(FIX, "registry.json")))


# ------------------------------------------------------------------ a test double of the signer's index API
def unpack(b):
    assert b[:6] == b"PAIFP\x01"
    unit, n = struct.unpack_from(">II", b, 6)
    frames, o = [], 14
    for _ in range(n):
        t, c = struct.unpack_from(">QB", b, o)
        trits = []
        for byte in b[o + 9 : o + 85]:
            for _ in range(5):
                trits.append(byte % 3)
                byte //= 3
        frames.append((t / unit, trits))
        o += 85
    return frames


def is_match(q, r, theta=180, m=16, theta1=60):
    """The AP0.1b rule (the signer runs the Rust port; this is only the test double)."""
    dist = lambda a, b: sum(abs(x - y) for x, y in zip(a, b))
    if len(q) == 1:
        return min(dist(q[0][1], f[1]) for f in r) < theta1
    ok = []
    for tq, sq in q:
        j, d = min(((j, dist(sq, f[1])) for j, f in enumerate(r)), key=lambda x: x[1])
        if d < theta:
            ok.append((tq, r[j][0]))
    best = 0
    for s in (0.8, 1.0, 1.25):
        bins = {}
        for a, b in ok:
            k = round((b - s * a) / 0.08)
            bins[k] = bins.get(k, 0) + 1
        for k in bins:
            best = max(best, sum(bins.get(k + d, 0) for d in (-1, 0, 1)))
    return best >= m


class FakeSigner:
    """The signer's index and public listener; refuses (like service.rs) a query of more than 1,500 frames."""

    def __init__(self, names=("t2v", "i2v", "edit", "sdr2hdr")):
        self.entries = [e for e in registry() if e["kind"] == "mp4" and e["name"] in names and os.path.exists(os.path.join(FIX, e["name"] + ".pfp"))]
        self.sets = {e["uuid"]: unpack(fixture(e["name"] + ".pfp")) for e in self.entries}
        self.seq = next(e["uuid"] for e in registry() if e["kind"] == "sequence")
        self.calls = 0

    def index(self, packed):
        from provdetect import detect

        self.calls += 1
        q = unpack(packed)
        if len(q) > 1500:
            raise detect.Unavailable("index answered HTTP 413")
        return [
            {"manifest": "urn:c2pa:" + e["uuid"], "class": e["class"], "has_visual_input": e["has_visual_input"], "time": e["time"], "public": True}
            for e in self.entries
            if is_match(q, self.sets[e["uuid"]])
        ]

    def public(self, path):
        if path == f"/v1/manifests/{self.seq}":
            return fixture("sdr2hdr-sequence.c2pa")
        if path == f"/v1/manifests/{self.seq}/frames-index":
            return fixture("sdr2hdr-frames-index.json")
        return None


class Deps:
    def __init__(self, ff, signer):
        self.ffmpeg, self.signer = ff, signer

    def index(self, packed):
        return self.signer.index(packed)

    def public(self, path):
        return self.signer.public(path)


@pytest.fixture(scope="session")
def ffmpeg():
    from provdetect.fingerprint import Ffmpeg

    assert FFMPEG and os.path.exists(FFMPEG), "PROVENANCE_FFMPEG must point at the pinned ffmpeg 7.0.2 (tests never skip)"
    return Ffmpeg(FFMPEG)


@pytest.fixture(scope="session", autouse=True)
def trust():
    from provdetect import credentials

    credentials.configure(fixture("anchors.pem").decode(), SIGNER_ORG, SIGNER_CN)


def run_detect(data, ffmpeg, tmp_path, signer=None, deps=None, deadline=None):
    """One detection exactly as the API runs it: the upload is a file in its own work directory."""
    from provdetect import detect

    work = tmp_path / f"work-{os.urandom(4).hex()}"
    work.mkdir()
    path = work / "upload"
    path.write_bytes(data)
    return detect.detect(str(path), deps or Deps(ffmpeg, signer or FakeSigner()), str(work), deadline=deadline)


def forge_mp4(src_bytes, tmp_path, org=SIGNER_ORG, cn=SIGNER_CN, generator="Platformless AI"):
    """A manifest that claims everything ours claims (generator name, organisation, common name, AI source type)
    but is signed by a chain the attacker made: what anyone can produce with public tools."""
    import c2pa
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

    now = datetime.datetime.now(datetime.timezone.utc)
    name = lambda c: x509.Name([x509.NameAttribute(NameOID.COUNTRY_NAME, "GB"), x509.NameAttribute(NameOID.ORGANIZATION_NAME, org), x509.NameAttribute(NameOID.COMMON_NAME, c)])
    ca_key, leaf_key = ec.generate_private_key(ec.SECP256R1()), ec.generate_private_key(ec.SECP256R1())
    ca = (
        x509.CertificateBuilder().subject_name(name("Forged Root")).issuer_name(name("Forged Root")).public_key(ca_key.public_key())
        .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(days=1)).not_valid_after(now + datetime.timedelta(days=30))
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .add_extension(x509.KeyUsage(False, False, False, False, False, True, True, False, False), critical=True)
        .sign(ca_key, hashes.SHA256())
    )
    leaf = (
        x509.CertificateBuilder().subject_name(name(cn)).issuer_name(ca.subject).public_key(leaf_key.public_key())
        .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(days=1)).not_valid_after(now + datetime.timedelta(days=30))
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(x509.KeyUsage(True, False, False, False, False, False, False, False, False), critical=True)
        .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.EMAIL_PROTECTION]), critical=False)
        .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(ca_key.public_key()), critical=False)
        .add_extension(x509.SubjectKeyIdentifier.from_public_key(leaf_key.public_key()), critical=False)
        .sign(ca_key, hashes.SHA256())
    )
    chain = leaf.public_bytes(serialization.Encoding.PEM) + ca.public_bytes(serialization.Encoding.PEM)
    key = leaf_key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
    signer = c2pa.Signer.from_info(c2pa.C2paSignerInfo(b"es256", chain, key, None))
    manifest = {
        "claim_generator_info": [{"name": generator, "version": "8.64.0"}],
        "assertions": [{"label": "c2pa.actions.v2", "data": {"actions": [{"action": "c2pa.created", "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"}]}}],
    }
    src, dst = tmp_path / "forge-src.mp4", tmp_path / "forge-out.mp4"
    src.write_bytes(src_bytes)
    with c2pa.Builder(manifest) as b, open(src, "rb") as fi, open(dst, "w+b") as fo:
        b.sign(signer, "video/mp4", fi, fo)
    return dst.read_bytes()


class SetIndex:
    """An index of arbitrary registered sets (no signing needed to test the query variants and the windows);
    records every query's frame count and refuses more than 1,500 like the signer."""

    def __init__(self, packed_sets):
        self.sets = {f"urn:c2pa:{i:08d}-0000-4000-8000-000000000000": unpack(p) for i, p in enumerate(packed_sets)}
        self.queries = []

    def index(self, packed):
        from provdetect import detect

        q = unpack(packed)
        self.queries.append(len(q))
        if len(q) > 1500:
            raise detect.Unavailable("index answered HTTP 413")
        return [{"manifest": k, "class": "generated", "has_visual_input": False, "time": 1} for k, s in self.sets.items() if is_match(q, s)]

    def public(self, path):
        return None


def transform(src_bytes, tmp_path, *args, ext="mp4"):
    """Run the pinned ffmpeg on a fixture (the attacker's tools: strip, mirror, pad, re-encode)."""
    src = tmp_path / "src.mp4"
    src.write_bytes(src_bytes)
    out = tmp_path / f"out.{ext}"
    subprocess.run([FFMPEG, "-v", "error", "-y", "-i", str(src), *args, str(out)], check=True)
    return out.read_bytes()
