#!/usr/bin/env bash
# Creates a real bot computer through the throwaway stack's own supervisor (HTTP API on :7091),
# then probes TCP reachability from inside it with the supervisor's exec endpoint.
# The supervisor token is read inside the supervisor container and never printed.
# Usage: DC=dc.sh computer-probe.sh <create|probe|delete> <botId> <spaceId> [host:port ...]
set -euo pipefail
D=${DC:?path to dc.sh}
action=$1 bot=$2 space=$3
shift 3

# sup <method> <path> [json-body]: calls the supervisor from inside its own container.
sup() {
  "$D" exec -T supervisor node -e '
    const [method, path, bot, space, body] = process.argv.slice(1);
    fetch(`http://127.0.0.1:7091${path}`, {
      method,
      headers: {
        authorization: `Bearer ${process.env.SANDBOX_SUPERVISOR_TOKEN}`,
        "content-type": "application/json",
        "x-rakazo-bot-id": bot,
        "x-rakazo-space-id": space,
      },
      body: body || undefined,
      signal: AbortSignal.timeout(600_000),
    }).then(async (r) => { console.log(r.status, await r.text()); })
      .catch((e) => { console.log("failed", e.message); process.exit(1); });
  ' "$1" "$2" "$bot" "$space" "${3:-}"
}

case $action in
  create)
    # A root supervisor never creates homes itself (the API normally does); make it as uid 1000.
    "$D" exec -T supervisor sh -c "mkdir -p /data/homes/$bot && chown 1000:1000 /data/homes/$bot"
    sup POST /computers "{\"botId\":\"$bot\",\"homePath\":\"/data/homes/$bot\",\"spaceId\":\"$space\"}"
    ;;
  probe)
    script='for t in "$@"; do
      h=${t%:*} p=${t##*:}
      ip=$(getent hosts "$h" | awk "{print \$1; exit}")
      if timeout 3 bash -c "exec 3<>/dev/tcp/$h/$p" 2>/dev/null; then r=open
      else rc=$?; [ $rc -eq 124 ] && r=timeout || r=refused/unresolved; fi
      printf "%-34s resolves=%-15s %s\n" "$t" "${ip:--}" "$r"
    done'
    args=$(jq -nc --arg s "$script" '$ARGS.positional as $t | {argv: (["bash","-c",$s,"probe"] + $t), timeoutMs: 120000}' --args "$@")
    sup POST "/computers/rakazo-bot-$bot/exec" "$args" |
      sed 's/^200 //' | jq -rj 'select(.type=="stdout" or .type=="stderr") | .data' 2>/dev/null || true
    ;;
  delete)
    sup DELETE "/computers/rakazo-bot-$bot"
    ;;
esac
