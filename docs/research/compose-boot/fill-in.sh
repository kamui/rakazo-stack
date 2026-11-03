#!/usr/bin/env bash
# Adapted from docs/research/setup-rpc-proof/fill-in.sh (branch research/setup-rpc-proof): Honcho service name is a variable.
# Proof of setup's "fill in only when missing" step against Rakazo v0.4.0.
#
# 1. Detect, without the Owner password: read-only SQL as the database owner.
# 2. Missing Owner: bootstrap-owner (env only; it ignores argv).
# 3. Missing voice or memory: sign in once, call the RPCs the web UI calls, sign out.
#
# Env:
#   DC              command that runs `docker compose` for the stack (e.g. a wrapper script)
#   API             Rakazo API base, e.g. http://127.0.0.1:3100
#   OWNER_EMAIL     Owner email
#   OWNER_PASSWORD  read only when something is missing (a real setup prompts for it here)
#   VOICE_BASE_URL  relay URL as seen from the api container, e.g. http://host.docker.internal:13306/api/v1
#   TTS_MODEL STT_MODEL
#   HONCHO_URL      Honcho as seen from the api container, e.g. http://honcho:8000
#   HONCHO_SERVICE  compose service running Honcho (default: honcho)
# Secrets are never printed: request and response bodies pass through redact().
set -euo pipefail
: "${DC:?}" "${API:?}" "${OWNER_EMAIL:?}" "${VOICE_BASE_URL:?}" "${TTS_MODEL:?}" "${STT_MODEL:?}" "${HONCHO_URL:?}"

dc() { $DC "$@"; }
sql() { dc exec -T postgres psql -U rakazo -d rakazo -qAtX -v ON_ERROR_STOP=1 -v email="$OWNER_EMAIL" "$@"; }
redact() {
  jq -c 'walk(if type == "object" then with_entries(
      if (.key | test("^(token|password|apiKey|secret|.*Token|.*Secret|cipher.*)$"; "i"))
      then .value = "<redacted>" else . end) else . end)'
}

detect() {
  sql <<'SQL'
\set QUIET on
with owner as (
  select u.id from "user" u
  join account a on a."userId" = u.id and a."providerId" = 'credential'
  where u.email = lower(:'email') and u."emailVerified"
), default_space as (
  select s.id from space_members m join spaces s on s.id = m."spaceId"
  where m."userId" = (select id from owner) and m.role = 'owner' and s."deletingAt" is null
  order by s."isDefault" desc, s."createdAt" asc limit 1
)
select json_build_object(
  'owner', exists(select 1 from owner),
  'defaultSpace', exists(select 1 from default_space),
  'voiceCredentials', (select coalesce(json_agg(provider order by provider), '[]')
                       from user_voice_credentials where "userId" = (select id from owner)),
  'memoryProvider', (select provider from space_memory_configs
                     where "spaceId" = (select id from default_space)));
SQL
}

snapshot() {
  sql <<'SQL'
\set QUIET on
select json_build_object(
  'user', (select json_agg(json_build_object('email', email, 'updatedAt', "updatedAt")) from "user"),
  'voiceCredentials', (select json_agg(json_build_object('provider', provider, 'secretId', "secretId", 'updatedAt', "updatedAt")) from user_voice_credentials),
  'voicePreferences', (select json_agg(json_build_object('voiceId', "voiceId", 'isDefault', "isDefault", 'updatedAt', "updatedAt")) from user_voice_preferences),
  'spaceVoicePreferences', (select json_agg(json_build_object('voiceId', "voiceId", 'isDefault', "isDefault", 'updatedAt', "updatedAt")) from space_voice_preferences),
  'memoryConfigs', (select json_agg(json_build_object('provider', provider, 'settings', settings, 'secretId', "secretId", 'updatedAt', "updatedAt")) from space_memory_configs),
  'secrets', (select json_agg(json_build_object('id', id, 'kind', kind, 'createdAt', "createdAt") order by "createdAt") from secrets));
SQL
}

# post <path> <json-body> [bearer]: prints the redacted exchange; the response body goes to $REPLY_FILE.
REPLY_FILE=$(mktemp)
trap 'rm -f "$REPLY_FILE"' EXIT
post() {
  local path=$1 body=$2 bearer=${3:-} status
  echo "-> POST $path"
  echo "   body: $(redact <<<"$body")"
  local -a auth=()
  [[ -n $bearer ]] && auth=(-H "Authorization: Bearer $bearer")
  status=$(curl -sS -o "$REPLY_FILE" -w '%{http_code}' -X POST "$API$path" \
    -H 'Content-Type: application/json' "${auth[@]}" --data-binary @- <<<"$body")
  echo "<- $status $( (redact <"$REPLY_FILE") 2>/dev/null || head -c 400 "$REPLY_FILE")"
  [[ $status == 2* ]]
}

echo "== detect (read-only SQL, no password)"
state=$(detect)
echo "   $state"
need_owner=$(jq -r '.owner | not' <<<"$state")
need_voice=$(jq -r '.voiceCredentials | length == 0' <<<"$state")
need_memory=$(jq -r '.memoryProvider == null' <<<"$state")
echo "   missing: owner=$need_owner voice=$need_voice memory=$need_memory"
echo "== snapshot before"
echo "   $(snapshot)"

if [[ $need_owner == false && $need_voice == false && $need_memory == false ]]; then
  echo "== nothing missing: no password asked, nothing written"
  exit 0
fi
: "${OWNER_PASSWORD:?something is missing, so the Owner password is needed}"

if [[ $need_owner == true ]]; then
  echo "== bootstrap-owner"
  OWNER_EMAIL=$OWNER_EMAIL OWNER_PASSWORD=$OWNER_PASSWORD \
    dc exec -T -e OWNER_EMAIL -e OWNER_PASSWORD api node dist/cli/bootstrap-owner.js
  echo "   exit $?"
fi

echo "== sign in"
post /api/auth/sign-in/email "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')"
token=$(jq -r .token "$REPLY_FILE")

if [[ $need_voice == true ]]; then
  echo "== voice.connect"
  post /rpc/voice/connect "$(jq -nc --arg b "$VOICE_BASE_URL" --arg t "$TTS_MODEL" --arg s "$STT_MODEL" \
    '{json:{provider:"openai-compatible",baseUrl:$b,ttsModel:$t,sttModel:$s}}')" "$token" ||
    echo "   voice: left for the checklist"
fi

if [[ $need_memory == true ]]; then
  echo "== memory.connectProvider"
  honcho_key=$(dc exec -T "${HONCHO_SERVICE:-honcho}" /app/.venv/bin/python scripts/generate_jwt.py --admin --print-only | tr -d '\r\n')
  post /rpc/memory/connectProvider "$(jq -nc --arg u "$HONCHO_URL" --arg k "$honcho_key" \
    '{json:{provider:"honcho",settings:{mode:"local",baseUrl:$u},credentials:{apiKey:$k}}}')" "$token" ||
    echo "   memory: left for the checklist"
  unset honcho_key
fi

echo "== sign out"
post /api/auth/sign-out '{}' "$token" || true
unset token

echo "== detect after"
echo "   $(detect)"
echo "== snapshot after"
echo "   $(snapshot)"
