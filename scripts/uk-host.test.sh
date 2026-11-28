#!/usr/bin/env bash
# Copyright (c) 2026 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Tests for scripts/uk-host (fake docker, nvidia-smi and curl on PATH; nothing real is started), for the
# UK compose file and its example env file, and for the S5_REGION check in scripts/start-with-s5-bridge.sh.
#   bash scripts/uk-host.test.sh
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
UKHOST="$HERE/uk-host"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAKEBIN="$WORK/bin"
mkdir -p "$FAKEBIN"
TOKEN="t0ken-for-tests-0123456789abcdef"

# Fakes share state in $WORK: "state" is up or down (containers running), "log" is the node container's
# log (kept across a plain `up -d` restart, as Docker does; reset by --force-recreate; gone after down),
# "calls" records every call.
cat >"$FAKEBIN/docker" <<'EOF'
#!/usr/bin/env bash
echo "docker $*" >>"$FAKE_DIR/calls"
args=" $* "
fresh_log() {
  echo "📦 Loading model: /app/models/qwen38-flash-next/x.gguf"
  [[ "${FAKE_LOAD_FAILS:-0}" == 1 ]] && echo "❌ Failed to load model: out of memory"
  [[ "${FAKE_LOAD_FAILS:-0}" != 1 && "${FAKE_NEVER_LOADS:-0}" != 1 ]] && echo "✅ Model loaded successfully (ID: qwen)"
  if [[ "${FAKE_BIG_LOG:-0}" == 1 ]]; then
    # well over 1 MB of log after the load line
    head -c 1500000 /dev/zero | tr '\0' 'x' | fold -w 100
    echo
  fi
  return 0
}
case "$args" in
  *" up -d "*)
    [[ "${FAKE_UP_FAILS:-0}" == 1 ]] && exit 1
    [[ "$args" == *" --force-recreate "* ]] && rm -f "$FAKE_DIR/log"
    fresh_log >>"$FAKE_DIR/log"
    echo up >"$FAKE_DIR/state" ;;
  *" down "*) echo down >"$FAKE_DIR/state"; rm -f "$FAKE_DIR/log" ;;
  *" ps "*)
    if [[ "$args" == *" -a "* ]]; then
      [[ "$(cat "$FAKE_DIR/state")" == up || "${FAKE_LINGER:-0}" == 1 ]] && echo abc123
    else
      [[ "$(cat "$FAKE_DIR/state")" == up && "${FAKE_NODE_EXITED:-0}" != 1 ]] && echo abc123
    fi ;;
  *" logs "*)
    if [[ -n "${FAKE_LOADING_CALLS:-}" && "$(cat "$FAKE_DIR/state")" == up ]]; then
      n=$(( $(cat "$FAKE_DIR/logreads" 2>/dev/null || echo 0) + 1 )); echo "$n" >"$FAKE_DIR/logreads"
      (( n >= FAKE_LOADING_CALLS )) && ! grep -q "Model loaded" "$FAKE_DIR/log" && echo "✅ Model loaded successfully (ID: qwen)" >>"$FAKE_DIR/log"
    fi
    [[ -f "$FAKE_DIR/log" ]] && exec cat "$FAKE_DIR/log" ;;
esac
exit 0
EOF

cat >"$FAKEBIN/nvidia-smi" <<'EOF'
#!/usr/bin/env bash
if [[ "$(cat "$FAKE_DIR/state")" == up ]]; then
  echo "${FAKE_USED_UP:-93000}, ${FAKE_FREE_UP:-4887}, 97887"
else
  echo "${FAKE_USED_DOWN:-800}, ${FAKE_FREE_DOWN:-97087}, 97887"
fi
EOF

