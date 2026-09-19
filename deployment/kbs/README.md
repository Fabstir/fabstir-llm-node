# fabstir-kbs deployment (design: `docs/development/DESIGN-PHASE5-KBS.md`)

| File | Purpose |
|---|---|
| `fabstir-kbs.service` | systemd unit: `User=fabstir-kbs`, `LD_LIBRARY_PATH=/opt/fabstir-kbs/lib` (driver stub), `RestartPreventExitStatus=78`, `StartLimit*` in `[Unit]`, `ReadWritePaths` = memo + capture only |
| `env.example` | `/etc/fabstir-kbs/env` template (every `KBS_*` in design §2) |
| `kbs-limits.conf` | the `http{}`-context `limit_req_zone`/`limit_conn_zone` snippet; the `limit_req zone=kbs burst=20 nodelay; limit_conn kbs_conn 16;` lines go inside the `/v1/kbs/` location of `deployment/phala/kbs-ca/nginx-kbs.conf` |
| `install.sh` | idempotent installer with the §13 assertions (nginx proxy present, the stub registered system-wide via `/etc/ld.so.conf.d` + `ldconfig` so `sudo -u fabstir-kbs fabstir-kbs …` starts too, `ldd` resolves it, NTP synchronised, ISRG Root YE present, `nginx -t`) |

Build (dev box, the node's build environment):

    cargo build --release --no-default-features --features inference,kbs --bin fabstir-kbs -j 4

Ship the binary and `/usr/local/cuda/lib64/stubs/libcuda.so` to the VPS, then
`./install.sh ./fabstir-kbs ./libcuda.so`. The tooling reads `/etc/fabstir-kbs/env` (or `KBS_ENV_FILE`) for `KBS_DATA_DIR`/`KBS_KEYRING_FILE`
when the process environment lacks them, so `sudo -u fabstir-kbs …` resolves the same keyring
path the broker will. The keyring is written as the service user
(`sudo -u fabstir-kbs fabstir-kbs keyring add --model-id … --provider 0x… --generate
--dek-out /dev/shm/dek.hex --test`), the DEK file is used for `seal --provider 0x…` (the
same address: the sealer refuses a policy signed by any other key, or already expired) and
shredded.
Every policy change is followed by `reseal` (same DEK, new `policy_hash`) and an
atomic swap of both files before the node restarts.
