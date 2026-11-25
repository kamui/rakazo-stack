#!/usr/bin/env bash
# replay.sh <botId> <first-line> <last-line> <tag>: sends day.txt lines in order. After each turn it
# waits until Hindsight has no pending work, then snapshots memory_units and per-turn LLM usage
# (Hindsight's own llm_requests table) into snapshots/<tag>-NN.tsv.
set -uo pipefail
cd "$(dirname "$0")"
bot=$1 first=$2 last=$3 tag=$4
mkdir -p snapshots
H() { ./dc.sh exec -T hindsight-db psql -U hindsight -d hindsight -qAtX -F $'\t' -c "$1" </dev/null; }
drain() {
  local t0=$(date +%s)
  while [ "$(H "select count(*) from async_operations where status in ('pending','processing')")" != 0 ]; do
    (( $(date +%s) - t0 > 600 )) && { echo "   drain timeout"; return; }
    sleep 3
  done
  echo "   hindsight idle after $(( $(date +%s) - t0 ))s"
}
n=$((first - 1))
sed -n "${first},${last}p" day.txt | while IFS= read -r line; do
  n=$((n + 1))
  since=$(date -u +%FT%T.%NZ)
  printf '>> %02d %s\n' "$n" "$line"
  ./chat.sh "$bot" "$line" </dev/null
  drain
  H "select bank_id, coalesce(document_id,'-'), coalesce(chunk_id,'-'), fact_type, id, created_at, consolidated_at, left(replace(text, E'\n', ' '), 160) from memory_units order by created_at" >"snapshots/$tag-$(printf %02d "$n").tsv"
  H "select scope, count(*), sum(input_tokens), sum(cached_tokens), sum(output_tokens), sum(thoughts_tokens) from llm_requests where started_at >= '$since' and scope in ('retain_extract_facts','consolidation') group by 1 order by 1" |
    sed 's/^/   llm /'
done
