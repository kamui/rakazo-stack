#!/usr/bin/env bash
# analyze.sh [since-ISO-time]: token and cost totals per service/kind from orlog/*.jsonl.
cd "$(dirname "$0")/orlog"
since=${1:-1970}
cat *.jsonl | jq -s --arg since "$since" '
  map(select(.t >= $since))
  | map(. + {kind: (if .path | test("embeddings") then "embeddings"
                    elif .svc == "honcho" then "honcho-chat" else "rakazo-chat" end)})
  | group_by(.kind)
  | map({
      kind: .[0].kind,
      calls: length,
      errors: (map(select(.status != 200)) | length),
      providers: (map(.res.provider // "none") | group_by(.) | map({(.[0]): length}) | add),
      efforts: (map(.req.reasoning.effort // .req.reasoning_effort // "none") | group_by(.) | map({(.[0]): length}) | add),
      finish: (map(.res.finish_reason // "none") | group_by(.) | map({(.[0]): length}) | add),
      prompt: (map(.res.usage.prompt_tokens // 0) | add),
      cached: (map(.res.usage.prompt_tokens_details.cached_tokens // 0) | add),
      completion: (map(.res.usage.completion_tokens // 0) | add),
      reasoning: (map(.res.usage.completion_tokens_details.reasoning_tokens // 0) | add),
      cost: (map(.res.usage.cost // 0) | add)
    })'
