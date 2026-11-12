# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""Content Credentials (C2PA) reading with c2pa-python against the Fabstir signing root.

Trust is per call: every Reader gets its own `c2pa.Context` built from the configured Settings. The global
`c2pa.load_settings` is THREAD-LOCAL in c2pa-python 0.38 (a read on a worker thread silently runs with the
default trust list and reports a genuine output Invalid), and detections run on worker threads.

"Ours" is never a self-asserted name. A manifest is ours only when its signing certificate chains to a
configured anchor (`signingCredential.trusted`), its claim signature verifies, and the leaf's organisation (and,
when configured, common name) is the pinned Fabstir signer. The anchors are the Fabstir signing root ONLY:
`configure` refuses any anchor that is not a CA of the pinned organisation, so a TSA root (or any other
root) can never make its own leaves pass as ours."""
import io
import json

import c2pa
from cryptography import x509
from cryptography.x509.oid import NameOID

SOURCE_CLASS = {
    "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia": "generated",
    "http://cv.iptc.org/newscodes/digitalsourcetype/compositeWithTrainedAlgorithmicMedia": "modified",
    "http://c2pa.org/digitalsourcetype/trainedAlgorithmicData": "data",
}
EKUS = "1.3.6.1.5.5.7.3.36\n1.3.6.1.4.1.62558.2.1\n1.3.6.1.5.5.7.3.4\n1.3.6.1.5.5.7.3.8\n"
# failure codes that mean the signature itself is not a valid Fabstir signature (vs the content being altered)
_SIGNER_FAILURES = ("signingCredential.", "claimSignature.")


class TrustError(ValueError):
    pass


_settings_json = None
_pin = None


def _split_pem(pem):
    marker = "-----BEGIN CERTIFICATE-----"
    return [marker + c for c in pem.split(marker)[1:]]


def configure(anchors_pem, signer_org, signer_cn=None):
    """Trust the Fabstir signing root(s) in `anchors_pem`, pinning the leaf organisation (and common name)."""
    global _settings_json, _pin
    certs = _split_pem(anchors_pem)
    if not certs or not signer_org:
        raise TrustError("no signing anchor or no pinned signer organisation")
    for pem in certs:
        c = x509.load_pem_x509_certificate(pem.encode())
        try:
            ca = c.extensions.get_extension_for_class(x509.BasicConstraints).value.ca
        except x509.ExtensionNotFound:
            ca = False
        org = [a.value for a in c.subject.get_attributes_for_oid(NameOID.ORGANIZATION_NAME)]
        if not ca or org != [signer_org]:
            raise TrustError(f"anchor {c.subject.rfc4514_string()} is not a {signer_org} signing root (keep TSA roots out)")
    _settings_json = json.dumps(
        {"trust": {"trust_anchors": anchors_pem, "trust_config": EKUS}, "verify": {"ocsp_fetch": False, "remote_manifest_fetch": False}}
    )
    _pin = (signer_org, signer_cn)


def _context():
    if _settings_json is None:
        raise TrustError("credentials.configure was not called")
    return c2pa.Context(c2pa.Settings.from_json(_settings_json))


def _summary(j):
    active = j.get("active_manifest")
    m = (j.get("manifests") or {}).get(active) or {}
    vr = (j.get("validation_results") or {}).get("activeManifest") or {}
    codes = lambda k: [c.get("code") for c in vr.get(k, [])]
    failures, successes = codes("failure"), codes("success")
    gen = (m.get("claim_generator_info") or [{}])[0]
    sig = m.get("signature_info") or {}
    assertions = {a.get("label"): a.get("data") for a in m.get("assertions", [])}
    actions = (assertions.get("c2pa.actions.v2") or {}).get("actions", [])
    cls = None
    for a in actions:
        st = a.get("digitalSourceType")
        if st in SOURCE_CLASS:
            cls = SOURCE_CLASS[st]
            if a.get("action") == "c2pa.edited":
                cls = "modified"
    ai = assertions.get("c2pa.ai-disclosure") or {}
    org, cn = _pin
    ours = (
        "signingCredential.trusted" in successes
        and not any(f and f.startswith(_SIGNER_FAILURES) for f in failures)
        and sig.get("issuer") == org
        and (cn is None or sig.get("common_name") == cn)
    )
    return {
        "manifest": active,
        "validation_state": j.get("validation_state"),
        "failures": failures,
        "ours": ours,
        "system": gen.get("name"),
        "version": gen.get("version"),
        "time": sig.get("time"),
        "class": cls,
        "model": ai.get("modelName"),
    }


def read_embedded(path, mime):
    """Summary of the manifest embedded in the file at `path`, or None when it carries none (or none readable)."""
    try:
        with open(path, "rb") as f:
            r = c2pa.Reader(mime, f, context=_context())
            j = json.loads(r.json())
    except TrustError:
        raise
    except Exception:
        return None
    return _summary(j)


def read_sidecar(manifest_bytes, data, mime):
    """Summary of a standalone manifest store bound to `data` (the EXR sequence manifest over its index), or
    None when it does not parse."""
    try:
        r = c2pa.Reader(mime, io.BytesIO(data), manifest_data=manifest_bytes, context=_context())
        j = json.loads(r.json())
    except TrustError:
        raise
    except Exception:
        return None
    return _summary(j)


def trusted(summary):
    """A valid Fabstir signature over unaltered content."""
    return summary is not None and summary["ours"] and summary["validation_state"] == "Trusted" and not summary["failures"]
