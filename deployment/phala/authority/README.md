# Host authority: keys and procedures (policy schema 3)

Design: `docs/development/IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md` (D4–D10, D14–D18). Every
model's sealed release policy names ONE host authority **root** (an Ethereum address). The
root signs a **manifest** naming the **signer** keys and a threshold; the signers sign an
epoch-versioned **host profile set** (the approved host identities: TD registers, OS image,
compose hashes, `app_id` + key provider pins, GPU models and floors). The broker reads
`{KBS_AUTHORITY_DIR}/{root without 0x}.json` = `{manifest, set}` (the "bundle") and releases a
key only to a host that matches a current profile. Changing which hosts are approved is a new
bundle, never a reseal.

## Who holds what (fill in at the ceremony, V3.7)

| Role | Address | Created | Where the key lives |
|---|---|---|---|
| root | (pending ceremony) | | separate medium (OQ-V3-1: encrypted USB by default; a hardware wallet works through `authority sign --signature-file`) |
| signer fabstir-1 | (pending ceremony) | | this machine, `~/fabstir-authority-keys/` |

With Fabstir alone: one root, one signer, threshold 1 (gap G-20). A signer key is never reused
across roots. The highest-signed ledger lives at `~/.fabstir-authority/{root}.json` on the
machine that runs `authority assemble`: back it up with the signer key.

## 1. Ceremony (once per root; Jules's hands)

```
# the root, on the separate medium (refuses a directory on the same filesystem as $HOME)
deployment/phala/authority/make-keys.sh root /media/<medium>/fabstir-authority
# the signer, on this machine
deployment/phala/authority/make-keys.sh signer fabstir-1
```

Each prints the address (computed by `fabstir-kbs key address`, the code that signs) and the
line for the table above. Then the manifest, two years (the daily check warns 30 days ahead):

```
{"doc":"fabstir-host-authority-manifest","schema_version":1,"root":"<root address>",
 "manifest_version":1,"signers":["<signer address>"],"threshold":1,
 "not_before":<now>,"expiry":<now + 2 years>}
fabstir-kbs authority sign --manifest manifest-v1.json \
    --key-file /media/<medium>/fabstir-authority/fabstir-authority-root.key \
    --first --out manifest-v1.signed.json
```

Unmount the medium. The root is needed again only for a new manifest (signer change,
renewal before expiry, recovery).

## 2. A profile from PREDICTED values (never from a capture)

A capture's `request.json` is attacker-writable (anyone reaching `/v1/kbs/` can plant one):
it only ever CONFIRMS. Registers come from `dstack-mr` (A-24), the OS image digest from the
published dstack image, the compose hash from Phala's rule over the compose file:

```
fabstir-kbs profile new --id phala/h200-<release> --class phala-h200 \
    --not-before <now> --expiry <now + at most 400 days> \
    --evidence "A-24 <shape>; dstack <version>; image <digest>" \
    --hwmodel GH100 --min-driver-version <floor> \
    --measurements <dstack-mr json> --os-image-digest <hex> \
    --compose-file deployment/phala/compose.gpu.yml \
    --compose-reference <attestation JSON of OUR earlier deployment> \
    --env-names <names, one per line> --out profile.json
fabstir-kbs profile check --profile profile.json <the same six inputs>
```

`profile new` prints the reference's platform fields (`reference.pre_launch_script…`,
`reference.kms_enabled…`): the prediction inherits them, so the reference must be our own
known-good deployment, never one fetched from the host being approved. Floors per family come
from NVIDIA's security bulletins (first H200 profile: run 1's observed driver `580.95.05`).

## 3. Set, bundle, deploy

```
fabstir-kbs profile assemble --root <root> --epoch 1 --issued-at <now> --first \
    --profile profile.json --out set-1.json
fabstir-kbs profile sign --set set-1.json --key-file ~/fabstir-authority-keys/signer-fabstir-1.key \
    --manifest manifest-v1.signed.json --out set-1.signed.json
fabstir-kbs authority assemble --manifest-signed manifest-v1.signed.json \
    --set-signed set-1.signed.json --first --out bundle-1.json
fabstir-kbs authority check --bundle bundle-1.json --policy <every policy bound to this root>
```

