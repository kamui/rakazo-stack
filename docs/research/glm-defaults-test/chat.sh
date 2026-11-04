#!/usr/bin/env bash
# chat.sh <botId> <text>: sends one message, waits for the run to finish, prints the bot's reply.
set -euo pipefail
cd "$(dirname "$0")"
bot=$1 text=$2
sent=$(./rpc.sh threads/send "$(jq -nc --arg b "$bot" --arg t "$text" '{botId:$b,text:$t}')")
run=$(jq -r '.json.runId // empty' <<<"$sent")
[[ -n $run ]] || { echo "send failed: $sent" >&2; exit 1; }
start=$(date +%s)
status=queued
while :; do
  status=$(./rpc.sh runs/list '{"filter":"recent"}' | jq -r --arg r "$run" '[.json.runs[] | select(.runId == $r)][0].status // "queued"')
  case $status in queued|pending|running|starting|active) ;; *) break ;; esac
  (( $(date +%s) - start > 300 )) && { echo "timeout waiting for $run" >&2; break; }
  sleep 2
done
secs=$(( $(date +%s) - start ))
reply=$(./rpc.sh threads/messages "$(jq -nc --arg b "$bot" '{botId:$b}')" |
  jq -r --arg r "$run" '[.json.messages[]? | select(.runId == $r and .role == "bot") | .blocks[]? | select(.kind == "text") | .text] | join(" / ")')
printf '[%s %ss] %s\n' "$status" "$secs" "$reply"
