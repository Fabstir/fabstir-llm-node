# provenance-detector deploy (104.238.170.24, `detect.fabstir.net`; Ubuntu 24.04; disposable box)

Every step needs Jules's go. The box holds ONLY the result-signing key (Ed25519) and the index secret.

1. `/opt/provenance-detector`: this directory; `python3 -m venv .venv && .venv/bin/pip install --require-hashes`-style install
   of `requirements.txt` (pinned); the pinned ffmpeg 7.0.2 at `/opt/provenance/ffmpeg` (same sha256 as the node,
   `e7e7fb30...`); `apt install nsjail` (or build it) for the decode sandbox, then load its AppArmor profile
   (`cp deploy/nsjail.apparmor /etc/apparmor.d/nsjail && apparmor_parser -r /etc/apparmor.d/nsjail`): Ubuntu 24.04
   blocks the user namespaces nsjail needs without it. The service self-tests the sandboxed ffmpeg at start
   (golden signature of `provdetect/selftest.mp4`) and refuses to start if the sandbox or the pin is wrong.
2. `useradd --system --shell /usr/sbin/nologin provdetect`.
3. Result key, generated ON the box: `openssl genpkey -algorithm ed25519 | systemd-creds encrypt --name=result-key.pem - /etc/credstore.encrypted/result-key.pem`.
   Publish its public key (GET /v1/public-key) on the "How to detect" page.
4. Index secret: the same value as the signer's, `systemd-creds encrypt --name=index-secret ...`.
5. `/etc/provenance-detector/anchors.pem` = the Fabstir signing root ONLY (staging root until the conformance
   certificate). Never a TSA root: timestamps verify without it, and any root in this file could issue leaves
   that read as ours. The service refuses to start if an anchor is not a CA of `DETECTOR_SIGNER_ORG`; set
   `DETECTOR_SIGNER_CN` to the production leaf's common name when it is issued. `/etc/provenance-detector/exempt-keys` = one SHA-256 hex per issued exempt key (create it EMPTY if none are issued yet: `LoadCredential=` makes the file mandatory).
6. Network: Caddy on 443 -> 127.0.0.1:8701 with `deploy/Caddyfile` (only `/v1/detect`, `/v1/public-key`,
   `/v1/health`; body cap 500 MB; header/body timeouts; it replaces any client X-Forwarded-For); WireGuard to the signer (10.8.0.2 -> 10.8.0.1:8603, the index listener).
7. `systemctl enable --now provenance-detector`; check `curl https://detect.fabstir.net/v1/health`.

Residual risk, accepted explicitly: C2PA parsing (c2pa-python, i.e. the c2pa-rs Rust library) runs inside the
service process, not in nsjail, because the Reader works on the in-process stream. It is memory-safe Rust
under the unit's systemd hardening (no new privileges, read-only system, no home, its own user, 6 GB memory cap);
ffmpeg, the C decoder with the larger attack surface, is the part that is sandboxed.

Load: at most `DETECTOR_CONCURRENCY` detections at once (more get 503 immediately), so the spool holds at most
CONCURRENCY x 500 MB. Media longer than 5 minutes is refused as unanalysable; longer queries never reach the
signer (it refuses more than 1,500 frames; the detector sends overlapping windows of at most 1,500).
