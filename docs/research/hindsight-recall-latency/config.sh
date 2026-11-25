#!/usr/bin/env bash
# One latency configuration: restart db+api with the given limits, take COLD cold samples
# (each after a full restart), then RUNS warm recalls per budget.
# usage: CPUS=.. CPUSET=.. API_ENV=.. config.sh LABEL RUNS [BUDGETS] [COLD]
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
LABEL=$1 RUNS=$2 BUDGETS=${3:-low mid high} COLD=${4:-1}
OUT=results/$LABEL.log
: > "$OUT"
export API_ENV="-e HINDSIGHT_API_ENABLE_AUTO_CONSOLIDATION=false ${API_ENV:-}"
for i in $(seq 1 "$COLD"); do
  { ./other.sh; ./up.sh; } >> "$OUT" 2>&1
  ./run.sh bench.py http://hsr-api:8888 bench mid 1 "$LABEL-cold$i" >> "$OUT"
done
./run.sh bench.py http://hsr-api:8888 bench mid 3 "$LABEL-warmup" >> "$OUT"
for b in $BUDGETS; do
  ./other.sh >> "$OUT"
  ./run.sh bench.py http://hsr-api:8888 bench "$b" "$RUNS" "$LABEL" >> "$OUT"
done
docker stats --no-stream --format '{{.Name}} {{.CPUPerc}} {{.MemUsage}}' hsr-api hsr-db >> "$OUT"
./shmpeak.sh >> "$OUT"
