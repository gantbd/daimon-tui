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

cargo build --quiet --manifest-path "$root/Cargo.toml" -p daimon-tui-cli \
  --bin daimon-tui --example real_daemon_fixture --example agent_daemon_fixture \
  --locked --offline
presence_fixture="$root/target/debug/examples/real_daemon_fixture"
agent_fixture="$root/target/debug/examples/agent_daemon_fixture"
client="$root/target/debug/daimon-tui"

"$presence_fixture" "$tmp/store" "$tmp/daimon.sock" \
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

output=$("$client" headless "$tmp/daimon.sock")
wait "$fixture_pid"
fixture_pid=

printf '%s\n' "$output" | grep '"complete": true' >/dev/null
printf '%s\n' "$output" | grep '"capability": "agents.presence.v1"' >/dev/null
printf '%s\n' "$output" | grep '"agents": 1' >/dev/null
printf '%s\n' "$output" | grep '"current": 1' >/dev/null

agent_session=07070707070707070707070707070707

"$agent_fixture" "$tmp/agent-headless.sock" >"$tmp/agent-headless.out" \
  2>"$tmp/agent-headless.err" &
fixture_pid=$!
attempt=0
while [ ! -S "$tmp/agent-headless.sock" ]; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 100 ]; then
    cat "$tmp/agent-headless.err" >&2
    exit 1
  fi
  sleep 0.05
done
output=$("$client" agent-inspect "$tmp/agent-headless.sock" "$agent_session")
wait "$fixture_pid"
fixture_pid=
printf '%s\n' "$output" | grep '"capability": "agents.session.inspect.v1"' >/dev/null
printf '%s\n' "$output" | grep '"phase": "ReattachRequired"' >/dev/null
printf '%s\n' "$output" | grep '"kind": "plan"' >/dev/null
printf '%s\n' "$output" | grep '"session_models": true' >/dev/null

"$agent_fixture" "$tmp/agent-interactive.sock" >"$tmp/agent-interactive.out" \
  2>"$tmp/agent-interactive.err" &
fixture_pid=$!
attempt=0
while [ ! -S "$tmp/agent-interactive.sock" ]; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 100 ]; then
    cat "$tmp/agent-interactive.err" >&2
    exit 1
  fi
  sleep 0.05
done
python3 - "$tmp" "$client" "$agent_session" <<'PYTHON'
from pathlib import Path
import shlex, subprocess, sys, time

scratch, binary, session = sys.argv[1:]
tmux_socket = str(Path(scratch) / "tmux.sock")
command = shlex.join([
    binary, "agent-interactive", str(Path(scratch) / "agent-interactive.sock"), session
])

def tmux(*args, check=True):
    return subprocess.run(
        ["tmux", "-S", tmux_socket, "-f", "/dev/null", *args],
        capture_output=True, text=True, check=check, timeout=5,
    )

try:
    tmux("new-session", "-d", "-x", "140", "-y", "18", "-s", "daimon", command)
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        screen = tmux("capture-pane", "-p", "-t", "daimon").stdout
        if all(marker in screen for marker in [
            "ReattachRequired/12", "load=true resume=true", "plan inspect before resuming",
        ]):
            break
        time.sleep(0.025)
    else:
        raise AssertionError(f"Daimon agent evidence did not render:\n{screen}")
    tmux("send-keys", "-t", "daimon", "q")
    deadline = time.monotonic() + 5
    while tmux("has-session", "-t", "daimon", check=False).returncode == 0:
        if time.monotonic() >= deadline:
            raise AssertionError("Daimon agent TUI did not exit")
        time.sleep(0.025)
finally:
    tmux("kill-server", check=False)
PYTHON
wait "$fixture_pid"
fixture_pid=

echo 'PASS: Daimon presence plus exact configured agent-session evidence in headless and interactive modes'
