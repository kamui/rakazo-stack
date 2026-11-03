# Does Lemonade serve Honcho's embeddings correctly?

Ticket: kamui/rakazo-stack#12 (prototype). Feeds the decision in #13 ("Hosted or local embeddings for Honcho?").
Pinned versions: Honcho `ghcr.io/plastic-labs/honcho:v3.5.0` (commit `0cd37ddb`), Lemonade `ghcr.io/lemonade-sdk/lemonade-server:v2026.44.1` (commit `69948a7`, bundled llama.cpp build 11393 / `dbe4c3ed4`), `pgvector/pgvector:pg18`.
Harness: [`lemonade-embeddings/`](lemonade-embeddings/) (throwaway compose project, no host ports; see "How it was tested").

## Answer

- **Yes, with three settings.** Honcho's `openai` embedding transport works against Lemonade's `/api/v1/embeddings` with `LEMONADE_API_KEY`. End to end with `nomic-embed-text-v1-GGUF` (768-dim) on Postgres 18, Honcho stored 768-dim vectors and semantic search worked. Six message searches and three conclusion queries, none sharing a word with their target, all ranked the right item first. The three settings:
  1. `EMBEDDING_VECTOR_DIMENSIONS` plus the one-time schema step.
  2. A lower `EMBEDDING_MAX_INPUT_TOKENS`.
  3. For nomic only, a larger llama-server batch.
- **Request shape (observed by a logging proxy):**
  - One `POST /api/v1/embeddings` per Honcho operation, with body `{model, input: [...], encoding_format: "float", dimensions: N}` and `Authorization: Bearer`.
  - Creating 12 messages sent one request with 12 inputs. Five conclusions sent one request with 5 inputs. A search sends 1 input.
  - The batch cap is `EMBEDDING_MODEL_CONFIG__MAX_BATCH_SIZE` (default 2048 inputs) and 300k tokens per request. Lemonade handled 2048-input batches.
  - Lemonade returns float lists of the model's native size, L2-normalised, in input order, with matching counts.
