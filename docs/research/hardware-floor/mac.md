# Apple-silicon voice, RAM and disk (#42)

Native Lemonade on the Mac, measured with [`bench_voice.py`](bench_voice.py) (at `2394de8`) and the [clips](clips/SOURCE.md) used for Linux and Windows. Lemonade `v2026.45.3` embeddable tarball (sha256 matches the release digest `ad2f418c…b1da`), whisper.cpp `v1.8.4`, Kokoros `b21`. lemond ran from a scratch directory with `--no-broadcast` on a loopback port, its own `HF_HOME` and a random `LEMONADE_API_KEY`. Only `whispercpp.backend=metal` was set, and there was no `--convert` because the clips are already 16 kHz WAV. Everything was deleted afterwards.

Raw logs: [`mac-m3pro-metal.log`](transcripts/mac-m3pro-metal.log), [`mac-m3pro-metal-rerun.log`](transcripts/mac-m3pro-metal-rerun.log), [`mac-m3pro-first-pass.log`](transcripts/mac-m3pro-first-pass.log), and [`mac-m3pro-lemond.log`](transcripts/mac-m3pro-lemond.log) for config, device lines, eviction, RAM and disk. Results: [`results/mac-m3pro.jsonl`](results/mac-m3pro.jsonl).

## Machine

- Apple M3 Pro: 11 CPU cores (5 performance, 6 efficiency) and a 14-core GPU
- 18 GB RAM
- macOS 27.2.1
- Docker Desktop 4.97.0, installed but not running during the test

**Background load:** the Mac was in normal use. Load average was 4.4–7 on 11 cores the whole time, from Edge, WindowServer, `kernel_task` and a Time Machine backup (`backupd`, about 35% CPU). Swap was already 6.8–7.0 GB of 8 GB, with 20–42% of memory free. Treat the numbers as a busy-laptop case rather than a best case.

## Transcription (Metal)

Warm medians of 3 runs, with the pass labelled `mac-m3pro-metal` first and the `-rerun` pass second. Short is 5.9 s of audio and long is 29.4 s. WER on the 20-clip accuracy set (276 words) comes from the first pass.

| Model | short | long | WER short / long | WER accuracy set |
|---|---|---|---|---|
| Whisper-Tiny | 0.08 / 0.08 s | 0.30 / 0.24 s | 0% / 17.6% | 11.2% (31 errors) |
| Whisper-Base | 0.14 / 0.11 s | 0.36 / 0.33 s | 0% / 10.3% | 9.1% (25) |
| Whisper-Small | 0.30 / 0.28 s | 1.00 / 0.90 s | 0% / 10.3% | 6.2% (17) |
| Whisper-Medium | 0.72 / 0.87 s | 1.68 / 1.90 s | 11.8% / 8.8% | 5.8% (16) |
| Whisper-Large-v3-Turbo | 0.81 / 0.95 s | 1.32 / 1.35 s | 0% / 5.9% | **5.1% (14)** |

- **Large-v3-Turbo is fast enough on Metal for voice calls.** It transcribes the 29 s clip in about 1.3 s, around 22× real time. It is also the most accurate model, and on the long clip it is faster than Medium. Nothing here argues for a smaller default on macOS.
- The first pass, made with the earlier version of the script, gave the same picture. Large-v3-Turbo took 0.86 / 0.83 s short and 1.13 / 1.13 s long. Spread between passes is up to about 20%, which matches the varying background load.
- Medium's 11.8% on the short clip is a single "we're" for "we are".
- **Compared with #16** (same Mac, v2026.44.1, WebM through `--convert`): #16 measured 0.82 s for a 3.9 s clip and 2.03 s for a 32 s clip. Here WAV with no conversion took 0.8–0.95 s for 5.9 s and 1.1–1.35 s for 29.4 s.

## Speech (Kokoro)

Warm medians of 3 runs. Kokoro returns the whole WAV at once.

| Reply | first pass (old script) | `mac-m3pro-metal` | `-rerun` |
|---|---|---|---|
| short (11 words) | 0.95 / 0.79 s | 1.16 s | 1.19 s |
| long (95 words) | 7.73 / 8.45 s | 8.54 s | 8.87 s |

- Kokoro runs on the CPU: `/api/v1/health` reports `device: cpu`. The `koko` binary links CoreML and lives in `bin/kokoro/metal/`, but it is not used as a GPU device.
- These times are slower than #16's 0.83 s and 6.67 s. The likely cause is the busier machine and the heavy swap.

## First loads

These times cover the `/api/v1/load` call only. Downloads happen in `/pull` beforehand and are not counted.

- **The first Whisper start on the Mac took 12.2 s** (Whisper-Tiny, first pass). That includes installing `whisper-server` (0.35 s) and about 11.5 s compiling Metal shaders. #16 saw 17 s, 11 s of it shaders.
- Once the shaders were cached, the first load of each new model was quick: Base 0.34 s, Small 0.65 s, Medium 1.87 s, Large-v3-Turbo 1.40 s. Later loads took 0.3–1.6 s.
- **The first Kokoro start took 3.2 s**, including 1.3 s to install `koko`. Later loads took 1.5–2.4 s.
- macOS keeps the shader cache in the user's shared Metal cache (`DARWIN_USER_CACHE_DIR/com.apple.metal`), outside Lemonade's folders. Removing Lemonade does not remove it, and other apps use the same cache, so it was left in place.

