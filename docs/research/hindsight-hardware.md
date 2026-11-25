# Re-measure the hardware an Instance needs with Hindsight

Task for [#46](https://github.com/kamui/rakazo-stack/issues/46). It repeats the RAM and fresh-disk parts of
[Measure the hardware an Instance needs on each OS](https://github.com/kamui/rakazo-stack/issues/42), with
Hindsight in place of Honcho, and checks the published minimums.

**Run on 2026-11-25:**
- Docker Desktop for Windows, WSL 2 backend. The VM gets 24 CPUs and 15.4 GiB (`MemTotal` 15,818 MiB).
- Everything ran as a throwaway Compose project, `rs46`, deleted afterwards. A separate Rakazo + Hindsight + Lemonade install on the same machine stayed up and idle. The VM totals below include it, and the per-container figures don't.
- Hindsight got a dummy model key and no internet, so nothing was spent.

**Linux is derived, not measured,** the same way #42 did it. Docker Desktop runs the same Linux containers in a VM, so the per-container figures carry over to Linux Docker Engine. Linux skips the VM and Docker Desktop's Windows processes.

**macOS rows are left to fill in.** [`hindsight-hardware/mac.md`](hindsight-hardware/mac.md) is a self-contained step list, and [`stack/measure-mac.sh`](hindsight-hardware/stack/measure-mac.sh) runs it.

## Answer

- **Memory on now costs about 1 GiB more than with Honcho.**
  - Hindsight's API with its worker uses **1.33–1.56 GiB idle** (1,360–1,601 MiB). Boot peaks at 1.57 GiB (1,607 MiB), and it peaks at 1.66 GiB (1,697 MiB) while embedding 500 memories and answering recalls.
  - `hindsight-db` uses 82 MiB empty and 126–165 MiB with 500 memories.
  - Together that's 1.45–1.82 GiB, against Honcho's 0.57 GiB (580 MiB) in #42.
  - These match #44's figures (1.2–1.6 GiB idle, about 1.8 GiB at startup).
- **Whole Instance, container sums**, with Whisper-Small and kokoro-v1 loaded:

  | | #42, with Honcho | Now, with Hindsight |
  |---|---|---|
  | Memory off | 4.0–4.7 GiB | 3.9–4.2 GiB |
  | Memory, Tailscale and one idle computer on | 4.1–4.9 GiB | **5.2–5.7 GiB** |
  | Same, with a computer using its full 2 GiB | about 6.0–6.8 GiB | **about 7.1–7.6 GiB** |

- **8 GB Linux no longer holds as a working minimum.**
  - Idle, the Instance fits on a headless 8 GB box, with about 1.5–2 GiB to spare.
  - Once a bot computer works hard, the Instance needs 7.1–7.6 GiB. An 8 GB machine has about 7.6 GiB usable before the OS takes its share, so it would swap or hit the OOM killer.
  - Raising the Linux minimum (12 or 16 GB), or capping the computer lower on small machines, is a decision for the map.
- **16 GB Windows still holds, with less room.**
  - A 16 GB PC gives the VM about 7.8 GiB. The idle Instance needs 5.2–5.7 GiB of that, and a busy computer brings it to 7.1–7.6 GiB, right at the limit.
  - Docker Desktop's Windows processes take about 0.63 GiB on top, outside the VM.
- **Fresh disk grows from about 9.5 GB to about 15.7 GB** (Whisper-Small).
  - `hindsight-api` is 6.38 GB, against Honcho's 0.58.
  - **The 25 GB minimum holds for a fresh install, but not for every update.** While an update pulls new images, both versions are on disk. A Rakazo update peaks near 21 GB. One that also updates Hindsight peaks near 27.5 GB.
- **Other numbers:**
  - Hindsight was healthy 68 s after starting.
  - With 500 memories, recall took a median 0.43 s (max 0.48 s), with no CPU limit.
  - Lemonade v2026.47.1 (newest stable) uses 2.8–3.3 GiB with Small and kokoro loaded. That's about 0.3 GiB more than v2026.45.3 in #42.

## Details

### 1. What ran

[`stack/compose.hw.yaml`](hindsight-hardware/stack/compose.hw.yaml) is laid over Rakazo v0.16.0's unmodified [`docker-compose.images.yml`](hindsight-hardware/stack/rakazo.v0.16.0.images.yml). It has the same shape as #42's layer, with Hindsight instead of Honcho.

| Service | Image (digest) | Notes |
|---|---|---|
| Rakazo api, worker, web, supervisor, migrate | local build of v0.16.0 (`6e24f7e6`) with upstream's `infra/compose/Dockerfile` | `app:v0.16.0` isn't published on GHCR (404) |
| postgres | `postgres:16@sha256:e17e8606…d3d5` (upstream's pin) | |
| Bot computer probe | `ghcr.io/elie222/rakazo/computer:v0.15.0@sha256:21c08ee7…8ce8` | `computer:v0.16.0` isn't published either |
| lemonade | `ghcr.io/lemonade-sdk/lemonade-server:v2026.47.1@sha256:715fbf04…913f` | newest stable release (2026-11-18), unmodified, `whispercpp.backend=cpu` |
| lemonade-relay | `oven/bun:1.4.4-alpine@sha256:4cf82f56…070a` | `relay/lang-code-relay.ts` from `prototype/compose-layout` |
| hindsight | `ghcr.io/vectorize-io/hindsight-api:1.3.0@sha256:c0547659…b242` | #45's env: GLM via OpenRouter at `REASONING_EFFORT=low`, pinned `BAAI/bge-small-en-v1.5` and `hindsight-ai/hs-rerank-1`, `RERANKER_MAX_CANDIDATES=100`, tenant key. Dummy LLM key with `SKIP_LLM_VERIFICATION=true` |
| hindsight-db | `pgvector/pgvector:pg18@sha256:38681dc6…9782` | `shm_size: 1g` |
| tailscale | `tailscale/tailscale:v1.104.4@sha256:ed6480b3…276c` | userspace, not logged in |

- The supervisor had no Docker socket. A bot computer was measured the way #42 did it: a probe container started with the supervisor's limits and options (2 GiB, 2 CPUs, `--cap-drop ALL`, no network).
- [`stack/measure-ram.sh`](hindsight-hardware/stack/measure-ram.sh) takes `docker stats` snapshots per stage. It reads the VM's `free -m` from a `--pid host` busybox and the Windows processes from `powershell.exe`.
- [`stack/use.py`](hindsight-hardware/stack/use.py) gives Hindsight work without a model:
  - It seeds 500 memories in `chunks` mode, which embeds each one with the local model.
  - It then runs 20 recalls the way Rakazo v0.16.0 sends them: budget `mid`, `max_tokens` 4,096. Each embeds the query and reranks up to 100 candidates.
- The full log is [`transcripts/win-ram.log`](hindsight-hardware/transcripts/win-ram.log).

### 2. RAM per part

`docker stats` memory in MiB, from the log. The Mac column is left to fill in from [`mac.md`](hindsight-hardware/mac.md).

| Part | #42 (Honcho, Lemonade v2026.45.3) | Windows / Linux, now | Mac |
|---|---|---|---|
| Rakazo: api, worker, web, supervisor, postgres | 775–1,020 | 750–1,098 | |
| Lemonade, no models loaded | 29 | 30 | native, see below |
| Lemonade with Whisper-Small + kokoro-v1 | about 2,500–3,060 | 3,032 after loading, 3,323 after first use, settling to 2,864–2,906 | |
| Native lemond + whisper-server (Turbo, Metal) + koko, footprint | — | — | |
| **Hindsight API + worker**, empty, idle | — | 1,601 | |
| Hindsight boot peak (start until 20 s after healthy) | — | 1,607 | |
| Hindsight peak while seeding 500 memories + 20 recalls | — | 1,697 | |
| Hindsight 30 s / 150 s after use, then idle | — | 1,469 / 1,376, then 1,360–1,373 | |
| **hindsight-db**: empty / with 500 memories | — | 82 / 126–165 | |
| Honcho: api, deriver, pgvector (replaced) | 580 | — | — |
| Tailscale, not logged in | 23 | 22–29 | |
| lemonade-relay | 10–16 | 6–12 | |
| One bot computer, idle / with Chromium on a `data:` page | 140–165 | 153 / 180 | |

**Whole-Instance totals**, summed from the snapshots:

| Stage | Windows / Linux, now | Mac |
|---|---|---|
| Memory off, no voice models | 1,133 MiB (1.1 GiB) | |
| Memory off, Small + kokoro loaded and used once | 3,967–4,257 MiB (3.9–4.2 GiB) | |
| + Hindsight, idle | 5,227–5,695 MiB (5.1–5.6 GiB) | |
| + Tailscale + one idle computer | 5,364 MiB measured; 5.2–5.7 GiB with Hindsight's range | |
| Same, with the computer at its 2 GiB limit | about 7.1–7.6 GiB (idle total − 153 MiB + 2,048 MiB) | |

- The higher end of each range is right after Hindsight starts or a model is first used. Like #42's figures, it settles lower after a few minutes.
- A computer "at its 2 GiB limit" is the cap, not a measurement. Real browsing wasn't measured, because the probe has no network.

**Docker Desktop VM on Windows:**
- The VM's `used` memory went from 3,797 MiB before the run (the other install and the VM itself) to 8,527 MiB at the idle peak with Hindsight, and 8,182 MiB at the end. That's +4.3–4.6 GiB.
- That's lower than the container sum, because `docker stats` also counts some page cache that `free` reports as cache.
- `vmmemWSL` held 13.4–15.0 GiB the whole time. WSL keeps freed memory as page cache unless the VM is under pressure, so its working set says little about what the Instance needs.
- Docker Desktop's Windows processes: `com.docker.backend` 326–352 MiB, `Docker Desktop` 285–299 MiB, `com.docker.build` 20–22 MiB, so 0.62–0.66 GiB in all, the same as #42's ~550 MiB plus a little.

### 3. Do the published minimums hold?

#42 published:

| OS | Minimum | Recommended |
|---|---|---|
| Linux (x86-64) | 4 threads, 8 GB RAM, 25 GB disk | 8 threads, 16 GB RAM, 50 GB disk |
| Windows | 4 threads, 16 GB RAM, 25 GB disk | 8 threads, 16 GB+ RAM, 50 GB disk |
| macOS | 16 GB RAM, 25 GB disk | 16 GB+ RAM, 50 GB disk |

- **Linux, 8 GB: holds idle, not under load.**
  - An 8 GB machine has about 7.6 GiB usable.
  - A headless distribution takes about 0.3–0.5 GiB, and a desktop session 1.5–2 GiB.
  - The idle Instance with memory on needs 5.2–5.7 GiB, so it fits.
  - A busy bot computer adds up to about 1.9 GiB more (7.1–7.6 GiB in all), which doesn't fit beside even a headless OS. #42's Honcho figure for the same case was about 6.0–6.8 GiB, which did.
  - Swap would carry a short spike, but Lemonade's models and Hindsight's resident memory sit in RAM all the time. So 8 GB Linux works for chat and memory, but not reliably for a bot computer that's browsing.
- **Windows, 16 GB: holds, tighter.** WSL gives the VM about half of the host's RAM, about 7.8 GiB on a 16 GB PC. The Instance needs 5.2–5.7 GiB idle and 7.1–7.6 GiB with a busy computer, before the VM's own kernel and daemons. The busy case is right at the limit.
- **macOS, 16 GB: not measured yet** ([`mac.md`](hindsight-hardware/mac.md)).
  - On a Mac, Docker Desktop's VM holds Rakazo, Hindsight and the computer, about 2.4–3.1 GiB idle by the figures above.
  - Native Lemonade adds about 3.2–3.4 GiB footprint with Turbo and kokoro (#42).
  - macOS and the user's apps need the rest.
- **25 GB of disk: holds for a fresh install, not for an update that changes everything** (section 4).

### 4. Disk

Image sizes are Docker's uncompressed sizes (`docker image inspect`), as in #42.

| | GB | Notes |
|---|---|---|
| Images | 14.55 | Rakazo app 3.08 (local v0.16.0 build; the published v0.15.0 is 3.00), computer 2.35 (v0.15.0), **hindsight-api 6.38**, Lemonade 1.08, pgvector:pg18 0.66, postgres:16 0.64, Tailscale 0.18, Bun 0.17, busybox 0.01 |
| Voice models | 0.91 | `lemonade-models` 0.84 (Whisper-Small + kokoro-v1) and `lemonade-data` 0.07 (backends) |
| Volumes | 0.24 | `pgdata` 74 MB, `webassets` 47 MB, `hindsight-pgdata` 120 MB after 500 memories (database 27 MB, the rest is WAL and catalog) |
| **Fresh Instance, Whisper-Small** | **about 15.7** | #42: about 9.5 |
| During an update, Rakazo only | about +5.4 | New app and computer images sit beside the old ones until the update succeeds. Then #42's decision removes the old ones |
| During an update that also changes Hindsight | about +11.8 | Hindsight's image alone is 6.38 GB |

- With these figures the 25 GB minimum leaves about 9 GB beside a fresh install. A Rakazo update peaks near 21 GB, and a Rakazo plus Hindsight update near 27.5 GB.
- On Windows, Docker Desktop's virtual disk grows and doesn't shrink on its own (#42), so the peak stays used afterwards.
- For a Turbo default, add 1.13 GB: Turbo is 1.62 GB, against Small's 0.49.

### 5. Found along the way

- **Hindsight retries consolidation for ever without a model.** With the dummy key, every seeded memory queued a consolidation task. The worker retried it with backoff for the whole run, and logged 57 `ERROR` or traceback lines. That didn't change RAM, but an Instance whose model key is wrong or out of credit will log this continuously. Setup's checks or the README's troubleshooting should mention it.
- **Rakazo v0.16.0 has no published images** (also found in [#45](https://github.com/kamui/rakazo-stack/issues/45)). This run used a local build of the tag.
- **The supervisor without a socket stays healthy**, so `wait_healthy` worked the same as in #42.

## Open questions / follow-ups

- **grilling:** Is 8 GB still the Linux minimum? It holds for chat and memory, but not for a bot computer browsing at its 2 GiB cap. The options:
  - Raise the minimum to 12 or 16 GB.
  - Keep 8 GB and have preflight warn that bot computers may be killed under load.
  - Lower the computer's memory cap on machines under 12 GB.
- **grilling:** Should the disk minimum rise from 25 GB to 30 GB, now that a fresh install is about 15.7 GB and an update that changes both Rakazo and Hindsight peaks near 27.5 GB? The alternative is for `rakazo-stack update` to update Hindsight and Rakazo in separate steps.
- **task:** Run [`mac.md`](hindsight-hardware/mac.md) on an Apple-silicon Mac with Docker Desktop running, and fill in the Mac column of section 2. #42 couldn't measure Docker Desktop on the Mac either.
- **task:** Update the README's hardware guide and preflight thresholds once the minimums are decided.
- **research:** Hindsight's RAM while it extracts and consolidates with a live model wasn't measured here (#45's stack wasn't instrumented for RAM). The local work in those paths is embedding and reranking, which `use.py` exercised. Confirm on a stack with a real key.
- **Unverified:** Linux figures are derived from Docker Desktop, not measured on bare Linux. The computer's 2 GiB is its cap, not a measured browsing load.

## Sources

- Scripts and the log: [`hindsight-hardware/stack/`](hindsight-hardware/stack/) and [`hindsight-hardware/transcripts/win-ram.log`](hindsight-hardware/transcripts/win-ram.log).
- #42's method, figures and decision: [`research/hardware-floor`](https://github.com/kamui/rakazo-stack/tree/research/hardware-floor/docs/research/hardware-floor) and its answer on [#42](https://github.com/kamui/rakazo-stack/issues/42).
- #44's Hindsight figures: [`research/hindsight-needs`](https://github.com/kamui/rakazo-stack/blob/research/hindsight-needs/docs/research/hindsight-needs.md), section 8.
- Rakazo v0.16.0: https://github.com/elie222/rakazo/tree/v0.16.0 (`infra/compose/docker-compose.images.yml`, `infra/compose/Dockerfile`); GHCR manifests for `app` and `computer` (anonymous registry API, 2026-11-25).
- Lemonade releases: https://github.com/lemonade-sdk/lemonade/releases (v2026.47.1 is the newest non-prerelease on 2026-11-25).
- Hindsight 1.3.0: https://github.com/vectorize-io/hindsight/tree/v1.3.0.
