#!/usr/bin/env bash
# Setup's memory path against Rakazo v0.16.0 + Hindsight 1.3.0, as #11/#21 proved it for Honcho:
#   1. detect with read-only SQL (no password)
#   2. missing Owner -> bootstrap-owner (env only)
#   3. missing memory -> sign in, POST /rpc/memory/connectProvider {provider:"hindsight"}, sign out
#   4. detect again
# Secrets never printed: bodies pass through redact(). Run from this directory.
set -euo pipefail
cd "$(dirname "$0")"
set -a; . ./.env; set +a
API=http://127.0.0.1:$RAKAZO_API_PORT
DC=./dc.sh
sql() { $DC exec -T postgres psql -U rakazo -d rakazo -qAtX -v ON_ERROR_STOP=1 -v email="$OWNER_EMAIL" "$@"; }
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
  'hindsight', exists(select 1 from space_memory_configs
                      where "spaceId" = (select id from default_space) and provider = 'hindsight'),
  'aiConsents', (select coalesce(json_agg("recipientKey" order by "recipientKey"), '[]')
                 from ai_data_consents where "userId" = (select id from owner)));
SQL
}

REPLY_FILE=$(mktemp)
trap 'rm -f "$REPLY_FILE"' EXIT
post() { # post <path> <json-body> [bearer]
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
state=$(detect); echo "   $state"
need_owner=$(jq -r '.owner | not' <<<"$state")
need_memory=$(jq -r '.hindsight | not' <<<"$state")
if [[ $need_owner == false && $need_memory == false ]]; then echo "== nothing missing"; exit 0; fi

if [[ $need_owner == true ]]; then
  echo "== bootstrap-owner"
  $DC exec -T -e OWNER_EMAIL -e OWNER_PASSWORD api node dist/cli/bootstrap-owner.js
fi

echo "== sign in"
post /api/auth/sign-in/email "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')"
token=$(jq -r .token "$REPLY_FILE")

echo "== memory.connectProvider"
post /rpc/memory/connectProvider "$(jq -nc --arg k "$HINDSIGHT_TENANT_API_KEY" \
  '{json:{provider:"hindsight",settings:{mode:"local",baseUrl:"http://hindsight:8888"},credentials:{apiKey:$k}}}')" "$token" ||
  echo "   memory: left for the checklist"

echo "== sign out"
post /api/auth/sign-out '{}' "$token" || true
unset token

echo "== detect after"
echo "   $(detect)"
