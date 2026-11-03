#!/usr/bin/env bash
# Linux Docker Engine half of #27, run as root on a throwaway GitHub-hosted Ubuntu runner (it changes the host firewall).
# Does Rakazo v0.4.0's unmodified restrict-computer-egress.sh, run from a host-network NET_ADMIN sidecar with
# SANDBOX_COMPUTER_EGRESS=restricted, isolate a real bot computer, and how does it behave per firewall setup?
# Usage: SANDBOX_SUPERVISOR_TOKEN=<random> linux-test.sh <nft|legacy|ufw|firewalld|docker-nftables>
# Expects mini/restrict-computer-egress.sh (v0.4.0) and the app/computer images already present.
set -uo pipefail
SCENARIO=$1
HERE=$(cd "$(dirname "$0")" && pwd)
export DC=$HERE/dc.sh
P=$HERE/computer-probe.sh
SCRIPT=$HERE/mini/restrict-computer-egress.sh
COMPUTER_IMAGE=ghcr.io/elie222/rakazo/computer:v0.4.0
: "${SANDBOX_SUPERVISOR_TOKEN:?}"
HOSTIP=$(ip -4 route get 1.1.1.1 | awk '{for (i = 1; i < NF; i++) if ($i == "src") { print $(i + 1); exit }}')

