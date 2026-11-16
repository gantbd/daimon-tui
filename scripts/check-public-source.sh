#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

for required in CHANGELOG.md CONTRIBUTING.md LICENSE-APACHE LICENSE-MIT README.md SECURITY.md rust-toolchain.toml release/github-source.json docs/advisory-attestation.md; do
  [[ -s "$required" ]] || { echo "missing Daimon TUI public-source file: $required" >&2; exit 1; }
done

rg -q '^channel = "1\.95\.0"$' rust-toolchain.toml
rg -q '"remoteReachabilityQualified": false' release/github-source.json
for revision in 2e4faf10ec0a70805aa1db1555f93c77fcf0a1e4 b272106cf6d0093839a7d696c829bf8a408b0306 ebff27188388cd0249d07751b1cc848c6ef1a3f9 325e662a6a945cf8a064b9812ebbbdac02be4130; do
  rg -q "\"revision\": \"$revision\"" release/github-source.json || { echo "missing pinned dependency revision $revision" >&2; exit 1; }
done
expected=$(awk -F': ' '$1 == "lockfile-sha256" { print $2 }' docs/advisory-attestation.md)
actual=$(shasum -a 256 Cargo.lock | awk '{print $1}')
[[ -n "$expected" && "$actual" == "$expected" ]] || { echo 'Daimon TUI lockfile changed without a new advisory attestation' >&2; exit 1; }

echo 'Daimon TUI public-source contract pins the qualified execution dependency chain'
