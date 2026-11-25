#!/usr/bin/env bash
# RAM and disk for a throwaway rs46 Instance with Hindsight on an Apple-silicon Mac:
# native lemond (Metal Whisper-Large-v3-Turbo + kokoro-v1, #42's macOS default) plus Docker Desktop running
# Rakazo, the relay, Hindsight, Tailscale and one bot computer. Same stages as measure-ram.sh on Windows.
# Only this project's containers are summed; the VM and macOS totals include everything else running.
# Works with macOS's bash 3.2. Usage: measure-mac.sh <clips-dir> <work-dir>   (run from this directory)
set -uo pipefail
CLIPS=$(cd "$1" && pwd)
W=$(mkdir -p "$2" && cd "$2" && pwd); rm -f "$W/lemond.pid"
OUTDIR=$PWD/out; mkdir -p "$OUTDIR"
P=rs46
COMPUTER_IMAGE=${COMPUTER_IMAGE:?set COMPUTER_IMAGE, e.g. ghcr.io/elie222/rakazo/computer:v0.16.0}
LEMONADE_VERSION=${LEMONADE_VERSION:-2026.47.1}
LEMONADE_SHA256=${LEMONADE_SHA256:-b6efc95a4e674c4ac74c5ee97a43ced97bb3a367297e81d13fa732479e474df8}
STT_MODEL=${STT_MODEL:-Whisper-Large-v3-Turbo}
d() { for _ in 1 2 3; do docker "$@" && return; sleep 3; done; return 1; }
dc() { d compose -p $P --env-file .env -f rakazo.v0.16.0.images.yml -f compose.hw.yaml "$@"; }
envv() { grep "^$1=" .env | cut -d= -f2-; }
KEY=$(envv LEMONADE_API_KEY); LP=$(envv HW_LEMONADE_PORT); B=http://127.0.0.1:$LP/api/v1

mib() { awk '{v=$1; u=$1; gsub(/[0-9.]/,"",u); gsub(/[A-Za-z]/,"",v); f=(u=="GiB")?1024:(u=="KiB")?1/1024:(u=="B")?1/1048576:1; printf "%.0f\n", v*f}'; }

footprint_of() { # pid -> "rss=… MiB footprint=…" (footprint includes Metal buffers and swapped pages)
  local rss fp
  rss=$(ps -o rss= -p "$1" 2>/dev/null | awk '{printf "%.0f", $1/1024}')
  fp=$(vmmap --summary "$1" 2>/dev/null | awk '/^Physical footprint:/{print $3; exit}')
  echo "rss=${rss:-?} MiB footprint=${fp:-?}"
}

native() { # lemond and the model servers it starts (whisper-server, koko)
  local root pid; root=$(cat "$W/lemond.pid" 2>/dev/null) || return 0
  for pid in $root $(pgrep -P "$root"); do
    printf '  mac %-16s pid %-6s %s\n' "$(basename "$(ps -o comm= -p "$pid")")" "$pid" "$(footprint_of "$pid")"
  done
}

vm() { # Docker Desktop's VM from inside, then the macOS processes that make it up
  d run --rm --pid host busybox:1 sh -c 'free -m | awk "/Mem:/{printf \"  vm: total=%s used=%s cache=%s avail=%s MiB\n\", \$2, \$3, \$6, \$7}"'
  ps -axo rss=,comm= | awk '/Virtualization|com\.docker|Docker Desktop/ { n=$0; sub(/^ *[0-9]+ +/, "", n); sub(/.*\//, "", n); s[n]+=$1 }
    END { for (k in s) printf "  mac %-40s %6.0f MiB rss\n", k, s[k]/1024 }' | sort
  for pid in $(pgrep -f 'com.apple.Virtualization.VirtualMachine'); do echo "  mac VirtualMachine pid $pid $(footprint_of "$pid")"; done
  memory_pressure 2>/dev/null | awk '/System-wide memory free percentage/{print "  mac " $0}'
  sysctl -n vm.swapusage | sed 's/^/  mac swap: /'
}

