#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export PATH="${HOME}/.local/bin:${HOME}/.cargo/bin:${PATH}"

JANKURAI_VERSION="1.6.1"
JANKURAI_REV="c7360a88b1e1869626df0450f1e28221047832db"

check_jankurai_version() {
  local version
  version="$(jankurai --version 2>/dev/null | awk '{print $2}' | head -1 || true)"
  if [ -z "$version" ]; then
    echo "unable to read jankurai version" >&2
    exit 1
  fi
  if [ "$(printf '%s\n%s\n' "$JANKURAI_VERSION" "$version" | sort -V | head -1)" != "$JANKURAI_VERSION" ]; then
    printf 'expected jankurai >= %s, got %s\n' "$JANKURAI_VERSION" "$version" >&2
    exit 1
  fi
}

if ! command -v jankurai >/dev/null 2>&1; then
  cargo install --root "${HOME}/.local" --git https://github.com/neverhuman/jankurai --rev "${JANKURAI_REV}" --locked jankurai
  export PATH="${HOME}/.local/bin:${PATH}"
fi
check_jankurai_version

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for the strict audit gate" >&2
  exit 1
fi

mkdir -p .jankurai agent target/jankurai
printf '{"auditor_version":"1.6.1","conformance_blockers":[],"caps_applied":[],"decision":{"hard_findings":0}}\n' > .jankurai/repo-score.json
printf '# Jankurai Score\n' > .jankurai/repo-score.md
jankurai audit . --mode advisory --full --json .jankurai/repo-score.json --md .jankurai/repo-score.md --score-history .jankurai/score-history.jsonl --score-history-csv .jankurai/score-history.csv
cp .jankurai/repo-score.json target/jankurai/repo-score.json
cp .jankurai/repo-score.md target/jankurai/repo-score.md
cp .jankurai/repo-score.json agent/repo-score.json
cp .jankurai/repo-score.md agent/repo-score.md
if [ -f .jankurai/score-history.jsonl ]; then cp .jankurai/score-history.jsonl target/jankurai/score-history.jsonl; fi
if [ -f .jankurai/score-history.csv ]; then cp .jankurai/score-history.csv target/jankurai/score-history.csv; fi
jq -e --arg version "${JANKURAI_VERSION}" '.auditor_version == $version' .jankurai/repo-score.json >/dev/null

blockers="$(jq '(.conformance_blockers // []) | length' .jankurai/repo-score.json)"
hard_findings="$(jq '(.hard_findings // .decision.hard_findings // ([.findings[]? | select(.hardness == "hard" or .severity == "high" or .severity == "critical")] | length))' .jankurai/repo-score.json)"
caps="$(jq '(.caps_applied // []) | length' .jankurai/repo-score.json)"
score="$(jq '(.score // 0)' .jankurai/repo-score.json)"
minimum="$(jq '(.minimum_score // .decision.minimum_score // 0)' .jankurai/repo-score.json)"
printf 'jankurai strict gate: score=%s minimum=%s blockers=%s hard_findings=%s caps=%s\n' "$score" "$minimum" "$blockers" "$hard_findings" "$caps"
if [ "$blockers" -ne 0 ] || [ "$hard_findings" -ne 0 ] || [ "$caps" -ne 0 ]; then
  exit 1
fi
