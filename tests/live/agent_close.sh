#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/daimon-agent-close.XXXXXX")
fixture_pid=
cleanup() {
  if [ -n "$fixture_pid" ]; then
    kill "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

cargo build --quiet --manifest-path "$root/Cargo.toml" -p daimon-tui-cli \
  --bin daimon-tui --example agent_close_fixture --locked --offline
fixture="$root/target/debug/examples/agent_close_fixture"
client="$root/target/debug/daimon-tui"
session=07070707070707070707070707070707
key=09090909090909090909090909090909
confirmation="CLOSE DAIMON SESSION $session REVISION 12 KEY $key AT 34"

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
set +e
refused=$(
  "$client" agent-close "$tmp/refused.sock" "$session" 12 "$key" 34 \
    "CLOSE" 2>"$tmp/refused.client.err"
)
status=$?
set -e
test "$status" -ne 0
wait "$fixture_pid"
fixture_pid=
printf '%s\n' "$refused" | grep '"status": "confirmation_refused"' >/dev/null
test ! -e "$tmp/refused.marker"

start_fixture "$tmp/success.sock" "$tmp/success.marker"
output=$(
  "$client" agent-close "$tmp/success.sock" "$session" 12 "$key" 34 "$confirmation"
)
wait "$fixture_pid"
fixture_pid=
printf '%s\n' "$output" | grep '"capability": "agents.session.close.v1"' >/dev/null
printf '%s\n' "$output" | grep '"status": "closed"' >/dev/null
printf '%s\n' "$output" | grep '"revision": 13' >/dev/null
test "$(cat "$tmp/success.marker")" = closed

echo 'PASS: exact-confirmed Daimon ready-session close -> one public-client mutation without retry'
