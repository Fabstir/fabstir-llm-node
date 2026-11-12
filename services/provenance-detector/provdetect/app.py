# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""HTTP API (D14, Code Measures 2.1-2.3).

POST /v1/detect     body = the file (any Content-Type but JSON), or JSON {"url": "..."}; returns a signed coarse result
GET  /v1/public-key the result-signing key
GET  /v1/health

Load: at most `concurrency` detections at once (more get 503 at once, before their body is read), so the spool
never holds more than concurrency x max_bytes. The body streams to the request's own directory with an
incremental SHA-256 under a read deadline; the blocking work (fetch, ffmpeg, c2pa, the signer's index) runs on a
worker thread, never on the event loop. anyio waits for a worker thread to finish before a cancelled request
unwinds, so the directory is never removed under a running decoder.

Zero retention: every request works in its own directory under SPOOL_DIR (memory-backed /run on the box) that is
removed before the response is sent, on every path; nothing about the content is logged (uvicorn runs with
--no-access-log). Only a completed search is signed: an outage, a busy signer or an undecodable file is
answered unsigned (503 / 422), never as "no mark found".

Quotas: per client address (IPv6 per /64), one process (uvicorn --workers 1) so the count is global, pruned
as it goes; an exempt tier for API keys issued to authorities, media, fact-checkers, researchers and civil
society. Behind Caddy the client address is the right-most X-Forwarded-For entry, believed only from a
configured proxy address."""
import asyncio
import hashlib
import ipaddress
import json
import logging
import os
import shlex
import shutil
import tempfile
import threading
import time
import uuid

import httpx
from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import JSONResponse
from starlette.requests import ClientDisconnect
from starlette.concurrency import run_in_threadpool

from . import credentials, detect, fetch, fingerprint
from .signing import ResultSigner

FEEDBACK = "https://platformlessai.org/verify#feedback"
log = logging.getLogger("provdetect")
MAX_JSON_BODY = 8 * 1024


class Quota:
    def __init__(self, per_hour, exempt_key_hashes, clock=time.time):
        self.per_hour = per_hour
        self.exempt = set(exempt_key_hashes)
        self.hits = {}
        self.lock = threading.Lock()
        self.clock = clock
        self.last_sweep = clock()

    @staticmethod
    def bucket(ip):
        """IPv6 clients are counted per /64 (one subscriber's allocation), IPv4 per address."""
        try:
            a = ipaddress.ip_address(ip)
        except ValueError:
            return ip
        if isinstance(a, ipaddress.IPv6Address):
            if a.ipv4_mapped:
                return str(a.ipv4_mapped)
            return str(ipaddress.ip_network(f"{a}/64", strict=False))
        return str(a)

    def is_exempt(self, api_key):
        return bool(api_key) and hashlib.sha256(api_key.encode()).hexdigest() in self.exempt

    def allow(self, ip, api_key):
        if self.is_exempt(api_key):
            return True
        now = self.clock()
        key = self.bucket(ip)
        with self.lock:
            if now - self.last_sweep > 60:
                self.hits = {k: v for k, v in self.hits.items() if v and now - v[-1] < 3600}
                self.last_sweep = now
            h = [t for t in self.hits.get(key, []) if now - t < 3600]
            if len(h) >= self.per_hour:
                self.hits[key] = h
                return False
            h.append(now)
            self.hits[key] = h
            return True


class SignerClient:
    """The signer's internal index API (matches, never raw fingerprints) and its public manifest listener.
    Anything but a definite answer raises `detect.Unavailable` (the signer's 413/429 included: the detector
    keeps queries within MAX_QUERY_FRAMES, so either means the signer is busy or misconfigured)."""

    def __init__(self, index_url, index_secret, public_url, timeout=20.0, transport=None):
        self.index_url, self.secret, self.public_url = index_url.rstrip("/"), index_secret, public_url.rstrip("/")
        self.http = httpx.Client(timeout=timeout, trust_env=False, follow_redirects=False, transport=transport)

    def index(self, packed):
        try:
            r = self.http.post(f"{self.index_url}/v1/index/match", content=packed, headers={"x-pai-index-secret": self.secret})
        except httpx.HTTPError as e:
            raise detect.Unavailable(f"index unreachable ({type(e).__name__})") from e
        if r.status_code != 200:
            raise detect.Unavailable(f"index answered HTTP {r.status_code}")
        try:
            return r.json()["matches"]
        except (ValueError, KeyError, TypeError) as e:
            raise detect.Unavailable("index answer unreadable") from e

    def public(self, path):
        try:
            r = self.http.get(f"{self.public_url}{path}")
        except httpx.HTTPError as e:
            raise detect.Unavailable(f"manifest listener unreachable ({type(e).__name__})") from e
        if r.status_code == 200:
            return r.content
        if r.status_code == 404:
            return None
        raise detect.Unavailable(f"manifest listener answered HTTP {r.status_code}")


class Deps:
    def __init__(self, ffmpeg, signer_client):
        self.ffmpeg = ffmpeg
        self._c = signer_client

    def index(self, packed):
        return self._c.index(packed)

    def public(self, path):
        return self._c.public(path)


class _TooLarge(Exception):
    pass


class _Busy(Exception):
    pass


def client_ip(request, trusted_proxies):
    peer = request.client.host if request.client else "?"
    if peer in trusted_proxies:
        xff = request.headers.get("x-forwarded-for", "")
        last = xff.split(",")[-1].strip()
        if last:
            return last
    return peer


def make_app(
    *,
    deps,
    result_signer: ResultSigner,
    spool_dir,
    detector_url,
    max_bytes=500 * 1024 * 1024,
    quota=None,
    resolver=None,
    concurrency=2,
    uploads=8,
    spool_budget=2 * 1024 * 1024 * 1024,
    body_deadline=120.0,
    fetch_deadline=120.0,
    request_budget=540.0,
    trusted_proxies=(),
    cors_origins=(),
    health=None,
):
    """`uploads` bodies may stream in at once (all together within `spool_budget` bytes); `concurrency` of them
    decode at once; one check per client address (or /64) at a time unless it holds an exempt key. `health`
    (callable, True when the sandboxed decoder still reproduces its golden signature) is re-checked, cached for a
    minute, when a file is unanalysable: a decoder that broke after start reads as 503, not as a bad file."""
    app = FastAPI(title="Platformless AI provenance detector", docs_url=None, redoc_url=None, openapi_url=None)
    if cors_origins:
        app.add_middleware(CORSMiddleware, allow_origins=list(cors_origins), allow_methods=["GET", "POST"], allow_headers=["content-type", "x-api-key"])
    quota = quota or Quota(60, [])
    upload_slots = threading.BoundedSemaphore(uploads)
    decode_slots = threading.BoundedSemaphore(concurrency)
    trusted_proxies = set(trusted_proxies)
    lock = threading.Lock()
    state = {"spool": 0, "active": set(), "health": (None, 0.0)}
    os.makedirs(spool_dir, exist_ok=True)

    def err(status, msg):
        return JSONResponse({"error": msg, "feedback": FEEDBACK}, status_code=status)

    def healthy():
        if health is None:
            return True
        with lock:
            ok, at = state["health"]
        if ok is not None and time.monotonic() - at < 60:
            return ok
        try:
            ok = bool(health())
        except Exception:
            ok = False
        with lock:
            state["health"] = (ok, time.monotonic())
        return ok

    def reserve(n):
        with lock:
            if state["spool"] + n > spool_budget:
                return False
            state["spool"] += n
            return True

    def unreserve(n):
        with lock:
            state["spool"] -= n

    @app.get("/v1/health")
    def health_route():
        ok = healthy()
        return JSONResponse({"ok": ok}, status_code=200 if ok else 503)

    @app.get("/v1/public-key")
    def public_key():
        return {"alg": "Ed25519", "public_key": result_signer.public_b64, "key_id": result_signer.key_id}

    async def read_to(request, path, cap, held):
        h, n = hashlib.sha256(), 0
        with open(path, "wb") as f:
            async for chunk in request.stream():
                n += len(chunk)
                if n > cap:
                    raise _TooLarge()
                if not reserve(len(chunk)):
                    raise _Busy()
                held[0] += len(chunk)
                h.update(chunk)
                f.write(chunk)
        return h.hexdigest(), n

    def job(path, work, sha, url, deadline):
        if url is not None:
            kw = {"resolver": resolver} if resolver else {}
            sha = fetch.fetch_to(url, path, max_bytes, deadline=fetch_deadline, **kw)
        return sha, detect.detect(path, deps, work, sha, deadline)

    @app.post("/v1/detect")
    async def detect_route(request: Request):
        ip = client_ip(request, trusted_proxies)
        api_key = request.headers.get("x-api-key")
        if not quota.allow(ip, api_key):
            return err(429, "quota exceeded; ask for an exempt key if you are an authority, journalist, fact-checker, researcher or NGO")
        declared = request.headers.get("content-length")
        if declared and declared.isdigit() and int(declared) > max_bytes:
            return err(413, "file too large")
        who = None if quota.is_exempt(api_key) else Quota.bucket(ip)
        with lock:
            if who is not None and who in state["active"]:
                return err(429, "one check at a time from one address")
            if not upload_slots.acquire(blocking=False):
                return err(503, "the detector is busy; try again in a minute")
            if who is not None:
                state["active"].add(who)
        work, held, decoding = None, [0], False
        try:
            try:
                work = tempfile.mkdtemp(prefix=f"req-{uuid.uuid4().hex}-", dir=spool_dir)
            except OSError:
                log.warning("spool unavailable")
                return err(503, "the detector is busy; try again in a minute")
            path = os.path.join(work, "upload")
            is_json = request.headers.get("content-type", "").startswith("application/json")
            try:
                sha, n = await asyncio.wait_for(read_to(request, path, MAX_JSON_BODY if is_json else max_bytes, held), body_deadline)
            except _TooLarge:
                return err(413, "file too large")
            except _Busy:
                return err(503, "the detector is busy; try again in a minute")
            except asyncio.TimeoutError:
                return err(408, "upload too slow")
            except ClientDisconnect:
                return err(400, "upload interrupted")
            url = None
            if is_json:
                try:
                    with open(path, "rb") as f:
                        url = json.loads(f.read())["url"]
                    if not isinstance(url, str):
                        raise TypeError
                except (ValueError, KeyError, TypeError, RecursionError):
                    return err(400, "body must be the file, or JSON {\"url\": ...}")
                os.remove(path)
                if not reserve(max_bytes):
                    return err(503, "the detector is busy; try again in a minute")
                held[0] += max_bytes
            elif n == 0:
                return err(400, "empty file")
            if not decode_slots.acquire(blocking=False):
                return err(503, "the detector is busy; try again in a minute")
            decoding = True
            deadline = time.monotonic() + request_budget
            try:
                sha, result = await run_in_threadpool(job, path, work, sha, url, deadline)
            except fetch.FetchError as e:
                return err(400, f"could not fetch the URL: {e}")
            except detect.Unavailable:
                return err(503, "the detector cannot complete a search right now; try again later (no result was given)")
            except detect.Unanalysable as e:
                if not await run_in_threadpool(healthy):
                    log.warning("decoder self-test failing; answering 503")
                    return err(503, "the detector cannot complete a search right now; try again later (no result was given)")
                return err(422, f"this file could not be analysed: {e} (no result was given)")
            except Exception as e:
                log.warning("detection failed: %s", type(e).__name__)  # the class only: never content or a path
                return err(500, "internal error (no result was given)")
        finally:
            if work is not None:
                shutil.rmtree(work, ignore_errors=True)
            unreserve(held[0])
            if decoding:
                decode_slots.release()
            upload_slots.release()
            if who is not None:
                with lock:
                    state["active"].discard(who)
        payload = {"content_sha256": sha, "detector": detector_url, "time": int(time.time()), "result": result, "feedback": FEEDBACK}
        return JSONResponse(result_signer.sign(payload))

    return app


def from_env():
    """Production wiring (systemd: credentials from $CREDENTIALS_DIRECTORY). Fails closed: no sandbox, a TSA or
    foreign root in the anchors, or an ffmpeg that does not reproduce the golden signature through the sandbox,
    and the service does not start."""
    creds = os.environ.get("CREDENTIALS_DIRECTORY", "/run/credentials/provenance-detector")
    with open(os.environ["DETECTOR_TRUST_ANCHORS"]) as f:
        credentials.configure(f.read(), os.environ["DETECTOR_SIGNER_ORG"], os.environ.get("DETECTOR_SIGNER_CN") or None)
    sandbox = shlex.split(os.environ.get("DETECTOR_SANDBOX", ""))
    if not sandbox or "{work}" not in " ".join(sandbox):
        raise RuntimeError("DETECTOR_SANDBOX must be set and bind {work} (the request's directory) only")
    ff = fingerprint.Ffmpeg(os.environ["DETECTOR_FFMPEG"], sandbox=sandbox)
    spool = os.environ.get("DETECTOR_SPOOL", "/run/provenance-detector/spool")
    os.makedirs(spool, exist_ok=True)
    st = tempfile.mkdtemp(prefix="selftest-", dir=spool)
    try:
        ff.self_test(st)
    finally:
        shutil.rmtree(st, ignore_errors=True)
    def health():
        d = tempfile.mkdtemp(prefix="health-", dir=spool)
        try:
            ff.self_test(d)
            return True
        finally:
            shutil.rmtree(d, ignore_errors=True)

    with open(os.path.join(creds, "index-secret")) as f:
        secret = f.read().strip()
    client = SignerClient(os.environ["DETECTOR_INDEX_URL"], secret, os.environ["DETECTOR_PUBLIC_URL"])
    exempt = []
    if os.path.exists(os.path.join(creds, "exempt-keys")):
        with open(os.path.join(creds, "exempt-keys")) as f:
            exempt = [l.strip() for l in f if l.strip()]
    return make_app(
        deps=Deps(ff, client),
        result_signer=ResultSigner.from_pem_file(os.path.join(creds, "result-key.pem")),
        spool_dir=spool,
        detector_url=os.environ.get("DETECTOR_URL", "https://detect.fabstir.net/v1/detect"),
        quota=Quota(int(os.environ.get("DETECTOR_PER_HOUR", "60")), exempt),
        concurrency=int(os.environ.get("DETECTOR_CONCURRENCY", "2")),
        health=health,
        trusted_proxies=[p for p in os.environ.get("DETECTOR_TRUSTED_PROXIES", "127.0.0.1").split(",") if p],
        cors_origins=[o for o in os.environ.get("DETECTOR_CORS_ORIGINS", "https://platformlessai.org").split(",") if o],
    )
