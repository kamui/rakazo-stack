# What does Hindsight need?

Ticket: kamui/rakazo-stack#44. Versions read: Hindsight v1.3.0 (commit `93788619`, released 2026-11-24), Rakazo v0.16.0 (commit `6e24f7e6`, released 2026-11-24). Both are the newest stable releases. Image facts come from the GHCR registry API and from running `ghcr.io/vectorize-io/hindsight-api:1.3.0` locally with no internet access and a dummy LLM key, on a 24-thread x86-64 Docker Engine host. No real model provider key was used and nothing was spent.

## Answer

- **Model provider.** Hindsight 1.3.0 accepts about 30 LLM providers through `HINDSIGHT_API_LLM_PROVIDER`, including `openrouter`, `openai` (any OpenAI-compatible endpoint), `gemini` and `anthropic`. One key from any of the four v1 providers can run all of memory on its own:
  - Every call is structured output (a JSON schema). Hindsight uses no native tool calling, so the model doesn't need tool support.
  - The model must allow at least 64,000 output tokens, because retain asks for that much.
  - OpenRouter default: `~z-ai/glm-flash-latest` with `HINDSIGHT_API_LLM_REASONING_EFFORT=low`. Without that setting Hindsight sends no effort, and GLM reasons at `max`. Hindsight's own OpenRouter default (`qwen/qwen3.5-9b`) shouldn't be used.
- **Embeddings and reranking run inside Hindsight's container by default, so no provider is needed for them.**
  - Embedder: `BAAI/bge-small-en-v1.5` (384 dimensions). Reranker: `hindsight-ai/hs-rerank-1`. Both run on the CPU.
  - Both models are baked into the full image. It booted and loaded them with no network.
  - So **Anthropic can now keep memory on.** Haiku 5.5 is Hindsight's Anthropic default.
- **Infrastructure.**
  - Images on GHCR are tagged `1.3.0`, `1.3`, `1` and `latest` (no `v`) and built for linux/amd64 and linux/arm64. `latest` follows releases only. Recommended image: `ghcr.io/vectorize-io/hindsight-api` (API and worker, no UI): 1.9 GB download, 6.4 GB on disk.
  - One container runs the API on :8888 with a built-in task worker, so there is no separate deriver and no Redis. The web UI (:9999) is a separate, optional image that Rakazo doesn't need.
  - The database is either embedded pg0 (PostgreSQL 18.1 + pgvector 0.8.5 inside the container) or any external PostgreSQL 14+ with pgvector. The stack should use an external `pgvector/pgvector:pg18`, as it did for Honcho. That combination was booted and tested here.
- **Settings and secrets.**
  - Auth is off by default. Turn it on with `HINDSIGHT_API_TENANT_EXTENSION=hindsight_api.extensions.builtin.tenant:ApiKeyTenantExtension` plus one shared `HINDSIGHT_API_TENANT_API_KEY`. That was tested: no key or a wrong key gets 401, the right key gets 200.
  - Setup must generate that key and the Postgres password. It writes the Self-hoster's provider key as `HINDSIGHT_API_LLM_API_KEY`.
  - With no LLM key, Hindsight refuses to start. With a key it can't reach, it starts and logs a warning.
  - `/health` and `/metrics` answer without the key. Nothing from Hindsight should be published or reachable by bot computers.
- **How Rakazo connects.** Rakazo v0.16.0 has no Hindsight environment variables. The connection is a per-Space setting stored in Rakazo's database:
  - It holds `mode=local`, `baseUrl`, a stored `trust` value, optional recall and learning toggles, and an encrypted `apiKey`. The key is optional for a server without auth, 8–4096 characters otherwise.
  - The operator must set `MCP_ALLOW_PRIVATE_ENDPOINT=true`. The URL must be loopback or a private address, and a Compose service name counts.
  - Setup can fill it in the same way it did for Honcho: `POST /rpc/memory/connectProvider` with `{"provider":"hindsight","settings":{"mode":"local","baseUrl":"http://hindsight:8888"},"credentials":{"apiKey":<tenant key>}}`. The contract is unchanged. This call was read from source, not run, on v0.16.0.
  - Rakazo creates `rakazo-<sha256>` banks itself on first write.
- **Upgrades and backup.**
  - Alembic migrations run on every start by default and only go forward.
  - An older image on a newer schema logs "Database is at a newer migration revision" and carries on, so rolling back by swapping the image isn't safe to rely on. Roll back by restoring the Backup, which is what [How does the stack pin and upgrade its components?](https://github.com/kamui/rakazo-stack/issues/32) already does.
  - A Backup needs a `pg_dump` of Hindsight's database plus the tenant key. Hindsight keeps its files in Postgres, so there's no volume to save.
  - The embedding dimension is locked once the database has vectors. The stack should pin the embedder and reranker models in env, so that a future upstream change of default can't stop the server from booting.
