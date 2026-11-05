#!/usr/bin/env bash
# Recreate the Tailscale container with NO auth key: identity must come back from the tailscale-state volume.
set -uo pipefail
D=$(dirname "$0")
before=$("$D/dc.sh" exec -T tailscale tailscale status --json | jq -r '.Self.ID+" "+.Self.HostName')
"$D/dc.sh" up -d --no-deps --force-recreate tailscale 2>&1 | tail -2
for i in $(seq 1 40); do st=$("$D/dc.sh" exec -T tailscale tailscale status --json 2>/dev/null | jq -r .BackendState); [[ $st == Running ]] && break; sleep 2; done
after=$("$D/dc.sh" exec -T tailscale tailscale status --json | jq -r '.Self.ID+" "+.Self.HostName')
echo "backend: $st"
echo "node before: $before"
echo "node after:  $after"
[[ $before == "$after" ]] && echo "same node identity: yes" || echo "same node identity: NO"
"$D/dc.sh" logs --since 2m tailscale 2>&1 | rg -i 'serve proxy|got cert|cert\(.*(cache|valid)|authkey|auth' | tail -6
