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

mkdir -p .jankurai target/jankurai agent
args=(audit . --mode advisory --full --json .jankurai/repo-score.json --md .jankurai/repo-score.md --score-history .jankurai/score-history.jsonl --score-history-csv .jankurai/score-history.csv)
if [[ "${1:-}" == "--fast" ]]; then
  args=(audit . --mode advisory --full --json .jankurai/repo-score.json --md .jankurai/repo-score.md --score-history .jankurai/score-history.jsonl --score-history-csv .jankurai/score-history.csv)
fi
jankurai "${args[@]}"

cp .jankurai/repo-score.json target/jankurai/repo-score.json
cp .jankurai/repo-score.md target/jankurai/repo-score.md
cp .jankurai/repo-score.json agent/repo-score.json
cp .jankurai/repo-score.md agent/repo-score.md
for history_file in score-history.jsonl score-history.csv; do
  if [[ -f ".jankurai/${history_file}" ]]; then
    cp ".jankurai/${history_file}" "target/jankurai/${history_file}"
  fi
done

version="$(jq -r '(.auditor_version // .jankurai_version // empty)' .jankurai/repo-score.json)"
if [[ -n "$version" && "$version" != "$JANKURAI_VERSION" ]]; then
  echo "warning: expected jankurai auditor ${JANKURAI_VERSION}, got report schema ${version}" >&2
fi
blockers="$(jq '(.conformance_blockers // []) | length' .jankurai/repo-score.json)"
hard_findings="$(jq '(.hard_findings // .decision.hard_findings // ([.findings[]? | select(.hardness == "hard" or .severity == "high" or .severity == "critical")] | length))' .jankurai/repo-score.json)"
caps="$(jq '(.caps_applied // []) | length' .jankurai/repo-score.json)"
score="$(jq '(.score // 0)' .jankurai/repo-score.json)"
minimum="$(jq '(.minimum_score // .decision.minimum_score // 0)' .jankurai/repo-score.json)"
printf 'jankurai strict gate: score=%s minimum=%s blockers=%s hard_findings=%s caps=%s\n' "$score" "$minimum" "$blockers" "$hard_findings" "$caps"
if [[ "$blockers" -ne 0 || "$hard_findings" -ne 0 || "$caps" -ne 0 ]]; then
  exit 1
fi
