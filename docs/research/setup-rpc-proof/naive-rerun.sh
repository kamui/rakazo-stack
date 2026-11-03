#!/usr/bin/env bash
# Counter-example: what a re-run that skips detection and calls both RPCs again does.
# Env: same as fill-in.sh, plus OWNER_PASSWORD.
set -euo pipefail
: "${DC:?}" "${API:?}" "${OWNER_EMAIL:?}" "${OWNER_PASSWORD:?}" "${VOICE_BASE_URL:?}" "${TTS_MODEL:?}" "${STT_MODEL:?}" "${HONCHO_URL:?}"
REPLY_FILE=$(mktemp)
trap 'rm -f "$REPLY_FILE"' EXIT
redact() {
  jq -c 'walk(if type == "object" then with_entries(
      if (.key | test("^(token|password|apiKey|secret|.*Token|.*Secret|cipher.*)$"; "i"))
      then .value = "<redacted>" else . end) else . end)'
}
post() {
  local path=$1 body=$2 bearer=${3:-} status
  local -a auth=()
  [[ -n $bearer ]] && auth=(-H "Authorization: Bearer $bearer")
  echo "-> POST $path $(redact <<<"$body")"
  status=$(curl -sS -o "$REPLY_FILE" -w '%{http_code}' -X POST "$API$path" \
    -H 'Content-Type: application/json' "${auth[@]}" --data-binary @- <<<"$body")
  echo "<- $status $(redact <"$REPLY_FILE" | head -c 1500)"
}
ids() {
  $DC exec -T postgres psql -U rakazo -d rakazo -qAtX -c \
    'select json_build_object($$voice$$, (select json_agg(json_build_object($$secretId$$, c."secretId", $$voiceId$$, p."voiceId")) from user_voice_credentials c join user_voice_preferences p on p."credentialId" = c.id), $$memorySecretId$$, (select "secretId" from space_memory_configs), $$secretRows$$, (select count(*) from secrets))'
}
echo "== before: $(ids)"
post /api/auth/sign-in/email "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')" >/dev/null
token=$(jq -r .token "$REPLY_FILE")
post /rpc/voice/connect "$(jq -nc --arg b "$VOICE_BASE_URL" --arg t "$TTS_MODEL" --arg s "$STT_MODEL" \
  '{json:{provider:"openai-compatible",baseUrl:$b,ttsModel:$t,sttModel:$s}}')" "$token"
honcho_key=$($DC exec -T honcho-api /app/.venv/bin/python scripts/generate_jwt.py --admin --print-only | tr -d '\r\n')
post /rpc/memory/connectProvider "$(jq -nc --arg u "$HONCHO_URL" --arg k "$honcho_key" \
  '{json:{provider:"honcho",settings:{mode:"local",baseUrl:$u},credentials:{apiKey:$k}}}')" "$token"
unset honcho_key
post /api/auth/sign-out '{}' "$token" >/dev/null
echo "== after: $(ids)"
