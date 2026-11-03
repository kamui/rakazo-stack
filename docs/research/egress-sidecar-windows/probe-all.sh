#!/usr/bin/env bash
# Probes every target from inside the bot computer and replaces machine-specific addresses with placeholders.
set -uo pipefail
export RS27_TOKEN_FILE=$HOME/rs27work/token DC=$HOME/rs27work/mini/dc.sh
P=$HOME/rs27work/computer-probe.sh
bot=${1:-rs27bot}
LAN=$(cat "${RS27_LANIP_FILE:-$HOME/rs27work/lanip}")  # Windows LAN IPv4, kept out of the repo
WSL=$(ip -4 -o addr show eth0 | awk '{split($4,a,"/"); print a[1]}')
TS=$(ip -4 -o addr show tailscale0 2>/dev/null | awk '{split($4,a,"/"); print a[1]}')
ip_of() { docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' "$1" 2>/dev/null | awk '{print $1}'; }
API=$(ip_of rakazo-api-1); WEB=$(ip_of rakazo-web-1); SUP=$(ip_of rs27-mini-supervisor-1); RELAY=$(ip_of rs27-mini-relay-standin-1)
GW=$(docker network inspect -f '{{range .IPAM.Config}}{{.Gateway}} {{end}}' $(docker network ls -q --filter label=rakazo.botId=$bot) | awk '{print $1}')
python3 -m http.server --bind 0.0.0.0 18928 --directory /tmp >/dev/null 2>&1 & WL=$!
sleep 1
$P probe $bot rs27space \
  host.docker.internal:18927 192.168.65.254:18927 \
  host.docker.internal:5173 host.docker.internal:3100 192.168.65.254:5173 192.168.65.254:3100 \
  host.docker.internal:7091 gateway.docker.internal:18927 $GW:22 $GW:18927 \
  169.254.169.254:80 \
  192.168.65.1:3128 http.docker.internal:3128 192.168.65.7:53 gateway.docker.internal:53 vm.docker.internal:22 192.168.65.3:2375 \
  $WSL:18928 $TS:18928 $LAN:445 $LAN:5173 \
  $API:3100 $WEB:5173 $SUP:7091 $RELAY:80 supervisor:7091 \
  1.1.1.1:443 example.com:443 github.com:22 \
  | sed -e "s/$LAN\b/<host LAN IP>/g" -e "s/$WSL\b/<WSL distro IP>/g" -e "s/$TS\b/<WSL tailnet IP>/g" \
        -e "s/$API\b/<live api IP>/g" -e "s/$WEB\b/<live web IP>/g" -e "s/$SUP\b/<test supervisor IP>/g" \
        -e "s/$RELAY\b/<relay stand-in IP>/g" -e "s/$GW\b/<bridge gateway>/g"
kill $WL 2>/dev/null