cat >"$FAKEBIN/curl" <<'EOF'
#!/usr/bin/env bash
url="${*: -1}"
echo "curl $url" >>"$FAKE_DIR/calls"
[[ -n "${FAKE_TOKEN:-}" && "$*" == *"$FAKE_TOKEN"* ]] && echo "TOKEN_IN_ARGV" >>"$FAKE_DIR/calls"
config=""
if [[ " $* " == *" -K - "* || " $* " == *" --config - "* ]]; then config="$(cat)"; fi
case "$url" in
  https://*/v1/version) [[ "${FAKE_PUBLIC_DOWN:-0}" == 1 ]] && exit 7; echo '{"version":"8.67.0","public":true}' ;;
  http://127.0.0.1:*/v1/version)
    [[ "$(cat "$FAKE_DIR/state")" == up && "${FAKE_NEVER_LOADS:-0}" != 1 ]] || exit 7
    # the node opens its API only after the model has loaded
    grep -q "Model loaded" "$FAKE_DIR/log" 2>/dev/null || exit 7
    echo '{"version":"8.67.0"}' ;;
  http://127.0.0.1:*/v1/sessions/active)
    [[ "$(cat "$FAKE_DIR/state")" == up ]] || exit 7
    # the node: 404 when it has no token set, 401 on a wrong or missing bearer token
    [[ -n "${FAKE_TOKEN:-}" ]] || exit 22
    [[ "$config" == *"Authorization: Bearer ${FAKE_TOKEN}"* ]] || exit 22
    echo "{\"active_sessions\": ${FAKE_SESSIONS:-0}}" ;;
  http://localhost:5522/health) echo '{"status":"healthy"}' ;;
  *) exit 22 ;;
esac
EOF

