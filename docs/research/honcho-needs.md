# What does Honcho need?

Ticket: kamui/rakazo-stack#7. Pinned versions: Honcho v3.5.0 (commit `0cd37ddb`), Lemonade v2026.44.1, Rakazo v0.4.0.

## Answer

- **LLM providers.** Honcho 3.5.0 supports three transports: `openai`, `anthropic`, `gemini`. The `openai` transport works with any OpenAI-compatible endpoint, including OpenRouter. Every call needs a model that supports tool calling.
- **Model settings.** There is no single global model setting. Nine model configs each need `TRANSPORT`, `MODEL` and, when not using OpenAI itself, an endpoint: `DERIVER_`, `SUMMARY_`, `DIALECTIC_LEVELS__{minimal,low,medium,high,max}__`, `DREAM_DEDUCTION_`, `DREAM_INDUCTION_` (each followed by `MODEL_CONFIG__...`). Defaults are `openai` / `gpt-5.4-mini`. `LLM_OPENAI_API_KEY` and `LLM_OPENAI_BASE_URL` set the key and endpoint for every `openai`-transport model that has no per-module override, so one OpenRouter key and base URL cover all nine. The model IDs must still be set to OpenRouter names.
- **Sharing the Self-hoster's model provider.** Yes for OpenRouter. If the Self-hoster brings an Anthropic, Gemini or OpenAI key instead, the matching `LLM_{ANTHROPIC,GEMINI,OPENAI}_API_KEY` works directly.
- **Embeddings.** The transport is `openai` (default `text-embedding-3-small`, 1536 dimensions) or `gemini`. The `openai` transport accepts any OpenAI-compatible `/embeddings` endpoint through `EMBEDDING_MODEL_CONFIG__OVERRIDES__BASE_URL`. Embeddings are configured separately from the LLM modules and do not inherit `LLM_OPENAI_BASE_URL`, so the embeddings base URL must be set explicitly or calls go to `api.openai.com`.
- **OpenRouter has an embeddings endpoint.** `POST https://openrouter.ai/api/v1/embeddings` is OpenAI-compatible and lists `openai/text-embedding-3-small` and `openai/text-embedding-3-large` among its models. `openai/text-embedding-3-small` is 1536 dimensions, which matches Honcho's default schema and needs no database step. So one OpenRouter key can cover chat and embeddings.
- **Local embeddings in Lemonade are possible but cost a one-time schema step.** Lemonade v2026.44.1 serves `POST /v1/embeddings` (also under `/api/v1`) for `llamacpp` models and ships embedding models: `nomic-embed-text-v1-GGUF` (768 dimensions), `nomic-embed-text-v2-moe-GGUF` (768), `Qwen3-Embedding-0.6B-GGUF` (1024), `-4B` (2560), `-8B` (4096). None is 1536 dimensions, so Honcho's database must be altered with `scripts/configure_embeddings.py` before first start.
- **Dimension constraints.**
  - The dimension is fixed for the life of an Instance: Honcho refuses to start on a mismatch, and changing it later means a new deployment and re-ingest.
  - The pgvector HNSW index limit of 2000 dimensions rules out Lemonade's 4B and 8B Qwen models.
  - Changing the embedding model at the same dimension is not detected and silently degrades recall.
  - Local embeddings also change the Instance's privacy and quality tradeoffs, which the map should decide (see follow-ups).
