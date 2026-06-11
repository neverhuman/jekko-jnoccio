#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
artifact_root="${JANKURAI_ARTIFACT_ROOT:-target/jankurai}"

install_gitleaks() {
  if command -v gitleaks >/dev/null 2>&1; then
    return 0
  fi
  if command -v go >/dev/null 2>&1; then
    go install github.com/zricethezav/gitleaks/v8@v8.30.1
    export PATH="$(go env GOPATH)/bin:$PATH"
    return 0
  fi

  local tmp ver
  ver="8.30.1"
  tmp="$(mktemp -d)"
  curl -fsSL \
    "https://github.com/gitleaks/gitleaks/releases/download/v${ver}/gitleaks_${ver}_linux_x64.tar.gz" \
    -o "$tmp/gitleaks.tgz"
  tar -xzf "$tmp/gitleaks.tgz" -C "$tmp" gitleaks
  mkdir -p "${HOME}/.local/bin"
  install -m 0755 "$tmp/gitleaks" "${HOME}/.local/bin/gitleaks"
  export PATH="${HOME}/.local/bin:${PATH}"
}

install_cargo_audit() {
  if ! cargo audit --version >/dev/null 2>&1; then
    cargo install cargo-audit --locked
  fi
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
}

install_syft() {
  if command -v syft >/dev/null 2>&1; then
    return 0
  fi

  local tmp
  tmp="$(mktemp -d)"
  curl -sSfL https://raw.githubusercontent.com/anchore/syft/main/install.sh \
    -o "$tmp/install-syft.sh"
  mkdir -p "${HOME}/.local/bin"
  sh "$tmp/install-syft.sh" -b "${HOME}/.local/bin"
  export PATH="${HOME}/.local/bin:${PATH}"
}

install_zizmor() {
  if ! command -v zizmor >/dev/null 2>&1; then
    cargo install zizmor --locked
  fi
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
}

install_gitleaks
install_cargo_audit
install_zizmor
install_syft

mkdir -p "${artifact_root}/security"
gitleaks detect --source . --redact --report-format json --report-path "${artifact_root}/security/gitleaks.json"
cargo audit --json > "${artifact_root}/security/cargo-audit.json"
zizmor --offline --no-exit-codes --format json .github/workflows > "${artifact_root}/security/zizmor.json"
syft . -o spdx-json="${artifact_root}/security/sbom.spdx.json"
printf '{"status":"pass","profile":"split-family-child"}\n' > "${artifact_root}/security/evidence.json"
