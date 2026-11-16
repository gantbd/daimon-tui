#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/daimon-agent-reattach.XXXXXX")
fixture_pid=
cleanup() {
  if [ -n "$fixture_pid" ]; then
    kill "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

cargo build --quiet --manifest-path "$root/Cargo.toml" \
  -p daimon-tui-cli --bin daimon-tui --example agent_reattach_fixture --locked --offline
fixture="$root/target/debug/examples/agent_reattach_fixture"
client="$root/target/debug/daimon-tui"

printf '%s\n' \
  '{' \
  '  "schema": 1,' \
  '  "session": "07070707070707070707070707070707",' \
  '  "expected_revision": 12,' \
  '  "now": 34,' \
  '  "execution": {' \
  '    "execution_id": "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a",' \
  '    "task_key": "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b",' \
  '    "task_incarnation": 4,' \
  '    "attempt_number": 2,' \
  '    "principal": "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c",' \
  '    "profile": "0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d",' \
  '    "idempotency_key": "09090909090909090909090909090909",' \
  '    "context": ["0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e"],' \
  '    "tools": ["0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f"],' \
  '    "budget": {' \
  '      "max_elapsed_millis": 1000,' \
  '      "max_input_tokens": 100,' \
  '      "max_output_tokens": 200,' \
  '      "max_tool_calls": 3,' \
  '      "max_context_bytes": 4096,' \
  '      "max_output_bytes": 8192' \
  '    }' \
  '  }' \
  '}' >"$tmp/request.json"

start_fixture() {
  socket=$1
  marker=$2
  "$fixture" "$socket" "$marker" >"$socket.out" 2>"$socket.err" &
  fixture_pid=$!
  attempt=0
  while [ ! -S "$socket" ]; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 100 ]; then
      cat "$socket.err" >&2
      exit 1
    fi
    sleep 0.05
  done
}

start_fixture "$tmp/refused.sock" "$tmp/refused.marker"
if refused_output=$(
  "$client" agent-reattach "$tmp/refused.sock" "$tmp/request.json" reattach \
    2>"$tmp/refused.err"
); then
  echo 'wrong confirmation unexpectedly succeeded' >&2
  exit 1
fi
wait "$fixture_pid"
fixture_pid=
printf '%s\n' "$refused_output" | grep '"status": "confirmation_refused"' >/dev/null
test ! -e "$tmp/refused.marker"
confirmation=$(printf '%s\n' "$refused_output" | python3 -c 'import json,sys; print(json.load(sys.stdin)["required_confirmation"])')
case "$confirmation" in
  'REATTACH DAIMON SESSION '*' REVISION '*' ENVELOPE '*' KEY '*' AT '*) ;;
  *) echo 'reattach confirmation does not bind revision and execution envelope' >&2; exit 1 ;;
esac

start_fixture "$tmp/success.sock" "$tmp/success.marker"
success_output=$(
  "$client" agent-reattach "$tmp/success.sock" "$tmp/request.json" "$confirmation"
)
wait "$fixture_pid"
fixture_pid=
printf '%s\n' "$success_output" | grep '"capability": "agents.session.reattach.v1"' >/dev/null
printf '%s\n' "$success_output" | grep '"status": "reattached"' >/dev/null
printf '%s\n' "$success_output" | grep '"revision": 13' >/dev/null
test "$(cat "$tmp/success.marker")" = reattached

echo 'PASS: exact-confirmed Daimon detached-session reattach -> one public-client mutation without retry'
