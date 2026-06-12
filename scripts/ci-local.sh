#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bash "$ROOT/ops/ci/quality-gates.sh"
bash "$ROOT/ops/ci/jankurai.sh"
