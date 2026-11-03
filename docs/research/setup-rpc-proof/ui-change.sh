#!/usr/bin/env bash
# Signs in as the Owner, shows what the RPC read side reports, then changes the default voice
# with voice.setVoice, the call Settings -> Voice makes (apps/web/src/pages/VoiceSettingsOverlay.tsx).
# Usage: ui-change.sh [reads-only]. Env: API OWNER_EMAIL OWNER_PASSWORD.
set -euo pipefail
: "${API:?}" "${OWNER_EMAIL:?}" "${OWNER_PASSWORD:?}"
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
  [[ $status == 2* ]]
}

post /api/auth/sign-in/email "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')"
token=$(jq -r .token "$REPLY_FILE")

post /rpc/voice/credentials '{}' "$token"
post /rpc/voice/status '{}' "$token"
post /rpc/memory/providerConfig '{}' "$token"

if [[ ${1:-} != reads-only ]]; then
  current=$(post /rpc/voice/status '{}' "$token" >/dev/null && jq -r .json.voiceId "$REPLY_FILE")
  post /rpc/voice/voices '{"json":{"provider":"openai-compatible"}}' "$token" >/dev/null
  next=$(jq -r --arg c "$current" '[.json[].id | select(. != $c)][0]' "$REPLY_FILE")
  echo "== voices offered: $(jq -c '[.json[].id]' "$REPLY_FILE" | head -c 300)"
  echo "== change voice $current -> $next (as the UI would)"
  post /rpc/voice/setVoice "$(jq -nc --arg v "$next" '{json:{scope:"account",voiceId:$v,provider:"openai-compatible"}}')" "$token"
fi

post /api/auth/sign-out '{}' "$token" >/dev/null
echo "== signed out"
