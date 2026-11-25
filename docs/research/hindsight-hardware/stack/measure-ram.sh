#!/usr/bin/env bash
# RAM and disk for a throwaway rs46 Instance with Hindsight, on Docker Desktop for Windows (run from WSL),
# repeating #42's stages with Hindsight in place of Honcho:
#   memory off -> + Hindsight (boot peak, idle, after use) -> + Tailscale -> + one bot computer.
# Only this project's containers are summed; the VM totals include everything else on the host.
# Usage: measure-ram.sh <clips-dir>   (run from this directory; secrets come from .env, never printed)
set -uo pipefail
CLIPS=$(realpath "$1")
OUTDIR=$PWD/out; mkdir -p "$OUTDIR"
P=rs46
DOCKER=${DOCKER:-docker}
COMPUTER_IMAGE=${COMPUTER_IMAGE:-ghcr.io/elie222/rakazo/computer:v0.16.0}
d() { for _ in 1 2 3; do "$DOCKER" "$@" && return; sleep 3; done; return 1; }
dc() { d compose -p $P --env-file .env -f rakazo.v0.16.0.images.yml -f compose.hw.yaml "$@"; }
envv() { grep "^$1=" .env | cut -d= -f2-; }
KEY=$(envv LEMONADE_API_KEY); LP=$(envv HW_LEMONADE_PORT); B=http://127.0.0.1:$LP/api/v1

mib() { awk '{v=$1; u=$1; gsub(/[0-9.]/,"",u); gsub(/[A-Za-z]/,"",v); f=(u=="GiB")?1024:(u=="KiB")?1/1024:(u=="B")?1/1048576:1; printf "%.0f\n", v*f}'; }

