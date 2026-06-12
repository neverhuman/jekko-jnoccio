#!/usr/bin/env bash
set -euo pipefail
missing=()
for cmd in cargo rustc just jq rg jankurai; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    missing+=("$cmd")
  fi
done
if [[ ${#missing[@]} -gt 0 ]]; then
  printf 'missing required local CI tools:
' >&2
  printf '  %s
' "${missing[@]}" >&2
  exit 1
fi
printf 'All required local CI tools are installed.
'