step() { printf '\n## %s  [%s]\n' "$*" "$(date -u +%T)"; }
now() { date -u +%Y-%m-%dT%H:%M:%SZ; }
# Drops the computer image's harmless NWRAP (nss_wrapper) stderr lines under docker exec.
redact() { grep -v '^NWRAP_ERROR' | sed -e "s/${HOSTIP:-<none>}/<host IP>/g"; }
rules() { echo "  rakazo-c+ rules: v4 $(iptables-save 2>/dev/null | grep -c 'rakazo-c+') (INPUT $(iptables -S INPUT 2>/dev/null | grep -c 'rakazo-c+')), v6 $(ip6tables-save 2>/dev/null | grep -c 'rakazo-c+')"; }
wait_rules() {
  local i
  for i in $(seq "${1:-60}"); do [[ $(iptables-save 2>/dev/null | grep -c 'rakazo-c+') -ge 14 ]] && break; sleep 2; done
  rules
}
docker_up() { until docker info >/dev/null 2>&1; do sleep 1; done; }
docker_restart() { systemctl restart docker; docker_up; }
gw_of() { docker inspect -f '{{range .NetworkSettings.Networks}}{{.Gateway}} {{end}}' "$1" | awk '{print $1}'; }
ip_of() { docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' "$1" | awk '{print $1}'; }
verdict() {
  local since=$1 l i
  for i in $(seq 120); do
    l=$(docker logs --since "$since" rs27l-supervisor-1 2>&1 | grep -oE 'restricted computer egress (verified|unavailable)' | tail -1)
    [[ -n $l ]] && { echo "  supervisor log: $l"; return 0; }
    sleep 2
  done
  echo "  supervisor log: no egress verdict within 240 s"
}
states() {
  local c
  for c in "$@"; do printf '  %-28s %s\n' "$c" "$(docker inspect -f '{{.State.Status}} exit={{.State.ExitCode}} restarts={{.RestartCount}} started={{.State.StartedAt}}' "$c" 2>&1 | cut -c1-110)"; done
}
create() { echo "  create $1 -> $("$P" create "$1" rs27space 2>&1 | cut -c1-300)"; }
bridge_of() {
  local n
  n=$(docker network ls -q --filter "label=rakazo.botId=$1")
  [[ -n $n ]] && echo "  $1 network bridge name: $(docker network inspect -f '{{index .Options "com.docker.network.bridge.name"}}' "$n")"
}

# TCP connect from inside a container (bash /dev/tcp, 4 s): open, timeout (dropped) or refused/unresolved.
PROBE='for t in "$@"; do label=${t%%=*}; hp=${t#*=}; h=${hp%:*}; p=${hp##*:}
  if timeout 4 bash -c "exec 3<>/dev/tcp/$h/$p" 2>/dev/null; then r=open
  elif [ $? -eq 124 ]; then r=timeout; else r=refused/unresolved; fi
  printf "  %-52s %s\n" "$label" "$r"; done'
targets_full() {
  local gw
  gw=$(gw_of "$1")
  printf '%s\n' \
    "host listener 0.0.0.0:18928 via bridge gateway=$gw:18928" \
    "host listener 127.0.0.1:18927 via bridge gateway=$gw:18927" \
    "host listener 0.0.0.0:18928 via docker0 172.17.0.1=172.17.0.1:18928" \
    "host listener 0.0.0.0:18928 via <host IP>=$HOSTIP:18928" \
    "published 127.0.0.1:3100 via <host IP>=$HOSTIP:3100" \
    "published 127.0.0.1:5173 via bridge gateway=$gw:5173" \
    "published 0.0.0.0:8080 via <host IP>=$HOSTIP:8080" \
    "published 0.0.0.0:8080 via bridge gateway=$gw:8080" \
    "pubapp container by IP :3100=$(ip_of rs27l-pubapp-1):3100" \
    "supervisor container by IP :7091=$(ip_of rs27l-supervisor-1):7091" \
    "169.254.169.254:80 (cloud metadata)=169.254.169.254:80" \
    "168.63.129.16:80 (Azure WireServer, public range)=168.63.129.16:80" \
    "1.1.1.1:443 (internet)=1.1.1.1:443" \
    "example.com:443 (DNS + internet)=example.com:443" \
    "github.com:22 (SSH out)=github.com:22"
}
targets_short() {
  local gw
  gw=$(gw_of "$1")
  printf '%s\n' \
    "host listener 0.0.0.0:18928 via bridge gateway=$gw:18928" \
    "published 0.0.0.0:8080 via <host IP>=$HOSTIP:8080" \
    "169.254.169.254:80 (cloud metadata)=169.254.169.254:80" \
    "1.1.1.1:443 (internet)=1.1.1.1:443" \
    "example.com:443 (DNS + internet)=example.com:443"
}
probe() {
  local c=$1 kind=${2:-full} t
  mapfile -t t < <("targets_$kind" "$c")
  docker exec "$c" bash -c "$PROBE" probe "${t[@]}" 2>&1 | redact
  [[ $kind == full ]] && echo "  DNS lookup example.com (embedded DNS): $(docker exec "$c" getent hosts example.com >/dev/null 2>&1 && echo ok || echo failed)"
}
relay() {
  local gw
  gw=$(gw_of rs27l-relay-standin-1)
  docker exec rs27l-relay-standin-1 sh -c "for t in $gw:18928 pubapp:3100 supervisor:7091 1.1.1.1:443; do printf '  relay-standin -> %-24s ' \$t; nc -z -w 4 \${t%:*} \${t#*:} && echo open || echo fail; done" |
    sed "s/$gw:/<bridge gateway>:/" | redact
}
up_stack() {
  local since
  since=$(now)
  if "$DC" up -d --build --wait --wait-timeout 150 computer-egress >/dev/null 2>&1; then echo "  sidecar healthy"; else echo "  sidecar NOT healthy"; fi
  echo "  sidecar log tail: $(docker logs --tail 4 rs27l-computer-egress-1 2>&1 | grep -v '^OK:' | tr '\n' '|' | cut -c1-300)"
  "$DC" up --no-deps data-init >/dev/null 2>&1
  "$DC" up -d --no-deps --wait --wait-timeout 300 supervisor relay-standin web pubapp >/dev/null 2>&1 || echo "  (up --wait for supervisor/stand-ins reported a problem)"
  rules
  verdict "$since"
}

step "Environment: scenario $SCENARIO"
. /etc/os-release
echo "  $PRETTY_NAME, kernel $(uname -r), egress interface $(ip -4 route get 1.1.1.1 | awk '{for (i = 1; i < NF; i++) if ($i == "dev") { print $(i + 1); exit }}')"
echo "  Docker Engine $(docker version --format '{{.Server.Version}}'), compose $(docker compose version --short)"
echo "  host iptables: $(iptables -V) (alternative: $(update-alternatives --query iptables 2>/dev/null | awk '/^Value:/{print $2}'))"
echo "  ufw: $(ufw status 2>&1 | head -1); firewalld: $(systemctl is-active firewalld 2>/dev/null)"
echo "  host global IPv6 addresses: $(awk '$4 == "00"' /proc/net/if_inet6 | wc -l)"
echo "  script sha256: $(sha256sum "$SCRIPT" | cut -c1-64)"

step "Scenario setup: $SCENARIO"
case $SCENARIO in
  nft) echo "  no changes (Ubuntu default: iptables-nft, ufw inactive)" ;;
  legacy)
    update-alternatives --set iptables /usr/sbin/iptables-legacy
    update-alternatives --set ip6tables /usr/sbin/ip6tables-legacy
    systemctl stop docker docker.socket; nft flush ruleset; systemctl start docker; docker_up
    echo "  host iptables now: $(iptables -V)"
    echo "  DOCKER-USER in legacy tables: $(iptables-legacy -S DOCKER-USER 2>&1 | head -1); in nf_tables: $(iptables-nft -S DOCKER-USER 2>&1 | head -1)"
    ;;
  ufw)
    ufw --force enable >/dev/null
    echo "  $(ufw status verbose | grep -E '^(Status|Default)' | tr '\n' ' ')"
    ;;
  firewalld)
    export DEBIAN_FRONTEND=noninteractive
    apt-get -qq update >/dev/null && apt-get -qq install -y firewalld >/dev/null
    systemctl enable --now firewalld >/dev/null 2>&1
    docker_restart
    echo "  firewalld $(firewall-cmd --version): $(firewall-cmd --state); backend $(awk -F= '/^FirewallBackend/{print $2}' /etc/firewalld/firewalld.conf)"
    echo "  active zones: $(firewall-cmd --get-active-zones | tr '\n' ' ')"
    ;;
  docker-nftables)
    f=/etc/docker/daemon.json
    [[ -s $f ]] || echo '{}' >"$f"
    jq '. + {"firewall-backend": "nftables"}' "$f" >/tmp/daemon.json && cp /tmp/daemon.json "$f"
    systemctl stop docker docker.socket; nft flush ruleset
    if ! timeout 90 bash -c 'systemctl start docker && until docker info >/dev/null 2>&1; do sleep 1; done'; then
      echo "  dockerd did not start with firewall-backend nftables:"; journalctl -u docker --no-pager -n 15 | sed 's/^/    /'
      exit 0
    fi
    echo "  $(docker info 2>/dev/null | grep -i 'firewall' | xargs)"
    echo "  iptables DOCKER-USER: $(iptables -S DOCKER-USER 2>&1 | head -1)"
    echo "  nft tables: $(nft list tables | tr '\n' ' ')"
    ;;
  *) echo "unknown scenario $SCENARIO" >&2; exit 2 ;;