snapshot() { # label -> per-container MiB and total
  echo "--- $1"
  d stats --no-stream --format '{{.Name}}\t{{.MemUsage}}' | grep -E "^$P[-_]" | sort > "$OUTDIR/stats.tmp"
  while IFS=$'\t' read -r n u; do printf '  %-32s %6s MiB\n' "$n" "$(echo "${u%% /*}" | mib)"; done < "$OUTDIR/stats.tmp"
  printf '  %-32s %6s MiB\n' TOTAL "$(cut -f2 "$OUTDIR/stats.tmp" | sed 's| /.*||' | mib | awk '{s+=$1} END{print s}')"
  vm
}

vm() { # Docker Desktop VM and Windows-side processes
  d run --rm --pid host busybox:1 sh -c 'free -m | awk "/Mem:/{printf \"  vm: total=%s used=%s cache=%s avail=%s MiB\n\", \$2, \$3, \$6, \$7}"'
  for _ in 1 2 3; do
    out=$(powershell.exe -NoProfile -Command "Get-Process | Where-Object { \$_.ProcessName -match '^(vmmem|vmmemwsl|Docker Desktop|com\.docker\..*)$' } | Group-Object ProcessName | ForEach-Object { '  win {0,-24} {1,6} MiB' -f \$_.Name, [math]::Round((\$_.Group | Measure-Object WorkingSet64 -Sum).Sum/1MB) }" 2>/dev/null | tr -d '\r') && [ -n "$out" ] && { echo "$out"; return; }
    sleep 3
  done
  echo "  win: (powershell.exe unavailable)"
}

wait_healthy() {
  for _ in $(seq 120); do
    starting=$(d ps --filter "label=com.docker.compose.project=$P" --format '{{.Status}}' | grep -c 'health: starting' || true)
    unhealthy=$(d ps -a --filter "label=com.docker.compose.project=$P" --format '{{.Names}} {{.Status}}' | grep -E 'unhealthy|Restarting|Exited \([1-9]' || true)
    [ -n "$unhealthy" ] && { echo "  not healthy: $unhealthy"; return 1; }
    [ "$starting" = 0 ] && return 0
    sleep 5
  done
  return 1
}

peak_while() { # container, pid-to-watch -> prints peak MiB while the pid runs
  local c=$1 pid=$2 peak=0 v
  while kill -0 "$pid" 2>/dev/null; do
    v=$(d stats --no-stream --format '{{.MemUsage}}' "$c" 2>/dev/null | sed 's| /.*||' | mib); [ "${v:-0}" -gt "$peak" ] && peak=$v
  done
  echo "$peak"
}

j() { curl -sS -m 1200 -o /dev/null -w "$1 %{http_code} %{time_total}s\n" -H "authorization: Bearer $KEY" "${@:2}"; }

echo "=== $(date -u +%FT%TZ) baseline: nothing from $P running"; vm

echo "=== stage 1: memory off (Rakazo + relay + Lemonade)"
dc --profile lemonade up -d --quiet-pull 2>&1 | tail -2; wait_healthy; sleep 60
snapshot "idle, no voice models loaded"
d exec $P-lemonade-1 lemonade config set whispercpp.backend=cpu >/dev/null
for m in Whisper-Small kokoro-v1; do
  j "pull $m" "$B/pull" -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}"
  j "load $m" "$B/load" -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}"
done
sleep 20; snapshot "idle, Whisper-Small + kokoro-v1 loaded"
j stt "$B/audio/transcriptions" -F model=Whisper-Small -F language=en -F "file=@$CLIPS/short.wav;type=audio/wav"
j tts "$B/audio/speech" -H 'content-type: application/json' -d '{"model":"kokoro-v1","voice":"af_heart","input":"Your dinner reservation is confirmed for eight tonight."}'
sleep 20; snapshot "idle, Whisper-Small + kokoro-v1 after one transcription and one reply"

echo "=== stage 2: + Hindsight"
started=$(date +%s)
dc --profile lemonade --profile hindsight up -d --quiet-pull 2>&1 | tail -2 &
upp=$!
sleep 5
( until [ "$(d inspect -f '{{.State.Health.Status}}' $P-hindsight-1 2>/dev/null)" = healthy ]; do sleep 1; done; sleep 20 ) &
hp=$!
echo "  hindsight peak from start until 20 s after healthy: $(peak_while $P-hindsight-1 $hp) MiB"
wait $upp; echo "  hindsight healthy after about $(( $(date +%s) - started - 20 ))s"
wait_healthy; sleep 60
snapshot "idle, with Hindsight (empty)"
HS_KEY=$(envv HINDSIGHT_TENANT_API_KEY) d run --rm --network ${P}_stack -e HS_KEY -v "$PWD":/w:ro --entrypoint python \
  ghcr.io/vectorize-io/hindsight-api:1.3.0 -I /w/use.py http://hindsight:8888 rs46-probe 500 20 &
up2=$!
echo "  hindsight peak while seeding 500 memories and running 20 recalls: $(peak_while $P-hindsight-1 $up2) MiB"
wait $up2
sleep 30; snapshot "with Hindsight, 30 s after seeding 500 memories and 20 recalls"
sleep 120; snapshot "with Hindsight, 150 s after use"

echo "=== stage 3: + Tailscale (not logged in; no auth key)"
dc --profile lemonade --profile hindsight --profile tailscale up -d --quiet-pull 2>&1 | tail -2; sleep 45
snapshot "idle, with Hindsight and Tailscale"

echo "=== stage 4: + one bot computer (same limits and options the supervisor uses)"
d run -d --name $P-computer-probe --label com.docker.compose.project=$P --network none --user 1000:1000 \
  --memory 2g --cpus 2 --pids-limit 2048 --shm-size 256m --cap-drop ALL --security-opt no-new-privileges:true \
  -e RAKAZO_COMPUTER_CONTROL_TOKEN="$(openssl rand -hex 16)" "$COMPUTER_IMAGE" \
  /usr/bin/env -u LD_PRELOAD /usr/local/bin/rakazo-computer >/dev/null
sleep 45; snapshot "idle, with one bot computer ($COMPUTER_IMAGE)"
BR=$(d exec $P-computer-probe sh -c 'command -v chromium || command -v chromium-browser || command -v google-chrome' 2>/dev/null)
if [ -n "$BR" ]; then
  d exec -d $P-computer-probe sh -c "\"$BR\" --no-sandbox --no-first-run --user-data-dir=/tmp/hwprof 'data:text/html,<h1>rakazo-stack hardware probe</h1>' >/dev/null 2>&1"
  sleep 30; snapshot "one bot computer with a browser open ($BR)"
fi

echo "=== disk"
d system df -v --format '{{json .}}' > "$OUTDIR/df.json" 2>/dev/null || true
d system df -v | awk -v p="$P" 'NR==1 || /^VOLUME NAME/ || index($1, p"_")==1' | grep -E "^($P|VOLUME)" || true
echo "=== done $(date -u +%FT%TZ)"