snapshot() {
  echo "--- $1"
  d stats --no-stream --format '{{.Name}}\t{{.MemUsage}}' | grep -E "^$P[-_]" | sort > "$OUTDIR/stats.tmp"
  while IFS=$'\t' read -r n u; do printf '  %-32s %6s MiB\n' "$n" "$(echo "${u%% /*}" | mib)"; done < "$OUTDIR/stats.tmp"
  printf '  %-32s %6s MiB\n' TOTAL "$(cut -f2 "$OUTDIR/stats.tmp" | sed 's| /.*||' | mib | awk '{s+=$1} END{print s}')"
  native
  vm
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

peak_while() {
  local c=$1 pid=$2 peak=0 v
  while kill -0 "$pid" 2>/dev/null; do
    v=$(d stats --no-stream --format '{{.MemUsage}}' "$c" 2>/dev/null | sed 's| /.*||' | mib); [ "${v:-0}" -gt "$peak" ] && peak=$v
  done
  echo "$peak"
}

j() { curl -sS -m 1200 -o /dev/null -w "$1 %{http_code} %{time_total}s\n" -H "authorization: Bearer $KEY" "${@:2}"; }

echo "=== $(date -u +%FT%TZ) machine"
echo "  $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.ncpu) cores, $(( $(sysctl -n hw.memsize) / 1073741824 )) GiB, macOS $(sw_vers -productVersion)"
echo "  Docker Desktop VM: $(d info --format '{{.NCPU}} CPUs, MemTotal {{.MemTotal}} bytes, {{.OperatingSystem}}, {{.ServerVersion}}')"
echo "=== baseline: nothing from $P or lemond running"; vm

echo "=== native Lemonade $LEMONADE_VERSION"
TGZ=$W/lemonade-embeddable-$LEMONADE_VERSION-macos-arm64.tar.gz
[ -f "$TGZ" ] || curl -fsSL -o "$TGZ" "https://github.com/lemonade-sdk/lemonade/releases/download/v$LEMONADE_VERSION/$(basename "$TGZ")"
got=$(shasum -a 256 "$TGZ" | cut -d' ' -f1)
[ "$got" = "$LEMONADE_SHA256" ] || { echo "  tarball sha256 $got does not match $LEMONADE_SHA256"; exit 1; }
echo "  tarball sha256 ok"
mkdir -p "$W/lemonade" "$W/hf" && tar -xzf "$TGZ" -C "$W/lemonade"
LEMOND=$(find "$W/lemonade" -type f -name lemond -perm -u+x | head -1)
LEMONADE_CLI=$(dirname "$LEMOND")/lemonade
env -i HOME="$HOME" PATH=/usr/bin:/bin:/usr/sbin:/sbin HF_HOME="$W/hf" LEMONADE_API_KEY="$KEY" \
  "$LEMOND" --no-broadcast --port "$LP" --host 127.0.0.1 "$(dirname "$LEMOND")" > "$OUTDIR/lemond.log" 2>&1 &
echo $! > "$W/lemond.pid"
for _ in $(seq 60); do curl -fsS -o /dev/null -H "authorization: Bearer $KEY" "$B/health" && break; sleep 1; done
echo "  lemond $(curl -sS -H "authorization: Bearer $KEY" "$B/health" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p') up"
LEMONADE_PORT=$LP LEMONADE_API_KEY=$KEY "$LEMONADE_CLI" config set whispercpp.backend=metal >/dev/null && echo "  whispercpp.backend=metal"
for m in "$STT_MODEL" kokoro-v1; do
  j "pull $m" "$B/pull" -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}"
  j "load $m" "$B/load" -H 'content-type: application/json' -d "{\"model_name\":\"$m\"}"
done
j stt "$B/audio/transcriptions" -F "model=$STT_MODEL" -F language=en -F "file=@$CLIPS/short.wav;type=audio/wav"
j tts "$B/audio/speech" -H 'content-type: application/json' -d '{"model":"kokoro-v1","voice":"af_heart","input":"Your dinner reservation is confirmed for eight tonight."}'
sleep 20
echo "--- native voice, after one transcription and one reply"; native

echo "=== stage 1: memory off (Rakazo + relay; Lemonade is native)"
dc up -d --quiet-pull 2>&1 | tail -2; wait_healthy; sleep 60
snapshot "idle, memory off"

echo "=== stage 2: + Hindsight"
started=$(date +%s)
dc --profile hindsight up -d --quiet-pull 2>&1 | tail -2 &
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
dc --profile hindsight --profile tailscale up -d --quiet-pull 2>&1 | tail -2; sleep 45
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
d system df -v | awk -v p="$P" 'NR==1 || /^VOLUME NAME/ || index($1, p"_")==1' | grep -E "^($P|VOLUME)" || true
d image ls --format '{{.Repository}}:{{.Tag}}\t{{.Size}}' | grep -E 'rakazo|hindsight|pgvector|postgres|tailscale|oven/bun|busybox' || true
du -sh "$W/lemonade" "$W/hf"; du -sh "$W"/hf/hub/* 2>/dev/null
echo "=== done $(date -u +%FT%TZ)"