esac
echo "  Docker firewall: $(docker info 2>/dev/null | grep -i 'firewall backend' | xargs || true)"

step "Listeners and baseline container"
mkdir -p /tmp/rs27www && echo ok >/tmp/rs27www/index.html
(cd /tmp/rs27www && exec python3 -m http.server --bind 127.0.0.1 18927 >/dev/null 2>&1) &
(cd /tmp/rs27www && exec python3 -m http.server --bind 0.0.0.0 18928 >/dev/null 2>&1) &
docker network create rs27-plain >/dev/null
docker run -d --name rs27-baseline --network rs27-plain --entrypoint sleep "$COMPUTER_IMAGE" infinity >/dev/null
echo "  host listeners on 127.0.0.1:18927 and 0.0.0.0:18928; baseline = computer image on an ordinary bridge (no rules apply)"

if [[ $SCENARIO == legacy ]]; then
  step "Stack up, sidecar with default RAKAZO_IPTABLES (Alpine's iptables = nf_tables) on a legacy-backend host"
  up_stack
  create rs27bot
  step "Sidecar recreated with RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy (supervisor not restarted)"
  export RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy
  if "$DC" up -d --wait --wait-timeout 150 computer-egress >/dev/null 2>&1; then echo "  sidecar healthy"; else echo "  sidecar NOT healthy"; fi
  rules
  echo "  same rules in nf_tables: $(iptables-nft-save 2>/dev/null | grep -c 'rakazo-c+')"
else
  step "Stack up (sidecar, supervisor restricted, relay/web/pubapp stand-ins)"
  up_stack
fi
echo "  network namespace: host $(readlink /proc/self/ns/net), sidecar $(docker exec rs27l-computer-egress-1 readlink /proc/self/ns/net 2>&1)"
echo "  INPUT head: $(iptables -S INPUT | sed -n '2,5p' | tr '\n' '|')"
echo "  DOCKER-USER: $(iptables -S DOCKER-USER 2>&1 | tr '\n' '|' | cut -c1-400)"

