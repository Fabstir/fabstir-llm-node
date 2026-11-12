# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""AP4 API tests: signed results, zero retention on every path, bounded load, unsigned errors for an incomplete
search, quotas (IPv6 /64, proxies, pruning) with an exempt tier, URL fetch SSRF rules."""
import hashlib
import os
import socket
import threading

import httpx
import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from fastapi.testclient import TestClient

from conftest import FakeSigner, Deps, fixture
from provdetect import app as appmod
from provdetect import detect, fetch, signing


def mk(ffmpeg, tmp_path, deps=None, **kw):
    spool = tmp_path / "spool"
    kw.setdefault("quota", appmod.Quota(2, [hashlib.sha256(b"press-key").hexdigest()]))
    a = appmod.make_app(deps=deps or Deps(ffmpeg, FakeSigner()), result_signer=signing.ResultSigner(Ed25519PrivateKey.generate()), spool_dir=str(spool), detector_url="https://detect.example/v1/detect", **kw)
    return TestClient(a), spool


@pytest.fixture
def client(ffmpeg, tmp_path):
    return mk(ffmpeg, tmp_path)


def unsigned(r, status):
    assert r.status_code == status, r.text
    assert "signature" not in r.json() and "result" not in r.json()


def test_result_signed_with_hash_url_time(client):
    c, _ = client
    body = fixture("t2v.mp4")
    r = c.post("/v1/detect", content=body, headers={"content-type": "application/octet-stream"})
    assert r.status_code == 200
    s = r.json()
    assert signing.verify(s)
    p = s["payload"]
    assert p["content_sha256"] == hashlib.sha256(body).hexdigest()
    assert p["detector"] == "https://detect.example/v1/detect" and isinstance(p["time"], int)
    assert p["kind"] == signing.KIND and len(p["key_id"]) == 16
    # the detection ran on a worker thread and still verified against the Fabstir root
    assert p["result"]["statement"] == detect.STATEMENT_GENERATED
    s["payload"]["result"]["statement"] = "tampered"
    with pytest.raises(Exception):
        signing.verify(s)


def test_signed_envelope_binds_kind_and_key_id():
    import base64

    rs = signing.ResultSigner(Ed25519PrivateKey.generate())
    good = rs.sign({"result": 1})
    assert signing.verify(good)
    # VALID signatures by the same key over payloads that are not detector results, or that name another key
    for payload in ({"result": 1}, {"result": 1, "kind": "something-else/1", "key_id": rs.key_id}, {"result": 1, "kind": signing.KIND, "key_id": "0" * 16}):
        forged = {**good, "payload": payload, "signature": base64.b64encode(rs.key.sign(signing.canonical(payload))).decode()}
        with pytest.raises(ValueError):
            signing.verify(forged)


def test_zero_retention(client, monkeypatch):
    c, spool = client
    seen = []
    real = appmod.detect.detect

    def spy(path, deps, work, sha=None, deadline=None):
        seen.append((os.listdir(spool), os.path.dirname(path) == work, os.path.getsize(path), sha))
        return real(path, deps, work, sha, deadline)

    monkeypatch.setattr(appmod.detect, "detect", spy)
    body = fixture("edit.mp4")
    c.post("/v1/detect", content=body)
    listing, in_work, size, sha = seen[0]
    assert len(listing) == 1 and in_work, "the upload streams into the request's own directory"
    assert size == len(body) and sha == hashlib.sha256(body).hexdigest(), "hashed as it streamed"
    assert os.listdir(spool) == [], "nothing is left after the response"


def test_zero_retention_on_error_paths(ffmpeg, tmp_path, monkeypatch):
    for exc, status in ((RuntimeError("boom"), 500), (detect.Unavailable("x"), 503), (detect.Unanalysable("x"), 422)):
        def boom(*a, **k):
            raise exc

        monkeypatch.setattr(appmod.detect, "detect", boom)
        c, spool = mk(ffmpeg, tmp_path, quota=appmod.Quota(100, []))
        unsigned(c.post("/v1/detect", content=fixture("edit.mp4")), status)
        assert os.listdir(spool) == []


def test_garbage_upload_is_unanalysable_unsigned(client):
    c, spool = client
    unsigned(c.post("/v1/detect", content=os.urandom(5000)), 422)
    assert os.listdir(spool) == []


def mock_signer(handler):
    return appmod.SignerClient("http://10.8.0.1:8603", "s", "https://provenance.example", transport=httpx.MockTransport(handler))


def test_signer_busy_or_down_is_503_never_a_result(ffmpeg, tmp_path):
    def too_big(q):
        return httpx.Response(413, json={"error": "body too large"})

    def busy(q):
        return httpx.Response(429, json={"error": "busy"})

    def down(q):
        raise httpx.ConnectError("refused")

    for h in (too_big, busy, down):
        c, spool = mk(ffmpeg, tmp_path, deps=Deps(ffmpeg, mock_signer(h)), quota=appmod.Quota(100, []))
        unsigned(c.post("/v1/detect", content=_stripped(ffmpeg, tmp_path)), 503)
        assert os.listdir(spool) == []


def _stripped(ffmpeg, tmp_path):
    from conftest import transform

    return transform(fixture("t2v.mp4"), tmp_path, "-map_metadata", "-1", "-c", "copy")


def test_signer_client_maps_404_to_not_found_and_the_rest_to_unavailable():
    assert mock_signer(lambda q: httpx.Response(404)).public("/v1/manifests/x") is None
    assert mock_signer(lambda q: httpx.Response(200, content=b"m")).public("/v1/manifests/x") == b"m"
    for code in (500, 502, 429, 403):
        with pytest.raises(detect.Unavailable):
            mock_signer(lambda q, c=code: httpx.Response(c)).public("/v1/manifests/x")
    with pytest.raises(detect.Unavailable):
        mock_signer(lambda q: httpx.Response(200, content=b"not json")).index(b"PAIFP")
    assert mock_signer(lambda q: httpx.Response(200, json={"matches": []})).index(b"PAIFP") == []


def test_concurrency_is_bounded_before_the_body_is_read(ffmpeg, tmp_path, monkeypatch):
    gate, entered = threading.Event(), threading.Event()

    def held(*a, **k):
        entered.set()
        gate.wait(10)
        return {"found": False, "basis": [], "statement": detect.STATEMENT_NONE, "provenance": None}

    monkeypatch.setattr(appmod.detect, "detect", held)
    c, _ = mk(ffmpeg, tmp_path, concurrency=1, quota=appmod.Quota(100, []), trusted_proxies=["testclient"])
    at = lambda ip: {"x-forwarded-for": ip}
    first = {}
    t = threading.Thread(target=lambda: first.setdefault("r", c.post("/v1/detect", content=b"x" * 100, headers=at("1.1.1.1"))))
    t.start()
    assert entered.wait(10)
    unsigned(c.post("/v1/detect", content=b"y" * 100, headers=at("1.1.1.1")), 429)  # one check per address
    unsigned(c.post("/v1/detect", content=b"y" * 100, headers=at("2.2.2.2")), 503)  # the decode slot is taken
    gate.set()
    t.join(10)
    assert first["r"].status_code == 200
    assert c.post("/v1/detect", content=b"z" * 100, headers=at("1.1.1.1")).status_code == 200, "slot and address released"


def test_spool_byte_budget(ffmpeg, tmp_path):
    c, spool = mk(ffmpeg, tmp_path, spool_budget=1000)
    unsigned(c.post("/v1/detect", content=b"x" * 5000), 503)
    assert os.listdir(spool) == []
    unsigned(c.post("/v1/detect", json={"url": "https://example.com/x"}), 503)  # a fetch reserves max_bytes


def test_request_budget_spent_is_503(ffmpeg, tmp_path):
    c, _ = mk(ffmpeg, tmp_path, request_budget=0)
    unsigned(c.post("/v1/detect", content=_stripped(ffmpeg, tmp_path)), 503)


def test_broken_decoder_reads_as_503_not_a_bad_file(ffmpeg, tmp_path):
    bad = os.urandom(5000)
    c, _ = mk(ffmpeg, tmp_path, health=lambda: False, quota=appmod.Quota(100, []))
    unsigned(c.post("/v1/detect", content=bad), 503)
    assert c.get("/v1/health").status_code == 503
    c2, _ = mk(ffmpeg, tmp_path / "b", health=lambda: True, quota=appmod.Quota(100, []))
    unsigned(c2.post("/v1/detect", content=bad), 422)
    assert c2.get("/v1/health").status_code == 200


def test_spool_failure_releases_the_slot(ffmpeg, tmp_path, monkeypatch):
    c, _ = mk(ffmpeg, tmp_path, uploads=1, concurrency=1, quota=appmod.Quota(100, []))
    real = appmod.tempfile.mkdtemp
    calls = []

    def once(*a, **k):
        calls.append(1)
        if len(calls) == 1:
            raise OSError("no space")
        return real(*a, **k)

    monkeypatch.setattr(appmod.tempfile, "mkdtemp", once)
    unsigned(c.post("/v1/detect", content=fixture("t2v.mp4")), 503)
    assert c.post("/v1/detect", content=fixture("t2v.mp4")).status_code == 200, "the slot and the address were released"


def test_slow_upload_is_cut_off(ffmpeg, tmp_path):
    c, spool = mk(ffmpeg, tmp_path, body_deadline=0)
    unsigned(c.post("/v1/detect", content=b"x" * 100_000), 408)
    assert os.listdir(spool) == []


def test_exempt_tier_is_unthrottled(client):
    c, _ = client
    body = fixture("t2v.mp4")
    assert c.post("/v1/detect", content=body).status_code == 200
    assert c.post("/v1/detect", content=body).status_code == 200
    assert c.post("/v1/detect", content=body).status_code == 429
    for _ in range(5):
        assert c.post("/v1/detect", content=body, headers={"x-api-key": "press-key"}).status_code == 200
    assert c.post("/v1/detect", content=body, headers={"x-api-key": "made-up"}).status_code == 429


def test_quota_counts_ipv6_per_64_and_prunes():
    now = [1000.0]
    q = appmod.Quota(1, [], clock=lambda: now[0])
    assert q.allow("2001:db8:1:2::1", None)
    assert not q.allow("2001:db8:1:2:ffff::9", None), "same /64, same bucket"
    assert q.allow("2001:db8:1:3::1", None)
    assert q.allow("::ffff:1.2.3.4", None) and not q.allow("1.2.3.4", None)
    now[0] += 3700
    q.allow("5.6.7.8", None)
    assert set(q.hits) == {"5.6.7.8"}, "expired buckets are dropped"


def test_proxy_header_believed_only_from_the_proxy(ffmpeg, tmp_path):
    body = fixture("t2v.mp4")
    c, _ = mk(ffmpeg, tmp_path, quota=appmod.Quota(1, []), trusted_proxies=["testclient"])
    assert c.post("/v1/detect", content=body, headers={"x-forwarded-for": "1.1.1.1"}).status_code == 200
    assert c.post("/v1/detect", content=body, headers={"x-forwarded-for": "1.1.1.1"}).status_code == 429
    assert c.post("/v1/detect", content=body, headers={"x-forwarded-for": "2.2.2.2"}).status_code == 200
    # a client-supplied entry in front of the proxy's own is ignored
    assert c.post("/v1/detect", content=body, headers={"x-forwarded-for": "9.9.9.9, 2.2.2.2"}).status_code == 429
    c2, _ = mk(ffmpeg, tmp_path / "b", quota=appmod.Quota(1, []))
    assert c2.post("/v1/detect", content=body, headers={"x-forwarded-for": "3.3.3.3"}).status_code == 200
    assert c2.post("/v1/detect", content=body, headers={"x-forwarded-for": "4.4.4.4"}).status_code == 429, "not from a proxy: ignored"


def test_cors_allows_the_site_only(ffmpeg, tmp_path):
    c, _ = mk(ffmpeg, tmp_path, cors_origins=["https://platformlessai.org"])
    pre = lambda o: c.options("/v1/detect", headers={"origin": o, "access-control-request-method": "POST"})
    assert pre("https://platformlessai.org").headers.get("access-control-allow-origin") == "https://platformlessai.org"
    assert "access-control-allow-origin" not in pre("https://evil.example").headers


def test_oversize_is_refused_before_detection(ffmpeg, tmp_path):
    c, spool = mk(ffmpeg, tmp_path, max_bytes=1000)
    unsigned(c.post("/v1/detect", content=b"x" * 2000), 413)
    assert os.listdir(spool) == []


def resolver_for(*ips):
    calls = []

    def r(host, port, proto=0):
        calls.append(host)
        return [(socket.AF_INET6 if ":" in ip else socket.AF_INET, socket.SOCK_STREAM, proto, "", (ip, port)) for ip in ips]

    r.calls = calls
    return r


def test_bad_urls_are_400_at_the_route(ffmpeg, tmp_path):
    c, spool = mk(ffmpeg, tmp_path, quota=appmod.Quota(100, []), resolver=resolver_for("100.64.1.1"))
    for body in ({"url": "https://example.com/x"}, {"url": "https://[::1"}, {"url": "http://example.com:8080/x"}, {"url": "https://exa\u0000mple.com/"}, {"url": 5}, {"nourl": 1}, [1]):
        unsigned(c.post("/v1/detect", json=body), 400)
    assert os.listdir(spool) == []


def answering():
    """A transport that WOULD serve the file: a refusal must come from the rule, never from a failed connect."""
    hits = []

    def h(q):
        hits.append(str(q.url))
        return httpx.Response(200, content=b"clip")

    return httpx.MockTransport(h), hits


def test_url_fetch_refuses_private_cgnat_and_rebinding(tmp_path):
    out = str(tmp_path / "o")
    t, hits = answering()
    assert fetch.fetch_to("https://example.com/clip.mp4", out, 1000, resolver=resolver_for("93.184.216.34"), transport=t)
    assert len(hits) == 1, "the control: a public address is fetched"
    hits.clear()
    for ips in (["127.0.0.1"], ["10.1.2.3"], ["169.254.169.254"], ["192.168.0.5"], ["100.64.0.1"], ["100.127.255.254"], ["::1"], ["::ffff:127.0.0.1"], ["fd00::1"], ["93.184.216.34", "10.0.0.1"], ["0.0.0.0"]):
        with pytest.raises(fetch.FetchError):
            fetch.fetch_to("https://example.com/clip.mp4", out, 1000, resolver=resolver_for(*ips), transport=t)
    for bad in ("file:///etc/passwd", "ftp://example.com/x", "https://user:pw@example.com/x", "gopher://x", "https://example.com:8443/x", "http://example.com:22/", "https://[::1", "http://exa mple.com:99999/"):
        with pytest.raises(fetch.FetchError):
            fetch.fetch_to(bad, out, 1000, resolver=resolver_for("93.184.216.34"), transport=t)
    assert hits == [], "nothing refused ever reached the network"


def test_url_fetch_resolves_once_pins_the_address_and_ignores_proxy_env(tmp_path, monkeypatch):
    monkeypatch.setenv("HTTPS_PROXY", "http://10.0.0.1:3128")
    made = []
    real_client = httpx.Client

    def client(*a, **k):
        made.append(k.get("trust_env"))
        return real_client(*a, **k)

    monkeypatch.setattr(fetch.httpx, "Client", client)
    seen = {}

    def handler(request):
        seen["host"] = request.url.host
        seen["header"] = request.headers["host"]
        seen["sni"] = request.extensions.get("sni_hostname")
        return httpx.Response(200, content=b"clip-bytes")

    r = resolver_for("93.184.216.34")
    out = tmp_path / "o"
    sha = fetch.fetch_to("https://example.com/clip.mp4", str(out), 1000, resolver=r, transport=httpx.MockTransport(handler))
    assert out.read_bytes() == b"clip-bytes" and sha == hashlib.sha256(b"clip-bytes").hexdigest()
    assert r.calls == ["example.com"], "one resolution, no second lookup a rebinding DNS could answer"
    assert seen["host"] == "93.184.216.34" and seen["header"] == "example.com" and seen["sni"] == "example.com"
    assert made == [False], "no proxy from the environment"
    fetch.fetch_to("https://[2606:4700::1111]/x", str(out), 1000, resolver=resolver_for("2606:4700::1111"), transport=httpx.MockTransport(handler))
    assert seen["header"] == "[2606:4700::1111]"
    idn = resolver_for("93.184.216.34")
    fetch.fetch_to("https://b\u00fccher.example/x", str(out), 1000, resolver=idn, transport=httpx.MockTransport(handler))
    assert idn.calls == ["xn--bcher-kva.example"] and seen["header"] == "xn--bcher-kva.example" and seen["sni"] == "xn--bcher-kva.example"


def test_url_fetch_never_follows_redirects_caps_size_and_time(tmp_path, monkeypatch):
    out = str(tmp_path / "o")
    r = resolver_for("93.184.216.34")
    with pytest.raises(fetch.FetchError):
        fetch.fetch_to("https://example.com/a", out, 1000, resolver=r, transport=httpx.MockTransport(lambda q: httpx.Response(302, headers={"location": "http://127.0.0.1/"})))
    with pytest.raises(fetch.FetchError):
        fetch.fetch_to("https://example.com/a", out, 10, resolver=resolver_for("93.184.216.34"), transport=httpx.MockTransport(lambda q: httpx.Response(200, content=b"x" * 100)))
    with pytest.raises(fetch.FetchError):
        fetch.fetch_to("https://example.com/a", out, 1000, deadline=-1, resolver=resolver_for("93.184.216.34"), transport=httpx.MockTransport(lambda q: httpx.Response(200, content=b"x" * 100)))