The `authority check --policy <every bound policy>` dry run is mandatory before every deploy:
a bundle the policies refuse still raises the root's high-water mark once loaded, so a
mis-minted bundle is fix-forward only. Deploy = put the file at
`/var/lib/fabstir-kbs/public/authority/{root}.json` (owner `fabstir-kbs`, 0644, via a
temporary name and `mv`), then **confirm loaded** (`deployment/kbs/README.md`).

## 4. The first deployment of an image: two boots, one sealed policy (D18)

Production policies require both pins (`require_app_id_pinned` and
`require_key_provider_pinned`). Phala's `app_id` exists only once the CVM does, so:

1. epoch 1 carries the predicted profile UNPINNED; boot 1 is refused by design with "matches
   `<id>` but the policy requires a pinned app id" (which also confirms registers and compose)
   and its pre-verify capture is written (the daily check warns meanwhile);
2. `fabstir-kbs profile pin --profile profile.json --capture <that capture dir> --app-id
   <the app id from the Phala dashboard for OUR CVM> --out profile-pinned.json` fills ONLY
   `app_id` and `key_provider` (Phala's KMS id, committed as `PHALA_KMS_KEY_PROVIDER`), and
   only after every identity row of the capture equals the profile and its app-id /
   key-provider events equal those values;
3. epoch 2 (`profile assemble --previous bundle-1.json --epoch 2 …`), deploy, confirm loaded;
4. restart the CVM from the dashboard; boot 2 releases.

**STOP rule:** if a capture's `key-provider` event differs from the committed KMS id, or its
`app-id` differs from the dashboard, STOP. Never take either value from the capture (that is
exactly what an attacker wants copied); `--key-provider-override` exists for a documented KMS
change only and prints this rule. This recurs for every new Phala deployment (each is a new
`app_id`) unless Phala returns the `app_id` before the CVM starts (OQ-V3-2).

## 5. Routine epochs

Add, renew or remove a profile; a compose change (a node release) is a new profile or a new
compose hash in one: `profile assemble --previous <the bundle now served> --epoch <n+1>`,
sign, `authority assemble --previous <the bundle now served>`, check, deploy, confirm loaded.
Counters step by 0 or +1, never more, and a 0 step needs an identical body. The ledger refuses
a number already assembled with another body: abandoned staged epochs are deployed in order
(at most 16 per confirmed load), never re-used. Deleting `~/.fabstir-authority/{root}.json` is
the knowing override for a staged number whose bundle is lost; the broker's high-water mark
stays the backstop.

Revocation (a host or an image withdrawn): the next epoch without its profile. It takes
effect only once the broker has LOADED that epoch (confirm loaded); a refused host is told
which archived epoch still held the profile.

## 6. Signers

- **Add a signer** (G-20): manifest v<n+1> naming both, signed by the root
  (`authority sign --previous <served bundle>`); dry-run it with a test set signed by the
  NEW key (`profile sign` + `authority assemble` to a scratch name + `authority check`)
  before deploy. Transition sets carry both signatures (`profile sign … --append <the set signed by the
  first key>`): a signature
  by a key outside the current manifest is ignored and logged, never refused, so the set
  verifies under the old manifest and the new one.
- **Remove or replace a compromised signer:** manifest v<n+1> without it, assembled with
  `--previous <the archived accepted bundle>` (the archive under
  `/var/lib/fabstir-kbs/archive/{root}/` is why accepted bundles are kept). The high-water
  mark (with digests) and the +16 jump bound stop the old signer's sets from being replayed or
  burning the counter. No model is resealed.

## 7. The root

- **Root compromise or loss** (G-23): a lost root cannot renew the manifest, and every model
  trusting it stops releasing at the manifest's expiry; a compromised root can appoint any
  signer. Both are remedied only by resealing every model to a NEW root. Hence the separate
  medium, the two-year manifest and the daily expiry warning. Where the root key and any
  backup copy of it live is OQ-V3-1 (Jules's decision, before V3.7).
- A TCB recovery (new Intel TCB status or advisory) is a reseal of every model (the TCB and
  advisory lists live in the sealed policy; F11).
