#!/usr/bin/env bash
# Speech and transcription through lemonade-relay, plus the direct-to-Lemonade auth check.
# Runs in a one-off container of the stack's own Lemonade image (it has curl and ffmpeg) on the
# internal `stack` network. It sends no key itself: only the relay holds LEMONADE_API_KEY.
set -euo pipefail
D=${DC:?path to dc.sh}
"$D" run --rm --no-deps -T -e LEMONADE_API_KEY= --entrypoint bash lemonade -c '
set -u
relay=http://lemonade-relay:13306/api/v1
cd /tmp
code=$(curl -sS -o speech.wav -w "%{http_code}" "$relay/audio/speech" -H "content-type: application/json" \
  -d "{\"model\":\"kokoro-v1\",\"voice\":\"af_heart\",\"lang_code\":\"a\",\"response_format\":\"wav\",\"input\":\"The quick brown fox jumps over the lazy dog.\"}")
echo "relay POST /audio/speech: $code $(ffprobe -v error -show_entries format=format_name,duration -of csv=p=0 speech.wav)"
ffmpeg -v error -y -i speech.wav -c:a libopus -b:a 32k speech.webm
echo "ffmpeg WebM: $(ffprobe -v error -show_entries format=format_name,duration:stream=codec_name -of csv=p=0 speech.webm | tr "\n" " ")"
code=$(curl -sS -m 300 -o stt.json -w "%{http_code}" "$relay/audio/transcriptions" \
  -F model=Whisper-Large-v3-Turbo -F "file=@speech.webm;type=audio/webm")
echo "relay POST /audio/transcriptions (WebM): $code $(head -c 300 stt.json)"
for path in models audio/speech; do
  method=GET; [ $path = models ] || method=POST
  echo "direct $method lemonade:13305/api/v1/$path, no key: $(curl -sS -o /dev/null -w "%{http_code}" -X $method http://lemonade:13305/api/v1/$path -H "content-type: application/json" -d "{}")"
done
echo "direct GET lemonade:13305/api/v1/models, wrong key: $(curl -sS -o /dev/null -w "%{http_code}" http://lemonade:13305/api/v1/models -H "authorization: Bearer wrong")"
echo "relay GET /models: $(curl -sS -o /dev/null -w "%{http_code}" $relay/models)"
'
