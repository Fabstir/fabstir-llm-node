# fabstir-kbs deployment (design: `docs/development/DESIGN-PHASE5-KBS.md`; policy schema 3: `docs/development/IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md`)

| File | Purpose |
|---|---|
| `fabstir-kbs.service` | systemd unit: `User=fabstir-kbs`, `LD_LIBRARY_PATH=/opt/fabstir-kbs/lib` (driver stub), `RestartPreventExitStatus=78`, `StartLimit*` in `[Unit]`, `ReadWritePaths` = memo + capture + hwm + archive only |
| `fabstir-kbs-check.service`, `fabstir-kbs-check.timer`, `fabstir-kbs-check.sh` | the daily, read-only `authority check` of every served bundle against every served policy naming its root (D17); a failed unit plus `CRITICAL:` journal lines is the alert |
| `env.example` | `/etc/fabstir-kbs/env` template (every `KBS_*` in design §2, plus `KBS_AUTHORITY_DIR`) |
| `kbs-limits.conf` | the `http{}`-context `limit_req_zone`/`limit_conn_zone` snippet; the `limit_req zone=kbs burst=20 nodelay; limit_conn kbs_conn 16;` lines go inside the `/v1/kbs/` location of `deployment/phala/kbs-ca/nginx-kbs.conf` |
| `install.sh` | idempotent installer with the §13 assertions (nginx proxies `/v1/kbs/` and serves `/authority/`, the stub registered system-wide via `/etc/ld.so.conf.d` + `ldconfig` so `sudo -u fabstir-kbs fabstir-kbs …` starts too, `ldd` resolves it, NTP synchronised, ISRG Root YE present, `nginx -t`); creates the data tree below and enables the check timer |