step "Baseline probe (unrestricted container, ordinary bridge)"
probe rs27-baseline

step "Real bot computer"
create rs27bot
bridge_of rs27bot
if docker inspect rakazo-bot-rs27bot >/dev/null 2>&1 && [[ $(docker inspect -f '{{.State.Status}}' rakazo-bot-rs27bot) == running ]]; then
  probe rakazo-bot-rs27bot
  step "Relay path (ordinary Compose network)"
  relay
else
  echo "  no running computer to probe"
fi

case $SCENARIO in
  nft)
    step "Screens (network level; web is an alpine stand-in)"
    "$HERE/screen-check.sh" rs27bot rs27space 2>&1 | redact
    net=$(docker inspect -f '{{.HostConfig.NetworkMode}}' rakazo-bot-rs27bot)
    supb=$(docker inspect -f "{{(index .NetworkSettings.Networks \"$net\").IPAddress}}" rs27l-supervisor-1)
    docker exec rakazo-bot-rs27bot bash -c "$PROBE" probe "supervisor on the computer bridge :7091=$supb:7091"

    step "Rules removed while the supervisor keeps running (cached verdict)"
    "$DC" stop computer-egress >/dev/null 2>&1
    bash "$SCRIPT" --remove >/dev/null 2>&1
    rules
    create rs27bot2
    probe rakazo-bot-rs27bot2 short

    step "Supervisor restarted with the rules missing (fail closed)"
    since=$(now); "$DC" restart supervisor >/dev/null 2>&1; verdict "$since"
    create rs27bot3
    step "Sidecar started again, supervisor not restarted"
    "$DC" start computer-egress >/dev/null 2>&1; wait_rules
    create rs27bot3
    probe rakazo-bot-rs27bot3 short
    echo "  rs27bot2 (created while the rules were missing):"; probe rakazo-bot-rs27bot2 short

    step "systemctl restart docker (sidecar running)"
    since=$(now); docker_restart; wait_rules 90
    states rs27l-computer-egress-1 rs27l-supervisor-1 rakazo-bot-rs27bot
    verdict "$since"
    create rs27bot
    probe rakazo-bot-rs27bot short

    step "Simulated boot: docker stopped, nft ruleset flushed, docker started (sidecar running before)"
    systemctl stop docker docker.socket; nft flush ruleset; rules
    since=$(now); systemctl start docker; docker_up; wait_rules 90
    states rs27l-computer-egress-1 rs27l-supervisor-1
    verdict "$since"
    create rs27bot
    probe rakazo-bot-rs27bot short

    step "Simulated boot with the sidecar stopped"
    "$DC" stop computer-egress >/dev/null 2>&1
    systemctl stop docker docker.socket; nft flush ruleset
    since=$(now); systemctl start docker; docker_up; sleep 5
    states rs27l-computer-egress-1 rs27l-supervisor-1
    rules
    verdict "$since"
    create rs27bot
    "$DC" start computer-egress >/dev/null 2>&1; wait_rules
    create rs27bot
    probe rakazo-bot-rs27bot short
    ;;
  ufw)
    step "ufw reload"
    ufw reload >/dev/null; echo "  INPUT head: $(iptables -S INPUT | sed -n '2,5p' | tr '\n' '|')"; rules
    probe rakazo-bot-rs27bot short
    step "ufw disable + enable"
    ufw disable >/dev/null; ufw --force enable >/dev/null
    echo "  INPUT head: $(iptables -S INPUT | sed -n '2,5p' | tr '\n' '|')"; rules
    probe rakazo-bot-rs27bot short
    step "after one sidecar loop (65 s)"
    sleep 65; echo "  INPUT head: $(iptables -S INPUT | sed -n '2,5p' | tr '\n' '|')"; rules
    probe rakazo-bot-rs27bot short
    ;;
  firewalld)
    step "firewall-cmd --reload"
    firewall-cmd --reload >/dev/null; rules
    probe rakazo-bot-rs27bot short
    step "after one sidecar loop (65 s)"
    sleep 65; rules
    probe rakazo-bot-rs27bot short
    step "systemctl restart firewalld"
    systemctl restart firewalld; sleep 3; rules
    probe rakazo-bot-rs27bot short
    sleep 65; echo "  after 65 s:"; rules
    probe rakazo-bot-rs27bot short
    ;;
esac
step "done"
