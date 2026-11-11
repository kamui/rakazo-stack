#!/usr/bin/env bash
# Run bench_voice.py against a throwaway Lemonade container under several thread counts and CPU limits.
# Usage: LEMONADE_API_KEY=... run-docker.sh <docker-cli> <container> <base_url> <machine-label> <out.jsonl>
set -euo pipefail
DOCKER=$1 C=$2 BASE=$3 M=$4 OUT=$5
HERE=$(cd "$(dirname "$0")" && pwd)
ALL=Whisper-Tiny,Whisper-Base,Whisper-Small,Whisper-Medium,Whisper-Large-v3-Turbo
SMALLER=Whisper-Tiny,Whisper-Base,Whisper-Small,Whisper-Large-v3-Turbo

run() { # cpus threads models extra-flags...
  local cpus=$1 t=$2 models=$3; shift 3
  if [ "$cpus" = all ]; then "$DOCKER" update --cpus 0 "$C" >/dev/null; else "$DOCKER" update --cpus "$cpus" "$C" >/dev/null; fi
  if [ "$t" = default ]; then args=; else args="-t $t"; fi
  "$DOCKER" exec "$C" lemonade config set "whispercpp.cpu_args=$args" >/dev/null
  echo "### cpus=$cpus threads=$t host-load: $(cut -d' ' -f1-3 /proc/loadavg)"
  python3 -I "$HERE/bench_voice.py" --base "$BASE" --label "$M-cpus$cpus-t$t" --models "$models" --out "$OUT" "$@"
}

"$DOCKER" exec "$C" lemonade config set whispercpp.backend=cpu >/dev/null
run all default "$ALL" --accuracy --tts
run all 8 "$ALL"
run all 12 "$ALL" --tts
run 8 8 "$SMALLER" --tts
run 4 4 "$SMALLER" --tts
run 2 2 "$SMALLER" --tts
run 2 default Whisper-Small,Whisper-Large-v3-Turbo
"$DOCKER" update --cpus 0 "$C" >/dev/null
"$DOCKER" exec "$C" lemonade config set whispercpp.cpu_args= >/dev/null
echo "### done"