- **Infrastructure.** Honcho needs an API process and a deriver process (both from one image), Postgres with the `vector`, `pg_trgm` and `btree_gin` extensions, and optionally Redis. Upstream's own compose uses `pgvector/pgvector:pg15` and `redis:8.2`. LanceDB is not needed: `VECTOR_STORE_TYPE=pgvector` is the default, and LanceDB only ships in a source build with `INSTALL_LANCEDB=true`. The upstream `mcp` service is not needed by Rakazo.
- **Secrets.** `AUTH_USE_AUTH=true` plus `AUTH_JWT_SECRET` (a random hex string; `scripts/generate_jwt_secret.py` uses `secrets.token_hex(32)`). The admin token is an HS256 JWT signed with that secret, minted by `scripts/generate_jwt.py --admin --print-only`. Also needed: one provider key (`LLM_OPENAI_API_KEY` or equivalent) and a generated Postgres password.
- **Images.** Upstream publishes multi-arch (linux/amd64 and linux/arm64) images to `ghcr.io/plastic-labs/honcho`, tagged per release (`v3.5.0` exists; the bare `3.5.0` does not) plus a moving `latest`. No source build is needed. The v3.5.0 index digest is `sha256:23ff96bf62ab2ca14928e6766db1a30a7729f5b7c9e2285af0cd4a179c0d393d`.
- **How Rakazo connects.** Rakazo v0.4.0 has no Honcho environment variables. The connection is a per-Space setting stored in Rakazo's database, entered under Settings > Memory > Honcho > Local. It needs:
  - a base URL (loopback or a private/Compose-network hostname),
  - an API key (an admin JWT, so each Space gets its own workspace),
  - optional workspace and user-peer IDs.

  The operator must also set `MCP_ALLOW_PRIVATE_ENDPOINT=true` on Rakazo's API and worker containers.
- **Simpler join than the socat bridge.** Yes. The bridge exists because the reference install runs Honcho from its own compose project, and Honcho's service is named `api`, which collides with Rakazo's `api` on a shared network. rakazo-stack writes its own compose file around the pinned images, so it can run everything in one project and name Honcho's service `honcho` (or alias it). Then `http://honcho:8000` resolves from Rakazo with no relay container.

## Details

### 1. LLM providers and per-feature settings

