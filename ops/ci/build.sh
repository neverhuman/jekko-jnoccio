#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
source ops/ci/lib.sh
ci_setup_cargo_cache "$ROOT"
cargo build --locked --workspace --all-targets
