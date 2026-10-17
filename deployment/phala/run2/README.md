# Phase 5 run 2 artefacts (2026-09-28 to 2026-10-01)

Kept as the record of run 2. Under policy schema 3 (`docs/development/IMPLEMENTATION-POLICY-
V3-HOST-PROFILES.md`) the host measurements no longer live in the sealed policy, so:

- **`vps-switch.sh` and `vps-stage-video.sh` are retired (removed).** They swapped the SERVED
  policy and container between a "base" and a "video" slot because a schema-2 policy pinned
  one `compose_hash`: a compose change meant a second policy, a second container and a slot
  switch. Under schema 3 a compose change is a new host profile epoch signed by the host
  authority (`deployment/phala/authority/README.md`); the model's policy and container stay
  untouched, so there is nothing to switch. Their schema-2 readers would also refuse every
  schema-3 file.
- **`vps-seal-run2.sh` reads schema 3 now.** It checks the signed policy's document tag,
  schema, version, signer, `encrypted_ref`, model id and authority root, and seals with
  `--authority` so a root, minimum or class the bundle cannot meet is refused before the
  29 GB seal. The compose hash is no longer the policy's: `fabstir-kbs authority check
  --policy … --replay …` confirms the profile instead. The mainnet cut is a fresh `seal`
  under a new DEK (D19), not a reseal of run 2's container.
