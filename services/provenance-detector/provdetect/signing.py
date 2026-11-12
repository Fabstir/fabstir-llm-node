# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""Signed results (Code Measure 2.1): Ed25519 over the canonical JSON of the payload.

Canonicalisation (published on the "How to detect" page): JSON with keys sorted, separators "," and ":", no
whitespace, UTF-8, non-ASCII characters unescaped. The payload carries `kind` (domain separation: this key signs
nothing but detector results) and `key_id` (the first 16 hex digits of the SHA-256 of the raw public key) so a
rotated key is identifiable. A signed result says what the detector found at `time` for the file whose SHA-256
is `content_sha256`; it says nothing about any other file. The detector key is worthless for C2PA (T5)."""
import base64
import hashlib
import json

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey

KIND = "platformlessai-detector-result/1"


def canonical(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


class ResultSigner:
    def __init__(self, key: Ed25519PrivateKey):
        self.key = key
        raw = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        self.public_b64 = base64.b64encode(raw).decode()
        self.key_id = hashlib.sha256(raw).hexdigest()[:16]

    @classmethod
    def from_pem_file(cls, path):
        with open(path, "rb") as f:
            k = serialization.load_pem_private_key(f.read(), password=None)
        if not isinstance(k, Ed25519PrivateKey):
            raise ValueError("result key must be Ed25519")
        return cls(k)

    def sign(self, payload):
        payload = {**payload, "kind": KIND, "key_id": self.key_id}
        return {"payload": payload, "signature": base64.b64encode(self.key.sign(canonical(payload))).decode(), "public_key": self.public_b64, "alg": "Ed25519"}


def verify(signed):
    if signed["payload"].get("kind") != KIND:
        raise ValueError("not a detector result")
    raw = base64.b64decode(signed["public_key"])
    if signed["payload"].get("key_id") != hashlib.sha256(raw).hexdigest()[:16]:
        raise ValueError("key id does not match the public key")
    Ed25519PublicKey.from_public_bytes(raw).verify(base64.b64decode(signed["signature"]), canonical(signed["payload"]))
    return True
