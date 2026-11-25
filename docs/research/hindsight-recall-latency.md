# Is Hindsight's recall fast enough on slow CPUs and Apple silicon?

Ticket: kamui/rakazo-stack#47. Versions: Hindsight v1.3.0 (commit `93788619`), image `ghcr.io/vectorize-io/hindsight-api:1.3.0` (`sha256:c054765e…`), `pgvector/pgvector:pg18` (`sha256:38681dc6…`, PostgreSQL 18.7, pgvector 0.8.7) and Rakazo v0.16.0 (commit `6e24f7e6`). Measured on one x86-64 host: an AMD Ryzen 9 7900X (12 cores / 24 threads, Zen 4, AVX-512) running Docker Desktop 29.9.3 on Windows with the WSL 2 backend (24 CPUs and 15.45 GiB in the Docker VM). Apple silicon was not measured (section 6). No model provider key was used and nothing was spent.

## Answer

- **Not with Hindsight's defaults on a slow CPU.** Two recalls at once can pass Rakazo's 15 s timeout. Every bot reply in the Shared scope makes two recalls at once.
  - On 2 cores / 4 threads of this host, warm `mid` recall over 5,000 memories takes **3.0 s** median (p95 3.5 s). With two at once it takes 5.0–5.1 s, and with four at once 8.8 s.
  - When Hindsight gets about half of each core (a stand-in for a 4-thread CPU half as fast), `mid` takes **7.9 s** alone and **10.5 s** two at once (max 13.2 s).
  - At about a third of each core it takes **8.7 s** alone and **14.1 s** two at once: p95 15.6 s, max 16.0 s, past the timeout.
  - When recall times out, Rakazo logs an error and replies without memory.
- **The reranker is the cost.** Hindsight's own per-phase log puts reranking at more than 90% of every recall, with retrieval at 40–80 ms. That matches upstream's own docs.
  - The cost follows how many candidates are reranked. `mid` and `high` hit Hindsight's cap of 300 at this bank size, so a larger bank won't make recall much slower.
  - Per-core speed matters far more than core count: 1 core / 2 threads (3.7 s) is barely slower than 2 cores / 4 threads (3.0 s).
- **Cold and warm are the same.** The models load at boot, which takes 58–65 s. The first recall after that is within about 10% of the warm ones.
- **Mitigations, measured:**

  | Setting | ½-speed `mid`, alone / two at once | ⅓-speed `mid`, alone / two at once | Top 5 same as default `mid` | Relevance proxy |
  | --- | --- | --- | --- | --- |
  | Default (`local` reranker, 300 candidates) | 7.9 / 10.5 s | 8.7 / 14.1 s | n/a | 85% |
  | **`HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100`** | **2.6 / 3.5 s** | **2.9 / 4.0 s** | **89–93%, same top result 50/50** | **85%** |
  | `HINDSIGHT_API_RERANKER_PROVIDER=flashrank` | 3.1 / 4.5 s | 3.1 / 5.2 s | 57%, same top result 19/30 | 88% |
  | `recallBudget: low` (default reranker) | 4.3 s alone | 4.7 s alone | 97%, same top result 30/30 | 85% |

- **Recommendation: setup writes `HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100` for the `hindsight` service and leaves Rakazo's `recallBudget` at `mid`.**
  - It keeps the shipped model and needs no download.
  - Recall is about 3× faster, and the result list barely changes.
  - Even at a third of a core, it leaves 3.7× headroom under the timeout with two recalls at once.
- **Why not the other two:**
  - FlashRank is fast, but its model isn't in the image: it downloads 33 MB from Hugging Face on first use, and the stack's `hindsight` service has no internet. It also changes about half of the top 5.
  - `low` alone doesn't fix the two-at-once case.
- **Apple silicon is unmeasured.** [measure.sh](hindsight-recall-latency/measure.sh) runs the whole measurement on a Mac in about an hour (section 6).
  - The x86 numbers settle the decision: with the cap, a Mac core would have to be about 11 times slower than a Zen 4 desktop core before two recalls at once timed out.
  - Running it on a Mac is a confirmation task, not a blocker.

## Details

### 1. How Rakazo calls recall

