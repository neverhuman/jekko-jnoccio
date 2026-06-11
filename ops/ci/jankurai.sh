#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
jankurai --version | grep -q "1.6.1"
mkdir -p agent
jankurai audit . --mode advisory --json agent/repo-score.json --md agent/repo-score.md
