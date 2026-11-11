#!/usr/bin/env bash
# RAM and disk for a throwaway rkz-hw Instance on Docker Desktop for Windows (run from WSL).
# Stages: memory off -> + Honcho -> + Tailscale -> + one bot computer (idle, then with a browser open).
# Usage: measure-ram.sh <clips-dir>   (run from this directory; secrets come from .env and are never printed)
set -uo pipefail
CLIPS=$(realpath "$1")
OUTDIR=$PWD/out; mkdir -p "$OUTDIR"
P=rkz-hw
DOCKER=docker.exe
d() { for i in 1 2 3; do "$DOCKER" "$@" && return; sleep 3; done; return 1; }
dc() { d compose --env-file .env -f rakazo.v0.10.0.images.yml -f compose.hw.yaml "$@"; }
envv() { grep "^$1=" .env | cut -d= -f2-; }
KEY=$(envv LEMONADE_API_KEY); LP=$(envv HW_LEMONADE_PORT); B=http://127.0.0.1:$LP/api/v1

mib() { awk '{v=$1; u=$1; gsub(/[0-9.]/,"",u); gsub(/[A-Za-z]/,"",v); f=(u=="GiB")?1024:(u=="KiB")?1/1024:(u=="B")?1/1048576:1; printf "%.0f\n", v*f}'; }

snapshot() { # label -> per-container MiB and total
  echo "--- $1"
  d stats --no-stream --format '{{.Name}}\t{{.MemUsage}}' | grep -E "^$P[-_]" | sort > $OUTDIR/stats.tmp
  while IFS=$'\t' read -r n u; do printf '  %-32s %6s MiB\n' "$n" "$(echo "${u%% /*}" | mib)"; done < $OUTDIR/stats.tmp
  printf '  %-32s %6s MiB\n' TOTAL "$(cut -f2 $OUTDIR/stats.tmp | sed 's| /.*||' | mib | awk '{s+=$1} END{print s}')"
  vm
}

vm() { # Docker Desktop VM and Windows-side processes
  d run --rm --pid host busybox:1 sh -c 'free -m | awk "/Mem:/{printf \"  vm: total=%s used=%s cache=%s avail=%s MiB\n\", \$2, \$3, \$6, \$7}"; ps -o rss,comm | awk "\$2 ~ /^(dockerd|containerd|com.docker|vpnkit|docker-proxy|containerd-shim|buildkitd)/ {s[\$2]+=\$1} END{for (k in s) printf \"  vm-proc %-26s %6d MiB\n\", k, s[k]/1024}"'
  powershell.exe -NoProfile -Command "Get-Process | Where-Object { \$_.ProcessName -match '^(vmmem|vmmemwsl|Docker Desktop|com\.docker\..*)$' } | Group-Object ProcessName | ForEach-Object { '  win {0,-24} {1,6} MiB' -f \$_.Name, [math]::Round((\$_.Group | Measure-Object WorkingSet64 -Sum).Sum/1MB) }" | tr -d '\r'
}

wait_healthy() {
  for _ in $(seq 90); do
    bad=$(d ps -a --filter "label=com.docker.compose.project=$P" --format '{{.Names}} {{.Status}}' | grep -Ev 'healthy\)|Exited \(0\)|Up [0-9]+ (seconds|minutes|hours)$' | grep -v 'health: starting' || true)
    starting=$(d ps --filter "label=com.docker.compose.project=$P" --format '{{.Status}}' | grep -c 'health: starting' || true)
    [ -z "$bad" ] && [ "$starting" = 0 ] && return 0
    sleep 5
  done
  echo "not healthy:"; d ps -a --filter "label=com.docker.compose.project=$P" --format '{{.Names}} {{.Status}}'; return 1
}

j() { curl -sS -m 1200 -o /dev/null -w "$1 %{http_code} %{time_total}s\n" -H "authorization: Bearer $KEY" "${@:2}"; }

echo "=== baseline: nothing from $P running"; vm

echo "=== stage 1: memory off (Rakazo + relay + Lemonade)"
dc --profile lemonade up -d --quiet-pull 2>&1 | tail -2; wait_healthy; sleep 60
snapshot "idle, no voice models loaded"
d exec $P-lemonade-1 lemonade config set whispercpp.backend=cpu >/dev/null
for m in Whisper-Large-v3-Turbo kokoro-v1; do
  j "pull $m" "$B/pull" -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}"
  j "load $m" "$B/load" -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}"
done
sleep 20; snapshot "idle, Large-v3-Turbo + kokoro-v1 loaded"
( for _ in 1 2 3; do j stt "$B/audio/transcriptions" -F model=Whisper-Large-v3-Turbo -F language=en -F "file=@$CLIPS/long.wav;type=audio/wav"; done ) &
tp=$!; peak=0
while kill -0 $tp 2>/dev/null; do
  v=$(d stats --no-stream --format '{{.MemUsage}}' $P-lemonade-1 | sed 's| /.*||' | mib); [ "${v:-0}" -gt "$peak" ] && peak=$v
done; wait $tp
echo "  lemonade peak during 3x long.wav transcription (Large-v3-Turbo): $peak MiB"
j "unload Turbo" "$B/unload" -H 'content-type: application/json' -d '{"model_name":"Whisper-Large-v3-Turbo"}'
j "pull Small" "$B/pull" -H 'content-type: application/json' -d '{"model_name":"Whisper-Small"}'
j "load Small" "$B/load" -H 'content-type: application/json' -d '{"model_name":"Whisper-Small"}'
sleep 10; snapshot "idle, Whisper-Small + kokoro-v1 loaded"

echo "=== stage 2: + Honcho"
dc --profile lemonade --profile honcho up -d --quiet-pull 2>&1 | tail -2; wait_healthy; sleep 60
snapshot "idle, with Honcho"

echo "=== stage 3: + Tailscale (not logged in; no auth key)"
dc --profile lemonade --profile honcho --profile tailscale up -d --quiet-pull 2>&1 | tail -2; sleep 45
snapshot "idle, with Honcho and Tailscale"

echo "=== stage 4: + one bot computer (same limits and options the supervisor uses)"
d run -d --name $P-computer-probe --label com.docker.compose.project=$P --network none --user 1000:1000 \
  --memory 2g --cpus 2 --pids-limit 2048 --shm-size 256m --cap-drop ALL --security-opt no-new-privileges:true \
  -e RAKAZO_COMPUTER_CONTROL_TOKEN="$(openssl rand -hex 16)" ghcr.io/elie222/rakazo/computer:v0.10.0 \
  /usr/bin/env -u LD_PRELOAD /usr/local/bin/rakazo-computer >/dev/null
sleep 45; snapshot "idle, with one bot computer"
d exec $P-computer-probe sh -c 'ls "$PLAYWRIGHT_BROWSERS_PATH" 2>/dev/null; command -v chromium chromium-browser google-chrome firefox 2>/dev/null' | head
BR=$(d exec $P-computer-probe sh -c 'command -v chromium || command -v chromium-browser || command -v google-chrome || ls -d "$PLAYWRIGHT_BROWSERS_PATH"/chromium-*/chrome-linux*/chrome 2>/dev/null | head -1')
if [ -n "$BR" ]; then
  d exec -d $P-computer-probe sh -c "\"$BR\" --no-sandbox --no-first-run --user-data-dir=/tmp/hwprof 'data:text/html,<h1>rakazo-stack hardware probe</h1>' >/dev/null 2>&1"
  sleep 30; snapshot "one bot computer with a browser open ($BR)"
else
  echo "  no browser found in the computer image"
fi

echo "=== done"