- `ModelTransport = Literal["anthropic", "openai", "gemini"]`, `EmbeddingTransport = Literal["openai", "gemini"]` ([src/config.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py)).
- The configuration guide lists the transports and says OpenAI-compatible endpoints (OpenRouter, Together, Fireworks, LiteLLM, vLLM, Ollama) use `transport = openai` plus `MODEL_CONFIG__OVERRIDES__BASE_URL` ([configuration.mdx](https://github.com/plastic-labs/honcho/blob/v3.5.0/docs/v3/contributing/configuration.mdx)). It also states all agents (deriver, dialectic, dream) require tool calling.
- Setting names are in [.env.template](https://github.com/plastic-labs/honcho/blob/v3.5.0/.env.template) and [config.toml.example](https://github.com/plastic-labs/honcho/blob/v3.5.0/config.toml.example). The features that call an LLM are:
  - deriver: `DERIVER_MODEL_CONFIG__*`
  - summary: `SUMMARY_MODEL_CONFIG__*`
  - dialectic: five levels, `DIALECTIC_LEVELS__<level>__MODEL_CONFIG__*`
  - dream: `DREAM_DEDUCTION_MODEL_CONFIG__*` and `DREAM_INDUCTION_MODEL_CONFIG__*`
- Each model config also accepts `FALLBACK__MODEL/TRANSPORT`, `OVERRIDES__BASE_URL`, `OVERRIDES__API_KEY_ENV`, `THINKING_EFFORT` and `STRUCTURED_OUTPUT_MODE`. The last is `json_object` for providers without `json_schema` support. It applies only to the deriver on the `openai` transport (configuration.mdx).
- Client selection ([src/llm/registry.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/llm/registry.py), `default_client` / `client_for_model_config`):
  - A model config with no `api_key` and no `base_url` override uses the default client built from `LLM_OPENAI_API_KEY` and `LLM_OPENAI_BASE_URL`.
  - A model config with a `base_url` override uses that URL and falls back to `LLM_OPENAI_API_KEY` for the key.
  - The OpenRouter base URL gets attribution headers automatically.
- The self-hosting guide ([self-hosting.mdx, "Running without a model provider"](https://github.com/plastic-labs/honcho/blob/v3.5.0/docs/v3/contributing/self-hosting.mdx)) confirms `LLM_OPENAI_BASE_URL` redirects the default client, that embeddings "resolve through a separate client that reads the base URL only from the per-module override", and that setting a per-module credential override makes that module ignore the global base URL.
- **Gotcha (read from source, not run).** `_normalize_model_transport` in config.py splits a model string such as `openai/foo` or `anthropic/foo` into transport and bare model when `transport` is unset. OpenRouter model IDs have exactly this shape, so always set `*_MODEL_CONFIG__TRANSPORT=openai` explicitly. The upstream examples do the same.
- **Gotcha.** The built-in default model `gpt-5.4-mini` is a bare OpenAI model name, and OpenRouter model IDs carry a vendor prefix (as in the upstream OpenRouter examples, such as `google/gemini-2.5-flash`), so redirecting the base URL alone is not enough. All nine model IDs must be set.
- Upstream's guidance on tiers (light for deriver/summary/low dialectic, heavier for dream and max dialectic) is in configuration.mdx. Which models rakazo-stack defaults to is a decision for the map.

### 2. Embeddings

- Defaults: `EMBEDDING_VECTOR_DIMENSIONS=1536`, `EMBEDDING_MODEL_CONFIG__TRANSPORT=openai`, `__MODEL=text-embedding-3-small` ([config.py, `EmbeddingSettings`](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py)).
- Settings that matter for non-OpenAI endpoints:
  - `__OVERRIDES__BASE_URL`
  - `__OVERRIDES__API_KEY_ENV` (the name of an environment variable holding the key)
  - `__MAX_BATCH_SIZE`
  - `__DIMENSIONS_MODE` (`auto`, `always`, `never`)
  - `__ENCODING_FORMAT_MODE`
- The embedding client ([src/embedding_client.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py)) uses `AsyncOpenAI(api_key, base_url)` and falls back to `LLM_OPENAI_API_KEY` when no per-module key is set. So an embeddings endpoint on a different host than the LLM endpoint (for example Lemonade next to OpenRouter) needs its own key variable through `OVERRIDES__API_KEY_ENV`. Otherwise the provider key is sent to the local server. Honcho requires a non-empty key, so a placeholder value is needed for an unauthenticated server.
- `resolve_encoding_format()` requests `float` for any host other than `api.openai.com`, so OpenRouter and Lemonade get floats.
- `resolve_send_dimensions()` in `auto` mode sends a `dimensions=` request parameter only if `EMBEDDING_VECTOR_DIMENSIONS` was set explicitly. Lemonade's documented embeddings parameters are only `input`, `model` and `encoding_format`, so `dimensions` is not documented there (**unverified** whether llama-server ignores or rejects it). Use `DIMENSIONS_MODE=never` for Lemonade.

**OpenRouter.** Official docs:
- [Embeddings guide](https://openrouter.ai/docs/api/reference/embeddings.md): "OpenRouter provides a unified API to access embedding models from multiple providers"; the request is `POST /embeddings`.
- [API reference](https://openrouter.ai/docs/api/api-reference/embeddings/submit-an-embedding-request.md): server `https://openrouter.ai/api/v1`; request body `model`, `input` (string or array), optional `dimensions` and `encoding_format` (`float` or `base64`). The documented example is `openai/text-embedding-3-small` with `dimensions: 1536`.
- The live model list (`GET https://openrouter.ai/api/v1/embeddings/models`, queried 2026-11-02) returned 38 models, including `openai/text-embedding-3-small`, `openai/text-embedding-3-large`, `qwen/qwen3-embedding-4b` and `-8b`, `baai/bge-m3`, `google/gemini-embedding-001` and a few free NVIDIA models. The list changes over time, so a pinned model ID should be re-checked at spec time.
- Using `openai/text-embedding-3-small` through OpenRouter keeps the schema at 1536 dimensions, so no `configure_embeddings.py` step is needed. The required explicit `TRANSPORT=openai` is covered by the gotcha above.

**Lemonade (v2026.44.1).**
- [docs/api/openai.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/api/openai.md), section "POST /v1/embeddings": request `{model, input, encoding_format}`, OpenAI response shape. The endpoint "is only available for models using the `llamacpp` or `flm` recipes". The same file shows clients using base URL `http://localhost:13305/api/v1`, and Lemonade's API key is optional (`LEMONADE_API_KEY`).
- Model labels: a model has exactly one deployment mode, so an embeddings model is registered as its own `embeddings` model, separate from chat models (same file, "Model Labels").
- Built-in embedding models ([server_models.json](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/server_models.json)): `nomic-embed-text-v1-GGUF` (Q4_K_S, 0.078 GB), `nomic-embed-text-v2-moe-GGUF`, `Qwen3-Embedding-0.6B-GGUF`, `-4B-GGUF`, `-8B-GGUF`. All `suggested: true`, label `embeddings`.
- Native dimensions, from the upstream model cards:
  - [nomic-embed-text-v1](https://huggingface.co/nomic-ai/nomic-embed-text-v1/raw/main/config.json): `n_embd` 768.
  - [nomic-embed-text-v2-moe](https://huggingface.co/nomic-ai/nomic-embed-text-v2-moe): 768, Matryoshka down to 256.
  - Qwen3-Embedding [0.6B](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B): up to 1024, [4B](https://huggingface.co/Qwen/Qwen3-Embedding-4B): up to 2560, [8B](https://huggingface.co/Qwen/Qwen3-Embedding-8B): up to 4096. Each supports custom output dimensions on the model side, but Lemonade documents no way to request one (**unverified** whether llama.cpp truncation is reachable through Lemonade).
- pgvector: a `vector` column can be HNSW-indexed only up to 2,000 dimensions ([pgvector README](https://github.com/pgvector/pgvector/blob/v0.8.0/README.md), "Indexing"/"vector" type table). Honcho creates HNSW indexes on both embedding columns (`src/models.py`). So the 4B and 8B Qwen models at native size cannot be used.
- Practical local choices: `nomic-embed-text-v1-GGUF` (768) or `Qwen3-Embedding-0.6B-GGUF` (1024). Either needs `EMBEDDING_VECTOR_DIMENSIONS` set to the model's native dimension plus the one-time schema step.

**The schema step.** Migrations create `vector(1536)` columns. Both API and deriver run `validate_embedding_schema` at boot and crash on a mismatch ([src/startup/embedding_validator.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/startup/embedding_validator.py)). `docker/entrypoint.sh` only runs migrations (`scripts/provision_db.py`), not `configure_embeddings.py`. The documented sequence for a non-1536 install is `alembic upgrade head`, then `scripts/configure_embeddings.py --yes`, then start API and deriver ([script header](https://github.com/plastic-labs/honcho/blob/v3.5.0/scripts/configure_embeddings.py)). The script refuses to alter populated tables, so it must run on a fresh database. Both scripts ship in the published image (the Dockerfile copies `scripts/`). Switching dimension or model afterwards is documented as "you can't, in place" ([changing-embeddings.mdx](https://github.com/plastic-labs/honcho/blob/v3.5.0/docs/v3/contributing/changing-embeddings.mdx)).

Rakazo's own docs say the same: "set `EMBEDDING_VECTOR_DIMENSIONS` before its first start" ([self-host-integrations.md, v0.4.0](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md)).

### 3. Infrastructure

- Upstream compose ([docker-compose.yml.example](https://github.com/plastic-labs/honcho/blob/v3.5.0/docker-compose.yml.example)): `api` (port 8000), `deriver`, `mcp` (3000), `database` = `pgvector/pgvector:pg15` with `database/init.sql`, `redis` = `redis:8.2`. API and deriver both set `DB_CONNECTION_URI=postgresql+psycopg://...`, `CACHE_URL=redis://...`, `CACHE_ENABLED=true`.
- `database/init.sql` creates `vector`, `pg_trgm`, `btree_gin`. Honcho also runs `CREATE EXTENSION IF NOT EXISTS vector` itself, which needs privilege unless it was pre-created (self-hosting.mdx, "Enable Extensions"). `pg_trgm` and `btree_gin` are optional; migrations skip the indexes that need them.
- Postgres version: upstream CI and compose use pg15 ([unittest.yml](https://github.com/plastic-labs/honcho/blob/v3.5.0/.github/workflows/unittest.yml)); the docs mention 15, 16 and 17 for native installs. Upstream gives no statement about 18 (**unverified** for Honcho, though `pgvector/pgvector` publishes `-pg18` tags, for example `0.8.x-pg18`, on Docker Hub). The migration lock needs session-mode Postgres connections.
- Honcho needs its own database with pgvector. Rakazo's stock `postgres` image does not include pgvector (**unverified** by query here), so the stack should run a separate pgvector Postgres for Honcho, and a Postgres major upgrade for it is a separate pinning question.
- Redis: caching is optional. `config.toml.example` defaults `[cache] ENABLED = false`, and the API logs a warning and continues if the cache is unreachable (`src/main.py`, `init_cache`). The upstream compose turns it on. Whether to include Redis is a v1 choice.
- LanceDB: only for `VECTOR_STORE_TYPE=lancedb`, which needs a source build with `INSTALL_LANCEDB=true` (Dockerfile). The published image does not include it. Not needed.
- Hardening notes for the stack: upstream compose uses `postgres/postgres` credentials and `POSTGRES_HOST_AUTH_METHOD=trust` with ports published on loopback, labelled in a comment as not for production. rakazo-stack should generate a Postgres password, drop `trust`, and not publish the database or Honcho API beyond the internal network.

### 4. Secrets and admin token

- `AuthSettings` ([config.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py)): `AUTH_USE_AUTH` (default false) and `AUTH_JWT_SECRET`; startup fails if auth is on and the secret is unset.
- `scripts/generate_jwt_secret.py` returns `secrets.token_hex(32)`. [scripts/generate_jwt.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/scripts/generate_jwt.py) `--admin [--expires 1y] --print-only` prints an admin token. `create_jwt` ([src/security.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/security.py)) is plain HS256 over `{t, ad: true, ...}`, with no expiry unless requested, so the stack can mint the admin token itself or run the script from the published image (`docker run --rm -e AUTH_JWT_SECRET ... <image> python scripts/generate_jwt.py --admin --print-only`).
- Rotating `AUTH_JWT_SECRET` invalidates every issued token, including the one stored in Rakazo, so rotation has to update both.
- Other secrets: the provider key(s) (`LLM_*_API_KEY`) and the Postgres password. `WEBHOOK_SECRET` is only for webhooks and is not needed.
- Rakazo's docs recommend enabling `AUTH_USE_AUTH` and using the admin JWT ([self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md)).

### 5. Container images

- [docker-build.yml](https://github.com/plastic-labs/honcho/blob/v3.5.0/.github/workflows/docker-build.yml) builds on pushes to `main` and tags `v*`, for `linux/amd64,linux/arm64`, pushes to `ghcr.io/plastic-labs/honcho` with tags `type=ref,event=tag` plus `latest`, and generates build provenance attestation.
- Verified live against the registry's anonymous token and manifest endpoints (2026-11-02): `ghcr.io/plastic-labs/honcho:v3.5.0` resolves to an OCI index with amd64 and arm64 manifests, digest above. `3.5.0` (no `v`) returns 404.
- The self-hosting guide says "A pre-built image is published at `ghcr.io/plastic-labs/honcho:latest` ... it is not on Docker Hub" and the upstream `honcho start` CLI template runs `${HONCHO_IMAGE:-ghcr.io/plastic-labs/honcho:latest}` ([template](https://github.com/plastic-labs/honcho/blob/v3.5.0/honcho-cli/src/honcho_cli/local/templates/docker-compose.yml)), so published images are the supported self-host path.
- The same image runs both services: API uses `entrypoint: ["sh", "docker/entrypoint.sh"]` (migrations, then `fastapi run`), deriver uses `/app/.venv/bin/python -m src.deriver`.
- Image-based config: the image has no `.env`; all settings can be passed as environment variables, and `config.toml` is optional (the Dockerfile copies `config.toml*`; the CLI template mounts one read-only). Environment variables win over `.env`, which wins over `config.toml`. Caveat: `load_dotenv(override=True)` means an `.env` file inside the container working directory beats exported variables (self-hosting.mdx).
- The `mcp` image is a separate build (`./mcp`), not published as far as found (**unverified**), and Rakazo does not use it.
- `latest` tracks `main`, not the last release, so the stack should pin `v3.5.0` (or a digest).

### 6. How Rakazo connects (Rakazo v0.4.0)

Sources are inside the v0.4.0 image and the tag: `packages/adapters/src/honcho-memory-provider.ts`, `honcho-client.ts`, `local-service-endpoint.ts`, `apps/web/src/pages/memory-providers/HonchoSettingsForm.tsx`, `docs/self-host-integrations.md`.

- Honcho appears only as one of three memory providers. The Rakazo v0.4.0 `docker-compose.images.yml` and `.env.images.example` contain no Honcho variable. Configuration lives in the database as a per-Space `spaceMemoryConfig` (`settings` plus an encrypted `credentials.apiKey`).
- Settings for mode `local`: `mode=local`, `baseUrl`, optional `workspaceId`, optional `userPeerId`, optional `reasoningLevel` (`minimal|low|medium|high|max`). Credential: `apiKey`, 8 to 4096 characters.
- Connect-time check (`prepareHonchoConnection`): with a workspace entered it does get-or-create on that workspace; without one it lists workspaces with an admin key (`POST /v3/workspaces/list?size=1`). Without a workspace, each Space creates its own `rakazo-<spaceId>` workspace on first use, which needs an admin key. So an admin JWT and an empty workspace field is the "per-Space workspace" setup.
- API paths used: `/v3/workspaces...` (Honcho v3 API), bearer auth.
- Network rule: `MCP_ALLOW_PRIVATE_ENDPOINT=true` is required on API and worker for any non-cloud endpoint, even loopback. The URL must be loopback or resolve only to private addresses (a Compose service name qualifies). Hostnames are DNS-pinned per request. The cloud URL is refused in local mode.
- Because the setting is per Space and stored in Rakazo's database, a setup command cannot pass it through compose environment. It would need either the Self-hoster to paste URL and key in the UI (Rakazo's documented flow) or the stack to call Rakazo's API on their behalf (not investigated).

**Join options (reference install vs. simpler).** The reference install keeps Honcho in a separate compose project and bridges it into Rakazo's network with an `alpine/socat` container carrying the alias `honcho`. Upstream's compose names the Honcho API service `api`; attaching that service directly to Rakazo's network would register a second `api` DNS name there, shadowing Rakazo's own `api`. Simpler options, all of which avoid a relay:
1. One compose project for the whole stack, Honcho service named `honcho` (this is the natural fit: rakazo-stack owns the compose file).
2. If Honcho stays a separate project, attach only its API container with a unique alias (a renamed service, or `docker network connect --alias honcho`), never under the name `api`.
Only the API (port 8000) needs to be reachable from Rakazo; the deriver, database and Redis stay on Honcho's own internal network. Option 1 is the proposal; whether the stack uses a single project or several is a design decision for the spec.

## Open questions / follow-ups

- **grilling:** Embeddings default: OpenRouter `openai/text-embedding-3-small` (1536, no schema step, sends message text to a hosted embedder) versus Lemonade local (768 or 1024, schema step, fully local). This is permanent per Instance. It touches the "self-sufficient" and privacy principles. Which is the v1 default, and is a switch offered at all?
- **research:** Does Lemonade/llama-server (v2026.44.1, CPU, `llamacpp`) honour or reject a `dimensions` parameter, and does `Qwen3-Embedding-0.6B`/`nomic-embed-text-v1` actually return float vectors of the expected size and batch sizes Honcho sends? A short prototype against the pinned image would settle it. (**prototype**)
- **research:** Which models to default for the nine Honcho model slots on OpenRouter (tool calling and `json_schema` structured output for the deriver must work), and what is the expected token cost? Also what happens if the Self-hoster's provider is not OpenRouter (Anthropic/Gemini/OpenAI direct): does the setup flow map each provider onto the transports?
- **research:** Rakazo v0.4.0 connection is a per-Space database setting. Is there an API or CLI path (authenticated, scriptable) to set the Honcho connection and `MCP_ALLOW_PRIVATE_ENDPOINT`-gated flags, or must the Self-hoster paste values in the UI? Affects "the setup command ... writes config and starts everything".
- **task/grilling:** Single compose project versus several; whether to include Redis for Honcho; Honcho's Postgres major version (pg15 is what upstream tests; pg18 is unverified for Honcho). These belong with the "Version pinning and upgrades" fog on the map.
- **research:** Backup/restore for Honcho (Postgres dump of its database; `AUTH_JWT_SECRET` must be restored with it or Rakazo's stored admin token must be reminted). Feeds the backup ticket.
- **research:** Memory and CPU footprint of Honcho's API, deriver and an embedding model next to Lemonade (voice) on the hardware floor. Not measured here.
- **Unverified in this research:** whether Honcho starts without any provider key (the docs say it fails to start; the code path only returns no default client); `dimensions` handling in Lemonade; Postgres 18 support in Honcho; whether the `mcp` image is published; the Rakazo v0.4.0 mid-flow behaviour when Honcho is unreachable at connect time was not exercised.

## Sources

Honcho v3.5.0 (https://github.com/plastic-labs/honcho/tree/v3.5.0):
- `.env.template`, `config.toml.example`, `docker-compose.yml.example`, `Dockerfile`, `docker/entrypoint.sh`, `database/init.sql`
- `src/config.py`, `src/embedding_client.py`, `src/llm/registry.py`, `src/startup/embedding_validator.py`, `src/security.py`, `src/models.py`, `src/main.py`
- `scripts/generate_jwt.py`, `scripts/generate_jwt_secret.py`, `scripts/configure_embeddings.py`, `scripts/provision_db.py`
- `docs/v3/contributing/self-hosting.mdx`, `configuration.mdx`, `changing-embeddings.mdx`
- `.github/workflows/docker-build.yml`, `.github/workflows/unittest.yml`
- `honcho-cli/src/honcho_cli/local/templates/docker-compose.yml`
- `CHANGELOG.md` ([3.5.0, 2026-10-29](https://github.com/plastic-labs/honcho/blob/v3.5.0/CHANGELOG.md))
- GHCR registry API for `ghcr.io/plastic-labs/honcho` (anonymous pull token, manifest `v3.5.0`)

Lemonade v2026.44.1 (https://github.com/lemonade-sdk/lemonade/tree/v2026.44.1):
- `docs/api/openai.md`, `src/cpp/resources/server_models.json`

OpenRouter:
- https://openrouter.ai/docs/api/reference/embeddings.md
- https://openrouter.ai/docs/api/api-reference/embeddings/submit-an-embedding-request.md
- https://openrouter.ai/api/v1/embeddings/models

Model cards and pgvector:
- https://huggingface.co/nomic-ai/nomic-embed-text-v1, https://huggingface.co/nomic-ai/nomic-embed-text-v2-moe
- https://huggingface.co/Qwen/Qwen3-Embedding-0.6B, `-4B`, `-8B`
- https://github.com/pgvector/pgvector/blob/v0.8.0/README.md
- https://hub.docker.com/r/pgvector/pgvector/tags

Rakazo v0.4.0 (https://github.com/elie222/rakazo/tree/v0.4.0), also read inside the published v0.4.0 image of a reference install:
- `docs/self-host-integrations.md`, `docs/self-host-models.md`
- `packages/adapters/src/honcho-memory-provider.ts`, `honcho-client.ts`, `local-service-endpoint.ts`, `memory-provider-factory.ts`
- `apps/web/src/pages/memory-providers/HonchoSettingsForm.tsx`, `registry.ts`
- `infra/compose/docker-compose.images.yml`, `.env.images.example`
