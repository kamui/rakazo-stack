#!/usr/bin/env bash
# Throwaway test: does Rakazo's restrict-computer-egress.sh, applied inside the
# Docker Desktop VM from a host-network NET_ADMIN container, stop a container on a
# matching bridge from reaching a listener on the Mac's 127.0.0.1?
# Rules use a scratch prefix (rs25t-c) so they can only match this test's bridge.
# Usage: vm-egress-test.sh <path-to-rakazo-v0.4.0-restrict-computer-egress.sh>
set -euo pipefail
SRC="$1"
WORK="$(mktemp -d /tmp/rs25exp.XXXXXX)"
PORT=18925
IMG=alpine:3
LABEL=rs25.test=1

sed 's/^BRIDGE_PREFIX="rakazo-c"$/BRIDGE_PREFIX="rs25t-c"/' "$SRC" >"$WORK/egress.sh"
grep -q '^BRIDGE_PREFIX="rs25t-c"$' "$WORK/egress.sh"
mkdir -p "$WORK/www"; echo ok >"$WORK/www/index.html"

python3 -m http.server --bind 127.0.0.1 "$PORT" --directory "$WORK/www" >/dev/null 2>&1 &
LISTENER=$!

cleanup() {
  echo "## cleanup"
  docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW \
    -v "$WORK/egress.sh:/egress.sh:ro" "$IMG" sh -c \
    'apk add -q --no-cache bash iptables ip6tables >/dev/null && bash /egress.sh --remove >/dev/null 2>&1;
     echo "rules left (v4):"; iptables -S DOCKER-USER | grep -c rs25t || true;
     echo "rules left (v6):"; ip6tables -S DOCKER-USER 2>/dev/null | grep -c rs25t || true' || true
  docker network rm rs25-computer rs25-relay >/dev/null 2>&1 || true
  kill "$LISTENER" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

docker network create --label "$LABEL" -o com.docker.network.bridge.name=rs25t-c0 rs25-computer >/dev/null
docker network create --label "$LABEL" rs25-relay >/dev/null

probe() {
  local net="$1"
  docker run --rm --network "$net" "$IMG" sh -c '
    PORT='"$PORT"'
    t() { s=$(date +%s); if timeout 5 "$@" >/dev/null 2>&1; then r=open; else r="blocked/failed"; fi; echo "$r ($(( $(date +%s) - s ))s)"; }
    gw=$(ip route | awk "/default/ {print \$3}")
    hdi4=$(getent ahostsv4 host.docker.internal | awk "NR==1{print \$1}")
    echo "  host.docker.internal -> $(getent hosts host.docker.internal | awk "{print \$1}" | tr "\n" " ")"
    echo "  http://host.docker.internal:$PORT (Mac 127.0.0.1 listener): $(t wget -q -T 4 -O- http://host.docker.internal:$PORT/)"
    echo "  http://$hdi4:$PORT (by IPv4): $(t wget -q -T 4 -O- http://$hdi4:$PORT/)"
    echo "  bridge gateway $gw:22: $(t nc -z -w 3 $gw 22)"
    echo "  1.1.1.1:443 (internet): $(t nc -z -w 3 1.1.1.1 443)"
    echo "  example.com:443 (DNS + internet): $(t nc -z -w 3 example.com 443)"
    echo "  ipv6 addrs: $(ip -6 addr show scope global | awk "/inet6/ {print \$2}" | tr "\n" " ")"
  '
}

echo "## Docker: $(docker version --format '{{.Server.Version}} API {{.Server.APIVersion}}')"
echo "## VM firewall before (host-network container)"
docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW "$IMG" sh -c \
  'apk add -q --no-cache iptables ip6tables >/dev/null;
   echo "iptables: $(iptables -V)"; iptables -S DOCKER-USER; echo "ip6tables:"; ip6tables -S DOCKER-USER 2>&1 | head -3;
   echo "legacy tables present: $(iptables-legacy -S 2>/dev/null | wc -l) lines";
   echo "global IPv6 in VM: $(awk "\$4==\"00\"" /proc/net/if_inet6 | wc -l)"'

echo "## Before rules: computer-like network (bridge rs25t-c0)"; probe rs25-computer
echo "## Before rules: relay-like network (br-*)"; probe rs25-relay

echo "## Apply rules (restrict-computer-egress.sh --apply, prefix rs25t-c)"
docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW \
  -v "$WORK/egress.sh:/egress.sh:ro" "$IMG" sh -c \
  'apk add -q --no-cache bash iptables ip6tables >/dev/null && bash /egress.sh --apply && echo applied;
   iptables -S DOCKER-USER | grep rs25t | head -4; echo ...; ip6tables -S DOCKER-USER | grep rs25t | head -3'

echo "## After rules: computer-like network (bridge rs25t-c0)"; probe rs25-computer
echo "## After rules: relay-like network (br-*)"; probe rs25-relay
