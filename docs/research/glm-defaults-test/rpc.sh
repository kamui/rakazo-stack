#!/usr/bin/env bash
# rpc.sh <procedure path> [json input]   e.g. rpc.sh bots/list
# Signs in once (token cached in .token) and posts {"json": input} to /rpc/<path>.
set -euo pipefail
cd "$(dirname "$0")"
API=http://127.0.0.1:13900
set -a; . ./.env; set +a
if [[ ! -s .token ]]; then
  umask 077
  curl -sS -X POST "$API/api/auth/sign-in/email" -H 'Content-Type: application/json' \
    -d "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')" | jq -r .token >.token
fi
input=${2:-'{}'}
curl -sS -X POST "$API/rpc/$1" -H 'Content-Type: application/json' -H "Authorization: Bearer $(cat .token)" \
  -d "{\"json\":$input}"
