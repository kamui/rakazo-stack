#!/usr/bin/env bash
set -euo pipefail
R=$(mktemp); trap 'rm -f $R' EXIT
p() { local s; s=$(curl -sS -o $R -w '%{http_code}' -X POST "$API$1" -H 'Content-Type: application/json' ${3:+-H "Authorization: Bearer $3"} --data-binary "$2"); echo "-> $1 $(jq -c 'walk(if type=="object" then with_entries(if (.key|test("^(token|password|apiKey)$")) then .value="<redacted>" else . end) else . end)' <<<"$2")"; echo "<- $s $(head -c 700 $R)"; }
echo "== no token"; p /rpc/voice/status '{}'
p /api/auth/sign-in/email "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')" >/dev/null; t=$(jq -r .token $R)
echo "== token shape: length=${#t} dots=$(tr -cd . <<<"$t" | wc -c)"
echo "== voice server unreachable"; p /rpc/voice/connect '{"json":{"provider":"openai-compatible","baseUrl":"http://host.docker.internal:1/api/v1","ttsModel":"kokoro-v1","sttModel":"Whisper-Large-v3-Turbo"}}' "$t"
echo "== honcho key rejected"; p /rpc/memory/connectProvider '{"json":{"provider":"honcho","settings":{"mode":"local","baseUrl":"http://honcho-api:8000"},"credentials":{"apiKey":"not-a-valid-jwt-123"}}}' "$t"
echo "== UI disconnect of voice"; p /rpc/voice/disconnect '{"json":{"provider":"openai-compatible"}}' "$t"
p /api/auth/sign-out '{}' "$t" >/dev/null
