#!/usr/bin/env bash
# Throwaway test on Docker Desktop for Windows (WSL 2): does Rakazo's restrict-computer-egress.sh,
# applied from a host-network NET_ADMIN container, stop a container on a matching bridge from
# reaching Windows' 127.0.0.1, published host ports, Docker Desktop's endpoints and private ranges?
# Rules use a scratch prefix (rs27t-c) so they can only match this test's bridge.
# Run from a WSL distro with Docker Desktop's WSL integration and Windows interop.
# Usage: rules-test.sh <path-to-rakazo-v0.4.0-restrict-computer-egress.sh>
# Optional: HOST_LAN_IP (Windows' LAN IPv4) adds probes to it; printed as <host LAN IP>.
set -euo pipefail
SRC="$1"
WORK="$(mktemp -d /tmp/rs27rules.XXXXXX)"
PORT=18927      # Windows-side listener, 127.0.0.1 only
WSL_PORT=18928  # listener inside this WSL distro, all addresses
IMG=alpine:3
LABEL=rs27.test=1

sed 's/^BRIDGE_PREFIX="rakazo-c"$/BRIDGE_PREFIX="rs27t-c"/' "$SRC" >"$WORK/egress.sh"
grep -q '^BRIDGE_PREFIX="rs27t-c"$' "$WORK/egress.sh"
mkdir -p "$WORK/www"; echo ok >"$WORK/www/index.html"