cat >"$FAKEBIN/cargo" <<'EOF'
#!/usr/bin/env bash
echo "FAKE NODE STARTED $*"
EOF
chmod +x "$FAKEBIN"/*

PASS=0
FAIL=0
ENVFILE="$WORK/env"
echo "API_PORT=8083" >"$ENVFILE"

# run STATE [VAR=VALUE ...] -- ARGS: runs uk-host with fresh fakes; sets OUT and RC. A running state
# starts with a loaded log; FAKE_STALE_LOG=1 leaves a stopped container whose old log holds a failure.
run() {
  local state="$1"; shift
  local -a vars=()
  while [[ "$1" != -- ]]; do vars+=("$1"); shift; done
  shift
  rm -f "$WORK/log" "$WORK/logreads"
  if [[ "$state" == loading ]]; then
    # container running, model still loading (API not open yet)
    state=up
    printf '%s\n' "📦 Loading model: x" >"$WORK/log"
  elif [[ "$state" == up ]]; then
    printf '%s\n' "📦 Loading model: x" "✅ Model loaded successfully (ID: qwen)" >"$WORK/log"
  fi
  echo "$state" >"$WORK/state"
  [[ " ${vars[*]-} " == *" FAKE_STALE_LOG=1 "* ]] && printf '%s\n' "📦 Loading model: x" "❌ Failed to load model: old run" >"$WORK/log"
  : >"$WORK/calls"
  OUT="$(timeout 20 env PATH="$FAKEBIN:$PATH" FAKE_DIR="$WORK" UK_HOST_ENV_FILE="$ENVFILE" UK_HOST_POLL_SECS=0 \
    UK_HOST_PROGRESS_SECS=0 "${vars[@]}" bash "$UKHOST" "$@" </dev/null 2>&1)"
  RC=$?
  # 124 = the run hung past its 20 s bound: always a failure of the check that follows
}

check() {
  local name="$1"; shift
  if "$@"; then PASS=$((PASS + 1)); echo "ok - $name"; else FAIL=$((FAIL + 1)); echo "not ok - $name"; echo "$OUT" | tail -n 15 | sed 's/^/    | /'; fi
}
has() { grep -qF -- "$1" <<<"$OUT"; }
lacks() { ! grep -qF -- "$1" <<<"$OUT"; }
called() { grep -qF -- "$1" "$WORK/calls"; }
not_called() { ! grep -qF -- "$1" "$WORK/calls"; }
rc_is() { [[ "$RC" == "$1" ]]; }

# 1. start refused on low VRAM: exit 2, names the likely holders, never calls up
run down FAKE_FREE_DOWN=1800 FAKE_USED_DOWN=96087 -- start
check "start refused when free VRAM is low: exit 2" rc_is 2
check "start refused names Unsloth, DaVinci Resolve, Blender" bash -c 'grep -q Unsloth <<<"$1" && grep -q "DaVinci Resolve" <<<"$1" && grep -q Blender <<<"$1"' _ "$OUT"
check "start refused does not start anything" not_called " up -d"
check "start refused makes no claim about what nvidia-smi shows under WSL" lacks "do not show in nvidia-smi"

# 1b. the threshold override is honoured
run down FAKE_FREE_DOWN=1800 UK_HOST_MIN_FREE_MIB=1000 -- start
check "UK_HOST_MIN_FREE_MIB lowers the threshold" rc_is 0

# 2. start ok: compose up with the UK file and env file, fresh containers, waits for the load line
run down -- start
check "start ok: exit 0" rc_is 0
check "start runs compose up with the UK compose file, env file and fresh containers" called "docker compose -f docker-compose.uk-3xsz.yml --env-file $ENVFILE up -d --force-recreate"
check "start waits for /v1/version on 127.0.0.1:API_PORT" called "curl http://127.0.0.1:8083/v1/version"
check "start reports loaded and VRAM used after" bash -c 'grep -q "loaded and answering" <<<"$1" && grep -q "VRAM: 93000 MiB used" <<<"$1"' _ "$OUT"

# 2a. a stopped container whose old log holds "Failed to load model" does not fail a new start
run down FAKE_STALE_LOG=1 -- start
check "start ignores a failure line left in the log of a previous run" rc_is 0

# 2a'. more than 1 MB of log after the load line (SIGPIPE under pipefail must not hide the match)
run down FAKE_BIG_LOG=1 UK_HOST_START_TIMEOUT_SECS=5 -- start
check "start finds the load line early in a log of over 1 MB" rc_is 0

# 2d. start re-run while a load is in progress: no recreate, no VRAM gate (the load holds the VRAM), resume
run loading FAKE_LOADING_CALLS=3 -- start
check "start during a load in progress: exit 0 once loaded" rc_is 0
check "start during a load in progress does not recreate the containers" not_called " up -d"
check "start during a load in progress says it resumes waiting" has "resuming the wait"
run loading FAKE_LOADING_CALLS=3 UK_HOST_MIN_FREE_MIB=0 -- start
check "start during a load in progress recreates nothing even when the VRAM gate would pass" not_called " up -d"
run loading FAKE_LOADING_CALLS=1000 UK_HOST_START_TIMEOUT_SECS=1 UK_HOST_POLL_SECS=1 -- start
check "a resumed wait keeps the start timeout" has "was not loaded and answering after 1 s"
check "a resumed wait that times out still recreates nothing" not_called " up -d"

# 2e. start when already running and loaded: reports it, recreates nothing
run up -- start
check "start when already loaded: exit 0" rc_is 0
check "start when already loaded says so" has "already running and loaded"
check "start when already loaded recreates nothing" not_called " up -d"

# 2b. start fails when the model load fails, when the node exits, and when the bridge refuses (up fails)
run down FAKE_LOAD_FAILS=1 FAKE_NEVER_LOADS=1 UK_HOST_START_TIMEOUT_SECS=5 -- start
check "start fails on a model load failure" rc_is 1
check "start shows the load failure, without waiting for the timeout" bash -c 'grep -q "failed to load the model" <<<"$1" && ! grep -q "was not loaded and answering" <<<"$1"' _ "$OUT"
run down FAKE_NODE_EXITED=1 FAKE_NEVER_LOADS=1 UK_HOST_START_TIMEOUT_SECS=5 -- start
check "start fails when the node container exits" rc_is 1
check "start says the node exited, without waiting for the timeout" bash -c 'grep -q "it exited" <<<"$1" && ! grep -q "was not loaded and answering" <<<"$1"' _ "$OUT"
run down FAKE_UP_FAILS=1 -- start
check "start fails when compose up fails (bridge region guard)" rc_is 1
check "start shows the bridge log" called "logs --no-color --tail 30 s5-bridge"

# 2c. start times out rather than waiting for ever
run down FAKE_NEVER_LOADS=1 UK_HOST_START_TIMEOUT_SECS=1 UK_HOST_POLL_SECS=1 -- start
check "start times out when the model never loads" rc_is 1
check "start timeout says so" has "was not loaded and answering after 1 s"

# 3. stop with open sessions: default route /v1/sessions/active with the bearer token from the env
#    file; refused without --force (no terminal to confirm on)
printf 'API_PORT=8083\nACTIVE_SESSIONS_TOKEN=%s\n' "$TOKEN" >"$ENVFILE"
run up FAKE_TOKEN="$TOKEN" FAKE_SESSIONS=2 -- stop
check "stop queries /v1/sessions/active by default" called "curl http://127.0.0.1:8083/v1/sessions/active"
check "stop with 2 sessions refused: exit 2" rc_is 2
check "stop with sessions names the count" has "2 open session(s)"
check "stop without a terminal says to use --force" has "no terminal to confirm on"
check "stop refused does not bring anything down" not_called " down"
check "the token is never on a command line" not_called "TOKEN_IN_ARGV"

# 3b. 401 (wrong token) and 404 (node has no token) are "unknown": refused without --force
run up FAKE_TOKEN="other-token" FAKE_SESSIONS=0 -- stop
check "stop with a rejected token (401) refused without --force" rc_is 2
check "stop says it cannot report sessions" has "cannot report its open sessions"
run up FAKE_SESSIONS=0 -- stop
check "stop when the route is absent (404) refused without --force" rc_is 2

# 4. stop ok: no sessions, or --force; waits for the node container to be gone, then reports VRAM
run up FAKE_TOKEN="$TOKEN" FAKE_SESSIONS=0 UK_HOST_STOP_TIMEOUT_SECS=3 -- stop
check "stop with 0 sessions: exit 0" rc_is 0
check "stop runs compose down" called "docker compose -f docker-compose.uk-3xsz.yml --env-file $ENVFILE down"
check "stop waits for the node container to be gone" called "ps -a -q llm-node"
check "stop prints the freed VRAM" bash -c 'grep -q "VRAM: 800 MiB used, 97087 MiB free" <<<"$1" && grep -q "92200 MiB freed" <<<"$1"' _ "$OUT"
run up FAKE_TOKEN="$TOKEN" FAKE_SESSIONS=3 UK_HOST_STOP_TIMEOUT_SECS=3 -- stop --force
check "stop --force with sessions: exit 0" rc_is 0
check "stop --force brings it down" called " down"

# 4b. something else still holds the GPU after the node is gone: say so, do not blame the node
run up FAKE_USED_DOWN=60000 FAKE_FREE_DOWN=37887 UK_HOST_VRAM_SETTLE_SECS=1 UK_HOST_POLL_SECS=1 -- stop --force
check "stop with the GPU held by something else: exit 0" rc_is 0
check "stop says the node is gone and something else holds the VRAM" bash -c 'grep -q "node is gone" <<<"$1" && grep -q "60000 MiB still in use by other programs" <<<"$1"' _ "$OUT"

# 4c. the node container does not go away: fail, and say that
run up FAKE_LINGER=1 UK_HOST_STOP_TIMEOUT_SECS=1 UK_HOST_POLL_SECS=1 -- stop --force
check "stop fails when the node container is still there" rc_is 1
check "stop says the container is still there" has "node container is still present"

# 5. status
printf 'API_PORT=8083\nACTIVE_SESSIONS_TOKEN=%s\n' "$TOKEN" >"$ENVFILE"
run up UK_HOST_PUBLIC_URL=https://3xsz.example.ts.net FAKE_TOKEN="$TOKEN" FAKE_SESSIONS=1 -- status
check "status: exit 0" rc_is 0
check "status says loaded" has "Node: loaded and answering"
check "status shows VRAM" has "VRAM: 93000 MiB used, 4887 MiB free"
check "status shows active sessions" has "Active sessions: 1"
check "status checks the public URL" called "curl https://3xsz.example.ts.net/v1/version"
check "status shows the public answer" has "answering: {\"version\":\"8.67.0\",\"public\":true}"
run down UK_HOST_PUBLIC_URL=https://3xsz.example.ts.net FAKE_PUBLIC_DOWN=1 -- status
check "status when stopped says stopped" has "Node: stopped"
check "status when the public URL is down says so" has "NOT answering"

# 5b. settings come from the env file when not in the environment, also when it has CRLF endings
printf 'API_PORT=8083\r\nUK_HOST_PUBLIC_URL=https://fromfile.example.ts.net\r\nACTIVE_SESSIONS_TOKEN=%s\r\n' "$TOKEN" >"$ENVFILE"
run up FAKE_TOKEN="$TOKEN" FAKE_SESSIONS=4 -- status
check "a CRLF env file is read: no error on API_PORT" rc_is 0
check "a CRLF env file is read: the public URL without its carriage return" called "curl https://fromfile.example.ts.net/v1/version"
check "a CRLF env file is read: the token without its carriage return" has "Active sessions: 4"
echo "API_PORT=8083" >"$ENVFILE"

# 6. usage
run down -- bogus
check "unknown command exits 64" rc_is 64

# 7. the UK compose file and its example env file
OUT="$(python3 - "$REPO/docker-compose.uk-3xsz.yml" "$REPO/.env.uk-3xsz.example" <<'PY' 2>&1
import sys, yaml
compose, example = open(sys.argv[1]).read(), open(sys.argv[2]).read()
d = yaml.safe_load(compose)
node, bridge = d["services"]["llm-node"], d["services"]["s5-bridge"]
env = node["environment"]
def say(ok, what): print(("PASS " if ok else "FAIL ") + what)
say(env.get("REQUIRE_MODEL_VALIDATION") == "${REQUIRE_MODEL_VALIDATION:-false}", "validation defaults to false")
say(str(env.get("ACTIVE_SESSIONS_TOKEN", "")).startswith("${ACTIVE_SESSIONS_TOKEN:?"), "session token required")
path = env.get("CUDA_CACHE_PATH")
vols = [v for v in node.get("volumes", []) if isinstance(v, str) and path and v.split(":")[1:2] == [path]]
named = vols and vols[0].split(":")[0] in (d.get("volumes") or {})
say(bool(path) and bool(named), "CUDA cache on a named volume at CUDA_CACHE_PATH")
say(env.get("CUDA_CACHE_MAXSIZE") == "4294967296", "CUDA cache size kept")
say("s5.platformlessai.ai" not in compose, "no Global host")
say(node.get("ports") == ["127.0.0.1:${API_PORT:-8083}:${API_PORT:-8083}"] and "ports" not in bridge, "API on 127.0.0.1 only")
say(node.get("restart") == "no" and bridge.get("restart") == "no", "no restart")
lines = [l for l in example.splitlines()]
say(any(l.startswith("REQUIRE_MODEL_VALIDATION=") or l.startswith("# REQUIRE_MODEL_VALIDATION=") for l in lines)
    and "set true from U7" in example, "example documents REQUIRE_MODEL_VALIDATION")
say(any(l.startswith("ACTIVE_SESSIONS_TOKEN=") for l in lines) and "openssl rand -hex 32" in example, "example documents ACTIVE_SESSIONS_TOKEN")
PY
)"
while IFS= read -r line; do
  name="compose: ${line#* }"
  if [[ "$line" == PASS* ]]; then PASS=$((PASS + 1)); echo "ok - $name"; else FAIL=$((FAIL + 1)); echo "not ok - $name"; fi
done <<<"$OUT"

# 8. start-with-s5-bridge.sh needs S5_REGION (no default). Run in an empty tree so nothing real starts.
SWB="$WORK/swb"
mkdir -p "$SWB/services/s5-bridge"
tr -d '\r' <"$REPO/scripts/start-with-s5-bridge.sh" >"$SWB/start.sh"
OUT="$(cd "$SWB" && timeout 20 env -u S5_REGION PATH="$FAKEBIN:$PATH" FAKE_DIR="$WORK" S5_SEED_PHRASE=x bash start.sh 2>&1)"; RC=$?
check "start-with-s5-bridge refuses without S5_REGION" rc_is 1
check "start-with-s5-bridge names S5_REGION" has "S5_REGION"
check "start-with-s5-bridge starts nothing without S5_REGION" lacks "FAKE NODE STARTED"
OUT="$(cd "$SWB" && timeout 20 env PATH="$FAKEBIN:$PATH" FAKE_DIR="$WORK" S5_SEED_PHRASE=x S5_REGION=global bash start.sh 2>&1)"; RC=$?
check "start-with-s5-bridge goes on with S5_REGION set" has "FAKE NODE STARTED"

echo "# pass $PASS"
echo "# fail $FAIL"
(( FAIL == 0 ))
