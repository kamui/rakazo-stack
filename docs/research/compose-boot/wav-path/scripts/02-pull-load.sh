#!/usr/bin/env bash
# Pull then load each model on rkz-wavtest-lemonade, printing status and wall time.
# The key is read from the container's env into a shell variable; it is never echoed.
set -euo pipefail
B=http://127.0.0.1:23415/api/v1
K=$(docker exec rkz-wavtest-lemonade printenv LEMONADE_API_KEY)
for m in "$@"; do for step in pull load; do
  t0=$(date +%s.%N)
  out=$(curl -sS -m 1800 -w ' HTTP=%{http_code}' "$B/$step" -H "authorization: Bearer $K" \
    -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}")
  printf '%s %s %.1fs %s\n' "$step" "$m" "$(echo "$(date +%s.%N) - $t0" | bc)" "$(echo "$out" | tail -c 250 | tr -s '\n' ' ')"
done; done