# Windows listener: python.exe started detached through PowerShell so its PID can be stopped later.
WIN_PID=$(cd /mnt/c && powershell.exe -NoProfile -Command \
  "\$d = Join-Path \$env:TEMP 'rs27www'; New-Item -ItemType Directory -Force \$d | Out-Null; Set-Content (Join-Path \$d 'index.html') 'ok';
   (Start-Process python.exe -ArgumentList '-m','http.server','--bind','127.0.0.1','$PORT','--directory',\$d -WindowStyle Hidden -PassThru).Id" | tr -d '\r')
python3 -m http.server --bind 0.0.0.0 "$WSL_PORT" --directory "$WORK/www" >/dev/null 2>&1 &
WSL_LISTENER=$!
WSL_IP=$(ip -4 -o addr show eth0 | awk '{split($4, a, "/"); print a[1]}')
TS_IP=$(ip -4 -o addr show tailscale0 2>/dev/null | awk '{split($4, a, "/"); print a[1]}' || true)
API_IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' rakazo-api-1 2>/dev/null | awk '{print $1}' || true)
WEB_IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' rakazo-web-1 2>/dev/null | awk '{print $1}' || true)

cleanup() {
  echo "## cleanup"
  docker network rm rs27-computer rs27-relay >/dev/null 2>&1 || true
  docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW \
    -v "$WORK/egress.sh:/egress.sh:ro" "$IMG" sh -c \
    'apk add -q --no-cache bash iptables ip6tables >/dev/null && bash /egress.sh --remove >/dev/null 2>&1;
     echo "rs27t rules left (v4, any chain or table): $(iptables-save | grep -c rs27t || true)";
     echo "rs27t rules left (v6, any chain or table): $(ip6tables-save | grep -c rs27t || true)"' || true
  (cd /mnt/c && powershell.exe -NoProfile -Command "Stop-Process -Id $WIN_PID -ErrorAction SilentlyContinue; Remove-Item -Recurse -Force (Join-Path \$env:TEMP 'rs27www')") || true
  kill "$WSL_LISTENER" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

docker network create --label "$LABEL" -o com.docker.network.bridge.name=rs27t-c0 rs27-computer >/dev/null
docker network create --label "$LABEL" rs27-relay >/dev/null

# Targets are "label=host:port"; labels keep machine-specific addresses out of the transcript.
targets=(
  "host.docker.internal:$PORT (Windows 127.0.0.1 listener)=host.docker.internal:$PORT"
  "<host.docker.internal IPv4>:$PORT=HDI4:$PORT"
  "host.docker.internal:5173 (published web)=host.docker.internal:5173"
  "host.docker.internal:3100 (published api)=host.docker.internal:3100"
  "<host.docker.internal IPv4>:5173=HDI4:5173"
  "gateway.docker.internal:$PORT=gateway.docker.internal:$PORT"
  "<bridge gateway>:22=GW:22"
  "<bridge gateway>:$PORT=GW:$PORT"
  "169.254.169.254:80 (metadata)=169.254.169.254:80"
  "192.168.65.1:3128 (DD http proxy)=192.168.65.1:3128"
  "http.docker.internal:3128=http.docker.internal:3128"
  "192.168.65.7:53 (DD DNS, tcp)=192.168.65.7:53"
  "<WSL distro eth0>:$WSL_PORT (WSL listener)=$WSL_IP:$WSL_PORT"
  "1.1.1.1:443 (internet)=1.1.1.1:443"
  "example.com:443 (DNS + internet)=example.com:443"
  "github.com:22 (SSH out)=github.com:22"
)
[[ -n $TS_IP ]] && targets+=("<WSL tailnet IP>:$WSL_PORT=$TS_IP:$WSL_PORT")
[[ -n $API_IP ]] && targets+=("<live api container IP>:3100=$API_IP:3100")
[[ -n $WEB_IP ]] && targets+=("<live web container IP>:5173=$WEB_IP:5173")
[[ -n ${HOST_LAN_IP:-} ]] && targets+=("<host LAN IP>:5173=$HOST_LAN_IP:5173" "<host LAN IP>:445=$HOST_LAN_IP:445")

probe() {
  local net="$1"
  docker run --rm --network "$net" "$IMG" sh -c '
    gw=$(ip route | awk "/default/ {print \$3}")
    hdi4=$(getent ahostsv4 host.docker.internal | awk "NR==1{print \$1}")
    echo "  host.docker.internal -> $(getent hosts host.docker.internal | awk "{print \$1}" | tr "\n" " ")"
    for t in "$@"; do
      label=${t%%=*}; hp=${t#*=}; h=${hp%:*}; p=${hp##*:}
      [ "$h" = HDI4 ] && h=$hdi4; [ "$h" = GW ] && h=$gw
      s=$(date +%s)
      if timeout 6 nc -z -w 4 "$h" "$p" >/dev/null 2>&1; then r=open
      elif [ $(( $(date +%s) - s )) -ge 3 ]; then r=timeout
      else r=refused; fi
      printf "  %-52s %s\n" "$label" "$r"
    done
    s=$(date +%s); if nslookup -timeout=3 example.com >/dev/null 2>&1; then r=ok; else r=failed; fi
    printf "  %-52s %s\n" "DNS lookup example.com (embedded DNS)" "$r"
  ' probe "${targets[@]}"
}

fw() {
  docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW "$IMG" sh -c \
    'apk add -q --no-cache iptables ip6tables >/dev/null; echo "iptables: $(iptables -V)";
     echo "DOCKER-USER v4:"; iptables -S DOCKER-USER; echo "DOCKER-USER v6:"; ip6tables -S DOCKER-USER;
     echo "INPUT v4:"; iptables -S INPUT'
}

echo "## Docker: $(docker version --format 'Desktop {{.Server.Platform.Name}} / Engine {{.Server.Version}} API {{.Server.APIVersion}}')"
echo "## WSL networking mode: $(wslinfo --networking-mode 2>/dev/null || echo unknown)"
echo "## VM firewall before (host-network container)"; fw

echo "## Before rules: computer-like network (bridge rs27t-c0)"; probe rs27-computer
echo "## Before rules: relay-like network (br-*)"; probe rs27-relay

echo "## Apply rules (restrict-computer-egress.sh --apply, prefix rs27t-c)"
docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW \
  -v "$WORK/egress.sh:/egress.sh:ro" "$IMG" sh -c \
  'apk add -q --no-cache bash iptables ip6tables >/dev/null && bash /egress.sh --apply && echo applied'
echo "## VM firewall after"; fw

echo "## After rules: computer-like network (bridge rs27t-c0)"; probe rs27-computer
echo "## After rules: relay-like network (br-*)"; probe rs27-relay
