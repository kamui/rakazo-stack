# Does Hindsight need a larger shared memory size?

Ticket: kamui/rakazo-stack#48. Versions: Hindsight v1.3.0 (commit `93788619`), image `ghcr.io/vectorize-io/hindsight-api:1.3.0` (`sha256:c054765e…`), `pgvector/pgvector:pg18` (`sha256:38681dc6…`, PostgreSQL 18.7, pgvector 0.8.7) and Rakazo v0.16.0 (commit `6e24f7e6`). Tested on x86-64 Docker Desktop 29.9.3 on Windows with the WSL 2 backend. No model provider key was used and nothing was spent.

## Answer

- **The `hindsight` container doesn't need it; `hindsight-db` does. Set `shm_size: 1g` on `hindsight-db` and nothing on `hindsight`.**
- **Why upstream passes `--shm-size=1g`.** It is for the PostgreSQL embedded in the single-container image (pg0), not for Torch.
  - The flag was added in Sept 2026 by [PR #3896](https://github.com/vectorize-io/hindsight/pull/3896), "reserve shared memory for embedded PostgreSQL", closing [#3895](https://github.com/vectorize-io/hindsight/issues/3895). In Hindsight 0.9.2, pg0 tried to allocate a 533,794,976-byte shared-memory segment during bank index creation, and Docker's 64 MB `/dev/shm` refused it.
  - pg0 starts Postgres with `maintenance_work_mem=512MB` and `max_parallel_maintenance_workers=4` (measured).
  - Upstream's own compose file for an external database sets no `shm_size` on either service.
- **Torch, the embedder and the reranker don't use `/dev/shm`.** They run in-process on thread pools. Hindsight turns tokenizer parallelism off, and its only process pools are the boot-time migration runner and Knowledge-bank media parsing, which Rakazo doesn't use. Across 15,000 retains, recalls up to four at once, and every latency run in #47, the API container's `/dev/shm` never held more than **4 KB**.
- **The stack's separate pgvector database does need it, and fails without it.**
  - Hindsight builds three partial HNSW indexes on the shared `memory_units` table every time a bank is created. Rakazo creates a bank on the first write of each new bot and each Space's Shared-scope bank.
  - Once that table passes 8 MB (15,000 short notes here, from every bank combined), Postgres builds those indexes with parallel workers.
  - pgvector then sizes the shared segment to `maintenance_work_mem` minus 3 MB: 61 MB at the image's default 64 MB. Its source says this was chosen to fit Docker's 64 MB default exactly.
  - One build filled `/dev/shm` to 64,972 of 65,536 KiB. Postgres keeps some segments after use (3.5–13.4 MB measured here), and a recall's parallel query uses up to 8.6 MB.
  - So **every bank creation after the first on a database failed with HTTP 500 `could not resize shared memory segment … to 63999840 bytes: No space left on device`**: 19 of 19, whether alone, under recall load or two at once. The two that worked were each the first on a fresh database.
  - With `shm_size` 1g (13 banks) or 256m (9 banks), every one passed. The peak was 71.7 MB.
- **What a Self-hoster would see:** a bot added later, or a Space switched to the Shared scope, never gets memory. `save_memory` returns an error to the bot, and the learn-from-turn job keeps retrying into the same 500.
- **Defaults.** Docker Engine and Docker Desktop both give containers 64 MB (`--default-shm-size`, measured on Docker Desktop for Windows). Docker Desktop for Mac uses the same engine default (**unverified** on a Mac).
- **Value: `1g`.**
  - It matches upstream's flag and pgvector's README.
  - It is a cap on a `tmpfs`, not memory set aside: pages count against RAM only while Postgres uses them, and that measured 72 MB at most.
  - 256m also passed every test. 1g leaves room if `maintenance_work_mem` is ever raised for faster index builds.

## Details

### 1. Where `--shm-size=1g` comes from

- It appears only in the pg0 examples: `docker run … --shm-size=1g --stop-timeout 75` in [installation.md#L98](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md?plain=1#L98), under "Run everything in one container with embedded PostgreSQL", and the same line in [api/quickstart.mdx#L46](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/api/quickstart.mdx?plain=1#L46).
- `git log -S'shm-size'` on v1.3.0 finds one change, commit [`88f1472e`](https://github.com/vectorize-io/hindsight/commit/88f1472e52551d290e39fc9c6d8c608bc613213a) (2026-09-01): "docs(docker): reserve shared memory for embedded PostgreSQL (#3896)".
  - The PR's "Why": "Hindsight 0.9.2 attempted to allocate a 533,794,976-byte PostgreSQL shared-memory segment during bank index creation. Under Docker's default 64 MiB `/dev/shm`, that failed with `DiskFullError: could not resize shared memory segment ... No space left on device`. Recreating the container with 1 GiB shared memory resolved the failure."
  - [#3895](https://github.com/vectorize-io/hindsight/issues/3895) adds that 1 GiB "is a safe example rather than a universal minimum".
  - [#2695](https://github.com/vectorize-io/hindsight/issues/2695) saw the same 64 MB error earlier, on a self-hosted Postgres with pgvector 0.8.1 and `shm_size=64MB`. There, retain reported success while nothing was stored.
- **The 533,794,976 bytes.** In a pg0 container (v1.3.0, `pg0-embedded` 0.17.0), `pg_settings` shows `maintenance_work_mem = 524288 kB` and `max_parallel_maintenance_workers = 4`, both with source "command line": pg0 passes them when it starts Postgres. pgvector's parallel build reserves `maintenance_work_mem` minus 3 MB (section 3). For 512 MB that is 533,725,184 bytes, within 70 KB of the request in #3895.
- Upstream's compose example for an external database, [docker-compose/external-pg/docker-compose.yaml](https://github.com/vectorize-io/hindsight/blob/v1.3.0/docker/docker-compose/external-pg/docker-compose.yaml), sets no `shm_size` on `db` or `hindsight`. Neither does Rakazo v0.16.0 anywhere in its repository.

### 2. The `hindsight` container doesn't use `/dev/shm`

- **Source:**
  - The local embedder and reranker run in-process through sentence-transformers, each with a `ThreadPoolExecutor`: [embeddings.py#L486](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/embeddings.py#L486), [cross_encoder.py#L442](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/cross_encoder.py#L442) (FlashRank and ONNX the same: [#L1340](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/cross_encoder.py#L1340), [#L1540](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/cross_encoder.py#L1540)).
  - `TOKENIZERS_PARALLELISM=false` is set at startup ([main.py#L88](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/main.py#L88), [server.py#L43](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/server.py#L43)).
  - There is no PyTorch `DataLoader`, no `num_workers` and no `share_memory` in the API.
  - The only `ProcessPoolExecutor`s are the boot-time migration runner ([migrations.py#L1573](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/migrations.py#L1573), `spawn` context) and Knowledge-bank PDF and Office parsing ([knowledge/media.py#L510](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/knowledge/media.py#L510)). With `spawn`, data goes through pipes, and only POSIX semaphores live in `/dev/shm`.
- **Measured.** A watcher sampled `df /dev/shm` in the API container every 0.2 s during:
  - seeding 5,000 and then 10,000 more memories
  - recall at `low`, `mid` and `high`, one, two and four at once, on every CPU limit in #47
  - FlashRank and `rrf`
  - Its peak was **4 KiB** every time, with no bus errors and no `DataLoader` or worker failures in the logs.

### 3. The database container does

**Why it builds indexes in parallel.**

- With `HINDSIGHT_API_VECTOR_INDEX_MIN_ROWS` at its default of `0` ([config.py#L2566](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py#L2566)), "the bank-create transaction builds all three partial indexes" ([_vector_index.py#L283-L299](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/_vector_index.py#L283-L299)).
  - The statement is `CREATE INDEX … ON memory_units USING hnsw (embedding vector_cosine_ops) WHERE fact_type = '…' AND bank_id = '…'` ([ops_postgresql.py#L1378-L1392](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/db/ops_postgresql.py#L1378-L1392)).
  - The new bank has no rows, but Postgres still scans the whole shared table, and chooses parallel workers once its heap passes `min_parallel_table_scan_size` (8 MB).
  - In these tests the heap was 3.4 MB at 5,000 notes of about 300 characters (serial builds) and 10 MB at 15,000 (parallel). How soon a real Instance's extracted facts and observations pass 8 MB is **unverified**: it is the total for every bank.
  - Hindsight serialises index DDL inside one process (`_index_ddl_lock`, an `asyncio.Lock`, [ops_postgresql.py#L126](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/db/ops_postgresql.py#L126)), so one `hindsight` container runs one build at a time.
- pgvector's parallel build reserves `maintenance_work_mem` minus 3 MB for the graph. Its comment reads: "Docker has a default limit of 64 MB for shm_size / which happens to be the default value of maintenance_work_mem" ([hnswbuild.c#L961-L969](https://github.com/pgvector/pgvector/blob/v0.8.7/src/hnswbuild.c#L961-L969)).
  - pgvector's README: "If you increase `maintenance_work_mem`, make sure `--shm-size` is at least that size to avoid an error with parallel HNSW index builds", followed by `docker run --shm-size=1g` ([README.md#L1185-L1189](https://github.com/pgvector/pgvector/blob/v0.8.7/README.md?plain=1#L1185-L1189)).
  - The `postgres` image docs say the same about the 64 MB default and suggest `--shm-size=256MB` ([content.md#L237](https://github.com/docker-library/docs/blob/62b61480e2ee79e7e000a41a00a1700ba98f5286/postgres/content.md?plain=1#L237)).
  - `pgvector/pgvector:pg18` runs at Postgres defaults: `maintenance_work_mem = 64MB`, `max_parallel_maintenance_workers = 2`, `dynamic_shared_memory_type = posix` (measured). So one build asks for 63,999,840 bytes out of 67,108,864.

**Measured.** One `hindsight-db` with 15,000 memories. In the first session both containers were pinned to 16 threads; the repro ran unpinned. `/dev/shm` was sampled every 10 ms. "New bank" is the PATCH plus first retain that Rakazo's first write amounts to ([newbank.py](hindsight-shm/newbank.py)).

| `hindsight-db` `/dev/shm` | Test | Result |
| --- | --- | --- |
| 64 MB, fresh database (repro) | first new bank | OK |
| | the next two new banks | **both HTTP 500**, `could not resize shared memory segment … to 63999840 bytes: No space left on device` |
| | the same `CREATE INDEX` by hand | `building index … with request for 1 parallel workers`, then `ERROR: could not resize shared memory segment … to 63999488 bytes` |
| | idle after that | 3.5 MB of segments left in `/dev/shm` |
| 64 MB, first session | first new bank | OK |
| | `CREATE INDEX` by hand, `SET client_min_messages=debug1` | `using 2 parallel workers`; peak **64,972 of 65,536 KiB** |
| | same with `max_parallel_maintenance_workers=0` | `building index … serially`; peak 2.3 MB |
| | same with `maintenance_work_mem='256MB'` | `ERROR: could not resize shared memory segment … to 265326208 bytes: No space left on device` (upstream's error, at a smaller size) |
| | new bank while 4 recalls run, 3 trials | **3/3 HTTP 500** |
| | two new banks at once, 6 trials | **12/12 HTTP 500** |
| | new bank alone, after all that | **2/2 HTTP 500**; 13.4 MB of 1 MB and 192 KB segments idle in `/dev/shm` |
| 256 MB | 3 new banks with 4 recalls running, 6 in pairs | all OK; peak 71.7 MB |
| 1 GB | 1 new bank alone, 3 with 4 recalls running, 6 in pairs | all OK; peak 71.5 MB |
| 1 GB (repro) | one alone, then two at once, then by hand | all OK; `using 2 parallel workers` |

- The recall load ran without errors in every case, at 5.0–5.7 s per recall with four at once.
- In the first session, a `/dev/shm` sampler at 0.2 s missed the build's peak, which lasts under a second. The 10 ms sampler caught it.
- Which Postgres sessions hold the idle 1 MB and 192 KB segments wasn't traced (**unverified**). They stayed while Hindsight's connection pool stayed open.

**What it does to Rakazo.**

- Banks are `rakazo-<sha256(spaceId)>-<sha256(botId)>` for each bot and `rakazo-<sha256(spaceId)>` for a Shared-scope Space, created on first write ([hindsight-memory-provider.ts#L287-L290](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L287-L290)).
- Explicit saves are synchronous, and "a failure reaches the bot" ([#L560-L579](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L560-L579)).
- Turn learning "fails the attempt so the job can retry with the same operation IDs" ([#L583-L650](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts#L583-L650)). The retries hit the same full `/dev/shm`.
- Recall on a bank that doesn't exist returns 404, which Rakazo treats as empty. So nothing looks broken; the bot just never remembers.
- The first bots and the default Space are created while the table is small, so a fresh Instance is fine. The failure starts later, which makes it hard to notice.

### 4. Docker's defaults

- Docker Engine: `--default-shm-size bytes  Default shm size for containers (default 64MiB)` ([moby dockerd.md#L54](https://github.com/moby/moby/blob/v2.0.0-beta.29/docs/reference/dockerd.md?plain=1#L54)). `docker run --shm-size` falls back to it ([docker/cli container_run.md#L99](https://github.com/docker/cli/blob/v29.10.1/docs/reference/commandline/container_run.md?plain=1#L99)).
- Docker Desktop for Windows, WSL 2 backend, 29.9.3: both containers reported `ShmSize=67108864` and `/dev/shm` at 64M.
- Docker Desktop for Mac runs the same engine in its VM. No Mac was available, so its default is **unverified**; nothing documents a different one.
- Compose: `shm_size` "configures the size of the shared memory (`/dev/shm` partition on Linux) allowed by the service container", as a byte value ([compose-spec container_spec.md#L1285-L1288](https://github.com/compose-spec/compose-spec/blob/8f2dcf99fe9c62a09ae9915f1f8a9ba1b01bf65e/container_spec.md?plain=1#L1285-L1288)).
- `/dev/shm` is a `tmpfs`. Its size is a ceiling, and only pages in use take RAM, counted against the container's memory. Setting 1g doesn't take 1 GB from the 8 GB Linux floor in [#42](https://github.com/kamui/rakazo-stack/issues/42).

### 5. What the stack's compose should say

```yaml
services:
  hindsight-db:
    image: pgvector/pgvector:pg18
    shm_size: 1g   # pgvector's parallel HNSW builds need about maintenance_work_mem of /dev/shm (#48)
  hindsight:
    # no shm_size: the API never uses /dev/shm
```

**Rejected alternatives:**

- **`command: postgres -c max_parallel_maintenance_workers=0`.** It also works (serial builds peaked at 2.3 MB), but it slows every large HNSW build. That includes rebuilds during Hindsight's own migrations, which run at every boot after an Update, and `vector_index_maintenance`.
- **`HINDSIGHT_API_VECTOR_INDEX_MIN_ROWS`.** It only moves the build to later, when the bank has rows. The build still runs on the same shared table. Not tested.
- **pg0.** [What does Hindsight need?](https://github.com/kamui/rakazo-stack/issues/44) already rejected it. It would need `shm_size` on `hindsight` instead, and at 512 MB per build.

## Open questions / follow-ups

- **task:** Add `shm_size: 1g` to `hindsight-db` in the compose layout from [#10](https://github.com/kamui/rakazo-stack/issues/10). Add a check that a new bank can be created on a populated database to the canary from [#32](https://github.com/kamui/rakazo-stack/issues/32): [repro.sh](hindsight-shm/repro.sh) is a starting point.
- **task:** Run `docker run --rm alpine df -h /dev/shm` on a Mac under Docker Desktop to confirm the 64 MB default.
- **research:** How many memory units a real Instance builds up per month, to know when the table crosses 8 MB. This matters only for the `shm_size`-less case and for #46's disk figures.
- **Unverified in this research:** Docker Desktop for Mac's default; which Postgres sessions hold the idle segments; when a real Instance reaches 8 MB of `memory_units`.

## Sources

Hindsight v1.3.0 (https://github.com/vectorize-io/hindsight/tree/v1.3.0):
- `hindsight-docs/docs/developer/installation.md`, `hindsight-docs/docs/developer/api/quickstart.mdx`, `docker/docker-compose/external-pg/docker-compose.yaml`
- `hindsight-api-slim/hindsight_api/config.py`, `_vector_index.py`, `engine/db/ops_postgresql.py`, `engine/embeddings.py`, `engine/cross_encoder.py`, `main.py`, `server.py`, `migrations.py`, `knowledge/media.py`
- Git history: commit `88f1472e`, [PR #3896](https://github.com/vectorize-io/hindsight/pull/3896), issues [#3895](https://github.com/vectorize-io/hindsight/issues/3895) and [#2695](https://github.com/vectorize-io/hindsight/issues/2695)
- Local runs of `ghcr.io/vectorize-io/hindsight-api:1.3.0` with pg0 (settings read from `pg_settings`) and against `pgvector/pgvector:pg18`

pgvector v0.8.7: [src/hnswbuild.c](https://github.com/pgvector/pgvector/blob/v0.8.7/src/hnswbuild.c), [README.md](https://github.com/pgvector/pgvector/blob/v0.8.7/README.md).

Docker: [moby dockerd.md](https://github.com/moby/moby/blob/v2.0.0-beta.29/docs/reference/dockerd.md), [docker/cli container_run.md](https://github.com/docker/cli/blob/v29.10.1/docs/reference/commandline/container_run.md), [compose-spec container_spec.md](https://github.com/compose-spec/compose-spec/blob/8f2dcf99fe9c62a09ae9915f1f8a9ba1b01bf65e/container_spec.md), [docker-library postgres docs](https://github.com/docker-library/docs/blob/62b61480e2ee79e7e000a41a00a1700ba98f5286/postgres/content.md).

Rakazo v0.16.0: `packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts`.

Reproduction: [repro.sh](hindsight-shm/repro.sh) with [seed.py](hindsight-shm/seed.py) and [newbank.py](hindsight-shm/newbank.py); its output on the host above is [repro-output.txt](hindsight-shm/repro-output.txt).
