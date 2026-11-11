#!/usr/bin/env python3
"""Time Lemonade transcription and speech with fixed clips, so every machine's numbers compare.

Usage: LEMONADE_API_KEY=... python3 bench_voice.py --base http://127.0.0.1:13305/api/v1 \
         --label mac-m3pro --models Whisper-Tiny,Whisper-Large-v3-Turbo [--accuracy] [--tts] [--runs 3]
Appends one JSON line per measurement to --out and prints a summary. Python 3 stdlib only.
"""
import argparse, json, os, re, statistics, sys, time, urllib.error, urllib.request, uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
CLIPS = HERE / "clips"
REPLIES = {
    "short": "Sure, I've added that to your calendar for tomorrow at nine.",
    "long": "Here's a quick summary of the week. On Monday you have the team planning meeting at ten, "
    "followed by lunch with Priya. Tuesday is clear apart from a dentist appointment at four. On Wednesday "
    "the quarterly report is due, and I've drafted an outline based on last quarter's numbers. Thursday "
    "you're travelling to Boston, and your flight leaves at seven fifteen in the morning, so I'd suggest "
    "leaving home by five thirty. Friday is a remote day, and I've blocked the afternoon for focused work. "
    "Let me know if you'd like me to move anything.",
}


def call(base, key, path, *, body=None, fields=None, files=None, timeout=1800):
    headers = {"Authorization": f"Bearer {key}"}
    if files is not None:
        boundary = uuid.uuid4().hex
        parts = []
        for k, v in (fields or {}).items():
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
        for k, p in files.items():
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"; filename="{p.name}"\r\n'
                         f"Content-Type: audio/wav\r\n\r\n".encode() + p.read_bytes() + b"\r\n")
        data = b"".join(parts) + f"--{boundary}--\r\n".encode()
        headers["Content-Type"] = f"multipart/form-data; boundary={boundary}"
    else:
        data = json.dumps(body).encode() if body is not None else None
        if data is not None:
            headers["Content-Type"] = "application/json"
    req = urllib.request.Request(base + path, data=data, headers=headers)
    t0 = time.perf_counter()
    with urllib.request.urlopen(req, timeout=timeout) as r:
        payload = r.read()
    return time.perf_counter() - t0, payload


def words(text):
    t = text.lower().replace("-", " ")
    t = re.sub(r"\bmr\b\.?", "mister", t)
    t = re.sub(r"\bmrs\b\.?", "missus", t)
    t = re.sub(r"[^a-z0-9' ]+", " ", t)
    return t.split()


def wer(ref, hyp):
    r, h = words(ref), words(hyp)
    d = list(range(len(h) + 1))
    for i, rw in enumerate(r, 1):
        prev, d[0] = d[0], i
        for j, hw in enumerate(h, 1):
            prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (rw != hw))
    return d[len(h)], len(r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:13305/api/v1")
    ap.add_argument("--label", required=True, help="machine and settings, e.g. win-7900x-cpus4-t4")
    ap.add_argument("--models", default="Whisper-Tiny,Whisper-Base,Whisper-Small,Whisper-Medium,Whisper-Large-v3-Turbo")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--accuracy", action="store_true", help="also transcribe the 20-clip accuracy set once per model")
    ap.add_argument("--tts", action="store_true", help="also time kokoro-v1 speech")
    ap.add_argument("--out", default="results.jsonl")
    a = ap.parse_args()
    key = os.environ.get("LEMONADE_API_KEY") or sys.exit("set LEMONADE_API_KEY")
    refs = json.loads((CLIPS / "refs.json").read_text())
    out = open(a.out, "a")

    def emit(**rec):
        rec = {"label": a.label, "ts": time.strftime("%Y-%m-%dT%H:%M:%S"), **rec}
        out.write(json.dumps(rec) + "\n"); out.flush()
        print(json.dumps(rec), flush=True)

    def transcribe(model, clip):
        dt, body = call(a.base, key, "/audio/transcriptions", fields={"model": model, "language": "en"},
                        files={"file": CLIPS / clip["file"]})
        return dt, json.loads(body).get("text", "")

    def load(model):
        for m in json.loads(call(a.base, key, "/health")[1]).get("all_models_loaded", []):
            call(a.base, key, "/unload", body={"model_name": m["model_name"]})
        call(a.base, key, "/pull", body={"model_name": model})
        return call(a.base, key, "/load", body={"model_name": model})[0]

    for model in a.models.split(","):
        emit(kind="load", model=model, seconds=round(load(model), 3))
        transcribe(model, refs["short"])
        for name in ("short", "long"):
            clip = refs[name]
            times, text = [], ""
            for _ in range(a.runs):
                dt, text = transcribe(model, clip)
                times.append(dt)
            e, n = wer(clip["text"], text)
            emit(kind="stt", model=model, clip=name, audio_s=clip["seconds"], median_s=round(statistics.median(times), 3),
                 runs=[round(t, 3) for t in times], wer=round(e / n, 4), text=text.strip())
        if a.accuracy:
            errs = total = 0
            busy = 0.0
            for clip in refs["accuracy"]:
                dt, text = transcribe(model, clip)
                e, n = wer(clip["text"], text)
                errs, total, busy = errs + e, total + n, busy + dt
            emit(kind="accuracy", model=model, clips=len(refs["accuracy"]), words=total, errors=errs,
                 wer=round(errs / total, 4), seconds=round(busy, 1))

    if a.tts:
        emit(kind="load", model="kokoro-v1", seconds=round(load("kokoro-v1"), 3))
        speak = {"model": "kokoro-v1", "voice": "af_heart", "response_format": "wav"}
        call(a.base, key, "/audio/speech", body={**speak, "input": REPLIES["short"]})
        for name, text in REPLIES.items():
            times = [call(a.base, key, "/audio/speech", body={**speak, "input": text})[0] for _ in range(a.runs)]
            emit(kind="tts", model="kokoro-v1", reply=name, words=len(text.split()),
                 median_s=round(statistics.median(times), 3), runs=[round(t, 3) for t in times])


if __name__ == "__main__":
    try:
        main()
    except urllib.error.HTTPError as e:
        sys.exit(f"HTTP {e.code} {e.url}: {e.read()[:300]!r}")