- **Cost estimate, not measured.** Each finished turn costs about two LLM calls: a fact extraction of about 3.4k input tokens and one consolidation of about 5–8k. Recall makes no LLM call.
  - On GLM at the typical endpoint price, that is about **$0.0010–0.0018 per turn, or about $1–2 a month** at [Run Honcho on the default models through Rakazo](https://github.com/kamui/rakazo-stack/issues/37)'s 30 turns a day.
  - That is new spend. Honcho's own share in #37 was about $0.02 a month, and almost all of the ~$2–9 was Rakazo's chat.
  - The Instance total becomes roughly **$3–11 a month**. The Shared scope doubles the memory part.
  - A measured run is a follow-up.
- **Superseded decisions.** #7, #9, #10, #11/#21, #12, #13, #30, #31, #32, #33/#36, #37, #38 and #42 assume Honcho, as do the map's "Settled" notes and the README. Section 9 lists what each should change to. The biggest changes:
  - Anthropic keeps memory on.
  - There's no hosted embeddings model to keep fixed per Instance.
  - The memory container is larger: 1.2–1.8 GiB of RAM and 6.4 GB of disk.
  - Recall adds about 1.5–2.5 s before every reply on 4 CPUs.

## Details

### 1. Model provider

**Accepted providers.** `PROVIDER_DEFAULT_MODELS` in [config.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py) lists 31 values, `mock` and `none` included: `openai`, `openai-responses`, `anthropic`, `gemini`, `groq`, `minimax`, `deepseek`, `zai`, `opencode-go`, `atlas`, `meta`, `ollama`, `ollama-cloud`, `llamacpp`, `lmstudio`, `vertexai`, `openai-codex`, `chatgpt`, `claude-code`, `cursor`, `antigravity`, `github-copilot`, `litellm`, `bedrock`, `volcano`, `openrouter`, `requesty`, `fireworks`, `nous` and `xai-oauth`. The global default is `openai` / `gpt-5-nano`. The defaults for the v1 providers are:

| Provider | `HINDSIGHT_API_LLM_PROVIDER` | Hindsight default model | Notes |
| --- | --- | --- | --- |
| OpenRouter | `openrouter` | `qwen/qwen3.5-9b` | Base URL built in. On every call that wants JSON back, Hindsight adds `provider.require_parameters=true`. The Self-hoster's own `provider` keys from `HINDSIGHT_API_LLM_EXTRA_BODY` are kept and override it ([openai_compatible_llm.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/providers/openai_compatible_llm.py); [models.mdx, OpenRouter](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/models.mdx)). |
| OpenAI | `openai` | `gpt-5-nano` | Chat Completions; `HINDSIGHT_API_LLM_BASE_URL` accepts any compatible endpoint. |
| Gemini | `gemini` | `gemini-3.8-flash` | Uses Gemini's `CachedContent` prompt caching and Batch API. |
| Anthropic | `anthropic` | `claude-haiku-5-5` | Native `output_config.format` JSON schema ([anthropic_llm.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/providers/anthropic_llm.py)). |

So **one key from any v1 provider runs Hindsight.** Unlike Honcho, Hindsight doesn't need a second provider for embeddings (section 2).

**Calls and what they need.** Read from the `scope=` names passed to each LLM call in `engine/`:

| Call | When | Needs |
| --- | --- | --- |
| `retain_extract_facts` | Every retain, once per changed chunk (3,000 characters at most, `HINDSIGHT_API_RETAIN_CHUNK_SIZE`) | JSON-schema output. `max_completion_tokens` defaults to 64,000 (`DEFAULT_RETAIN_MAX_COMPLETION_TOKENS`). |
| consolidation (and `consolidation_dedup`) | Automatically after each retain (`DEFAULT_ENABLE_AUTO_CONSOLIDATION = True`), up to 8 new facts per call | JSON-schema output |
| reflect loop (`reflect`, `reflect_tool_call`, `reflect_structured`, `final_rewrite`) | Only when the bot calls Rakazo's `reflect_memory` tool | JSON-schema output; "tool calls" are schema replies |
| mental-model refresh | Only for mental models a bank defines. Rakazo defines none. | JSON |
| decision model (`decision_*`) | Off by default (`DEFAULT_DECISION_MODEL_PROVIDER = "none"`) | n/a |
| `verification` | Once per configured model at startup. A failure only logs a warning. Turn it off with `HINDSIGHT_API_SKIP_LLM_VERIFICATION=true`. | n/a |

[models.mdx, "Model requirements"](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/models.mdx) says that every call, reflect's included, is structured output. Native tool calling is not used. The model needs "at least 65,000 output tokens", or the operator must lower `HINDSIGHT_API_RETAIN_MAX_COMPLETION_TOKENS`, which has to stay above the chunk size. Recall makes no LLM call: temporal parsing uses `dateparser` ([query_analyzer.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/query_analyzer.py)), and search uses embeddings, BM25, graph links and the reranker.

**Default model on OpenRouter.** Keep `~z-ai/glm-flash-latest`, the stack's default from [How does setup configure the model provider?](https://github.com/kamui/rakazo-stack/issues/36):
- OpenRouter's live model API (queried 2026-11-24) lists `response_format`, `structured_outputs`, `reasoning` and `reasoning_effort` for it. Its 1M context covers any prompt Hindsight sends.
- `z-ai/glm-5.3-flash` has 42 endpoints, 33 of them ZDR. The typical price is $0.125 input / $0.03 cached / $0.50 output per million tokens, with a range of $0.036–0.225 for input.
- 41 of the 42 endpoints allow 128k or more output tokens. One (Darkbloom) allows 29,491, below Hindsight's 64,000 request. Whether OpenRouter routes around it is **unverified**.
- At boot, Hindsight logged `reasoning_effort=not sent` for OpenRouter by default, and `reasoning_effort=low` once `HINDSIGHT_API_LLM_REASONING_EFFORT=low` was set. GLM's own default effort is `max` (#36), so the stack must set `low`.
- `HINDSIGHT_API_LLM_EXTRA_BODY` is parsed with `json.loads` (config.py). So `{"provider":{"zdr":true,"data_collection":"deny"}}` reaches the request as an object from plain env. That avoids the Honcho problem in #37, where nested `EXTRA_BODY` arrived as a string.

For the other providers, keep #36's defaults: `gpt-6-luna` (OpenAI), `gemini-3.5-flash-lite` (Gemini) and `claude-haiku-5-5` (Anthropic: $0.10 / $0.50 per million tokens, 128K output). Whether `gpt-6-luna` and `gemini-3.5-flash-lite` allow 64,000 output tokens is **unverified**. Hindsight's Anthropic provider accepts a `temperature` argument but doesn't put it into the request (`anthropic_llm.py`, read from source). That matters because Haiku 5.5 rejects non-default temperatures.

### 2. Embeddings and reranking

- Defaults ([config.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/config.py)):
  - `DEFAULT_EMBEDDINGS_PROVIDER = "local"`, `DEFAULT_EMBEDDINGS_LOCAL_MODEL = "BAAI/bge-small-en-v1.5"`.
  - `DEFAULT_RERANKER_PROVIDER = "local"`, `DEFAULT_RERANKER_LOCAL_MODEL = "hindsight-ai/hs-rerank-1"`.
  - [models.mdx](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/models.mdx) confirms: "Only the LLM needs setting up: the embedding model and the reranker run locally by default."
- `hs-embed-1` is the model behind Hindsight's *hosted* inference provider (`DEFAULT_EMBEDDINGS_HINDSIGHT_MODEL`). It is not the local default. Its dimension wasn't checked here.
- The full image downloads `BAAI/bge-small-en-v1.5`, `hindsight-ai/hs-rerank-1` and `google/embeddinggemma-2` while it is built (`PRELOAD_ML_MODELS=true` in [docker/standalone/Dockerfile](https://github.com/vectorize-io/hindsight/blob/v1.3.0/docker/standalone/Dockerfile)). EmbeddingGemma is for Knowledge Banks, which Rakazo doesn't use, and it wasn't loaded at boot. The Hugging Face cache in the image is 1.8 GB.
- **First start needs no download.** Run with `--network none`, the container logged `Embeddings: local provider initialized (dim: 384, device: cpu)` and loaded the reranker. Migrations ran, and the API was ready after 58–65 s. Most of that time was loading the two models.
- **CPU and RAM, measured:**
  - Idle API process RSS was 1.6 GiB unrestricted and 1.2 GiB after a restart limited to 4 CPUs. The worker logged a peak of 1.8 GiB at startup.
  - The pgvector Postgres used 144 MiB.
  - Upstream's [installation.md, "Hardware"](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md) gives 1.5 GB minimum and 2 GB recommended for the full API image, and 512 MB–1 GB for Postgres.
- **Recall latency, measured.** On a bank of 500 memories retained with `retain_extraction_mode: chunks` (no LLM), `POST …/memories/recall` with `max_tokens: 4096`:
  - Unrestricted (24 threads): 2.1 s median.
  - Limited to 4 CPUs: 1.5 s (`low`), 2.5 s (`mid`, Rakazo's default) and 2.6 s (`high`).
  - Rakazo calls recall before every reply when **Recall automatically** is on, which is the default (`recallEachRun` in [hindsight-memory-provider.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts); [run-assembly-history.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/runs/assembly/run-assembly-history.ts)). Its timeout is 15 s. Upstream names the CPU reranker as "the main bottleneck" for recall.
- **The dimension is locked once vectors exist.** `ensure_embedding_dimension` in [migrations.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/migrations.py) resizes `vector(N)` columns on an empty table. On a populated one it raises `Cannot change embedding dimension …`, and the server doesn't start. Changing the embedding model on a populated Instance is done with `hindsight-admin export-bank` and `import-bank` into a fresh database. That re-embeds everything with no LLM re-extraction ([admin-cli.md, "Migrating a bank to a new instance"](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/admin-cli.md)). It is a supported path, unlike Honcho's.
- Consequence for setup: because nothing about embeddings depends on the Self-hoster's provider, switching between OpenRouter, OpenAI, Gemini and Anthropic never touches stored memories. The stack should still set `HINDSIGHT_API_EMBEDDINGS_LOCAL_MODEL=BAAI/bge-small-en-v1.5` and `HINDSIGHT_API_RERANKER_LOCAL_MODEL=hindsight-ai/hs-rerank-1` explicitly. Then a later Hindsight release that changes the default can't fail a boot after an Update (same dimension changes would silently degrade recall instead).

### 3. Infrastructure

**Images** (GHCR anonymous token and manifest API, 2026-11-24):

| Image | Index digest | amd64 / arm64 compressed |
| --- | --- | --- |
| `ghcr.io/vectorize-io/hindsight-api:1.3.0` (API + worker, full) | `sha256:c054765e9196fb1fe63497be06bcb55256c9024cc9314f951dc59858985ab242` | 1.93 / 1.87 GB |
| `ghcr.io/vectorize-io/hindsight-api:1.3.0-slim` (no local models) | `sha256:600ec51eefedd499e357cb84ece860a8f656fef62a22080e854498fb3971bb7e` | 0.48 / 0.46 GB |
| `ghcr.io/vectorize-io/hindsight:1.3.0` (API + UI) | `sha256:469ca4b49bf3863ec383cc0c509483bb58180def7c6ae0e6a592dc27369c9ff5` | 2.01 / 1.95 GB |
| `ghcr.io/vectorize-io/hindsight-control-plane:1.3.0` (UI only) | `sha256:df58ce2597e837f359684cfe0e004aaa93bbb89983f1f1b8bd2cab881b457554` | 0.10 / 0.10 GB |

- Each tag is an OCI index with linux/amd64 and linux/arm64, plus attestation manifests. `v1.3.0` returns 404.
- `latest` resolves to the same digest as `1.3.0`. [release.yml](https://github.com/vectorize-io/hindsight/blob/v1.3.0/.github/workflows/release.yml) builds on release tags with `platforms: linux/amd64,linux/arm64` and tags `{{version}}`, `{{major}}.{{minor}}`, `{{major}}` and a raw `latest`. So unlike Honcho, `latest` follows releases, not `main`.
- Images are signed with Cosign keyless ([installation.md, "Verifying image signatures"](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md)).
- The pulled `hindsight-api:1.3.0` is 6.38 GB on disk. Upstream's table says about 4.5 GB for the standalone image.
- The image runs as `hindsight` (UID 1000) with `CMD /app/start-all.sh`. It exposes 8888 and sets `HINDSIGHT_API_HOST=0.0.0.0`, `HINDSIGHT_API_PORT=8888` and `HINDSIGHT_ENABLE_CP=false`.
- Use the full image: the slim one needs hosted embeddings and reranking, which would bring back the problem this ticket removes.

**Services and ports** ([services.md](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/services.md)):
- **API on :8888, with the worker inside it.** By default the API polls Postgres for background tasks (retain and consolidation). The boot log showed `Worker poller started` with 10 slots, 2 of them reserved for consolidation. A separate `hindsight-worker` (metrics :8889) is only for high throughput, so v1 runs one container.
- Upstream recommends a stable `HINDSIGHT_API_WORKER_ID`, otherwise tasks in flight at a restart are orphaned. It also recommends a 75 s stop timeout (`stop_grace_period: 75s`) and `--shm-size=1g` in its `docker run` example ([installation.md](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md)). Why `shm-size` is needed isn't explained (**unverified**).
- **Control Plane (UI) on :9999** is a separate Next.js image. It needs `HINDSIGHT_CP_DATAPLANE_API_KEY` when the API has auth, and supports `HINDSIGHT_CP_ACCESS_KEY` for its own login ([configuration.mdx](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/configuration.mdx)). Rakazo doesn't need it, so leave it out of v1.
- **MCP** is on by default (`DEFAULT_MCP_ENABLED = True`) and goes through the same API-key check. Rakazo doesn't use it, so `HINDSIGHT_API_MCP_ENABLED=false` is a reasonable hardening step.

**Database.**
- With no `HINDSIGHT_API_DATABASE_URL`, Hindsight starts embedded **pg0**: PostgreSQL 18.1 with pgvector 0.8.5, `pg_trgm` and `btree_gin`, measured by querying the running instance. It lives in `/home/hindsight/.pg0` (117 MB when empty) and logs a warning that it created a new, empty instance.
- Upstream calls pg0 "not recommended for production" and requires PostgreSQL 14+ with pgvector, pgvectorscale, vchord or scann otherwise ([installation.md](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/installation.md); [storage.md](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/storage.md)).
- **Tested:** `pgvector/pgvector:pg18` (`sha256:38681dc6…`) with `POSTGRES_USER=hindsight`, on an `--internal` network with no internet access.
  - Hindsight installed the `vector` extension itself ("pgvector extension not found, attempting to install..."), so no `init.sql` is needed when its database user owns the database.
  - All 122 migrations ran and the API was ready in 62 s.
  - Rakazo's calls behaved as expected: bank probe 200, recall on a missing bank 404 (Rakazo treats that as empty), and an async append-mode retain accepted with an operation id and status `pending`.
- **Recommendation:** an external `hindsight-db` on pgvector pg18, like #10's `honcho-db`. That keeps `pg_dump` backups, the stack-owned Postgres major upgrades from #32, and no data volume on the API container. pg0 would hide a Postgres version inside Hindsight's image that the stack doesn't control.

### 4. Settings and secrets

- **Auth** ([extensions/builtin/tenant.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/extensions/builtin/tenant.py); [configuration.mdx, "Authentication"](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/configuration.mdx)):
  - The default `DefaultTenantExtension` accepts every request.
  - `ApiKeyTenantExtension` compares `Authorization: Bearer <key>` with `HINDSIGHT_API_TENANT_API_KEY` and refuses to start without one. There is one key per server, with no per-bank or per-Space keys.
  - Tested: no key → 401, wrong key → 401, right key → 200. `/health` and `/metrics` stay open.
  - Custom tenant extensions (JWT and others) exist but need custom code.
- **Variables the stack writes:**
  - `HINDSIGHT_API_DATABASE_URL=postgresql://hindsight:<generated>@hindsight-db:5432/hindsight`
  - `HINDSIGHT_API_TENANT_EXTENSION=hindsight_api.extensions.builtin.tenant:ApiKeyTenantExtension` and `HINDSIGHT_API_TENANT_API_KEY=<generated>`. This is the value setup also gives Rakazo. Rakazo requires 8–4096 characters.
  - `HINDSIGHT_API_LLM_PROVIDER`, `HINDSIGHT_API_LLM_API_KEY` (the Self-hoster's key), `HINDSIGHT_API_LLM_MODEL`, and on OpenRouter `HINDSIGHT_API_LLM_REASONING_EFFORT=low` plus `HINDSIGHT_API_LLM_EXTRA_BODY='{"provider":{"zdr":true,"data_collection":"deny"}}'`
  - `HINDSIGHT_API_EMBEDDINGS_LOCAL_MODEL` and `HINDSIGHT_API_RERANKER_LOCAL_MODEL`, pinned (section 2)
  - `HINDSIGHT_API_WORKER_ID=hindsight`
  - Optionally `HINDSIGHT_API_MCP_ENABLED=false`
- **With no LLM key, Hindsight won't start.** `MemoryEngine` raises `LLM API key is required. Set HINDSIGHT_API_LLM_API_KEY environment variable.` (observed; [memory_engine.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/memory_engine.py)). With a key it can't reach, it starts and logs `LLM connection verification failed … Server will start but LLM-dependent operations may fail`. So "blank key = memory off" still holds.
- **Must not be exposed:**
  - The API port: anyone holding the single key can read and write every Space's banks.
  - The database.
  - The LLM key.
  - `/metrics`, which is unauthenticated.
  - Hindsight joins the internal `stack` network only. Nothing is published, and the computer-egress sidecar from [Does every Instance run the computer-egress sidecar?](https://github.com/kamui/rakazo-stack/issues/40) already keeps bot computers off that network.
  - Rakazo stores the key encrypted, and Space members use it without seeing it ([self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.16.0/docs/self-host-integrations.md)).

### 5. How Rakazo connects (v0.16.0)

Sources: [hindsight-memory.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory.ts), [hindsight-memory-provider.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts), [hindsight-client.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/providers/memory/hindsight/hindsight-client.ts), [memory-provider-config.ts](https://github.com/elie222/rakazo/blob/v0.16.0/apps/api/src/integrations/memory-provider-config.ts), [bots-rpc.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/contracts/src/bots/bots-rpc.ts), [memory.md](https://github.com/elie222/rakazo/blob/v0.16.0/docs/memory.md), [self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.16.0/docs/self-host-integrations.md).

- **Stored per Space** in `space_memory_configs` (`provider='hindsight'`, a `settings` JSON, `secretId` pointing to the encrypted credential; [schema.prisma](https://github.com/elie222/rakazo/blob/v0.16.0/packages/db/prisma/schema.prisma)):
  - `mode`: `cloud` or `local`.
  - `baseUrl`: required for local. The Hindsight Cloud URL is refused in local mode.
  - A `trust` value: `prepareHindsightConnection` pins the host and stores what it resolved to.
  - `recallBanks`: other apps' banks, at most 10, none starting with `rakazo-`. They must already exist.
  - `retainConversations` ("Learn from conversations", on), `autoRecall` (on), `recallBudget` (`low`, `mid` or `high`; `mid` by default), `recallFacts` (off: observations only) and `skipUnrelated` (off).
  - Credential `apiKey`: optional in local mode, 8–4096 characters when given.
- **Network rules:**
  - Local mode needs `MCP_ALLOW_PRIVATE_ENDPOINT=true` on the API and worker. Without it, the connect call returns `FORBIDDEN`, checked both before and after DNS classification.
  - The URL must be loopback or resolve only to private addresses, and Rakazo pins the resolved address for every request.
  - Redirects are refused (`redirect: "error"`) and responses are capped at 1 MiB.
- **Endpoints Rakazo calls.** All are under `/v1/default/banks/{bank}`:
  - `GET /v1/default/banks?limit=1`: connect-time key probe.
  - `GET …/config`: checks that a `recallBanks` bank exists.
  - `POST …/memories`: retain. Explicit saves are synchronous. Finished turns are `async: true` with a stable `operation_id`, `document_id` `rakazo-<thread>-<generation>-<bot>`, `update_mode: "append"`, a `session:` tag and `observation_scopes: "shared"`.
  - `GET …/operations/{id}`, `POST …/memories/recall`, `POST …/reflect`, `DELETE …/documents/{id}` and `GET …/documents?tags=…`.
  - Banks are `rakazo-<sha256(spaceId)>` (Shared scope) and `rakazo-<sha256(spaceId)>-<sha256(botId)>` (each bot). They are created on first write, so nothing needs creating in Hindsight first.
- **What each finished turn sends** ([conversation-memory.ts](https://github.com/elie222/rakazo/blob/v0.16.0/packages/adapters/src/memory/conversation-memory.ts)):
  - Runs started by a person or another bot (`user`, `messaging`, `follow_up`, `bot_message`) in the bot's own direct thread or a group chat. Routines and webhooks are not sent.
  - In a direct chat: the message that started the run, messages added while it ran, and the replies. In a group: everything since the bot last replied.
  - Capped at 32,000 characters and 200 messages per turn, and 8,000 characters per message.
  - The content is a JSON array of `{role, content, timestamp}`, which Hindsight recognises as a conversation and chunks at turn boundaries ([fact_extraction.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/retain/fact_extraction.py), `iter_chunks`).
- **Can setup fill it?** Yes, with the same RPC and rules that [Can setup fill in Rakazo's database-only settings?](https://github.com/kamui/rakazo-stack/issues/11) and [Prove setup's sign-in, voice and memory calls on a throwaway v0.4.0 stack](https://github.com/kamui/rakazo-stack/issues/21) proved for Honcho:
  - The `memory.connectProvider` input is still `{provider, settings, credentials, defaultMemoryScope = "isolated"}`.
  - The handler still calls `requireSpaceOwner` and checks `mcpAllowPrivateEndpoint`, and `prepare` probes the server before saving. So Hindsight must be healthy first.
  - Body: `{"json":{"provider":"hindsight","settings":{"mode":"local","baseUrl":"http://hindsight:8888"},"credentials":{"apiKey":"<HINDSIGHT_API_TENANT_API_KEY>"}}}`.
  - The read-only detection SQL becomes `SELECT 1 FROM space_memory_configs WHERE "spaceId" = $1 AND provider = 'hindsight'`.
  - None of this was executed against v0.16.0. Under #32 it is best effort.

### 6. Upgrades and backup

- **Migrations.** `HINDSIGHT_API_RUN_MIGRATIONS_ON_STARTUP` defaults to `true` ([configuration.mdx](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/configuration.mdx)). Alembic runs `upgrade heads` under a Postgres advisory lock at boot. A fresh database took 122 revisions in under a second.
- **Rollback by image swap.**
  - If the database is ahead of the code, `run_migrations` catches the `ResolutionError`, logs "Database is at a newer migration revision than this code version knows about. This is expected during rolling deployments. Skipping migrations." and the server keeps running ([migrations.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/migrations.py)).
  - The downgrades are there to support rolling deployments; no tool runs them for an operator.
  - Whether an older release works correctly on a newer schema isn't documented (**unverified**, likely not across drops such as `e4a7c1b9d2f6`, "Drop the never-written `access_count` column").
  - So the stack should treat an Update rollback as "restore the Backup", not as a plain image swap.
- **Releases:** v1.2.0 (2026-11-03), v1.2.1 (11-10), v1.2.2 and v1.2.3 (11-19), v1.3.0 (11-24). That is weekly or faster, and the weekly CI canary from #32 will see a lot of them.
- **Backup contents:**
  - All Hindsight state is in Postgres, and uploaded files default to `DEFAULT_FILE_STORAGE_TYPE = "native"` (PostgreSQL `BYTEA`). A `pg_dump` of `hindsight-db` is therefore complete.
  - Upstream's own `hindsight-admin backup` writes a zip from a `REPEATABLE READ` snapshot ([admin-cli.md](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/admin-cli.md)). `pg_dump` matches #30 and is enough.
  - The secrets to keep are `HINDSIGHT_API_TENANT_API_KEY` (Rakazo's stored copy must match it), the Postgres password and the provider key, all already in `secrets.env`.
  - Operations in flight are rows in `async_operations` and resume after a restore, because the worker recovers stale tasks at boot ("recovered 0 stale tasks from previous run").
  - No model cache or pg0 volume is needed.

### 7. Cost (estimate from source; not measured)

**Prompt sizes**, measured with Hindsight's own `POST /v1/default/banks/{id}/prompts/preview`. It renders the exact messages without calling an LLM ([prompt_preview.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/prompt_preview.py)).

| Call | System | User template | Schema | Runtime data added |
| --- | --- | --- | --- | --- |
| Fact extraction (`concise` mode) | 7,428 characters | 275 characters | 3,509 characters | The chunk, at most 3,000 characters |
| Consolidation | 13,246 characters | 438 characters | n/a | Up to 8 facts, related observations (recall budget `low`, at most 512 tokens) and source facts (at most 4,096 tokens) |
| Reflect | 11,454 characters | 26 characters | n/a | Several rounds of recall results |

Both system prompts are the same for every bank, so a provider can cache them.

**Per finished turn.** Direct chat, Isolated scope, a turn of about 1.5k characters:
- Append concatenates the stored conversation and the new turn, then re-extracts only chunks whose hash changed ([orchestrator.py](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-api-slim/hindsight_api/engine/retain/orchestrator.py); [api/retain.mdx, `update_mode`](https://github.com/vectorize-io/hindsight/blob/v1.3.0/hindsight-docs/docs/developer/api/retain.mdx)). That is normally the conversation's last, partly filled chunk, so earlier turns in the same chunk are extracted again.
- Extraction is 1 call: about 3.4k input tokens and about 0.4–0.5k output.
- Consolidation is 1 call: about 5–8k input tokens and about 0.3k output.
- Total: about 8.5–11.5k tokens in and about 0.8k out.
- At $0.125 / $0.50 that is about $0.0015–0.0018 per turn. About $0.0010–0.0013 if the roughly 5k tokens of system prompt are served from cache at $0.03.
- A maximum turn (32,000 characters, about 11 chunks plus several consolidation calls) costs about $0.02.

**Per month**, at #37's 30 turns a day:

| | Memory (Hindsight) | Rakazo chat (#37) | Instance |
| --- | --- | --- | --- |
| Isolated scope (default) | ~$1–2 | ~$1.75–9 | ~$3–11 |
| Shared scope (writes 2 banks) | ~$2–3.5 | ~$1.75–9 | ~$4–12 |

Not included:
- Reasoning tokens. #37 saw GLM use at most 6 reasoning tokens at `low` on Honcho's short calls; Hindsight's calls are longer.
- `save_memory` calls: each is one synchronous retain, so the same two calls.
- `reflect_memory`: several calls, only when the bot asks.
- Re-consolidating facts that were deleted and re-extracted when a chunk changes (**unverified** how much this adds).

Haiku 5.5 on Anthropic ($0.10 / $0.50) comes out about the same.

**Compared with Honcho.** #37's ~$2–9 a month was almost entirely Rakazo's chat. Honcho itself cost $0.0006 a day, because Rakazo v0.4.0 only wrote explicit conclusions to it. Hindsight learns from every finished turn, so memory becomes a real but small line item. The README's "about $6 a month" for OpenRouter would become roughly "$3–11".

### 8. Hardware (feeds [Measure the hardware an Instance needs on each OS](https://github.com/kamui/rakazo-stack/issues/42))

- Hindsight API with its worker: 1.2–1.6 GiB idle, 1.8 GiB peak at startup. `hindsight-db`: about 0.15 GiB empty.
- This replaces Honcho's API and deriver. #42's idle Instance figure (4.2–4.9 GiB) should be re-measured with Hindsight. 8 GB Linux may become tight.
- Disk: 6.4 GB for the `hindsight-api` image (1.9 GB download) plus the pgvector image. This raises #42's ~10 GB fresh-disk figure.
- Recall adds about 1.5–2.5 s of CPU before every reply on 4 CPUs (section 2). On slow 4-thread CPUs it could approach the 15 s timeout (**unverified**).
- Boot takes about 60 s before Hindsight is healthy, and setup's `connectProvider` step has to wait for it.
- #13's tokenizer-cache volume and its chown step are no longer needed. Hindsight's models are inside the image.

## 9. Map decisions that assume Honcho

| Decision | What it says now | Change to |
| --- | --- | --- |
| Map "Settled" notes | "optional Honcho (memory…)"; computers must not reach "Honcho"; setup fills "the default Space's Honcho connection" | Hindsight in all three |
| [#7 What does Honcho need?](https://github.com/kamui/rakazo-stack/issues/7) | Honcho's images, nine model slots, embeddings | Superseded for v1 by this ticket. Keep it for the later Honcho option. |
| [#9 setup form](https://github.com/kamui/rakazo-stack/issues/9) | "Honcho optional (blank model provider key = off, `off` later keeps data)" | Hindsight. Blank key = off still holds, because Hindsight won't start without a key. |
| [#10 compose layout](https://github.com/kamui/rakazo-stack/issues/10) | Profile `honcho`; `honcho-db` on pgvector pg18, no Redis | Profile `hindsight`: services `hindsight` (`ghcr.io/vectorize-io/hindsight-api:<ver>`, API with its worker, `stop_grace_period: 75s`, stable `HINDSIGHT_API_WORKER_ID`) and `hindsight-db` (`pgvector/pgvector:pg18`) on `stack`. Nothing published, no UI, no Redis, no deriver. Rakazo reaches `http://hindsight:8888`. |
| [#41 Windows runs Docker from Windows](https://github.com/kamui/rakazo-stack/issues/41) | Honcho `init.sql` as an inline `config` | Drop it: Hindsight creates the `vector` extension itself as the database owner. |
| [#11](https://github.com/kamui/rakazo-stack/issues/11) / [#21](https://github.com/kamui/rakazo-stack/issues/21) | `connectProvider` with `provider:"honcho"`, `baseUrl` `:8000`, admin JWT | `provider:"hindsight"`, `baseUrl:"http://hindsight:8888"`, `apiKey` = `HINDSIGHT_API_TENANT_API_KEY`. Detection SQL checks `provider='hindsight'`. Only proven on v0.4.0 with Honcho. |
| [#12](https://github.com/kamui/rakazo-stack/issues/12) / [#13](https://github.com/kamui/rakazo-stack/issues/13) | Hosted 1536-d embeddings; tokenizer volume chowned to 100:101 | Moot: local 384-d embeddings in the image, no tokenizer volume, no chown step. The "Local embeddings for Honcho in Lemonade" out-of-scope line only matters for later Honcho. |
| [#29 Instance folder](https://github.com/kamui/rakazo-stack/issues/29) | `honcho/` (already removed by #41) | Nothing new in the folder |
| [#30 backup](https://github.com/kamui/rakazo-stack/issues/30) | `pg_dump` of "both databases" (Rakazo, Honcho) | Rakazo and `hindsight-db`. `HINDSIGHT_API_TENANT_API_KEY` must come back with the dump, and it does, because it lives in `secrets.env`. |
| [#31](https://github.com/kamui/rakazo-stack/issues/31) / [#32](https://github.com/kamui/rakazo-stack/issues/32) updates | Honcho migrates via its entrypoint and rolls back by image swap to `compat_floor`; first setup installs the newest Honcho | Hindsight migrates forward at every boot. An older image on a newer schema only warns, so roll back from the Backup. Resolve the GitHub release `vX.Y.Z` to image tag `X.Y.Z` and record the digest. Pin the local embedder and reranker in env. Expect weekly releases. |
| [#33](https://github.com/kamui/rakazo-stack/issues/33) / [#36](https://github.com/kamui/rakazo-stack/issues/36) providers | Anthropic = memory off; Honcho's nine slots with `THINKING_EFFORT=low`; embeddings model fixed per Instance, and a Gemini ↔ OpenRouter switch offers "memory off or start over" | **Anthropic keeps memory on** (Hindsight's `anthropic` provider, `claude-haiku-5-5`). Each provider is one `HINDSIGHT_API_LLM_*` set: OpenRouter `~z-ai/glm-flash-latest` + `REASONING_EFFORT=low` + `EXTRA_BODY` `provider` ZDR block; OpenAI `gpt-6-luna`; Gemini `gemini-3.5-flash-lite`. OpenAI and Gemini keys go into Hindsight's env even though Rakazo connects them per user. No embeddings question, and a provider switch never touches memory. The optional "memory model" override becomes one value. |
| [#37](https://github.com/kamui/rakazo-stack/issues/37) | Cost ~$2–9 a month; `…__PROVIDER_PARAMS` spelling; dreams never fire | Estimate ~$3–11 a month for the Instance (memory ~$1–2), to be measured on Hindsight (follow-up). The env spelling issue doesn't arise, because `HINDSIGHT_API_LLM_EXTRA_BODY` is parsed as JSON. |
| [#38 Honcho dreams](https://github.com/kamui/rakazo-stack/issues/38) | No stack scheduling of dreams | Moot. Hindsight consolidates automatically after every retain. Rakazo defines no mental models, so nothing else needs scheduling. |
| [#39 ZDR probe](https://github.com/kamui/rakazo-stack/issues/39) | Probe via a pinned non-ZDR embeddings request | Unchanged: it checks the OpenRouter account, not Honcho |
| [#40 egress sidecar](https://github.com/kamui/rakazo-stack/issues/40) | Keeps computers off Lemonade, the relay, Honcho and the databases | Same rule, with Hindsight and `hindsight-db` instead |
| [#42 hardware](https://github.com/kamui/rakazo-stack/issues/42) | Idle 4.2–4.9 GiB, ~10 GB fresh disk, tokenizer chown | Re-measure with Hindsight: about 1.2–1.8 GiB for its API, 6.4 GB image, and 1.5–2.5 s of recall on 4 CPUs before every reply |
| README | Line 12 ("Rakazo, Lemonade and Honcho"); "What's included" row `Honcho … Only if your model provider can run it: OpenRouter, OpenAI or Gemini`; lines 39–42 (cost, Anthropic memory off); lines 76, 95 and 98 | Row: `[Hindsight](https://github.com/vectorize-io/hindsight) \| Long-term memory \| Only if you give a model provider key (any of the four)`. Anthropic "runs chat and memory". Cost becomes roughly $3–11. Lines 98, 76 and 95 name Hindsight. |
| GLOSSARY "Model provider" | "for chat and, when it can, for memory" | "for chat and memory" |
| Out of scope | "Honcho as a memory option (planned after v1; v1 uses Hindsight)" | Already correct |

## Open questions / follow-ups

- **task:** Measured run on the default models, like #37 was for Honcho: a throwaway Rakazo v0.16.0 + Hindsight 1.3.0 stack with a real OpenRouter key and ZDR only on, `~z-ai/glm-flash-latest` at `REASONING_EFFORT=low`, 30 turns. Measure:
  - per-call tokens and cost, how much caching helps, and reasoning tokens
  - whether append re-extraction re-consolidates old facts
  - extraction quality on GLM, and whether OpenRouter ever routes the 64,000-token retain to an endpoint with a lower cap
  - whether `connectProvider` works on v0.16.0 as written
  - whether Rakazo v0.16.0 still needs the Owner's AI-recipient consent (`aiConsent`, seen in #37 on v0.4.0) before memory works
- **grilling:** The single Hindsight key reaches every Space's banks. Setup connects only the default Space (#11), and later Spaces are UI-only, so an Owner creating a Space needs the key. Should setup print where it lives in `secrets.env`, add a `rakazo-stack` subcommand that connects a Space, or document "use the same URL and key"?
- **grilling:** Learning from every turn now sends each finished conversation to the model provider a second time, for memory, as well as for chat. Is the default "Learn from conversations: on" acceptable for v1, and should setup's checklist mention it?
- **research:** Recall latency on slow 4-thread CPUs and on Apple silicon under Docker Desktop (arm64 image), with a few thousand memories. If it gets near Rakazo's 15 s recall timeout, consider `recallBudget: low` as setup's default, or `HINDSIGHT_API_RERANKER_PROVIDER=flashrank`.
- **task:** Re-measure #42's idle RAM and fresh-disk numbers with Hindsight in place of Honcho, and check whether 8 GB Linux still holds.
- **research:** Whether `--shm-size=1g` from upstream's `docker run` example is needed (Torch or the reranker) or only suggested, and whether the stack sets `shm_size` in compose.
- **Unverified in this research:** that an older Hindsight image runs correctly on a newer schema; that `gpt-6-luna` and `gemini-3.5-flash-lite` allow 64,000 output tokens; the Anthropic provider's behaviour against the real API (read from source only); the `hs-embed-1` dimension (not used by default); every Rakazo v0.16.0 call made against a live Rakazo (only replayed by hand against Hindsight).

## Sources

Hindsight v1.3.0 (https://github.com/vectorize-io/hindsight/tree/v1.3.0):
- `hindsight-api-slim/hindsight_api/config.py`, `migrations.py`, `extensions/builtin/tenant.py`
- `hindsight-api-slim/hindsight_api/engine/memory_engine.py`, `engine/query_analyzer.py`, `engine/prompt_preview.py`
- `hindsight-api-slim/hindsight_api/engine/retain/fact_extraction.py`, `engine/retain/orchestrator.py`, `engine/consolidation/consolidator.py`
- `hindsight-api-slim/hindsight_api/engine/providers/openai_compatible_llm.py`, `engine/providers/anthropic_llm.py`
- `docker/standalone/Dockerfile`, `.github/workflows/release.yml`
- `hindsight-docs/docs/developer/installation.md`, `models.mdx`, `configuration.mdx`, `services.md`, `storage.md`, `retain.md`, `observations.mdx`, `admin-cli.md`, `extensions.md`, `api/retain.mdx`
- GHCR registry API (anonymous token, tag lists and manifests) for `ghcr.io/vectorize-io/hindsight`, `hindsight-api` and `hindsight-control-plane`
- Local runs of `ghcr.io/vectorize-io/hindsight-api:1.3.0`: offline with pg0; offline on `pgvector/pgvector:pg18` with `ApiKeyTenantExtension`; prompt previews; a 500-memory recall benchmark at unrestricted and 4-CPU limits

Rakazo v0.16.0 (https://github.com/elie222/rakazo/tree/v0.16.0):
- `docs/memory.md`, `docs/self-host-integrations.md`
- `packages/adapters/src/providers/memory/hindsight/hindsight-client.ts`, `hindsight-memory-provider.ts`, `hindsight-memory.ts`
- `packages/adapters/src/memory/conversation-memory.ts`, `packages/adapters/src/runs/assembly/run-assembly-history.ts`, `packages/adapters/src/runs/tools/run-tools-memory.ts`
- `packages/contracts/src/bots/bots-rpc.ts`, `apps/api/src/integrations/memory-provider-config.ts`, `packages/db/prisma/schema.prisma`

OpenRouter (queried 2026-11-24):
- https://openrouter.ai/api/v1/models
- https://openrouter.ai/api/v1/models/z-ai/glm-5.3-flash/endpoints
- https://openrouter.ai/api/v1/endpoints/zdr

Anthropic: Claude Haiku 5.5 price and output limit from Anthropic's model table (cached 2026-11-03).

Earlier rakazo-stack research: `research/honcho-needs`, `research/seed-db-settings`, `research/glm-defaults-test`, and the answer comments on #7, #11, #33, #36 and #37.