- **When.** Rakazo v0.16.0 recalls before every reply when **Recall automatically** is on, which is the default. The recall runs alongside Rakazo's other context loading ([run-assembly-history.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/runs/assembly/run-assembly-history.ts#L209-L262)).
  - The timeout is `HINDSIGHT_RECALL_TIMEOUT_MS = 15_000` ([hindsight-client.ts#L13](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-client.ts#L13); applied at [hindsight-memory-provider.ts#L457](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L457)).
  - A failed or timed-out recall logs `semantic memory recall failed`, and the reply goes ahead without memory ([run-assembly-history.ts#L262](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/runs/assembly/run-assembly-history.ts#L262)).
- **Body.** `budget` comes from the Space's `recallBudget`: `mid` unless changed ([#L63](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L63)). The rest is fixed:
  - `max_tokens: 4096` ([#L50](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L50)).
  - `types: ["observation"]`, or all three types when **Recall facts** is on ([#L472](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L472)).
  - Rakazo keeps the top 5 (`MAX_RECALLED_MEMORIES = 5`, passed as `limit` at [run-assembly-history.ts#L223](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/runs/assembly/run-assembly-history.ts#L223)).
- **Concurrency.** Rakazo recalls the bot's own bank and, in the Shared scope, the Space's bank at the same moment (`Promise.all` at [#L476](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L476); `writeBanks` at [#L287-L290](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L287-L290)). So "two at once" is every Shared-scope reply, or two chats replying at once. Hindsight runs at most 4 reranks at a time (`DEFAULT_RERANKER_LOCAL_MAX_CONCURRENT = 4`, [config.py#L1528](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L1528)).

### 2. Method

- **Containers.** `hindsight-api:1.3.0` against its own `pgvector/pgvector:pg18` on an `--internal` network. The settings were a dummy OpenAI key, `HINDSIGHT_API_SKIP_LLM_VERIFICATION=true` and `HINDSIGHT_API_ENABLE_AUTO_CONSOLIDATION=false`.
  - Consolidation was turned off because, with no reachable LLM, every retain queues consolidation calls that retry in the background and would add load during timing.
  - Scripts: [up.sh](hindsight-recall-latency/up.sh), [config.sh](hindsight-recall-latency/config.sh).
- **Data.** 5,000 synthetic memories retained with `retain_extraction_mode: chunks`, so no LLM was involved ([seed.py](hindsight-recall-latency/seed.py)). Each memory is a templated note of about 200–450 characters: people, places, topics and dates. They are all stored as `world` facts.
  - Rakazo's default recalls only `observation`s, which the LLM produces during consolidation, so chunks mode can't make them. The benchmark therefore recalls all three types, which is Rakazo's **Recall facts** shape.
  - Recall returned about 86 results, and `mid` and `high` reranked the full 300 candidates (Hindsight's log: `300 candidates scored`). A real bank with this many observations sends the reranker the same amount of work. Real memory text that is much longer than these notes would make each rerank slower (**unverified**).
- **Client.** [bench.py](hindsight-recall-latency/bench.py) sends Rakazo's body (`max_tokens: 4096`, a budget) from a separate container pinned to two other threads. It cycles 30 different questions and reports the median and p95.
  - Most configurations ran 60 recalls per budget; the ½ and ⅓ stand-ins ran 30 and 20, the 1 core / 2 threads run ran 30, and the post-reboot repeat ran 30.
- **Cold.** "Cold" is the first recall after recreating both containers. The OS page cache stayed warm.
- **Slow-CPU stand-ins.** API and database got the same limits:
  - `--cpus 4` and `--cpus 2`: CFS quota, any of 20 host threads.
  - `--cpuset-cpus` on 2 cores / 4 threads (16–19) or 1 core / 2 threads (18–19). The WSL 2 topology pairs vCPUs 2n and 2n+1 on one core; whether Hyper-V keeps that mapping is **unverified**.
  - "½ speed" and "⅓ speed": 2 cores / 4 threads, sharing those threads with one or two busy-loop containers ([hog.sh](hindsight-recall-latency/hog.sh)). Each competing container takes an equal CFS share, so Hindsight gets about ½ or ⅓ of each thread while it works.
  - This stands in for a 4-thread CPU with slower cores, but only roughly: a real slow CPU also has smaller caches, and often no AVX-512, which Zen 4 has and Torch uses.
  - Hindsight sizes its native thread pools from the cgroup quota and CPU affinity, not the host's count ([_thread_limits.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/_thread_limits.py#L125)), so these limits reach Torch.
- **Host load.** Other containers on the host belonged to an idle Rakazo/Lemonade/Hindsight install, which wasn't stopped.
  - They used 1–50% of one CPU at the sampled points, under 15% most of the time ([other.sh](hindsight-recall-latency/other.sh) logged it before every measurement).
  - The host load average was 1–3, apart from a short spike to about 14 during the `--cpus 2` `low` and `high` runs.
  - The first round of runs (`--cpus 4`, `--cpus 2`, 1 core / 2 threads, concurrency, FlashRank, `rrf`) was cut off by a host reboot after it finished. Its summary numbers were recorded, but its per-recall logs were lost.
  - The 2 cores / 4 threads baseline was rerun after the reboot and matched (3.03 s `mid` both times).

### 3. Latency with the defaults

Median / p95 in seconds, warm, one recall at a time unless noted:

| Stand-in | Cold `mid` (first after boot) | `low` | `mid` | `high` | `mid`, 2 at once | `mid`, 4 at once |
| --- | --- | --- | --- | --- | --- | --- |
| `--cpus 4` | 2.97, 2.95 | 1.80 / 3.03 | 3.07 / 3.27 | 3.17 / 3.63 | | |
| `--cpus 2` | 4.13, 3.96 | 2.41 / 4.22 | 3.76 (3-run warm-up; 60-run line lost) | 4.40 / 4.98 | | |
| 2 cores / 4 threads | 2.88, 2.74 | 1.72 / 2.80 | 3.03 / 3.48 | 3.20 / 3.40 | 5.13 / 5.92 | 8.78 / 9.35 |
| same, after host reboot | 3.30 | 1.62 / 2.61 | 3.03 / 3.47 | | 5.00 / 5.97 | |
| 1 core / 2 threads | 3.86 | 1.99 / 3.31 | 3.71 / 3.88 | 3.62 / 4.02 | | |
| **½ speed** (2C/4T shared with 1 hog) | 7.35 | 4.29 / 7.14 | **7.93 / 8.23** | | **10.50 / 11.66 (max 13.19)** | |
| **⅓ speed** (2C/4T shared with 2 hogs) | 9.00 | 4.70 / 7.49 | **8.65 / 9.48** | | **14.07 / 15.58 (max 16.04)** | |

- **The reranker dominates.** Hindsight logs a `[phases]` line for every recall. On 2 cores / 4 threads with the local reranker, two at once logged `reranking=4729–5043ms, parallel_retrieval=41–49ms, generate_query_embedding=22–30ms`, with everything else under 5 ms.
  - Upstream: "the local reranker (cross-encoder) is the main bottleneck" ([installation.md#L85](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md?plain=1#L85)).
  - The model is `hindsight-ai/hs-rerank-1`: a 250 MB fp32 ModernBERT cross-encoder (hidden size 384), run through sentence-transformers on Torch ([config.py#L1526](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L1526); `config.json` in the image's Hugging Face cache).
- **Budgets** set Hindsight's retrieval depth: `DEFAULT_RECALL_BUDGET_FIXED_LOW/MID/HIGH = 100 / 300 / 1000` ([config.py#L2456-L2458](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L2456-L2458)).
  - The reranker caps candidates at `DEFAULT_RERANKER_MAX_CANDIDATES = 300` ([#L1559](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L1559)).
  - `low` reranked about 150–250 candidates here, while `mid` and `high` both reranked 300. That is why `mid` and `high` take about the same time, and `low` about half.
  - `low`'s p95 is high because a few questions still fill the candidate list.
- **Cores barely matter; core speed does.** Going from 2 cores / 4 threads to 1 core / 2 threads added only 22% at `mid`. Halving the share of each thread added 160%. So a slow 4-thread CPU is slow because of its per-core speed. Taking cores away from a fast host is not a good model of one.
- **Cold.** The first recall after a full restart was within about 10% of warm in every configuration (the "Cold" column), since the embedder and reranker load at boot. Boot took 58–65 s with the local reranker, and 35–37 s with FlashRank or `rrf`.
- **/dev/shm** in both containers stayed far below Docker's 64 MB during all of these runs: at most 8.6 MB in the database (four recalls at once) and 4 KB in the API. That question is [#48](https://github.com/kamui/rakazo-stack/issues/48).

### 4. Mitigations

All on 2 cores / 4 threads. Median / p95 in seconds.

| Setting | Stand-in | `low` | `mid` | `mid`, 2 at once |
| --- | --- | --- | --- | --- |
| Default | full | 1.62 / 2.61 | 3.03 / 3.47 | 5.00 / 5.97 |
| FlashRank | full | 0.73 / 1.07 | 1.14 / 1.35 | 2.06 / 2.26 |
| `rrf` (no reranker) | full | 0.07 | 0.07 | |
| Default | ½ | 4.29 / 7.14 | 7.93 / 8.23 | 10.50 / 11.66 |
| `MAX_CANDIDATES=100` | ½ | 2.68 / 2.89 | 2.64 / 2.85 | 3.48 / 4.07 |
| FlashRank | ½ | 1.76 / 2.81 | 3.08 / 3.42 | 4.55 / 5.50 |
| Default | ⅓ | 4.70 / 7.49 | 8.65 / 9.48 | 14.07 / 15.58 |
| `MAX_CANDIDATES=100` | ⅓ | 2.98 / 3.38 | 2.87 / 3.10 | 4.01 / 4.43 |
| FlashRank | ⅓ | 2.18 / 3.06 | 3.07 / 3.29 | 5.18 / 5.99 |

An earlier FlashRank run on the same 2 cores / 4 threads, before the reboot, gave 0.74 / 1.26 / 1.32 s (`low` / `mid` / `high`). The cap also wins with no limits at all: unrestricted on 24 threads, `mid` was 0.94 s with `MAX_CANDIDATES=100`, 2.09 s with the default and 1.83 s with FlashRank (2-run smoke test of [measure.sh](hindsight-recall-latency/measure.sh)).

**Which memories come back.** Two measures, comparing each setting with the default `mid` on the same bank:

- **Overlap.** The share of the top 5 that is the same, and how often the top result is the same, computed from the recorded result ids ([overlap.py](hindsight-recall-latency/overlap.py)).
- **Relevance proxy.** For 16 questions with an unambiguous answer phrase (for example "Who is learning the cello?" → "cello"), the share of the top 5 that contains every key phrase ([quality.py](hindsight-recall-latency/quality.py)). Some questions have fewer than 5 matching notes, so 100% isn't reachable; only compare the scores with each other.

| Setting | Top 5 same | Top result same | Relevance proxy (`low` / `mid` / `high`) |
| --- | --- | --- | --- |
| Default `low` | 97% | 30/30 | 85% (top result relevant 14/16) at every budget |
| Default `high` | 99% | 30/30 | |
| Default `mid` on `--cpus 4` vs 2 cores / 4 threads | 100% | 30/30 | |
| `MAX_CANDIDATES=100`, `mid` | 89% (½ run), 93% (⅓ run) | 30/30, 20/20 | 85% / 85% / 85%, top 14/16 |
| `MAX_CANDIDATES=100`, `low` | 85–90% | 28/30, 19/20 | |
| FlashRank `mid` | 57% | 19/30 | 88% / 88% / 88%, top 14/16 |
| FlashRank `low` | 54% | 20/30 | |
| `rrf` `mid` | 18% | 1/30 | not measured |

- **`HINDSIGHT_API_RERANKER_MAX_CANDIDATES`** ([config.py#L749](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L749); per-budget `…_LOW/_MID/_HIGH` overrides at [#L752-L754](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L752-L754)) caps how many fused candidates the cross-encoder scores. The rest keep their fusion order.
  - At 100, the top result was the same in every comparison, nine in ten of the top 5 matched, and the relevance proxy didn't move.
  - It is one environment variable on an image the stack already pins.
- **FlashRank** is in the image as a library: `flashrank` 0.2.10 is in the `local-ml` extra ([pyproject.toml#L137](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/pyproject.toml#L137)), and the provider value is `flashrank` ([cross_encoder.py#L2411](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/cross_encoder.py#L2411)). Its model is not in the image:
  - The image build preloads only `BAAI/bge-small-en-v1.5`, `hs-rerank-1` and EmbeddingGemma ([Dockerfile#L230-L245](https://github.com/vectorize-io/hindsight/blob/v1.3.0/docker/standalone/Dockerfile#L230-L245)).
  - FlashRank's default `ms-marco-MiniLM-L-12-v2` ([config.py#L1695](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L1695)) is downloaded from `huggingface.co/prithivida/flashrank` into `/tmp` on first use. It is 33 MB, and nothing in the image holds it.
  - Here it was downloaded once into a volume and mounted read-only with `HINDSIGHT_API_RERANKER_FLASHRANK_CACHE_DIR=/fr`. The stack would have to do the same, or give `hindsight` internet access.
  - What a FlashRank boot does with no network and no cache wasn't tried (**unverified**: probably a failed reranker initialisation).
  - It changes about half of the top 5. The proxy suggests that isn't worse on these notes, but a different model on real memories needs its own check.
- **`recallBudget: low`** is a per-Space Rakazo setting. Setup would have to write it into each Space's memory connection. It keeps almost the same top 5, but it only saves about 45% and doesn't fix two recalls at once at ½ or ⅓ speed. It can still be offered later; the candidate cap makes it mostly unnecessary.
- **`rrf`** turns reranking off. It is 40× faster, but only 18% of the top 5 survives, so it's not a reasonable default.

### 5. Recommendation for the stack

- **Set `HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100` on the `hindsight` service**, next to the pinned `HINDSIGHT_API_RERANKER_LOCAL_MODEL` from [What does Hindsight need?](https://github.com/kamui/rakazo-stack/issues/44).
  - It is an Instance-wide default. A Self-hoster on a fast machine can remove it in the settings file.
  - Upstream may change how the cap is used in a later release, so the weekly canary from [How does the stack pin and upgrade its components?](https://github.com/kamui/rakazo-stack/issues/32) should keep recall timing in its checks.
- **Leave Rakazo's `recallBudget` at its default (`mid`).** With the cap, `mid` costs the same as `low`.
- **Hardware text.** On a CPU much slower than a current desktop, the first reply of a conversation can still wait about 3–4 s for memory. That's worth saying in [Re-measure the hardware an Instance needs with Hindsight](https://github.com/kamui/rakazo-stack/issues/46)'s README text. It's not a reason to raise the 4-thread minimum.

### 6. Apple silicon (not measured)

There was no Mac here.

- Upstream lists macOS on Apple silicon as fully supported ([installation.md#L13-L18](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md?plain=1#L13-L18)), and the image is published for linux/arm64.
- Under Docker Desktop, Hindsight runs inside a Linux VM, so Metal isn't available. Hindsight never selects Torch's MPS backend anyway ([local_device.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/local_device.py)).
- So the reranker runs on arm64 CPU kernels instead of x86's AVX-512 ones. How that compares per core is the open part (**unverified**).
- The decision doesn't hang on it: with the candidate cap, two recalls at once took 4.0 s on the ⅓-speed stand-in. A Mac core would have to be about 11× slower than a Zen 4 desktop core to reach the timeout.

**To finish it**, on an Apple-silicon Mac with Docker Desktop running (about 1 hour and about 8 GB of disk; no key needed):

```sh
git clone https://github.com/kamui/rakazo-stack.git && cd rakazo-stack
git checkout research/hindsight-recall-latency
cd docs/research/hindsight-recall-latency
./measure.sh 2>&1 | tee mac-full.txt            # Docker Desktop's default CPU allotment
CPUS=4 KEEP=1 ./measure.sh 2>&1 | tee mac-4cpu.txt   # the 4-thread floor
docker volume rm hsr-pgdata hsr-flashrank; docker network rm hsr-net
```

[measure.sh](hindsight-recall-latency/measure.sh) prints:

- the chip, its performance and efficiency core counts, and the Docker VM's CPUs and memory
- the image architecture (it should say `arm64`)
- for each of the default reranker, `MAX_CANDIDATES=100` and FlashRank: cold `mid`, then `low`, `mid` and `high` warm, then `mid` with 2 and 4 at once, plus Hindsight's `[phases]` lines

Paste both files into the ticket. The stack's choice holds if `MAX_CANDIDATES=100` with 2 at once stays well under 15 s, say under 5 s. The run was smoke-tested on this x86 host (2 recalls per measurement).

### 6a. Apple silicon (measured)

- **Machine.** MacBook Pro (`Mac15,6`), Apple M3 Pro, 11 cores (5 performance, 6 efficiency), 18 GB, macOS 27.2.1 (26B131). Docker Desktop 4.97.0 (engine 29.9.2, `linux/arm64`, kernel 7.0.14-linuxkit), with its default VM of 11 CPUs and 7.75 GiB.
- **Software.** The same image digests as the x86 run: `hindsight-api:1.3.0` `sha256:c054765e…`, pulled as `arm64`, and `pgvector/pgvector:pg18` `sha256:38681dc6…` (PostgreSQL 18.7).
- **Run.** The two commands from section 6, unchanged, on 2026-11-25 from 05:21 to 06:15 UTC. `measure.sh` needed no changes on macOS. Raw output: [results/mac.log](hindsight-recall-latency/results/mac.log). Each warm measurement ran 60 recalls, each cold one a single recall, and no recall failed.
- **Host load.** Docker Desktop had just been started and ran nothing else. Before the run the load average was 2.8 / 2.0 / 1.7 and 0.87 GB of swap was in use. By the end, swap was 3.9 GB, and macOS background services (XProtect, media indexing, `logd`) were using about 3–4 cores. Processes weren't sampled during the run, so how much they overlapped the timings is **unverified**.
- **Boot.** 56–57 s with the local reranker and 32 s with FlashRank. Seeding 5,000 memories took 92–104 s.

Median / p95 in seconds. The x86 column is the closest x86 measurement from sections 3 and 4, with its stand-in named; "24T" is the unrestricted 2-run smoke test.

| SUM label (budget) | Mac, full VM (11 CPUs) | Mac, `CPUS=4` | x86 |
| --- | --- | --- | --- |
| `default-cold` (`mid`) | 2.22 | 2.51 | 2.97, 2.95 (`--cpus 4`) |
| `default` `low` | 1.23 / 1.86 | 1.35 / 2.25 | 1.80 / 3.03 (`--cpus 4`) |
| `default` `mid` | 2.09 / 2.45 | 2.42 / 2.53 | 3.07 / 3.27 (`--cpus 4`); 2.09 (24T) |
| `default` `high` | 2.07 / 2.16 | 2.44 / 2.54 | 3.17 / 3.63 (`--cpus 4`) |
| `default-conc2` (`mid`, 2 at once) | 3.59 / 3.69 | 5.64 / 5.84 | 5.13 / 5.92 (2C/4T) |
| `default-conc4` (`mid`, 4 at once) | 6.49 / 6.63 | **14.64 / 15.21 (max 15.28)** | 8.78 / 9.35 (2C/4T) |
| `cand100-cold` (`mid`) | 0.83 | 0.90 | |
| `cand100` `low` | 0.74 / 0.77 | 0.84 / 0.90 | 2.68 / 2.89 (½) |
| `cand100` `mid` | 0.74 / 0.79 | 0.84 / 0.89 | 0.94 (24T); 2.64 / 2.85 (½) |
| `cand100` `high` | 0.75 / 0.77 | 0.84 / 0.90 | |
| **`cand100-conc2`** (`mid`, 2 at once) | **1.23 / 1.27** | **1.88 / 1.98 (max 2.04)** | 3.48 / 4.07 (½); 4.01 / 4.43 (⅓) |
| `cand100-conc4` (`mid`, 4 at once) | 2.25 / 2.77 | 4.80 / 5.00 (max 5.04) | |
| `flashrank-cold` (`mid`) | 1.38 | 1.48 | |
| `flashrank` `low` | 0.75 / 1.24 | 0.79 / 1.31 | 0.73 / 1.07 (2C/4T) |
| `flashrank` `mid` | 1.31 / 1.38 | 1.38 / 1.46 | 1.14 / 1.35 (2C/4T); 1.83 (24T) |
| `flashrank` `high` | 1.31 / 1.38 | 1.40 / 1.47 | 1.32 (2C/4T, before the reboot) |
| `flashrank-conc2` (`mid`, 2 at once) | 1.88 / 2.01 | 2.72 / 2.85 | 2.06 / 2.26 (2C/4T) |
| `flashrank-conc4` (`mid`, 4 at once) | 3.45 / 3.64 | 6.86 / 7.32 | |

- **The 15 s recall timeout is not at risk with `HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100`.** On 4 CPUs, two recalls at once took 1.88 s (max 2.04 s, about 7× headroom), and four at once took at most 5.04 s.
- **Per recall, the M3 Pro is a little faster than the Zen 4 desktop.** With 4 CPUs, a single default `mid` recall took 2.42 s against 3.07 s on x86. Two at once was about the same (5.64 s against 5.13 s on 2C/4T).
- **Without the cap, four at once on 4 CPUs reaches the timeout.** Its p95 was 15.21 s, and Hindsight logged `reranking=13840–13959ms`. That is 2.6× the two-at-once time, where x86 2C/4T scaled 1.7×.
  - With the cap, the same case took 4.80 s, which strengthens section 5's recommendation.
  - `--cpus 4` is a CFS quota spread over all 11 VM CPUs, so the work may have landed on efficiency cores. Why it scales worse than on x86 is **unverified**.
- **The reranker is still the cost.** In the `[phases]` lines it takes 2.6–14.0 s, retrieval 31–85 ms and the query embedding 60–180 ms.
- **FlashRank is slower than the cap here** (1.31 s against 0.74 s for `mid` on the full VM). The cap also beat FlashRank on the unrestricted x86 host. So on this Mac, FlashRank has no speed advantage to weigh against its download and its changed results.

## Open questions / follow-ups

- **task:** Run [measure.sh](hindsight-recall-latency/measure.sh) on an Apple-silicon Mac (an 8 GB M1 or M2 if possible) and on a real slow 4-thread x86 machine (an Intel N100 or a 6th–8th-generation i3 or i5), and post the output on #47.
- **task:** Put `HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100` in the `hindsight` service's environment wherever the compose layout from [#10](https://github.com/kamui/rakazo-stack/issues/10) lists Hindsight's variables. Add a recall timing (`mid`, two at once, under 15 s) to the canary from #32.
- **research:** Recall latency on real memories. Measure on a bank built by LLM extraction (observations, the type Rakazo recalls by default), which [Run Hindsight on the default models through Rakazo](https://github.com/kamui/rakazo-stack/issues/45) will produce. Check that observation text length doesn't change the picture.
- **grilling:** Should setup offer FlashRank at all? It's faster on slow CPUs, but it needs a model download on a service that has no internet, and it changes results.
- **Unverified in this research:** that the ½ and ⅓ stand-ins match a real slow CPU (no AVX-512, smaller caches); arm64 per-core speed for this model; what a FlashRank boot does with no network; how long real observations are.

## Sources

Hindsight v1.3.0 (https://github.com/vectorize-io/hindsight/tree/v1.3.0):
- `hindsight-api-slim/hindsight_api/config.py` (reranker, budget and consolidation defaults), `_thread_limits.py`, `engine/cross_encoder.py`, `engine/local_device.py`, `hindsight-api-slim/pyproject.toml`
- `docker/standalone/Dockerfile`
- `hindsight-docs/docs/developer/installation.md` (Supported Platforms, Hardware)
- Local runs of `ghcr.io/vectorize-io/hindsight-api:1.3.0` (amd64) on `pgvector/pgvector:pg18`, with Hindsight's `[phases]` recall log lines, the FlashRank 0.2.10 package and the `hs-rerank-1` `config.json` read from the image

Rakazo v0.16.0 (https://github.com/elie222/rakazo/tree/v0.16.0):
- `packages/adapters/src/providers/memory/hindsight/hindsight-client.ts`, `hindsight-memory-provider.ts`
- `packages/adapters/src/runs/assembly/run-assembly-history.ts`, `packages/adapters/src/runtime/context/history-compaction.prompt.ts` (`MAX_RECALLED_MEMORIES = 5`)

FlashRank 0.2.10 `Config.py` (model URL `https://huggingface.co/prithivida/flashrank/resolve/main/{}.zip`, default cache `/tmp`), read from the image.

Earlier rakazo-stack research: `research/hindsight-needs` (section 2, the 500-memory benchmark).

Scripts used: everything in [hindsight-recall-latency/](hindsight-recall-latency/).
