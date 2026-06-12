#!/usr/bin/env bash
set -euo pipefail
JANKURAI_MIN_VERSION="1.6.1"

for tool in cargo git jq jankurai; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing $tool" >&2; exit 1; }
done

version="$(jankurai --version 2>&1 | awk '{print $2}' | head -1 || true)"
if [ -z "$version" ] || [ "$(printf '%s\n%s\n' "$JANKURAI_MIN_VERSION" "$version" | sort -V | head -1)" != "$JANKURAI_MIN_VERSION" ]; then
  printf 'expected jankurai >= %s, got: %s\n' "$JANKURAI_MIN_VERSION" "${version:-unknown}" >&2
  exit 1
fi

echo "split-family child CI prerequisites present"
