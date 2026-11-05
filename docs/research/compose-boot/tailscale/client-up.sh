#!/usr/bin/env bash
# A second ephemeral tailnet node with an outbound HTTP proxy, so curl can act as a tailnet device.
# Usage: client-up.sh <auth-key-file>
set -uo pipefail
docker network create rkz-tstest-cnet >/dev/null && echo "network rkz-tstest-cnet created"
TS_AUTHKEY="$(cat "$1")" docker run -d --name rkz-tstest-client --network rkz-tstest-cnet \
  -e TS_AUTHKEY -e TS_HOSTNAME=rkz-test-client -e TS_USERSPACE=true -e TS_OUTBOUND_HTTP_PROXY_LISTEN=0.0.0.0:1055 \
  tailscale/tailscale:v1.104.4 >/dev/null && echo "client started"
for i in $(seq 1 40); do
  st=$(docker exec rkz-tstest-client tailscale status --json 2>/dev/null | jq -r .BackendState)
  [[ $st == Running ]] && break; sleep 2
done
echo "client backend: $st"
docker exec rkz-tstest-client tailscale status 2>&1 | awk '{print $2}' | head -5
