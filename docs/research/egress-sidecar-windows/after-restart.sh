#!/usr/bin/env bash
# After a Docker Desktop restart, `wsl --shutdown` or a reboot: records when the sidecar, the rules and
# the supervisor came back, then resumes the bot computer without restarting the supervisor and probes it.
# Usage: after-restart.sh <botId> [spaceId]
set -uo pipefail
W=${RS27_WORK:-$HOME/rs27work}
export RS27_TOKEN_FILE=$W/token DC=$W/mini/dc.sh
bot=$1 space=${2:-rs27space}

until docker info >/dev/null 2>&1; do sleep 2; done
echo "docker up: $(date -u +%T)  engine $(docker version --format '{{.Server.Version}}')"
for c in rs27-mini-computer-egress-1 rs27-mini-supervisor-1 rs27-mini-relay-standin-1 rs27-mini-web-1 "rakazo-bot-$bot" rakazo-supervisor-1 rakazo-web-1 lemonade-server; do
  printf '  %-30s %s\n' "$c" "$(docker inspect -f '{{.State.Status}} restarts={{.RestartCount}} started={{.State.StartedAt}}' "$c" 2>&1 | cut -c1-90)"
done
for i in $(seq 60); do
  c=$("$W/vm-rules.sh" count 2>/dev/null)
  [[ $c == *"v4 14"* ]] && break
  sleep 5
done
echo "rules at $(date -u +%T): $c"
echo "sidecar log (last 3):"; docker logs --tail 3 rs27-mini-computer-egress-1 2>&1 | sed 's/^/  /'
echo "supervisor egress log lines since start:"
docker logs rs27-mini-supervisor-1 --since "$(docker inspect -f '{{.State.StartedAt}}' rs27-mini-supervisor-1)" 2>&1 |
  grep -oE '"timestamp":"[^"]*","level":"[a-z]*","message":"(restricted[^"]*|supervisor listening|computer egress[^"]*)"' | sed 's/^/  /'
echo "create (resume) $bot without restarting the supervisor:"
"$W/computer-probe.sh" create "$bot" "$space" | cut -c1-200 | sed 's/^/  /'
"$W/computer-probe.sh" probe "$bot" "$space" host.docker.internal:18927 host.docker.internal:5173 192.168.65.254:3100 169.254.169.254:80 1.1.1.1:443 example.com:443 | sed 's/^/  /'
echo "relay-standin -> host.docker.internal:18927: $("$DC" exec -T relay-standin wget -q -T 5 -O- http://host.docker.internal:18927/ >/dev/null 2>&1 && echo ok || echo fail)"
