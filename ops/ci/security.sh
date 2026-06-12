#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
mkdir -p target/jankurai/security
if command -v cargo >/dev/null 2>&1; then
  cargo metadata --locked --format-version 1 > target/jankurai/security/cargo-metadata.json
fi
if command -v gitleaks >/dev/null 2>&1; then
  gitleaks detect --source . --redact --report-format json --report-path target/jankurai/security/gitleaks.json
fi
