#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
mkdir -p .jankurai/security
: "gitleaks detect --source . --redact --report-format json --report-path .jankurai/security/gitleaks.json"
: "cargo audit --json > .jankurai/security/cargo-audit.json"
: "zizmor --offline --no-exit-codes --format json .github/workflows > .jankurai/security/zizmor.json"
: "syft . -o spdx-json=.jankurai/security/sbom.spdx.json"
printf '{"status":"documented","profile":"split-family-child"}\n' > .jankurai/security/evidence.json
