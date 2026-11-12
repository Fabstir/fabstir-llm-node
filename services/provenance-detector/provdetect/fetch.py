# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
"""URL fetch for detection by link (D14): http(s) on ports 80/443 only, the host resolved ONCE and pinned (no DNS
rebinding), only globally routable addresses (`is_global`: private, loopback, link-local, CGNAT 100.64/10,
multicast, reserved all refused), no redirects, no proxy from the environment, a streaming size cap and a
TOTAL deadline. The body streams to a file with an incremental SHA-256; nothing is held in memory. TLS still
verifies the original host name (SNI and certificate)."""
import hashlib
import ipaddress
import socket
import time
from urllib.parse import urlsplit, urlunsplit

import httpx


class FetchError(Exception):
    pass


ALLOWED_PORTS = {"http": 80, "https": 443}


def _public(ip):
    a = ipaddress.ip_address(ip)
    if isinstance(a, ipaddress.IPv6Address) and a.ipv4_mapped:
        a = a.ipv4_mapped
    return a.is_global and not (a.is_multicast or a.is_reserved or a.is_unspecified)


def resolve_public(host, port, resolver=socket.getaddrinfo):
    """One resolution; every returned address must be public; the first is used for the connection."""
    try:
        infos = resolver(host, port, proto=socket.IPPROTO_TCP)
    except (socket.gaierror, UnicodeError, ValueError) as e:
        raise FetchError("host does not resolve") from e
    try:
        ips = [i[4][0] for i in infos]
        if not ips or not all(_public(ip) for ip in ips):
            raise FetchError("address not allowed")
    except ValueError as e:
        raise FetchError("address not allowed") from e
    return ips[0]


def _host_header(host):
    return f"[{host}]" if ":" in host else host


def fetch_to(url, out_path, max_bytes, timeout=30.0, deadline=120.0, resolver=socket.getaddrinfo, transport=None):
    """Download `url` into `out_path`; returns the SHA-256 hex of what was written."""
    try:
        u = urlsplit(url)
        port = u.port
        host = u.hostname
    except (ValueError, UnicodeError) as e:
        raise FetchError("not a valid URL") from e
    if u.scheme not in ALLOWED_PORTS or not host or u.username or u.password:
        raise FetchError("only http(s) URLs without credentials")
    if ":" not in host:  # an IDN name: resolve, send and verify its ASCII (punycode) form
        try:
            host = host.encode("idna").decode("ascii")
        except UnicodeError as e:
            raise FetchError("not a valid host name") from e
    if port is not None and port != ALLOWED_PORTS[u.scheme]:
        raise FetchError("only the standard http(s) ports")
    port = ALLOWED_PORTS[u.scheme]
    ip = resolve_public(host, port, resolver)
    netloc = f"[{ip}]:{port}" if ":" in ip else f"{ip}:{port}"
    pinned = urlunsplit((u.scheme, netloc, u.path or "/", u.query, ""))
    headers = {"Host": _host_header(host), "User-Agent": "platformless-provenance-detector"}
    end = time.monotonic() + deadline
    h = hashlib.sha256()
    n = 0
    try:
        with httpx.Client(follow_redirects=False, timeout=timeout, transport=transport, trust_env=False) as c:
            with c.stream("GET", pinned, headers=headers, extensions={"sni_hostname": host}) as r:
                if r.status_code != 200:
                    raise FetchError(f"HTTP {r.status_code}")
                with open(out_path, "wb") as f:
                    for chunk in r.iter_bytes():
                        n += len(chunk)
                        if n > max_bytes:
                            raise FetchError("too large")
                        if time.monotonic() > end:
                            raise FetchError("took too long")
                        h.update(chunk)
                        f.write(chunk)
    except (httpx.HTTPError, httpx.InvalidURL) as e:
        raise FetchError(f"could not fetch the URL ({type(e).__name__})") from e
    if n == 0:
        raise FetchError("empty response")
    return h.hexdigest()
