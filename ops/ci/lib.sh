#!/usr/bin/env bash
set -euo pipefail

repo_root() {
  cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd
}

ci_setup_cargo_cache() {
  local root="$1"
  local cache_root="${root}/target/jankurai-cache"
  mkdir -p "$cache_root"
  export PATH="${HOME}/.local/bin:${HOME}/.cargo/bin:${PATH}"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${cache_root}/target}"
  export CARGO_HOME="${CARGO_HOME:-${cache_root}/cargo-home}"
  export SCCACHE_DIR="${SCCACHE_DIR:-${cache_root}/sccache}"
  export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
  if command -v sccache >/dev/null 2>&1; then
    export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
  fi
}

require_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "missing required command: $name" >&2
    exit 1
  fi
}
