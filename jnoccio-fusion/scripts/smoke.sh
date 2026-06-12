#!/usr/bin/env bash
set -euo pipefail

base_url="${JNOCCIO_BASE_URL:-http://127.0.0.1:4317}"
out_dir="${JNOCCIO_SMOKE_OUT:-receipts/smoke}"
mkdir -p "$out_dir"

curl -fsS "$base_url/health" | tee "$out_dir/health.json" >/dev/null
curl -fsS "$base_url/v1/models" | tee "$out_dir/models.json" >/dev/null
curl -fsS "$base_url/v1/jnoccio/status" | tee "$out_dir/status.json" >/dev/null

chat_body='{
  "model": "jnoccio/jnoccio-fusion",
  "messages": [
    {
      "role": "user",
      "content": "Return a one-sentence smoke test acknowledgement."
    }
  ],
  "stream": false
}'

chat_status=$(curl -sS -o "$out_dir/chat.json" -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d "$chat_body" \
  "$base_url/v1/chat/completions" || true)

printf '%s\n' "$chat_status" > "$out_dir/chat.status"
