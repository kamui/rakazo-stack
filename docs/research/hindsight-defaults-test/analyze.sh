#!/usr/bin/env bash
# analyze.sh [since-ISO-time]: token and cost totals per call kind from orlog/*.jsonl.
# Hindsight calls are told apart by the head of their own system prompt (kind() below).
cd "$(dirname "$0")/orlog"
since=${1:-1970} until=${2:-9999}
cat ./*.jsonl | jq -s --arg since "$since" --arg until "$until" '
  def kind: if .svc != "hindsight" then "rakazo-chat"
    elif (.req.sys_head // "") | startswith("Extract") then "hindsight-retain-extract"
    elif (.req.sys_head // "") | test("observation"; "i") then "hindsight-consolidate"
    else "hindsight-other" end;
  map(select(.t >= $since and .t < $until)) | map(. + {kind: kind})
  | group_by(.kind)
  | map({
      kind: .[0].kind,
      calls: length,
      errors: (map(select(.status != 200)) | length),
      providers: (map(.res.provider // "none") | group_by(.) | map({(.[0]): length}) | add),
      max_tokens: (map(.req.max_tokens // .req.max_completion_tokens // "none" | tostring) | group_by(.) | map({(.[0]): length}) | add),
      efforts: (map(.req.reasoning.effort // .req.reasoning_effort // "none") | group_by(.) | map({(.[0]): length}) | add),
      finish: (map(.res.finish_reason // "none") | group_by(.) | map({(.[0]): length}) | add),
      prompt: (map(.res.usage.prompt_tokens // 0) | add),
      cached: (map(.res.usage.prompt_tokens_details.cached_tokens // 0) | add),
      completion: (map(.res.usage.completion_tokens // 0) | add),
      reasoning: (map(.res.usage.completion_tokens_details.reasoning_tokens // 0) | add),
      max_prompt: (map(.res.usage.prompt_tokens // 0) | max),
      max_completion: (map(.res.usage.completion_tokens // 0) | max),
      max_reasoning: (map(.res.usage.completion_tokens_details.reasoning_tokens // 0) | max),
      cost: (map(.res.usage.cost // 0) | add)
    })'
