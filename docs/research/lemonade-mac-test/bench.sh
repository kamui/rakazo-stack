#!/usr/bin/env bash
# Time Lemonade speech and transcription with fixed phrases, so Mac and Linux runs compare.
# Usage: LEMONADE_API_KEY=... bench.sh [base_url] [runs] [label]
set -euo pipefail
BASE=${1:-http://127.0.0.1:13305/api/v1}
RUNS=${2:-5}
LABEL=${3:-run}
KEY=${LEMONADE_API_KEY:?set LEMONADE_API_KEY}
OUT=${OUT:-$(mktemp -d)}; mkdir -p "$OUT"
VOICE=${VOICE:-af_heart}
STT_MODEL=${STT_MODEL:-Whisper-Large-v3-Turbo}

SHORT="Sure, I've added that to your calendar for tomorrow at nine."
MEDIUM="I checked your inbox and found three new messages. One is from the landlord about the lease renewal, one is a shipping update for your order, and one is a newsletter you usually skip."
LONG="Here's a quick summary of the week. On Monday you have the team planning meeting at ten, followed by lunch with Priya. Tuesday is clear apart from a dentist appointment at four. On Wednesday the quarterly report is due, and I've drafted an outline based on last quarter's numbers. Thursday you're travelling to Boston, and your flight leaves at seven fifteen in the morning, so I'd suggest leaving home by five thirty. Friday is a remote day, and I've blocked the afternoon for focused work. Let me know if you'd like me to move anything."

median() { python3 -c 'import sys,statistics as s; v=[float(x) for x in sys.stdin.read().split()]; print(f"{s.median(v):.3f}")'; }

speak() { # name text -> writes $OUT/<name>.wav, prints "ttfb total"
  local body
  body=$(python3 -c 'import json,sys; print(json.dumps({"model":"kokoro-v1","voice":sys.argv[2],"input":sys.argv[1],"response_format":"wav"}))' "$2" "$VOICE")
  curl -sS -o "$OUT/$1.wav" -w '%{time_starttransfer} %{time_total}\n' \
    -H "Authorization: Bearer $KEY" -H 'content-type: application/json' -d "$body" "$BASE/audio/speech"
}

transcribe() { # file -> prints "total" and saves text
  curl -sS -o "$OUT/$(basename "$1").txt" -w '%{time_total}\n' \
    -H "Authorization: Bearer $KEY" -F "model=$STT_MODEL" -F language=en -F "file=@$1" "$BASE/audio/transcriptions"
}

echo "label=$LABEL base=$BASE runs=$RUNS out=$OUT"
for name in SHORT MEDIUM LONG; do
  text=${!name}
  speak "warm-$name" "$text" >/dev/null
  : > "$OUT/tts-$name.ttfb"; : > "$OUT/tts-$name.total"
  for _ in $(seq "$RUNS"); do
    read -r ttfb total < <(speak "$name" "$text")
    echo "$ttfb" >> "$OUT/tts-$name.ttfb"; echo "$total" >> "$OUT/tts-$name.total"
  done
  dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT/$name.wav")
  ffmpeg -v error -y -i "$OUT/$name.wav" -c:a libopus -b:a 32k "$OUT/$name.webm"
  printf 'tts  %-6s words=%3d audio=%5.2fs  ttfb_median=%ss total_median=%ss\n' "$name" "$(wc -w <<<"$text")" "$dur" \
    "$(median < "$OUT/tts-$name.ttfb")" "$(median < "$OUT/tts-$name.total")"
done
for name in SHORT MEDIUM LONG; do
  transcribe "$OUT/$name.webm" >/dev/null
  : > "$OUT/stt-$name.total"
  for _ in $(seq "$RUNS"); do transcribe "$OUT/$name.webm" >> "$OUT/stt-$name.total"; done
  printf 'stt  %-6s webm  total_median=%ss  text=%s\n' "$name" "$(median < "$OUT/stt-$name.total")" \
    "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("text","").strip()[:90])' "$OUT/$name.webm.txt")"
done
