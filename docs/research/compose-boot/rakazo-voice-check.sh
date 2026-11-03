#!/usr/bin/env bash
# "Hear a sample" and dictation through Rakazo: the same /api/voice/speak and /api/voice/transcribe
# calls the web app makes (apps/web/src/lib/tts.ts, apps/web/src/lib/dictation.ts at v0.4.0).
# The sample audio is re-encoded to WebM/Opus in a one-off container of the stack's Lemonade image,
# like a browser recording. Env: DC, API, OWNER_EMAIL, OWNER_PASSWORD. Secrets are never printed.
set -euo pipefail
: "${DC:?}" "${API:?}" "${OWNER_EMAIL:?}" "${OWNER_PASSWORD:?}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
token=$(curl -sS -X POST "$API/api/auth/sign-in/email" -H 'content-type: application/json' \
  -d "$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')" | jq -r .token)
auth=(-H "Authorization: Bearer $token")

code=$(curl -sS -o "$work/sample" -D "$work/headers" -w '%{http_code}' -X POST "$API/api/voice/speak" "${auth[@]}" \
  -H 'content-type: application/json' \
  -d '{"text":"Hi, this is how I will sound when I read replies out loud.","scope":"user"}')
echo "POST /api/voice/speak: $code $(grep -i '^content-type' "$work/headers" | tr -d '\r') bytes=$(wc -c <"$work/sample")"
[[ $code == 200 ]] || { head -c 300 "$work/sample"; echo; }

"$DC" run --rm --no-deps -T -e LEMONADE_API_KEY= --entrypoint sh lemonade \
  -c 'ffmpeg -v error -i pipe:0 -c:a libopus -b:a 32k -f webm pipe:1' <"$work/sample" >"$work/sample.webm" 2>/dev/null
echo "WebM: bytes=$(wc -c <"$work/sample.webm")"
jq -n --rawfile b <(base64 -w0 "$work/sample.webm") '{audioBase64:$b, mimeType:"audio/webm;codecs=opus", language:"en"}' >"$work/stt.json"
code=$(curl -sS -o "$work/stt.out" -w '%{http_code}' -X POST "$API/api/voice/transcribe" "${auth[@]}" \
  -H 'content-type: application/json' --data-binary @"$work/stt.json")
echo "POST /api/voice/transcribe: $code $(head -c 300 "$work/stt.out")"

curl -sS -o /dev/null -X POST "$API/api/auth/sign-out" "${auth[@]}" -H 'content-type: application/json' -d '{}'
