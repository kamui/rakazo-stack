#!/usr/bin/env bash
# Run bench_voice.py against a throwaway Lemonade container under several thread counts and CPU limits.
# Usage: LEMONADE_API_KEY=... run-docker.sh <docker-cli> <container> <base_url> <machine-label> <out.jsonl> [first-step]
# Steps are numbered from 1; pass first-step to resume after a failure.
set -euo pipefail
DOCKER=$1 C=$2 BASE=$3 M=$4 OUT=$5 FIRST=${6:-1}
HERE=$(cd "$(dirname "$0")" && pwd)
ALL=Whisper-Tiny,Whisper-Base,Whisper-Small,Whisper-Medium,Whisper-Large-v3-Turbo
SMALLER=Whisper-Tiny,Whisper-Base,Whisper-Small,Whisper-Large-v3-Turbo
step=0

# docker.exe through WSL interop occasionally fails with "UtilAcceptVsock: accept4 failed 110".
d() { for i in 1 2 3; do "$DOCKER" "$@" && return; sleep 3; done; return 1; }

host_cpu() { # whole-host CPU busy %, including Windows when run from WSL
  if command -v typeperf.exe >/dev/null; then
    typeperf.exe '\Processor(_Total)\% Processor Time' -sc 3 2>/dev/null | tr -d '\r"' | awk -F, '/^[0-9]/{s+=$2;n++} END{printf "%.0f%%", s/n}'
  else
    top -bn2 -d1 | awk '/^%?Cpu/{v=100-$8} END{printf "%.0f%%", v}'
  fi
}

run() { # cpus threads models extra-flags...
  step=$((step + 1)); [ "$step" -lt "$FIRST" ] && return
  local cpus=$1 t=$2 models=$3; shift 3
  [ "$cpus" = all ] && lim=$(d info --format '{{.NCPU}}') || lim=$cpus
  d update --cpus "$lim" "$C" >/dev/null  # --cpus 0 does not clear an existing limit
  if [ "$t" = default ]; then args=; else args="-t $t"; fi
  d exec "$C" lemonade config set "whispercpp.cpu_args=$args" >/dev/null
  echo "### step=$step cpus=$cpus threads=$t host-cpu-busy=$(host_cpu)"
  python3 -I "$HERE/bench_voice.py" --base "$BASE" --label "$M-step$step-cpus$cpus-t$t" --models "$models" --out "$OUT" "$@"
}

d exec "$C" lemonade config set whispercpp.backend=cpu >/dev/null
run all default "$ALL" --accuracy --tts
run all 8 "$SMALLER"
run all 12 "$SMALLER" --tts
run 8 8 "$SMALLER" --tts
run 4 4 "$SMALLER" --tts
run 2 2 "$SMALLER" --tts
run 2 default Whisper-Small,Whisper-Large-v3-Turbo
run all default "$SMALLER" --tts  # repeat of step 1, to check the numbers hold
d update --cpus "$(d info --format '{{.NCPU}}')" "$C" >/dev/null
d exec "$C" lemonade config set whispercpp.cpu_args= >/dev/null
echo "### done"