Build (dev box, the node's build environment):

    cargo build --release --no-default-features --features inference,kbs --bin fabstir-kbs -j 4

Ship the binary and `/usr/local/cuda/lib64/stubs/libcuda.so` to the VPS, then
`./install.sh ./fabstir-kbs ./libcuda.so`. The tooling reads `/etc/fabstir-kbs/env` (or `KBS_ENV_FILE`) for `KBS_DATA_DIR`/`KBS_KEYRING_FILE`
when the process environment lacks them, so `sudo -u fabstir-kbs …` resolves the same keyring
path the broker will. The keyring is written as the service user
(`sudo -u fabstir-kbs fabstir-kbs keyring add --model-id … --provider 0x… --generate
--dek-out /dev/shm/dek.hex`), the DEK file is used for `seal --provider 0x… --authority
<bundle>` (the same address: the sealer refuses a policy signed by any other key, or already
expired, or one the bundle cannot meet) and shredded.

## Data layout (`KBS_DATA_DIR`, default `/var/lib/fabstir-kbs`, 0711)

| Path | Mode | Written by | What |
|---|---|---|---|
| `keyring.json` | 0600 | `keyring add` (service user) | one entry per model: DEK, provider, `test`, `min_policy_version` |
| `memo/`, `capture/` | 0700 | the broker | collateral/JWKS memo; the capture rings (`request.json` is attacker-writable: a corpus, never a source of truth) |
| `hwm/` | 0700 | the broker only | per-root high-water mark (`{root}.json`: manifest version + digest, epoch + set digest) |
| `archive/` | 0700 | the broker only | every accepted bundle, write-once, `archive/{root}/{epoch}-{manifest}-{digest16}-{digest16}.json` |
| `public/policies/` | 0755 | operator | `{model_id}.json`: the provider-signed release policy (schema 3), served at `/policies/` |
| `public/blobs/` | 0755 | operator | the sealed containers, served at `/blobs/` |
| `public/authority/` (`KBS_AUTHORITY_DIR`) | 0755 | operator | `{root without 0x}.json`: the authority bundle (root-signed manifest + signed host profile set), served at `/authority/` for transparency; the node never fetches it |

## What changed with policy schema 3

The sealed policy no longer pins host measurements. It names the host **authority root**
(an Ethereum address) plus minimums and allowed classes; the root signs a manifest naming the
signer keys; the signers sign an epoch-versioned set of approved host profiles (registers, OS
image, compose hashes, pins, GPU model and floors). A node release, a compose change, a new
host shape or a revocation is a **new profile epoch**, deployed by replacing one bundle file:
no reseal, no re-download. Schema-2 policies are refused everywhere (`verification`, never a
retried fault). Authority procedures (keys, epochs, signers, recovery) are in
`deployment/phala/authority/README.md`.

## Provider flow (schema 3)

1. `policy migrate-v2` (once, from a signed v2 policy) or write the v3 policy by hand.
2. `policy sign --policy p.json --key-file provider.key --encrypted-ref qwen.enc --authority
   <bundle> --out p.signed.json`: `--authority` refuses a root that does not match, minimums
   above the bundle, `min_threshold` above the manifest's, or `allowed_classes` sharing no
   class with the set's profiles (extra classes for host families still to come are fine).
3. `seal … --authority <bundle>` (the same check, before hours of sealing). The mainnet cut
   is a FRESH seal under a new DEK, never a reseal of a run-1/run-2 container (D19).
4. Any change to the policy itself (floors, minimums, classes, pins, TCB/advisory lists,
   `expiry`) is a reseal of that model; host changes never are.

## Deploy order (an upgrade from schema 2; V3.7)

1. Stage on the box, as files beside their final names: the bundle, the new signed policy,
   the new sealed container, the new keyring entry (the old entry and the v6 policy file are
   kept for the rollback week).
2. `deployment/phala/kbs-ca/vps-install.sh install /etc/fabstir-kbs/tls/kbs.pem
   /etc/fabstir-kbs/tls/kbs-root.pem` re-installs the nginx site with `/authority/` (backup,
   `nginx -t`, TLS verify, rollback on failure).
3. Upgrade the binary, the env file and the unit TOGETHER: `./install.sh` (it installs the
   binary and both units and creates `hwm/`, `archive/`, `public/authority/`), plus the
   `KBS_AUTHORITY_DIR=/var/lib/fabstir-kbs/public/authority` line added to
   `/etc/fabstir-kbs/env` by hand (install.sh never overwrites an existing env file; the
   default is the same path, the line makes it explicit); in
   the same `mv` batch put the bundle at `public/authority/{root}.json`, the policy at
   `public/policies/{model_id}.json` (replacing the v2 file) and the container.
4. `systemctl restart fabstir-kbs`; `curl -s http://127.0.0.1:3030/v1/kbs/info`.
5. Dry runs on the box, as the service user. (a) The served documents: `sudo -u fabstir-kbs
   fabstir-kbs authority check --bundle /var/lib/fabstir-kbs/public/authority/{root}.json
   --policy <every served policy naming that root, absolute path>`. On the first deploy of an
   image it prints `result=warn` with "no candidate profile until one is pinned" (the D18
   first epoch); any `refused:` line stops the deploy. (b) The binary's judgement, on a known
   pair: copy `tests/kbs/fixtures/{authority-bundle.json,v3-policy-signed.json}` and a
   directory holding `run1-boot2-request.json` as `request.json` and
   `run1-boot2-verified.json` as `verified.json` to `/var/lib/fabstir-kbs/replay/` (owner
   `fabstir-kbs`), then `authority check --bundle <that bundle> --policy <that policy>
   --replay <that dir>`: it must print `outcome=release` (the fixtures' windows run to
   2027-09-01). A capture of the NEW deployment cannot release on the first epoch (unpinned,
   and run 1's capture is another compose), so it is never the replay here; once boot 2 has
   released, its `verified/` capture is the one to vendor for later checks.
6. **Confirm loaded** (below). Revocation and every new epoch take effect only once the
   broker has LOADED the bundle.

## Confirm loaded (every bundle deploy)

The broker loads a bundle on the next `challenge` or `request_key` for a model bound to its
root, and only then raises the high-water mark (HWM). After placing a new bundle:

1. force one load: one `challenge` for the model (any node start does it), and
2. `sudo -u fabstir-kbs fabstir-kbs authority check --bundle
   /var/lib/fabstir-kbs/public/authority/{root}.json --hwm-dir /var/lib/fabstir-kbs/hwm`: it
   must print `hwm=served`. `served != hwm` means
   not yet loaded (above the HWM) or refused (below it, or another body at the same numbers).

A bundle more than 16 epochs or manifest versions above the HWM is refused (a compromised
signer cannot burn the counter): deploy the assembled intermediates in order, at most 16 per
confirmed load.

## The daily check (D17)

`fabstir-kbs-check.timer` runs `fabstir-kbs-check.sh` once a day as the service user. Exit 0
is clean; 1 is a refusal (a bundle or policy that does not validate, a policy with no
candidate profile, a replay refusal, a served bundle the HWM disagrees with, a lost HWM, a
policy naming a root with no served bundle); 3 is warnings only (an expiry within 30 days, an
expired profile still in the set, the two-boot first epoch whose profile is not pinned yet,
no HWM yet). Any non-zero exit fails the unit and logs `CRITICAL:` lines
(`journalctl -u fabstir-kbs-check | grep CRITICAL`). Expected alerts: an expired profile
alerts every day until the next epoch drops it; the first epoch of a new Phala deployment
(D18) alerts until `profile pin` lands as epoch 2.

## Backups and the HWM

- **Back up `hwm/` and `archive/` WITH the keyring**, same schedule, same custodian.
- **There is no HWM reset.** Never delete or edit `hwm/` files. A lost HWM beside a non-empty
  archive is a `fault` at load (every request for that root fails); `serve` walks the HWM
  forward through the re-verified archive at its next start (`systemctl restart
  fabstir-kbs`, CRITICAL log per step) and refuses to start (exit 78) on an entry that does
  not verify or a stray `.json` inside `archive/{root}/`.
- After a restore from backup, run the check with `--hwm-dir` and compare with the served
  bundles' digests. Restoring `hwm/` AND `archive/` together from an old backup reopens the
  rollback window and is undetectable (an accepted residual): keep the backups as protected as
  the keyring.

## Rollback

Rollback = the previous binary + env file + unit, with the v6 policy file and the old keyring
entry (kept one week after the upgrade). That restores **"the broker runs"**, not
**"releases"**: the schema-3 node image refuses schema-2 policies, the v2 policies have
expired, and a schema-2 broker cannot read the schema-3 policy the new container is sealed
under. Fix forward with a new epoch or a new bundle whenever possible.

## Exit 78 at start (the unit stays down)

A config, keyring, permission or mode refusal, plus (schema 3) `hwm/` or `archive/` not
writable, `KBS_AUTHORITY_DIR` missing, or an archive that does not reconcile with its HWM.
Fix the cause, then `systemctl reset-failed fabstir-kbs && systemctl start fabstir-kbs`.
