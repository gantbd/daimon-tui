#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/daimon-tui-live.XXXXXX")
fixture_pid=
cleanup() {
  if [ -n "$fixture_pid" ]; then
    kill "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

cargo run --quiet --manifest-path "$root/Cargo.toml" -p daimon-tui-cli \
  --example real_daemon_fixture -- "$tmp/store" "$tmp/daimon.sock" \
  >"$tmp/daemon.out" 2>"$tmp/daemon.err" &
fixture_pid=$!

attempt=0
while [ ! -S "$tmp/daimon.sock" ]; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 100 ]; then
    cat "$tmp/daemon.err" >&2
    exit 1
  fi
  sleep 0.05
done

output=$(cargo run --quiet --manifest-path "$root/Cargo.toml" -p daimon-tui-cli \
  -- headless "$tmp/daimon.sock")
wait "$fixture_pid"
fixture_pid=

printf '%s\n' "$output" | grep '"complete": true' >/dev/null
printf '%s\n' "$output" | grep '"capability": "agents.presence.v1"' >/dev/null
printf '%s\n' "$output" | grep '"agents": 1' >/dev/null
printf '%s\n' "$output" | grep '"current": 1' >/dev/null
