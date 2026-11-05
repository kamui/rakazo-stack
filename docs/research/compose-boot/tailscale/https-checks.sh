#!/usr/bin/env bash
# Requests to the Instance URL as a tailnet device sees it: through tailscaled's own outbound proxy,
# so TLS, Serve and the Host header take the real tailnet path. Secrets are never printed.
set -uo pipefail
D=$(dirname "$0")
set -a; . "$D/stack/.env"; set +a
FQDN=$(cat "$D/.fqdn"); URL="https://$FQDN"
NET=rkz-tstest_app
curl_ts() { docker run --rm --network "$NET" -v "$W:/w" curlimages/curl:8.17.0 -sS -x http://tailscale:1055 "$@"; }
curl_in() { docker run --rm --network "$NET" -v "$W:/w" curlimages/curl:8.17.0 -sS "$@"; }
W=$(mktemp -d); chmod 777 "$W"; touch "$W/jar"; chmod 666 "$W/jar"; trap 'rm -rf "$W"' EXIT
body=$(jq -nc --arg e "$OWNER_EMAIL" --arg p "$OWNER_PASSWORD" '{email:$e,password:$p}')
printf '%s' "$body" > "$W/signin.json"; chmod 644 "$W/signin.json"

echo "== 1. GET $URL/ over the tailnet (certificate verified by curl)"
curl_ts -o /dev/null -w 'status=%{http_code} verify=%{ssl_verify_result} http=%{http_version}\n' "$URL/"
curl_ts -v -o /dev/null "$URL/" 2>&1 | rg -i 'subject:|issuer:|expire date|SSL connection using' | sed 's/^\* *//'

echo "== 2. Host header: Vite only accepts RAKAZO_HOST"
echo -n "   via Serve (Host = ts.net name): "; curl_ts -o /dev/null -w '%{http_code}\n' "$URL/"
echo -n "   direct to web:5173 with Host: web:5173: "; curl_in -o /w/blocked -w '%{http_code} ' http://web:5173/; head -c 120 "$W/blocked"; echo
echo -n "   direct to web:5173 with Host: <ts.net name>: "; curl_in -o /dev/null -w '%{http_code}\n' -H "Host: $FQDN" http://web:5173/

echo "== 3. Sign in over the tailnet"
curl_ts -o /w/si.out -D /w/si.h -c /w/jar -w 'status=%{http_code}\n' -X POST "$URL/api/auth/sign-in/email" \
  -H 'content-type: application/json' -H "Origin: $URL" --data-binary @/w/signin.json
rg -i '^set-cookie' "$W/si.h" | sed -E 's/=([^;]{6})[^;]*/=\1…/; s/\r//'
echo -n "   get-session with the cookie: "; curl_ts -b /w/jar "$URL/api/auth/get-session" | jq -c '{user: .user.email, session: (.session.id != null)}'
for r in voice/status spaces/list; do
  echo -n "   POST /rpc/$r through Serve, with cookie: "; curl_ts -o /w/rpc -w '%{http_code} ' -b /w/jar -X POST "$URL/rpc/$r" -H 'content-type: application/json' -H "Origin: $URL" -d '{"json":{}}'; head -c 100 "$W/rpc"; echo
  echo -n "   POST /rpc/$r through Serve, no cookie: "; curl_ts -o /dev/null -w '%{http_code}\n' -X POST "$URL/rpc/$r" -H 'content-type: application/json' -H "Origin: $URL" -d '{"json":{}}'
done
curl_ts -o /dev/null -b /w/jar -X POST "$URL/api/auth/sign-out" -H 'content-type: application/json' -H "Origin: $URL" -d '{}'

echo "== 4. Localhost fallback: sign in at http://127.0.0.1:$RAKAZO_WEB_PORT"
code=$(curl -sS -o "$W/lo.out" -D "$W/lo.h" -w '%{http_code}' -X POST "http://127.0.0.1:$RAKAZO_WEB_PORT/api/auth/sign-in/email" \
  -H 'content-type: application/json' -H "Origin: http://127.0.0.1:$RAKAZO_WEB_PORT" --data-binary @"$W/signin.json")
echo "   status=$code $(jq -c 'del(.token, .user)' "$W/lo.out" 2>/dev/null || echo '<unparsed body>')"
rg -i '^set-cookie' "$W/lo.h" | sed -E 's/=([^;]{6})[^;]*/=\1…/; s/\r//'
echo -n "   GET / at localhost: "; curl -sS -o "$W/lo.html" -w '%{http_code} ' "http://127.0.0.1:$RAKAZO_WEB_PORT/"; head -c 100 "$W/lo.html"; echo
echo -n "   GET / at localhost with Host: localhost:$RAKAZO_WEB_PORT: "; curl -sS -o "$W/lo2.html" -w '%{http_code} ' "http://localhost:$RAKAZO_WEB_PORT/"; head -c 100 "$W/lo2.html"; echo

echo "== 5. Nothing else on the node: other ports on the ts.net name"
for p in 80 5173 3100 13306; do echo -n "   :$p -> "; curl_ts -o /dev/null -m 8 -w '%{http_code}\n' "http://$FQDN:$p/" 2>&1 | tail -1; done
