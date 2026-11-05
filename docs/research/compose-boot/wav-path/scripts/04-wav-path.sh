#!/usr/bin/env bash
# Usage: 04-wav-path.sh <audio dir>
# kokoro-v1 speech -> browser-like WebM/Opus 48 kHz mono -> ffmpeg (Alpine) 16 kHz mono s16 WAV -> Whisper.
# lang_code must be an espeak code (en-us): Kokoros turns Kokoro's one-letter 'a' into near-empty audio.
# Plus controls: raw WebM, 48 kHz WAV, kokoro's native WAV, and a ~30 s clip.
set -euo pipefail
A=$(realpath "${1:?audio dir}")
B=http://127.0.0.1:23415/api/v1
K=$(docker exec rkz-wavtest-lemonade printenv LEMONADE_API_KEY)
SHORT="Please schedule the quarterly budget review for Thursday at three thirty in the afternoon."
LONG="Good morning, everyone. Before we start, a few reminders about the move to the new office. \
The movers arrive on Monday at eight, so please pack your desks by Friday evening and label every box with your name and floor number. \
Laptops, chargers and anything fragile should travel with you, not with the movers. \
Parking at the new building is on level two, and visitor badges are available from the front desk. \
If anything is missing after the move, send a short note to the facilities team and they will track it down within two days."

ff() { docker run --rm --user "$(id -u):$(id -g)" -v "$A:/a" -w /a rkz-wavtest-ffmpeg:local "$@"; }
probe() { ff ffprobe -v error -show_entries format=format_name,duration:stream=codec_name,sample_rate,channels,sample_fmt -of compact=p=0:nk=0 "$1" | tr '\n' ' '; }

speech() { # name text
  local code; code=$(curl -sS -o "$A/$1.kokoro.wav" -w '%{http_code}' "$B/audio/speech" -H "authorization: Bearer $K" \
    -H 'content-type: application/json' \
    -d "$(jq -nc --arg t "$2" '{model:"kokoro-v1",voice:"af_heart",lang_code:"en-us",response_format:"wav",input:$t}')")
  echo "POST /audio/speech $1: HTTP $code | $(probe "$1.kokoro.wav")"
  local dur; dur=$(ff ffprobe -v error -show_entries format=duration -of csv=p=0 "$1.kokoro.wav")
  (( $(echo "$dur > 2" | bc) )) || { echo "speech $1 is only ${dur}s: TTS produced near-empty audio" >&2; exit 1; }
}
stt() { # file mime label
  local t0 code body
  t0=$(date +%s.%N)
  code=$(curl -sS -m 600 -o "$A/stt.json" -w '%{http_code}' "$B/audio/transcriptions" -H "authorization: Bearer $K" \
    -F model=Whisper-Large-v3-Turbo -F "file=@$A/$1;type=$2")
  body=$(tr -s '\n' ' ' < "$A/stt.json")
  printf 'POST /audio/transcriptions [%s] %s (%s): HTTP %s in %.2fs\n  -> %s\n' "$3" "$1" "$2" "$code" "$(echo "$(date +%s.%N) - $t0" | bc)" "$body"
}

echo "## Synthesize"
speech short "$SHORT"
speech long "$LONG"

echo "## Encode like a browser upload (WebM, Opus, 48 kHz mono)"
for n in short long; do
  ff ffmpeg -v error -y -i $n.kokoro.wav -ar 48000 -ac 1 -c:a libopus -b:a 32k -f webm $n.webm
  echo "$n.webm: $(probe $n.webm)"
done

echo "## Relay conversion: ffmpeg -i in.webm -ar 16000 -ac 1 -c:a pcm_s16le out.wav"
for n in short long; do
  ff ffmpeg -v error -y -i $n.webm -ar 16000 -ac 1 -c:a pcm_s16le $n.16k.wav
  echo "$n.16k.wav: $(probe $n.16k.wav)"
done
echo "## Control conversion, no resample: ffmpeg -i in.webm -ac 1 -c:a pcm_s16le out.wav"
ff ffmpeg -v error -y -i short.webm -ac 1 -c:a pcm_s16le short.48k.wav
echo "short.48k.wav: $(probe short.48k.wav)"

echo "## Expected: $SHORT"
stt short.16k.wav audio/wav "check 2, 1st"
stt short.16k.wav audio/wav "check 2, warm"
stt short.16k.wav audio/wav "check 2, warm"
stt short.webm audio/webm "control a: raw WebM"
stt short.48k.wav audio/wav "control b: 48 kHz WAV"
stt short.kokoro.wav audio/wav "extra: kokoro native WAV"
echo "## Expected: $LONG"
stt long.16k.wav audio/wav "control c: ~30 s 16 kHz WAV"
