# RustSec advisory attestation

Status: clean root-lockfile audit against a pinned current RustSec database;
not an SBOM, source review, provider assessment, or legal opinion.

audited-at-utc: 2026-11-17T05:27:57Z
cargo-audit-version: 0.23.0
advisory-database-commit: 02e2517f457a359fc6d3440ea4560c8ea188fee8
advisory-database-commit-date: 2026-11-15T10:59:21+01:00
advisory-count: 1509
lockfile-sha256: c5e35d83a6ddb198805541f399fe0e2a733defeb370402c3b6f92ac900baa037
dependency-count: 116
vulnerability-count: 0
warning-count: 0

Produced with `cargo audit 0.23.0 --no-fetch --deny warnings` against the exact
database commit above. No advisory is ignored. CI audits against the then-current
database. This evidence covers only the coordinated source lockfile.
