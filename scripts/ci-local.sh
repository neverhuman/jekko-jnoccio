#!/usr/bin/env bash
set -euo pipefail
bash ops/ci/quality-gates.sh
bash ops/ci/jankurai.sh
