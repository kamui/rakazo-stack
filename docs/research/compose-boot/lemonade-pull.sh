#!/usr/bin/env bash
# Pulls and loads the voice models through lemonade-relay, from Rakazo's api container
# (the same network path Rakazo uses). The relay adds LEMONADE_API_KEY; this script never sees it.
# Usage: DC=dc.sh lemonade-pull.sh <model> [model ...]
set -euo pipefail
D=${DC:?path to dc.sh}
for model in "$@"; do
  for step in pull load; do
    "$D" exec -T api node -e '
      const [step, model] = process.argv.slice(1);
      const t = Date.now();
      fetch(`http://lemonade-relay:13306/api/v1/${step}`, {
        method: "POST", headers: {"content-type": "application/json"},
        body: JSON.stringify({model_name: model}), signal: AbortSignal.timeout(1_800_000),
      }).then(async r => console.log(step, model, r.status, ((Date.now()-t)/1000).toFixed(1)+"s", (await r.text()).slice(-200).replace(/\s+/g," ")))
        .catch(e => { console.log(step, model, "failed", e.message); process.exit(1); });
    ' "$step" "$model"
  done
done
