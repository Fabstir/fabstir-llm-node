# provenance-signer deploy (78.141.241.219, `provenance.fabstir.net`; Ubuntu 24.04)

Every step that touches the box needs Jules's go. Staging uses the staging chain only.

1. Build in the dev container: `cd services/provenance-signer && cargo build --release -j 4` (glibc 2.35 build
   runs on 24.04). Copy `target/release/provenance-signer` to `/opt/provenance-signer/` (root-owned, 0755).
2. `useradd --system --home /var/lib/provenance-signer --shell /usr/sbin/nologin provsigner`;
   `install -d -o provsigner -g provsigner -m 0700 /var/lib/provenance-signer`; `install -d -m 0755 /etc/provenance-signer`.
3. Key ceremony ON THE BOX (the key never leaves it): write a random passphrase straight into systemd-creds,
   never to a plain file:
   `head -c 32 /dev/urandom | base64 | systemd-creds encrypt --name=signing-key-passphrase - /etc/credstore.encrypted/signing-key-passphrase`
   Staging chain: `systemd-creds decrypt --name=signing-key-passphrase /etc/credstore.encrypted/signing-key-passphrase - | provenance-signer gen-staging-chain --out /etc/provenance-signer/staging --passphrase-file /dev/stdin`
   (writes ee.key.enc.pem encrypted, chain.pem, anchors.pem; CA keys discarded). Production: CSR from the CA
   flow; the key stays encrypted PKCS#8 under the same credential.
4. Index secret: `head -c 48 /dev/urandom | base64 | systemd-creds encrypt --name=index-secret - /etc/credstore.encrypted/index-secret`;
   the detector gets the same value in its own credential store.
5. `anchors.pem` = our root + the root of every configured TSA (the canary asserts `timeStamp.trusted`).
6. Network: TLS proxy (Caddy) on 443 for `provenance.fabstir.net` -> 127.0.0.1:8601 (`/v1/limits`, `/v1/reserve`,
   `/v1/sign`, `/v1/sign-sequence`) and -> 127.0.0.1:8602 (`/v1/manifests/*` only), with header/body read
   timeouts (slowloris) and a body cap of 1 MiB. The index listener binds the WireGuard address only; ufw allows
   8603 from the detector's tunnel address only. Nothing else listens.
7. `systemctl enable --now provenance-signer provenance-signer-canary.timer provenance-signer-reconcile.timer`.
8. Check: `curl https://provenance.fabstir.net/v1/limits`; run `provenance-signer canary --config ...` once.
