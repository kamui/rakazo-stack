#!/usr/bin/env bash
# Extra: same 16 kHz WAV on whispercpp.backend=cpu (auto picks Vulkan, which here is llvmpipe software).
set -euo pipefail
A=$(realpath "${1:?audio dir}")
B=http://127.0.0.1:23415/api/v1
K=$(docker exec rkz-wavtest-lemonade printenv LEMONADE_API_KEY)
docker exec rkz-wavtest-lemonade lemonade config set whispercpp.backend=cpu >/dev/null
docker exec rkz-wavtest-lemonade lemonade config | grep -E 'whispercpp\.(args|backend) '
j() { curl -sS -m 600 -w ' HTTP=%{http_code} %{time_total}s\n' "$B/$1" -H "authorization: Bearer $K" "${@:2}"; }
j unload -H 'content-type: application/json' -d '{"model_name":"Whisper-Large-v3-Turbo"}'
j load -H 'content-type: application/json' -d '{"model_name":"Whisper-Large-v3-Turbo"}'
docker logs rkz-wavtest-lemonade 2>&1 | grep 'Using backend' | tail -1
for f in short.16k.wav short.16k.wav long.16k.wav; do
  j audio/transcriptions -F model=Whisper-Large-v3-Turbo -F "file=@$A/$f;type=audio/wav" | tr -s '\n' ' '; echo " <- $f"
done