## Idle eviction

**Large-v3-Turbo was not evicted.** It was loaded on its own, used once, then left idle for 392 s:

- `/api/v1/health` still listed it as loaded (`backend_health: ready`, `device: gpu`), on the same `whisper-server` PID.
- The next transcription of `short.wav` took 1.17 s, against 0.86 s warm. The one after that took 0.87 s.
- The Mac logs the same per-model settings as the Linux container: `evict_idle_timeout=300, downsize_idle_timeout=60`. The global `auto_evict` is `false`, though, and the per-model `auto_evict` is `(none)`. lemond logged nothing about eviction or downsizing during the idle period.

**macOS swaps the idle model out instead.** Between 211 s and 241 s idle, `whisper-server`'s RSS fell from 1,606 MiB to 8 MiB. Its physical footprint stayed at 1,783 MB (`footprint`), with `vmmap` showing 1.7 GiB `swapped_out`. Kokoro did the same in the RAM test: 350 MiB RSS fell to 4 MiB within about two minutes, while its footprint stayed at 1.5 GiB. The extra 0.3 s on the first request after idle is that memory paging back in. Whether this happens on a Mac with no memory pressure was not tested, since this one had 7 GB of swap in use.

## RAM

Whisper-Large-v3-Turbo and kokoro-v1 were both loaded through `/api/v1/load`. RSS is from `ps`. Physical footprint is from `vmmap` and includes Metal buffers and swapped or compressed pages. All sizes here are binary units (KiB from `ps`, MiB and GiB from `vmmap` and `du`). On macOS, footprint is the number to budget for, because RSS drops whenever the system swaps an idle model out.

| Process | Idle RSS | Peak RSS | Footprint (current / peak) |
|---|---|---|---|
| `lemond` | 11 MiB | 17 MiB during transcription, 19 MiB during speech | 31 MiB / 43 MiB |
| `whisper-server` (Large-v3-Turbo, Metal) | 1,606 MiB | 1,639 MiB during `long.wav` (1.34 s) | 1.7–1.8 GiB / 1.8 GiB |
| `koko` (kokoro-v1, CPU) | 350 MiB after load, 166 MiB after one request | 1,118 MiB during the long reply | 1.5–1.6 GiB / 2.3 GiB |
| **Total** | **≈ 1.9 GiB** | | **≈ 3.2–3.4 GiB idle, ≈ 4.1 GiB peak** |

- The peak RSS figures come from sampling every 20 ms (83 samples during the transcription). The Kokoro row also shows its peak during the 95-word reply, which is where its memory spikes.
- **Docker Desktop:** not running (`docker info` could not reach the socket), so its memory limit, its VM process and `docker stats` were not measured. This needs a rerun with Docker Desktop started.

## Disk

| What | Size (`du -sh`) |
|---|---|
| Extracted Lemonade folder (`lemond`, `lemonade`, `resources/`) | 19 MiB (6.5 MiB tarball) |
| Backends lemond downloads on first use, in its cache dir | 56 MiB (`kokoro` 51 MiB, `whispercpp` 5.5 MiB) |
| `HF_HOME` with all five Whisper models and kokoro-v1 | 4.0 GiB |
| **`HF_HOME` with only Large-v3-Turbo and kokoro-v1** | **1.9 GiB** (`ggml-large-v3-turbo.bin` 1.51 GiB, kokoro 348 MiB) |
| Lemonade total for the macOS default | ≈ 1.93 GiB |

## Surprises and notes

- **No idle eviction, but idle models get swapped.** The 300 s timeout shows up in the log but is not applied while `auto_evict` is off. On this memory-pressured Mac the OS paged both models out after a few minutes anyway. That costs about 0.3 s on the next request and none of the footprint comes back.
- **Kokoro takes more memory than Whisper at its peak.** Its footprint is 1.5 GiB idle and 2.3 GiB at peak, from a 348 MiB model on the CPU.
- **Large-v3-Turbo beats Medium on both speed and accuracy on Metal.** Turbo has 4 decoder layers against Medium's 24.
- **Most of the time on short clips is fixed cost.** Large-v3-Turbo takes 0.8–0.95 s for 5.9 s of audio and only 1.3 s for 29.4 s.
- **`whispercpp.metal_args` is gone in v2026.45.3.** `lemonade config` lists `whispercpp.args`, `cpu_args` and `npu_args` only. #16 and the inputs for #17 say to set `whispercpp.metal_args=--convert`. Setting it was not tried here, so check before relying on it.
- **Child servers are still on host loopback without authentication.** `whisper-server` used `127.0.0.1:8001`, and `koko` used `:8001`, or `:8002` when Whisper held `:8001`. The WebSocket was on `:9000`. #16's exposure finding still applies to v2026.45.3.
- **The benchmark script changed during the run** (`2394de8`: WER number normalization, per-clip text, load average). Both passes were rerun with it, so `results/mac-m3pro.jsonl` matches the Linux and Windows runs. The earlier pass is kept only in `mac-m3pro-first-pass.log`.
- The child servers' stderr, such as `ggml_metal_init` or ONNX Runtime provider lines, does not reach lemond's info-level log in this version. The device evidence is therefore lemond's `Using backend: metal` line plus the `device` field in `/api/v1/health`.
