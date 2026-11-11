# Hardware floor: Windows (and Linux by inference)

Part of [Measure the hardware an Instance needs on each OS](https://github.com/kamui/rakazo-stack/issues/42).

**Machine:**
- AMD Ryzen 9 7900X (12 cores / 24 threads, Zen 4), 32 GB RAM, Windows 11 Pro.
- Docker Desktop 29.9.3 on WSL 2. Its VM gets 24 CPUs and 15.4 GiB, which is WSL's default of half the host RAM.
- Lemonade `v2026.45.3`, unmodified image, `whispercpp.backend=cpu`. whisper.cpp `v1.8.4` is built with AVX2, not AVX-512.

Everything ran as throwaway containers named `rkz-hw*`, which were deleted afterwards. A separate Rakazo install on the same machine was left running and mostly idle.

**Linux is inferred, not measured.**
- No bare Linux machine was available.
- Docker Desktop runs the same Linux containers in a Linux VM, so CPU-bound voice times should be close to Linux Docker Engine on the same CPU.
- For RAM, Linux skips Docker Desktop's VM and Windows processes.

## Voice

The clips are real read speech from LibriSpeech (see [`clips/SOURCE.md`](clips/SOURCE.md)), sent as 16 kHz WAV with `language=en`, as `lemonade-relay` sends them. Times are warm medians of 3 runs. The script is [`bench_voice.py`](bench_voice.py), the matrix is [`run-docker.sh`](run-docker.sh), and the raw data is in [`results/win-7900x.jsonl`](results/win-7900x.jsonl).

**Background load.** Windows itself kept 8–15% of the CPU busy throughout: Defender, iCloud Drive and the WSL VM. A first pass taken at the same time agrees with the reported numbers to within about 3%. Its file, [`results/win-7900x-pass1-noisy.jsonl`](results/win-7900x-pass1-noisy.jsonl), also includes a repeat step run under a CPU limit by mistake.

### Transcription: seconds for the 5.9 s clip / the 29.4 s clip

| Model | WER | No limit, 4 threads (Lemonade's default) | No limit, 8 threads | 8 CPUs, 8 threads | 4 CPUs, 4 threads | 2 CPUs, 2 threads |
|---|---|---|---|---|---|---|
| Whisper-Tiny | 10.9% | 0.26 / 0.73 | 0.20 / 2.41 | 0.17 / 2.40 | 0.27 / 0.70 | 0.48 / 1.19 |
| Whisper-Base | 9.1% | 0.57 / 0.91 | 0.35 / 3.56 | 0.42 / 3.65 | 0.56 / 0.89 | 1.00 / 1.48 |
| Whisper-Small | 6.2% | 1.94 / 4.55 | 1.22 / 8.59 | 1.20 / 8.50 | 1.95 / 4.55 | 3.58 / 8.16 |
| Whisper-Medium | 5.8% | 6.12 / 8.39 | | | | |
| Whisper-Large-v3-Turbo | 5.1% | 9.57 / 10.25 | 5.54 / 7.90 | 5.40 / 7.64 | 9.53 / 10.27 | 18.16 / 19.13 |

- **WER** is measured on the 20-clip accuracy set (276 words, 118 s). The text is lowercased, punctuation is removed, and small numbers are spelled out. Mid-word splits that Whisper inserts at line breaks (`inexhaust ible`, `f encing`) still count as errors, because Rakazo would show them too.
  - Small and Turbo differ by 3 words out of 276.
  - Most of Turbo's remaining errors are names (`Brion` → `Brienne`) and homophones (`kneading` → `needing`).
- **Lemonade starts `whisper-server` with no `-t`, so whisper.cpp uses 4 threads.**
  - More threads help Turbo, whose time goes mostly to the encoder: 9.6 s drops to 5.5 s at 8 threads. 12 threads is no better (5.5 s / 7.8 s).
  - More threads make smaller models slower on long clips, by 2–4×. Small at 12 threads takes 3.0 s / 10.9 s.
  - So a single thread setting can't suit every model.
- **Whisper encodes a full 30 s window whatever the clip length**, so a short clip costs most of what a long one does.
- **Turbo is slower than real time** even on a 12-core desktop with tuned threads: 5.4 s for a 5.9 s clip. On 2 CPUs it takes 18 s, which matches the figure from [Boot the drafted compose layout end to end](https://github.com/kamui/rakazo-stack/issues/24).
- **CPU limits approximate a smaller machine.** But each Zen 4 core is faster than an older or low-power core, so real small machines will do worse than the 2- and 4-CPU columns.

Swapping the default from Turbo to Small and back takes about 1.6 s to load each model. Models stay loaded while idle: `auto_evict` is `false`, and Whisper-Small was still loaded after 25 minutes idle.

### Speech (kokoro-v1, voice `af_heart`): seconds for the whole WAV

| | No limit | 8 CPUs | 4 CPUs | 2 CPUs |
|---|---|---|---|---|
| 11-word reply | 0.71 | 0.89 | 0.99 | 1.88 |
| 95-word reply (about 32 s of audio) | 4.51 | 5.66 | 9.71 | 18.99 |

Kokoro returns the whole file at once. The first long reply after a load takes about 1.8× longer (8.2 s with no limit) and isn't included in the medians.

## RAM

Measured with a throwaway full Instance:
- Rakazo `v0.10.0`'s unmodified compose file plus a measurement layer, [`stack/compose.hw.yaml`](stack/compose.hw.yaml). Its `relay/lang-code-relay.ts` comes from the `prototype/compose-layout` branch.
- Lemonade with no profile changes, the relay, Honcho `v3.6.1` without Redis, and Tailscale `v1.104.4` in userspace mode, not logged in.
- A bot computer started with the same image, limits and options the supervisor uses, with no network.
- Values are `docker stats` memory with the Instance idle. The script is [`stack/measure-ram.sh`](stack/measure-ram.sh), and the log is [`transcripts/win-ram.log`](transcripts/win-ram.log).

| Part | MiB | Notes |
|---|---|---|
| Rakazo: api, worker, web, supervisor, postgres | 775–1,020 | Upstream's limits for these add up to 6.5 GiB, plus 1.5 GiB for `migrate` at start |
| Lemonade, no models loaded | 29 | |
| + kokoro-v1 | +1,900 to 2,450 | The `koko` process uses 1.9 GB as soon as the model loads, and 2.4 GB after it first speaks, whatever the reply length (11 to 190 words) |
| + Whisper-Small | +580 | `whisper-server` RSS |
| + Whisper-Large-v3-Turbo instead | about +2,100 | Lemonade's total was 4,550 with Turbo and kokoro, 4,600 at the peak of transcribing the long clip |
| Honcho: api, deriver, pgvector | 580 | 276 + 247 + 60 |
| Tailscale, not logged in | 23 | |
| lemonade-relay | 10–16 | |
| One bot computer, idle | 140–165 | Upstream limits each computer to 2 GiB and 2 CPUs. Real browsing wasn't measured: the probe had no network |

**Totals for the whole Instance, idle with voice loaded,** from the snapshots in the log. The higher end of each Small range is right after a model loads, while Linux still has the model file in its page cache. It settles about 0.8 GiB lower after a few minutes.

| | Whisper-Small | Whisper-Large-v3-Turbo |
|---|---|---|
| Memory off | 4.0–4.7 GiB | 5.3 GiB |
| Memory, Tailscale and one idle computer on | 4.1–4.9 GiB (4.2 measured) | about 6.1 GiB |
| Same, with a computer using its full 2 GiB | about 6.0–6.8 GiB | about 8 GiB |

**Windows also needs:**
- Docker Desktop's Windows processes: about 550 MiB.
- The VM's own kernel and daemons.
- WSL gives the VM only half the host's RAM by default. On this machine that's 15.4 GiB, the `MemTotal` that `docker info` reports.
- So an 8 GB Windows PC has about 4 GB for the whole Instance. That doesn't fit Small with memory on, before counting the computer. A 16 GB PC gives the VM about 8 GB.

**Linux** doesn't need any of the Windows overhead above.

## Disk

| | GB | Notes |
|---|---|---|
| Images (Docker's uncompressed sizes) | 8.4 | Rakazo app 2.73 and computer 2.37 (v0.10.0); Lemonade 1.04; postgres:16 0.64; pgvector:pg18 0.63; Honcho 0.58; Tailscale 0.18; Bun 0.17 |
| Voice models | 2.0 | Large-v3-Turbo 1.62 + kokoro-v1 0.34. Small is 0.49 instead of Turbo, and Lemonade's backends add 0.07 |
| Volumes on a fresh Instance | 0.2 | Postgres 73 MB, Honcho's database 68 MB, web assets 44 MB |
| **Fresh Instance** | **about 10.6** | **about 9.5 with Small** |
| After one `rakazo-stack update`, old images kept | +4.7 | v0.9.0 and v0.10.0 share only 0.33 GB of the app image and 0.11 GB of the computer image. Rakazo shipped v0.8.0, v0.9.0 and v0.10.0 within 8 days |
| + new Lemonade, Honcho or Postgres images | up to +1.5 | When those also change |

The app image's size moves a lot between releases: 6.18 GB in v0.8.0, 2.71 GB in v0.9.0, 2.73 GB in v0.10.0. Docker Desktop's virtual disk on Windows grows and doesn't shrink on its own.

## Found along the way

- **Honcho can't write its tiktoken cache.** The `honcho-tiktoken` volume is mounted at `/tiktoken`, which doesn't exist in the image, so Docker creates it owned by root. The Honcho image runs as `app` (uid 100), so the API exits with `PermissionError` on `/tiktoken/...tmp`, and the deriver never starts.
  - It worked once the volume was chowned to `100:101`.
  - The layout needs a fix: mount the volume at a path the image already owns as `app`, or chown it in a one-shot step.
- **`docker update --cpus 0` doesn't remove an existing CPU limit.** Set it to the VM's CPU count instead.
- **`docker.exe` through WSL interop sometimes fails** with `UtilAcceptVsock: accept4 failed 110`. Retrying worked. Setup's Windows path should retry `docker.exe` calls that fail this way.
- **Kokoro's memory doesn't shrink after use.** Its working set stayed at about 2.4 GB after short and long replies alike.
