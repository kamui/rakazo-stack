#!/usr/bin/env bash
# chat.sh <botId> <text>: sends one message as the Owner, waits for the run to end, prints the reply.
set -euo pipefail
cd "$(dirname "$0")"
bot=$1 text=$2
sent=$(./rpc.sh threads/send "$(jq -nc --arg b "$bot" --arg t "$text" '{botId:$b,text:$t}')")
run=$(jq -r '.json.runId // empty' <<<"$sent")
[[ -n $run ]] || { echo "send failed: $sent" >&2; exit 1; }
start=$(date +%s)
while :; do
  status=$(./rpc.sh runs/list '{"filter":"recent"}' | jq -r --arg r "$run" '[.json.runs[] | select((.runId // .id) == $r)][0].status // "queued"')
  case $status in queued|leased|running) ;; *) break ;; esac
  (( $(date +%s) - start > 300 )) && { echo "timeout waiting for $run" >&2; break; }
  sleep 2
done
secs=$(( $(date +%s) - start ))
reply=$(./rpc.sh threads/messages "$(jq -nc --arg b "$bot" '{botId:$b}')" |
  jq -r --arg r "$run" '[.json.messages[]? | select(.runId == $r and .role != "user") | .blocks[]? | select(.kind == "text") | .text] | join(" / ")')
printf '[%s %ss run=%s] %s\n' "$status" "$secs" "$run" "$reply"
