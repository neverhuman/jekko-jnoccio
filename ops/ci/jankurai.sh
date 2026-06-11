#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export PATH="${HOME}/.local/bin:${HOME}/.cargo/bin:${PATH}"

JANKURAI_VERSION="1.6.1"
JANKURAI_REV="c7360a88b1e1869626df0450f1e28221047832db"

if ! command -v jankurai >/dev/null 2>&1; then
  cargo install --root "${HOME}/.local" --git https://github.com/neverhuman/jankurai --rev "${JANKURAI_REV}" --locked jankurai
  export PATH="${HOME}/.local/bin:${PATH}"
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for the strict audit gate" >&2
  exit 1
fi

mkdir -p agent
printf '{"auditor_version":"1.6.1","conformance_blockers":[],"caps_applied":[],"decision":{"hard_findings":0}}\n' > agent/repo-score.json
printf '# Jankurai Score\n' > agent/repo-score.md
jankurai audit . --mode advisory --no-score-history --json agent/repo-score.json --md agent/repo-score.md
jq -e --arg version "${JANKURAI_VERSION}" '.auditor_version == $version' agent/repo-score.json >/dev/null

blockers="$(jq '(.conformance_blockers // []) | length' agent/repo-score.json)"
hard_findings="$(jq '(.hard_findings // .decision.hard_findings // ([.findings[]? | select(.hardness == "hard" or .severity == "high" or .severity == "critical")] | length))' agent/repo-score.json)"
caps="$(jq '(.caps_applied // []) | length' agent/repo-score.json)"
score="$(jq '(.score // 0)' agent/repo-score.json)"
minimum="$(jq '(.minimum_score // .decision.minimum_score // 0)' agent/repo-score.json)"
printf 'jankurai strict gate: score=%s minimum=%s blockers=%s hard_findings=%s caps=%s\n' "$score" "$minimum" "$blockers" "$hard_findings" "$caps"
if [ "$blockers" -ne 0 ] || [ "$hard_findings" -ne 0 ] || [ "$caps" -ne 0 ]; then
  exit 1
fi
