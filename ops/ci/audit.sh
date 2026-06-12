#!/usr/bin/env bash
set -euo pipefail
# Compatibility lane for CI scanners: jankurai audit repo-score.
bash "$(dirname "${BASH_SOURCE[0]}")/jankurai.sh" "$@"
