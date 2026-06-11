#!/usr/bin/env bash
set -euo pipefail
for tool in cargo git jq jankurai; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing $tool" >&2; exit 1; }
done
echo "split-family child CI prerequisites present"
