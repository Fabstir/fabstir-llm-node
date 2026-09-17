# Private CA for node → broker TLS (P3.1, expert decision 2026-09-17)

Why a private root and not Let's Encrypt for `kbs.fabstir.net`: with a public root the
node-to-broker channel is only as strong as DNS plus domain validation (anyone who passes DV
for the name gets a certificate every node accepts), and a public certificate brings back a
renewal dependency that, combined with fail-closed, would stop every node on a silent renewal
failure. With a private root the channel's trust reduces to a key held offline. TLS here is
defence in depth (the DEK is ECIES-wrapped to `pk_att` regardless); it is simply far cheaper
to set up now, before an image digest and a `compose_hash` are pinned against it.

Shape: one self-signed root (10 years, key offline, never on the VPS); one server certificate
for `kbs.fabstir.net` (5 years) signed by it; no intermediate, no revocation path, no renewal
automation. The root PEM ships in the node image as the only trust anchor; the node verifies
chain and hostname and speaks TLS 1.3 only. Known gap G-15: compromise of either key means a
new root, a new image, a new digest, a new `compose_hash`, a re-pin.

## The flow, four commands, two of them Jules's

| # | Who | Where | Command | Output |
|---|---|---|---|---|
| 1 | **Jules** | WSL, offline | `deployment/phala/kbs-ca/make-root.sh` | root key at `~/fabstir-kbs-ca/` (never in the repo or on the VPS); `deployment/phala/kbs-root.pem` to **commit**; prints the root's expiry + fingerprint |
| 2 | node dev | VPS (over SSH) | `vps-install.sh csr` | server key stays on the VPS; the CSR is written to `deployment/phala/kbs-ca/issued/kbs.fabstir.net.csr` |
| 3 | **Jules** | WSL, offline | `deployment/phala/kbs-ca/sign-server-cert.sh` | `deployment/phala/kbs-ca/issued/kbs.fabstir.net.pem` to **commit**; prints the leaf's expiry |
| 4 | node dev | VPS (over SSH) | `vps-install.sh install …` then `vps-install.sh retire` | nginx serves the private cert over TLS 1.3, verified by `s_client` against the root before anything is retired; then certbot removed and port 80 closed, after proving nothing else listens on 80 |
| 5 | **Jules** | Vultr dashboard | remove TCP 80 from the `kbs-fabstir-net` firewall group | the box is 22 + 443 only |

Both offline scripts refuse to run twice (a second root would orphan every pinned image; a
second leaf while the first is valid is a mistake), refuse a CSR for any other name, and
print the exact lines to paste into the tracker's "Certificate expiries".

Steps 2 and 4 need the dev container to reach the VPS. It has no SSH client and no key; the
plan is a paramiko venv (prepared) plus a copy of the VPS key at the git-ignored path
`deployment/phala/kbs-ca/.ssh/kbs_ed25519` (Jules: `cp ~/.ssh/kbs_ed25519
deployment/phala/kbs-ca/.ssh/` from the repo root in WSL; delete it when the CA work is
done). Alternative if that is not wanted: Jules runs `vps-install.sh` stages himself, the
script decides everything.

## 5. Node side (P3.1, code)

`HttpKeyBrokerClient` builds its reqwest client with `tls_built_in_root_certs(false)`,
`add_root_certificate(<the PEM at TEE_KBS_CA_FILE, default /etc/fabstir/kbs-root.pem>)`,
`min_tls_version(TLS_1_3)`, hostname verification on (the default). The image `COPY`s
`kbs-root.pem` to that path, so the anchor is measured into `compose_hash` with everything
else. The guard test requires the PEM to be present in `deployment/phala/` and to be exactly
one certificate.