- **`dimensions` is ignored, not rejected.** Lemonade returns the native size whatever `dimensions` says. Honcho sends `dimensions` because `EMBEDDING_VECTOR_DIMENSIONS` is set explicitly; it's harmless when it equals the native size. There's no way to get 1536 from these models. `EMBEDDING_MODEL_CONFIG__DIMENSIONS_MODE=never` makes the request honest.
- **Long inputs break by default, and one bad input fails the whole batch.**
  - Honcho splits text using OpenAI's `cl100k` token count, at up to 8192 tokens.
  - Lemonade starts llama-server with an 8192-token context and the default physical batch of 512. nomic (a BERT-style model) then rejects any input over about 510 tokens with HTTP 500, and can never take more than its 2048-token training context.
  - Qwen3-Embedding-0.6B accepts up to about 8191 tokens.
  - Honcho sends a whole create call as one request, so the long message's short neighbours were not embedded either. They stay `pending` and are retried every 10 minutes, up to 20 times. Lowering the limit later does not re-split rows already stored.
  - **Fix:** `EMBEDDING_MAX_INPUT_TOKENS=1500` for nomic (about 7000 for Qwen3-0.6B). For nomic, also save llama-server args `-ub 2048 -b 2048` once via `POST /api/v1/models/nomic-embed-text-v1-GGUF/options` (persisted in Lemonade's config volume). With both, a 3000-token message was split into 3 chunks and everything embedded.
- **One-time schema step.**
  - The image's entrypoint only runs migrations. A non-1536 Instance needs a one-shot `scripts/provision_db.py && scripts/configure_embeddings.py --yes` before the API and deriver start. That step drops the HNSW indexes, `ALTER`s both `embedding` columns to `vector(N)` and recreates the indexes.
  - It is a no-op when the schema already matches, so it can run on every `up`. It refuses once any embedding exists.
  - API and deriver crash at boot on a mismatch.
  - `VECTOR_STORE_MIGRATED` does nothing for pgvector; it only concerns moving to an external vector store.
- **Candidate models (Lemonade v2026.44.1 registry):**

  | Model | Dims | Licence | Download | Notes |
  | --- | --- | --- | --- | --- |
  | `nomic-embed-text-v1-GGUF` (Q4_K_S) | 768 | Apache-2.0 | 78 MB | Fast; 2048-token limit |
  | `Qwen3-Embedding-0.6B-GGUF` (Q8_0) | 1024 | Apache-2.0 | 0.64 GB | Multilingual, 8k inputs, about 7× slower |

  Also in the registry: `nomic-embed-text-v2-moe` (768, 0.51 GB), and Qwen3 4B/8B (2560/4096 dims, over pgvector's 2000-dim HNSW limit, so unusable).
- **CPU speed** (llama-server uses 12 threads on a 12-core/24-thread Zen 4 desktop; short chat messages of about 37 tokens):

  | Model | Short messages, batched | One short message | 1000-token input | 2000-token input | 8000-token input | Memory |
  | --- | --- | --- | --- | --- | --- | --- |
  | nomic | 60–80 msg/s (≈2,500 tok/s, ≈200 tok/s per core) | 5–10 ms | 0.4 s | 0.9 s | n/a | under 1 GB with the model loaded |
  | Qwen3-0.6B | ≈10 msg/s (≈400 tok/s, ≈33 tok/s per core) | 20–30 ms | 1.8 s | 4 s | 30 s | about 2 GB peak |

  Fine for live chat on either model. Bulk re-embedding of 10k messages is about 2–3 min with nomic and about 17 min with Qwen.
- **Not measured:** how embeddings and Whisper/Kokoro slow each other down. Testing was stopped because the WSL2 VM restarted three times during it (next bullet).
- **Stability (unresolved).**
  - Under embedding load, the CPU llama-server segfaulted 7 times (`libggml-cpu-zen4.so`) across two sessions. The first followed a 100-concurrent-request burst; the other came during a large Qwen batch.
  - The whole WSL2 VM restarted three times during testing, twice shortly after these segfaults and once with no logged cause.
  - Whether Lemonade caused the restarts is **unverified**. Re-test before shipping local embeddings.
- **Quality (published, vendor-reported):**
  - nomic-embed-text-v1 MTEB average 62.39 vs text-embedding-3-small 62.26 and ada-002 60.99. That is nomic's own table, and it assumes `search_query:`/`search_document:` prefixes.
  - Qwen3-Embedding-0.6B: MTEB multilingual 64.33, English v2 70.70. Its card says skipping the query instruction costs 1–5%.
  - Neither Honcho nor Lemonade adds those prefixes or instructions, so real quality will be somewhat below the published figures (by how much is **unverified**).
- **Permanence and privacy.**
  - The dimension is fixed per Instance, and the model is fixed by convention: Honcho can't detect a model swap at the same dimension.
  - Upstream's switch path is a new deployment plus replaying data. Re-embedding messages in place by NULLing them works on paper, but conclusions aren't re-embedded in pgvector mode; untested.
  - Local embeddings keep message text away from an embedding provider. But Honcho's deriver and dialectic still send that text to the LLM provider, so "local" doesn't make memory private.
  - Honcho also downloads tiktoken's encoding files from `openaipublic.blob.core.windows.net` at runtime, whichever option is chosen, unless a cache volume is pre-seeded.

## Details

### 1. Honcho v3.5.0's embedding client

Source: `src/embedding_client.py`, `src/config.py` at <https://github.com/plastic-labs/honcho/tree/v3.5.0>.

- **Transport.** `openai` builds `AsyncOpenAI(api_key, base_url)` from `EMBEDDING_MODEL_CONFIG__OVERRIDES__BASE_URL` and the key named by `EMBEDDING_MODEL_CONFIG__OVERRIDES__API_KEY_ENV` ([embedding_client.py#L250-L265](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L250-L265), [config.py `resolve_embedding_model_config`](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L572-L591)). Observed User-Agent: `AsyncOpenAI/Python 2.36.0`.
- **Batching.**
  - `max_batch_size = config.max_batch_size or 2048` ([#L265](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L265)), plus a per-request token budget, `EMBEDDING_MAX_TOKENS_PER_REQUEST` (default 300,000) ([config.py#L836-L845](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L836-L845)).
  - Each batch is retried 3 times with 1s/2s backoff ([#L555-L647](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L555-L647)).
  - Up to 10 immediate-embed calls run concurrently per API process (`MAX_CONCURRENT_EMBEDDINGS`).
- **Token limits.**
  - Tokens are counted with tiktoken (`encoding_for_model`, falling back to `cl100k_base`) ([#L267-L271](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L267-L271)), not the embedding model's tokenizer.
  - Messages are split into chunks of at most `EMBEDDING_MAX_INPUT_TOKENS` (default 8192) with 20% overlap when created ([crud/message.py#L497](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/crud/message.py#L497)), and the chunks are stored as `message_embeddings` rows.
  - Conclusions are truncated to the limit (`on_oversize="truncate"`, [crud/document.py#L1307-L1309](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/crud/document.py#L1307-L1309)). Search queries over the limit get a validation error.
  - `MAX_MESSAGE_SIZE` is 25,000 characters ([config.py#L1661](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L1661)), so a single message can be about 6k tokens.
- **`dimensions`.** In `auto` mode, `resolve_send_dimensions()` sends `dimensions=` only when `EMBEDDING_VECTOR_DIMENSIONS` was set explicitly. `always` and `never` force it ([config.py#L858-L873](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L858-L873)).
- **`encoding_format`.** `auto` sends `"float"` to any host except `api.openai.com` ([config.py#L39](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L39), [#L875-L889](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L875-L889), [embedding_client.py#L285-L294](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L285-L294)).
- **What it expects back.**
  - Exactly one vector per input, or it raises "Embedding count mismatch".
  - Each vector exactly `EMBEDDING_VECTOR_DIMENSIONS` long, or it raises "Embedding dimension mismatch" ([#L277-L306](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L277-L306)).
- **Failure handling.**
  - The immediate path leaves rows `pending` on failure ([reconciler/embed_now.py#L211-L230](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/reconciler/embed_now.py#L211-L230)).
  - The reconciler re-embeds pending message rows in batches of up to 50, with a 10-minute backoff, and marks them `failed` after 20 attempts ([reconciler/sync_vectors.py#L33-L40](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/reconciler/sync_vectors.py#L33-L40), [#L417-L470](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/reconciler/sync_vectors.py#L417-L470)).
  - Each batch is a single request, so one oversized row also blocks every row batched with it.
- **Search** is hybrid: pgvector cosine search and full-text search, merged by reciprocal rank fusion ([utils/search.py#L382-L440](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/utils/search.py#L382-L440)). The test queries share no words with their targets, so the hits came from the vectors.
- **Schema step.**
  - [`scripts/configure_embeddings.py`](https://github.com/plastic-labs/honcho/blob/v3.5.0/scripts/configure_embeddings.py) (header L1-L23, apply L185-L245) runs after `alembic upgrade head`. It refuses if any non-null embedding exists, drops the HNSW indexes, runs `ALTER COLUMN embedding TYPE vector(N) USING NULL` on `documents` and `message_embeddings`, then recreates the indexes. When the settings already match it's a no-op.
  - [`src/startup/embedding_validator.py`](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/startup/embedding_validator.py) makes the API and deriver crash on a mismatch.
  - [`docker/entrypoint.sh`](https://github.com/plastic-labs/honcho/blob/v3.5.0/docker/entrypoint.sh) only runs `scripts/provision_db.py`.
  - Observed log: `altering public.documents.embedding to vector(768)` … `pgvector schema is now at dim 768`.
- **`VECTOR_STORE_MIGRATED`** ([config.py#L1550](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L1550)) is read only as `TYPE == "pgvector" or not MIGRATED`. That is always true for pgvector; the flag is for dual-writing during a move to Turbopuffer/LanceDB/Qdrant/Chroma. The validator's docstring says it replaced the old config-time `MIGRATED` gate for non-1536 dims.
- **Changing later.** [changing-embeddings.mdx](https://github.com/plastic-labs/honcho/blob/v3.5.0/docs/v3/contributing/changing-embeddings.mdx): "Short answer: you can't, in place." The dimension is machine-enforced. The model is "operator-owned … by contract", and a same-dimension swap "will silently succeed" and degrade recall. The supported path is a new deployment plus replaying data through the API.
  - Read from source, **untested**: the reconciler re-embeds message rows whose `embedding` is NULL in pgvector mode. But `documents` (conclusions) are re-embedded only when an external vector store is configured ([sync_vectors.py#L777-L805](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/reconciler/sync_vectors.py#L777-L805), [vector_store/__init__.py#L245-L260](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/vector_store/__init__.py#L245-L260)). So an in-place switch would need a custom script for conclusions.
- **tiktoken files.**
  - The image has no tiktoken cache, so the first tokenizer use downloads `cl100k_base` (embeddings) and `o200k_base` (message schemas, [schemas/api.py#L368](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/schemas/api.py#L368)) from `openaipublic.blob.core.windows.net`.
  - Observed: on a network without internet access, this fails with `NameResolutionError`.
  - The harness pre-seeds a `TIKTOKEN_CACHE_DIR` volume once, so Honcho itself ran without internet access. Without it, the files are re-downloaded whenever the container is recreated.

### 2. Lemonade v2026.44.1

- **Registry** ([server_models.json#L1829-L1880](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/server_models.json#L1829-L1880)), confirmed via `GET /api/v1/models?show_all=true`. Every model below has recipe `llamacpp` and label `embeddings`.

  | Model | Checkpoint | Size (GB) |
  | --- | --- | --- |
  | `nomic-embed-text-v1-GGUF` | `nomic-ai/nomic-embed-text-v1-GGUF:Q4_K_S` | 0.078 |
  | `nomic-embed-text-v2-moe-GGUF` | `…:Q8_0` | 0.51 |
  | `Qwen3-Embedding-0.6B-GGUF` | `…-Q8_0.gguf` | 0.64 |
  | `Qwen3-Embedding-4B-GGUF` | | 4.28 |
  | `Qwen3-Embedding-8B-GGUF` | | 8.05 |

  Pulls took 8 s (nomic) and 34 s (Qwen3-0.6B).
- **Dimensions and licences.**
  - [nomic-embed-text-v1](https://huggingface.co/nomic-ai/nomic-embed-text-v1): Apache-2.0, 768 (observed), "must include a task instruction prefix".
  - [Qwen3-Embedding-0.6B](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B): Apache-2.0, up to 1024, 32k context.
  - 4B and 8B exceed pgvector's 2000-dim HNSW limit, so they are unusable here (see #7 findings).
- **API.**
  - `POST /v1/embeddings` takes `model`, `input`, `encoding_format` (`float`|`base64`) ([docs/api/openai.md#L274-L290](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/api/openai.md#L274-L290)), and also works under `/api/v1`. `dimensions` is undocumented.
  - Observed: requesting 256 or 1536 returned the native size with HTTP 200, on both models.
  - Without the key, `GET /api/v1/models` returns 401.
  - Embeddings load the model on demand: 0.7–4 s cold.
- **How llama-server is launched.**
  - Embedding models get at least `--ctx-size 8192` (`EMBEDDING_CTX_SIZE`, [auto_tune.h#L21](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/auto_tune.h#L21), [llamacpp_server.cpp#L316-L324](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/llamacpp/llamacpp_server.cpp#L316-L324)), `--parallel 1` ([#L140](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/llamacpp/llamacpp_server.cpp#L140)) and `--embeddings`. No `-ub`/`-b` is set.
  - llama-server logged: `embeddings enabled with n_batch (2048) > n_ubatch (512)… setting n_batch = n_ubatch = 512`, and for nomic, `n_ctx_seq (8192) > n_ctx_train (2048)`, `n_ctx_slot = 2048`, `n_threads = 12`.
  - The default `llamacpp_backend` is `vulkan`, but the container has no Vulkan devices (`llama-server --list-devices` → none), so it runs on the CPU (`libggml-cpu-zen4.so` was chosen).
- **Per-model args.**
  - `llamacpp_args` may include `-ub`/`-b` (only `-m`, `--port`, `--ctx-size`, `-ngl`, `--jinja`, `--mmproj`, `--embeddings`, `--reranking` are disallowed; [docs/api/lemonade.md#L1137](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/api/lemonade.md#L1137)).
  - `POST /v1/models/{id}/options` saves them without loading ([#L460-L473](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/api/lemonade.md#L460-L473)).
  - Observed: they are stored in `/opt/lemonade/.config/lemonade/recipe_options.json`, which the #10 layout's `lemonade-config` volume persists.
- **Coexistence with speech.**
  - Each model type (LLM, Embedding, Reranking, Transcription, Image) has its own LRU pool, each sized by `max_loaded_models` (default 1) ([multi-model.md#L7-L21](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/multi-model.md#L7-L21)). So loading an embedding model should not evict Whisper.
  - Kokoro's pool is not listed (**unverified**).
  - CPU contention was not measured.

### 3. End-to-end prototype

Compose project `rs-embed-test`:

- `lemonade` with `LEMONADE_API_KEY`;
- `embed-spy`, a logging reverse proxy in front of Lemonade;
- `honcho-db` (`pgvector/pgvector:pg18` with `vector`, `pg_trgm`, `btree_gin`);
- `honcho-setup` (one-shot `provision_db.py` + `configure_embeddings.py --yes`);
- `honcho` (API) and `honcho-deriver` (v3.5.0).

`AUTH_USE_AUTH=false` (internal network only). The chat model slots got a dummy key and an unreachable base URL, so the deriver's LLM work (conclusions from messages, dialectic) was not exercised; that needs a real LLM. Conclusions were created directly through `POST /v3/workspaces/{id}/conclusions`, which embeds them without an LLM.

Results with `nomic-embed-text-v1-GGUF`, `EMBEDDING_VECTOR_DIMENSIONS=768`, Lemonade `-ub 2048 -b 2048`:

| Check | Default `EMBEDDING_MAX_INPUT_TOKENS=8192` | `EMBEDDING_MAX_INPUT_TOKENS=1500` |
| --- | --- | --- |
| Create 12 short messages | 1 request, 12 inputs → 12 × 768 float, stored | same (87 ms) |
| Create 1 short + 1 long message (3000 cl100k tokens, 13.6k chars) | 1 request, 2 inputs → **HTTP 500** `input (3109 tokens) is too large…`; both rows stay `pending`; 9 failed requests (3 immediate attempts plus the retry path) | 1 request, 4 inputs (short + 3 overlapping chunks) → 4 × 768, all `synced` |
| Message search, 6 queries with no shared words | 6/6 correct top hit | 6/6 |
| Conclusions: create 5, query 3 | 1 request, 5 inputs; 3/3 correct top hit | 3/3 |
| Schema | `documents.embedding` and `message_embeddings.embedding` are `vector(768)`, both HNSW indexes present | same |

Every request carried `{dimensions: 768, encoding_format: "float", input, model}` with `Authorization: Bearer`. Lemonade answered with float lists of 768, in order. Ten minutes after the default-limit run, the two rows of the failed batch were still `pending`; the higher-limit run did not re-split them.

A Qwen3-Embedding-0.6B end-to-end run got as far as `honcho-setup` (schema altered to `vector(1024)`) before the VM restarted, so it is **not verified end to end**. Its direct probe (same SDK and request shape) is below.

### 4. Direct probe, both models (OpenAI SDK 2.36.0 against `/api/v1`)

| Case | nomic-v1 (default Lemonade) | nomic-v1 (`-ub 2048 -b 2048`) | Qwen3-0.6B (default) |
| --- | --- | --- | --- |
| Native dims, float, normalised, deterministic | 768 ✓ | 768 ✓ | 1024 ✓ |
| `dimensions` = 256 / native / 1536 | ignored, 768 | ignored, 768 | ignored, 1024 |
| `encoding_format` omitted (SDK asks for base64) | works | works | works |
| Empty string | 1 vector | 1 vector | 1 vector |
| Batch 64 / 512 / 2048 short inputs | 0.9 s / 8.3 s / 33.9 s | 1.0 s / 6.8 s / 30.7 s | 4.7 s / 47.9 s / 214.6 s |
| One input of 511+ cl100k tokens | **500** "increase the physical batch size (512)" | OK up to 2000 | OK |
| 2047+ tokens | 500 | 500/400 (2048-token training context) | OK up to 8100 (29.7 s); 8192 → 400 context exceeded |
| Cosine: same / related / unrelated sentence | 1.0 / 0.70 / 0.44 | same | 1.0 / 0.84 / 0.23 |

Memory (`docker stats` for the Lemonade container, which includes page cache): 246 MiB idle, 482 MiB with nomic at default batch, 818 MiB with nomic at `-ub 8192`, and a 2.06 GiB peak during the Qwen3-0.6B probe.

These figures come from one machine: a 12-core/24-thread AMD Zen 4 desktop CPU, 16 GB available to WSL2, Docker Desktop. Another throwaway stack was running at the same time during part of the runs, so treat them as rough. llama-server used 12 threads (one per physical core), so a 4-core machine should see roughly a third of this throughput (**unverified** scaling).

### 5. Stability during testing

The kernel log of the WSL2 VM showed repeated llama-server crashes:

```
llama-server[…]: segfault at … error 6 in libggml-cpu-zen4.so
WSL (…- CaptureCrash): Capturing crash for pid: …, executable: !opt!lemonade!.cache!lemonade!bin!llamacpp!vulkan!llama-server, signal: 11
```

- **First session:** one crash during the Qwen3-0.6B probe, then the VM restarted about 40 s later.
- **Second session:** six crashes within a minute, right after a burst of 100 concurrent requests (the kernel also logged "Possible SYN flooding on port 13305"), then the VM restarted.
- **Third session:** the VM restarted while containers were starting, with no segfault or OOM logged.

Other projects were running on the same Docker Desktop VM, and the crash binary path is shared by every Lemonade container. So which process crashed, and whether the crashes caused the restarts, is **unverified**. The same embedding loads also passed several times.

### 6. Quality vs `text-embedding-3-small` (published, vendor-reported)

- nomic's card table (MTEB, classic English):

  | Model | Context | MTEB avg |
  | --- | --- | --- |
  | nomic-embed-text-v1 | 8192 | 62.39 |
  | text-embedding-3-small | 8191 | 62.26 |
  | text-embedding-ada-002 | 8191 | 60.99 |

  Source: <https://huggingface.co/nomic-ai/nomic-embed-text-v1>. These scores use nomic's task prefixes. OpenAI's own announcement page could not be fetched (HTTP 403), so the 3-small figure here is as reported by nomic.
- Qwen3-Embedding-0.6B card: MTEB multilingual mean 64.33 (text-embedding-3-large: 58.93 in the same table) and MTEB English v2 mean 70.70 (<https://huggingface.co/Qwen/Qwen3-Embedding-0.6B>). The English v2 and classic MTEB numbers are not comparable.
- Honcho and Lemonade add no task prefixes or query instructions (no `search_query`/`search_document`/`Instruct:` in either codebase at the pinned tags). Qwen's card puts the cost of skipping the query instruction at 1–5%; for nomic the cost is unquantified.
- nomic-v1 is English-centred and Qwen3 is multilingual. That matters for non-English Self-hosters (not tested).

### What a local-embeddings Instance needs (if #13 picks local)

1. Lemonade: pull the model (`POST /api/v1/pull {"model_name": …}`). For nomic, also `POST /api/v1/models/nomic-embed-text-v1-GGUF/options {"llamacpp_args": "-ub 2048 -b 2048"}`.
2. Honcho (API and deriver):
   - `EMBEDDING_MODEL_CONFIG__TRANSPORT=openai`
   - `EMBEDDING_MODEL_CONFIG__MODEL=<Lemonade id>`
   - `EMBEDDING_MODEL_CONFIG__OVERRIDES__BASE_URL=http://lemonade:13305/api/v1`
   - `EMBEDDING_MODEL_CONFIG__OVERRIDES__API_KEY_ENV=LEMONADE_API_KEY`, with `LEMONADE_API_KEY` passed through
   - `EMBEDDING_VECTOR_DIMENSIONS=768|1024`
   - `EMBEDDING_MAX_INPUT_TOKENS=1500|7000`
   - optionally `EMBEDDING_MODEL_CONFIG__DIMENSIONS_MODE=never`
3. A one-shot `honcho-setup` service (`provision_db.py && configure_embeddings.py --yes`) that `honcho` and `honcho-deriver` depend on with `service_completed_successfully`.
4. On macOS, where Lemonade runs natively, Honcho must reach the host's Lemonade instead of the `lemonade` service (not tested).

Hosted (via the #7 findings) needs only the OpenRouter base URL, key and `openai/text-embedding-3-small`: no schema step and no Lemonade work. Both options benefit from a tiktoken cache volume.

## Open questions / follow-ups

- **prototype:** Re-test Lemonade embedding stability on a clean machine, with nothing else on the Docker VM: sustained and concurrent load on the CPU build (`libggml-cpu-zen4`, llama.cpp `dbe4c3ed4`). Does llama-server segfault, and does it take the WSL2 VM down? This blocks shipping local embeddings. Voice (Whisper/Kokoro) uses other backends, so it is not implicated by this.
- **prototype:** CPU contention between embeddings and Whisper/Kokoro in one Lemonade (latency of a voice reply during a message burst), and whether to cap llama-server threads (`-t`) for the embedding model.
- **research:** Kokoro's model-type pool in Lemonade and the idle-eviction defaults (`evict_idle_timeout: 300` appears in the effective options): does the embedding model unload after 5 minutes idle, and is the cold reload of 1–4 s acceptable?
- **task:** Ship a tiktoken cache (volume pre-seeded by setup) for Honcho regardless of the embeddings choice, so Honcho doesn't depend on `openaipublic.blob.core.windows.net` at runtime.
- **grilling (for #13):** If local, which model: nomic-v1 (fast, English, 2k chunks) or Qwen3-0.6B (multilingual, about 7× slower)? Is an in-place switch ever offered, given conclusions can't be re-embedded by Honcho's pgvector reconciler?
- **research:** Upstream issues to file or check: Lemonade could set `-ub` to the context size for non-causal embedding models and document `dimensions`; Honcho could count tokens with the target model's tokenizer, or drop an oversized item instead of failing the whole batch.

## How it was tested

`lemonade-embeddings/run.sh nomic|qwen [long-message-tokens] [max-input-tokens]` brings up the compose project, pulls the model, applies the batch option, runs `probe_lemonade.py shape`, recreates Honcho's database, and runs `honcho_e2e.py`. `embed_spy.py` logs each request's shape (never text or keys) to a volume. Tear down with `docker compose -p rs-embed-test down -v --remove-orphans`. The throughput figures above came from an earlier version of the probe that also ran a sequential speed sweep and a 100-request concurrency burst; that burst was removed after the crashes in §5.

## Sources

Honcho v3.5.0 (<https://github.com/plastic-labs/honcho/tree/v3.5.0>):
- `src/embedding_client.py`, `src/config.py`, `src/crud/message.py`, `src/crud/document.py`, `src/utils/search.py`
- `src/reconciler/embed_now.py`, `src/reconciler/sync_vectors.py`, `src/vector_store/__init__.py`, `src/startup/embedding_validator.py`, `src/schemas/api.py`
- `scripts/configure_embeddings.py`, `scripts/provision_db.py`, `docker/entrypoint.sh`, `.env.template`
- `docs/v3/contributing/changing-embeddings.mdx`

Lemonade v2026.44.1 (<https://github.com/lemonade-sdk/lemonade/tree/v2026.44.1>):
- `src/cpp/resources/server_models.json`, `src/cpp/server/backends/llamacpp/llamacpp_server.cpp`, `src/cpp/include/lemon/auto_tune.h`
- `docs/api/openai.md`, `docs/api/lemonade.md`, `docs/guide/configuration/multi-model.md`

Model cards:
- <https://huggingface.co/nomic-ai/nomic-embed-text-v1>
- <https://huggingface.co/Qwen/Qwen3-Embedding-0.6B>

Related findings:
- #7 "What does Honcho need?": <https://github.com/kamui/rakazo-stack/blob/research/honcho-needs/docs/research/honcho-needs.md>
- #10 layout prototype: <https://github.com/kamui/rakazo-stack/tree/prototype/compose-layout/prototypes/compose-layout>

Observations: the throwaway prototype in this directory, run on a 12-core x86-64 machine under Docker Desktop/WSL2 on 2026-11-02.
