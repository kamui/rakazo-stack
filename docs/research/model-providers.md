# Which model providers does v1 document?

Research for [#33](https://github.com/kamui/rakazo-stack/issues/33). Pinned to **Rakazo v0.4.0**
and **Honcho v3.5.0**. Rakazo's model catalog comes from its bundled `@earendil-works/pi-ai`
1.3.0 package. Prices were read on 2026-11-03 from the provider pricing pages and the live
OpenRouter models API. They change often, so re-check them at spec time.

Links: `R` = `https://github.com/elie222/rakazo/blob/v0.4.0`,
`H` = `https://github.com/plastic-labs/honcho/blob/v3.5.0`.

## Answer

- **Recommendation: v1 documents OpenRouter first, then OpenAI and Google Gemini.** Anthropic
  is documented for Rakazo only. These are the only providers where one key can run both
  Rakazo and Honcho, including 1536-dimension embeddings. OpenRouter is the only one of them
  that Rakazo can also take from env for the whole Instance.
- **How Rakazo is configured.**
  - Only `openrouter` (`OPENROUTER_API_KEY`) and `anthropic` (`ANTHROPIC_API_KEY`) can be the
    deployment-wide model, selected with `PI_DEFAULT_PROVIDER`. Bedrock and Vertex can also
    run on host credentials.
  - Every other provider is a per-user connection stored in Rakazo's database. The user adds it
    in **Settings → Models**, or a script calls the `models.connect` RPC.
  - The Compose file passes `OPENAI_API_KEY`, `GEMINI_API_KEY` and others to the containers. In
    v0.4.0 source, though, a run with no stored credential gets a key only for the deployment
    provider or OpenRouter, so those env keys alone do not make a model usable (read from
    source, not run: **unverified** at runtime).
- **How Honcho is configured.**
  - Everything is set in env. The three transports are `openai` (any OpenAI-compatible
    endpoint, including OpenRouter), `anthropic` and `gemini`.
  - Embeddings can use `openai` or `gemini`.
  - Each of the nine model slots needs `TRANSPORT` and `MODEL`. OpenRouter also needs a base
    URL, set once globally for chat and again for embeddings.
- **What the nine slots need.** These were checked in Honcho's source:
  - **Deriver:** structured output (`json_schema`; `json_object` is a fallback).
  - **Summary:** plain text only.
  - **Five dialectic levels and two dream slots:** tool calling.

  Every candidate model below supports both tool calling and structured output, per
  OpenRouter's API.
- **Rakazo v0.4.0 uses only part of Honcho.** It writes conclusions and calls the dialectic,
  and it never posts messages. So the deriver and summary slots should stay idle. Spend comes
  from the dialectic level the Space picks (default `low`), from dreams, and from embeddings.
  Dreams triggering from conclusions written through the API is **unverified**.
- **1536-dimension embeddings.**
  - OpenRouter: yes, `openai/text-embedding-3-small`, $0.02 per million tokens.
  - OpenAI: yes, `text-embedding-3-small`, $0.02 per million tokens. This is Honcho's default.
  - Gemini: yes. `gemini-embedding-2` ($0.20 per million tokens) or `gemini-embedding-001`
    ($0.15) at `output_dimensionality=1536`, which Honcho sends automatically.
  - Anthropic: no. It has no embedding model.
- **Suggested defaults (proposal, not tested).**
  - OpenRouter: `openai/gpt-6-luna` for Rakazo (Rakazo's own default) and for all nine Honcho
    slots, plus `openai/text-embedding-3-small`.
  - OpenAI direct: the same models without the vendor prefix.
  - Gemini direct: `gemini-3.5-flash-lite` for all nine slots, plus `gemini-embedding-2`.
  - Optional upgrade: `anthropic/claude-sonnet-5` for dialectic `max` and the two dream slots.
- **Rough monthly cost for one active user.** See [Cost](#cost) for the assumptions.
  - Rakazo chat: about $3 on `gpt-6-luna` or `claude-haiku-5.5`, $10 on
    `gemini-3.5-flash-lite`, $19 on `gemini-3.8-flash` (about $39 after 2027-01-01) and $57 on
    `claude-sonnet-5`.
  - Honcho: about $1.30 on `gpt-6-luna`, $4.40 on `gemini-3.5-flash-lite` and $10 on Honcho's
    built-in `gpt-5.4-mini`. Embeddings cost under $0.05.
  - Total with the OpenRouter defaults: about **$4–5 per month**.

## Details

### 1. Provider support matrix

| Provider | Rakazo v0.4.0 | How Rakazo is configured | Honcho v3.5.0 LLM slots | Honcho 1536-d embeddings | One key for both? |
| --- | --- | --- | --- | --- | --- |
| **OpenRouter** | Yes (default deployment provider) | Env: `OPENROUTER_API_KEY` (+ optional `PI_DEFAULT_MODEL`), or per user in UI | Yes, `openai` transport + `LLM_OPENAI_BASE_URL=https://openrouter.ai/api/v1` | Yes, `openai/text-embedding-3-small` with `EMBEDDING_MODEL_CONFIG__OVERRIDES__BASE_URL` | **Yes** |
| **OpenAI** | Yes (catalog provider `openai`) | Per user: Settings → Models or `models.connect` RPC | Yes, `openai` transport (Honcho's default) | Yes, `text-embedding-3-small` (default) | **Yes** (Rakazo side per user) |
| **Google Gemini** | Yes (catalog provider `google`) | Per user: Settings → Models or `models.connect` RPC | Yes, `gemini` transport, `LLM_GEMINI_API_KEY` | Yes, `gemini` embedding transport, `output_dimensionality=1536` | **Yes** (Rakazo side per user) |
| **Anthropic** | Yes (second deployment provider) | Env: `PI_DEFAULT_PROVIDER=anthropic`, `ANTHROPIC_API_KEY`, or per user in UI | Yes, `anthropic` transport, `LLM_ANTHROPIC_API_KEY` | **No** (no Anthropic embedding model) | **No**, embeddings need a second provider |
| Others (Groq, Together, Fireworks, DeepSeek, Mistral, xAI, Z.ai, Vercel AI Gateway, …) | Yes, 40+ providers in the pi-ai 1.3.0 catalog, plus any OpenAI-compatible URL | Per user in UI | Usually, through the `openai` transport and a base URL (not checked per provider) | Per provider (not checked) | Not checked; out of v1 scope |

#### Rakazo sources

- **Deployment model.** `resolveDeploymentModel` maps keys only for these providers:
  `keys = { openrouter: OPENROUTER_API_KEY, anthropic: ANTHROPIC_API_KEY }`.
  - Default models: `openai/gpt-6-luna` on OpenRouter and `claude-sonnet-5` on Anthropic.
  - Any other `PI_DEFAULT_PROVIDER` gets no key and reports as not configured, unless it uses
    host credentials (`PI_DEFAULT_CREDENTIALS=host` for Bedrock or Vertex).
  - Sources: [R/packages/adapters/src/deployment-model.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/deployment-model.ts),
    [R/docs/self-host-models.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md)
    ("Deployment model").
- **Run-time key resolution.** `resolveModelKey`
  ([R/packages/adapters/src/run-model.ts#L154-L182](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/run-model.ts#L154-L182)):
  - Without a stored credential, the key is `deploymentKeyFor(provider)`. That function returns
    the key only when the provider equals `PI_DEFAULT_PROVIDER`.
  - The one fallback is `OPENROUTER_API_KEY` when the provider is `openrouter`.
  - Any other provider with no key throws `MISSING_MODEL_RUN_ERROR`.
  - So `OPENAI_API_KEY` or `GEMINI_API_KEY` in env does not by itself let a user run OpenAI or
    Gemini. self-host-models.md says Compose passes these keys "for models that have no stored
    credential", but the source path above does not use them for runs. **Unverified** at
    runtime.
- **Per-user connections.**
  - The catalog is every provider in pi-ai 1.3.0, built with
    `models.getProviders()` ([R/packages/adapters/src/pi-models.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/pi-models.ts)).
    The UI lists `openai-codex`, `anthropic`, `openai`, `openrouter` and `google` first
    ([R/packages/core/src/model-providers.ts#L78](https://github.com/elie222/rakazo/blob/v0.4.0/packages/core/src/model-providers.ts#L78)).
  - The env key names come from pi-ai's `env-api-keys.js`. For example, provider `google` uses
    `GEMINI_API_KEY` and `openai` uses `OPENAI_API_KEY`.
  - Connections are saved through RPC `models.connect`
    (`{provider, apiKey, modelId, makeDefault, …}`,
    [R/packages/contracts/src/domain.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/domain.ts)
    `ModelConnectInputSchema`; [R/packages/contracts/src/rpc.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/rpc.ts)
    `models.connect`).
  - Setup could call it for the Owner the same way the
    [seed-db-settings](https://github.com/kamui/rakazo-stack/blob/research/seed-db-settings/docs/research/seed-db-settings.md)
    research describes for voice and memory (not exercised: **unverified**). Other users connect
    for themselves.
- **Consequence.** With OpenRouter or Anthropic, setup writes one env key and every user of the
  Instance can chat. With OpenAI or Gemini, each user, starting with the Owner, must connect the
  provider in the UI, or setup must call `models.connect` for the Owner.

#### Honcho sources

- **Transports.**
  - `ModelTransport` is `anthropic | openai | gemini`, and `EmbeddingTransport` is
    `openai | gemini` ([H/src/config.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py)).
  - Keys: `LLM_OPENAI_API_KEY`, `LLM_ANTHROPIC_API_KEY`, `LLM_GEMINI_API_KEY`, plus the
    matching `*_BASE_URL` (`LLMSettings`, config.py L790).
  - Provider table and tier guidance:
    [H/docs/v3/contributing/configuration.mdx](https://github.com/plastic-labs/honcho/blob/v3.5.0/docs/v3/contributing/configuration.mdx)
    ("Provider Types", "Choosing Models").
- **Embedding keys.** The embedding key falls back to `LLM_OPENAI_API_KEY` for `openai` and to
  `LLM_GEMINI_API_KEY` for `gemini` (`_default_embedding_api_key`, config.py L564).
  - The Gemini embedding client always sends
    `config={"output_dimensionality": EMBEDDING_VECTOR_DIMENSIONS}`
    ([H/src/embedding_client.py#L319-L333](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/embedding_client.py#L319-L333)),
    so the default 1536 works with no schema step.
  - Honcho ranks by cosine distance (`vector_cosine_ops`,
    [H/src/models.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/models.py)). So
    Google's note that truncated `gemini-embedding-001` vectors must be normalized does not
    affect ranking.
- **Embedding dimensions.**
  - Gemini: "By default, both models output a 3072-dimensional embedding". The output size is
    "Flexible, supports: 128 - 3072, Recommended: 768, 1536, 3072"
    ([Gemini embeddings docs](https://aistudio.google.com/docs/embeddings)).
  - Anthropic: "Anthropic does not offer its own embedding model", and it points to Voyage AI,
    whose sizes are 256, 512, 1024 and 2048. None is 1536
    ([Claude embeddings docs](https://platform.claude.com/docs/en/build-with-claude/embeddings)).
    A Voyage key would be a second paid service, and Honcho has no Voyage transport.

### 2. Honcho's nine slots: what each needs and whether Rakazo uses it

| Slot | Env prefix | What the call needs (source) | Exercised by Rakazo v0.4.0? |
| --- | --- | --- | --- |
| Deriver | `DERIVER_MODEL_CONFIG__` | One structured-output call (`response_model=PromptRepresentation, json_mode=True`), no tools ([H/src/deriver/deriver.py#L188-L193](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/deriver/deriver.py#L188-L193)). On `openai` it sends `json_schema` unless `STRUCTURED_OUTPUT_MODE=json_object`. On `gemini` it uses native `response_schema`. On `anthropic` the schema goes into the prompt. | **No**. The deriver processes messages, and Rakazo never posts messages. |
| Summary | `SUMMARY_MODEL_CONFIG__` | Plain text; no tools, no schema ([H/src/utils/summarizer.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/utils/summarizer.py)) | **No**. Session summaries need messages. |
| Dialectic `minimal` | `DIALECTIC_LEVELS__minimal__MODEL_CONFIG__` | Tools, `tool_choice=auto`, 1 iteration, 250 output tokens | Only if the Space picks it |
| Dialectic `low` | `DIALECTIC_LEVELS__low__MODEL_CONFIG__` | Tools, `tool_choice=auto`, up to 5 iterations | **Yes**, Rakazo's default level |
| Dialectic `medium` / `high` / `max` | `DIALECTIC_LEVELS__<level>__MODEL_CONFIG__` | Tools, up to 2 / 4 / 10 iterations ([config.py `_default_dialectic_levels`, L1067](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/config.py#L1067)) | Only if the Space picks it |
| Dream deduction | `DREAM_DEDUCTION_MODEL_CONFIG__` | Tool loop, up to 12 iterations ([H/src/dreamer/specialists.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/dreamer/specialists.py), `DreamSettings` L1433) | Probably, after 50 new explicit conclusions, 60 idle minutes and at least 8 hours since the last dream (**unverified** for conclusions written through the API) |
| Dream induction | `DREAM_INDUCTION_MODEL_CONFIG__` | Tool loop, up to 10 iterations | Same as deduction |

Notes:

- Honcho's docs say "All Honcho agents (deriver, dialectic, dream) require tool calling". The
  deriver code path above passes no tools, so it needs structured output rather than tools. Pick
  models that support both, and every candidate here does.
- The dialectic requests `json_schema` only when the API caller sends a `response_format`
  ([H/src/schemas/api.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/schemas/api.py)).
  Rakazo's `honchoPeerChat` sends only `query`, `reasoning_level`, `stream: false` and an
  optional `target`
  ([R/packages/adapters/src/honcho-client.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/honcho-client.ts)).
- Rakazo calls exactly these Honcho endpoints: workspaces, `peers`, `conclusions` (create,
  query, delete) and `peers/{id}/chat` (same file). Recall runs automatically when a turn
  starts in a thread whose history has been compacted
  ([R/packages/adapters/src/executor.ts#L731-L745](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/executor.ts#L731-L745)),
  and when a bot uses its memory tool. Each recall makes one dialectic chat per perspective:
  one for `isolated` scope, two for `shared`, plus one when a user peer is set. Each recall also
  runs a conclusions query. The default level is `low`
  ([R/packages/adapters/src/honcho-memory-provider.ts#L43](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/honcho-memory-provider.ts#L43)).
- **OpenRouter routing.** Structured-output support "is determined per endpoint, not just per
  model". OpenRouter tells callers to set `require_parameters: true` in provider preferences to
  make sure `json_schema` is honoured
  ([OpenRouter structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs.md)).
  This matters only for the deriver, which Rakazo leaves idle. If the deriver is ever used,
  either pass that through `OVERRIDES__PROVIDER_PARAMS__EXTRA_BODY`, or set
  `DERIVER_MODEL_CONFIG__STRUCTURED_OUTPUT_MODE=json_object`. Whether the nested
  `provider` object can be expressed in env variables is **unverified**; `config.toml` can
  express it.

### 3. Candidate models and current prices

Prices are USD per million tokens: input / cache read / output. They come from the live
`GET https://openrouter.ai/api/v1/models` call on 2026-11-03, cross-checked against the
first-party pages: [OpenAI](https://developers.openai.com/api/docs/pricing),
[Anthropic](https://platform.claude.com/docs/en/about-claude/pricing) and
[Gemini](https://aistudio.google.com/docs/pricing). OpenRouter says "We never mark-up the pricing
of the underlying providers" but charges a fee on credit purchases
([FAQ](https://openrouter.ai/docs/faq.md); the fee amount was not rendered on the page:
**unverified**).

| Model (OpenRouter id / direct id) | Price | Tools | Structured outputs | In Rakazo catalog |
| --- | --- | --- | --- | --- |
| `openai/gpt-6-luna` / `gpt-6-luna` | 0.10 / 0.01 / 0.50 | yes | yes | yes (Rakazo's default) |
| `anthropic/claude-haiku-5.5` / `claude-haiku-5-5` | 0.10 / 0.01 / 0.50 (prompts ≤100k tokens) | yes | yes | yes |
| `google/gemini-3.5-flash-lite` / `gemini-3.5-flash-lite` | 0.30 / 0.03 / 2.50 | yes | yes | yes |
| `google/gemini-3.8-flash` / `gemini-3.8-flash` | 0.75 / 0.075 / 3.75. Google lists 1.50 / 7.50 from 2027-01-01. | yes | yes | yes |
| `openai/gpt-5.4-mini` / `gpt-5.4-mini` (Honcho built-in default) | 0.75 / 0.075 / 4.50 | yes | yes | yes |
| `anthropic/claude-sonnet-5` / `claude-sonnet-5` (Rakazo's Anthropic default) | 2.00 / 0.20 / 10.00 | yes | yes | yes |
| `anthropic/claude-haiku-4.5` (Honcho docs example) | 1.00 / 0.10 / 5.00 | yes | yes | yes |
| `google/gemini-2.5-flash` (Honcho docs example) | 0.30 / 0.03 / 2.50. Deprecated: Google routes it to 3.5 Flash-Lite from 2026-12-01. | yes | yes | yes |

| Embedding model | Price per million tokens | 1536-d |
| --- | --- | --- |
| `openai/text-embedding-3-small` (OpenRouter) / `text-embedding-3-small` (OpenAI) | 0.02 | native |
| `gemini-embedding-2` (Gemini) | 0.20 | via `output_dimensionality`, auto-normalized |
| `gemini-embedding-001` (Gemini) | 0.15 | via `output_dimensionality` |

Honcho's docs examples (`gemini-2.5-flash`, `claude-haiku-4-5`) are stale for November 2026.
Newer models are cheaper and the Gemini one is deprecated, so v1 should not copy them.

### 4. Suggested defaults per slot (proposal)

None of these defaults was tested against a running Honcho (**unverified** quality and
tool-loop reliability). Reasoning models such as `gpt-6-luna` may spend part of dialectic
`minimal`'s 250 output tokens on reasoning. If answers come back empty, set
`THINKING_EFFORT=minimal` on that slot (**unverified**).

| Slot | OpenRouter | OpenAI direct | Gemini direct |
| --- | --- | --- | --- |
| Rakazo main model | `PI_DEFAULT_PROVIDER=openrouter`, `PI_DEFAULT_MODEL` unset (`openai/gpt-6-luna`) | `gpt-6-luna` via Settings → Models | `gemini-3.5-flash-lite` (or a 3.x Flash) via Settings → Models |
| Deriver, summary | `openai/gpt-6-luna` | `gpt-6-luna` | `gemini-3.5-flash-lite` |
| Dialectic minimal, low, medium, high | `openai/gpt-6-luna` | `gpt-6-luna` | `gemini-3.5-flash-lite` |
| Dialectic max, dream deduction, dream induction | `openai/gpt-6-luna`; upgrade: `anthropic/claude-sonnet-5` | `gpt-6-luna`; upgrade: `gpt-6-sol` | `gemini-3.5-flash-lite`; upgrade: a 3.x Flash |
| Embeddings | `openai/text-embedding-3-small` | `text-embedding-3-small` (default) | `gemini-embedding-2` |
| Env shape | `LLM_OPENAI_API_KEY`, `LLM_OPENAI_BASE_URL=https://openrouter.ai/api/v1`, `*_TRANSPORT=openai` on all nine, `EMBEDDING_MODEL_CONFIG__OVERRIDES__BASE_URL=https://openrouter.ai/api/v1`, `EMBEDDING_MODEL_CONFIG__MODEL=openai/text-embedding-3-small` | `LLM_OPENAI_API_KEY`; set the nine `*_MODEL` values (Honcho's defaults would work, at about 7× the cost) | `LLM_GEMINI_API_KEY`, `*_TRANSPORT=gemini` and `*_MODEL` on all nine, `EMBEDDING_MODEL_CONFIG__TRANSPORT=gemini`, `EMBEDDING_MODEL_CONFIG__MODEL=gemini-embedding-2` |

The gotchas from the [Honcho findings](https://github.com/kamui/rakazo-stack/blob/research/honcho-needs/docs/research/honcho-needs.md)
still apply:

- Always set `TRANSPORT=openai` explicitly next to OpenRouter's `vendor/model` ids.
- Embeddings do not inherit `LLM_OPENAI_BASE_URL`.

Gemini: Google's pricing page marks free-tier data as "used to improve products" and paid-tier
data as not. If v1 documents Gemini, it should say a billing-enabled key is required.

**Anthropic for one key:** Rakazo works through `PI_DEFAULT_PROVIDER=anthropic`, but Honcho
would have no embeddings. That fits "memory on only when the Self-hoster gives a model provider
key" if Anthropic is treated as a key that does not enable memory. Alternatively, the docs could
steer Anthropic users to OpenRouter, which serves Claude models and embeddings under one key.

### 5. Cost

These are assumptions, not measurements. One active user, 30 days, with the provider's prompt
caching working.

- **Rakazo chat.**
  - 30 turns a day with 3 model calls per turn (tool loops) makes 2,700 calls.
  - Each call has 20k input tokens (system prompt, tools, history), 80% of them cache hits, and
    800 output tokens.
  - Totals: 54M input tokens (10.8M uncached, 43.2M cached) and 2.2M output tokens.
- **Honcho.**
  - Dialectic: 15 recalls a day at `low` in `isolated` scope makes 450 chats. At about 3 LLM
    calls each, with 6k input and 300 output tokens per call, that is 8.1M input and 0.4M
    output.
  - Dreams: about 6 a month at about 22 calls each, with 15k input and 1k output per call, which
    is 2.0M input and 0.13M output.
  - Deriver and summary: about 0.
  - Embeddings: about 0.2M tokens.
  - No caching is assumed.

| Workload | Model | Monthly |
| --- | --- | --- |
| Rakazo | `gpt-6-luna` (OpenRouter or OpenAI) | ≈ $2.60 |
| Rakazo | `claude-haiku-5.5` | ≈ $2.90 (including cache-write premium) |
| Rakazo | `gemini-3.5-flash-lite` | ≈ $9.90 |
| Rakazo | `gemini-3.8-flash` | ≈ $19 (≈ $39 after 2027-01-01) |
| Rakazo | `gpt-5.4-mini` | ≈ $21 |
| Rakazo | `claude-sonnet-5` | ≈ $57 (including cache-write premium) |
| Honcho | all slots `gpt-6-luna` or `claude-haiku-5.5` | ≈ $1.30 |
| Honcho | all slots `gemini-3.5-flash-lite` | ≈ $4.40 |
| Honcho | built-in `gpt-5.4-mini` everywhere | ≈ $10 |
| Honcho | `claude-haiku-4.5` everywhere (docs example) | ≈ $13 |
| Honcho | upgrade: `claude-sonnet-5` on dreams | +≈ $5 |
| Honcho | embeddings (`text-embedding-3-small`) | < $0.01 (`gemini-embedding-2`: ≈ $0.04) |

Cost scales roughly linearly with turns per day and context size. Heavy agent use, such as long
tool loops or computer use, can be 10× this. Rakazo's main model dominates the bill, so the
choice of Rakazo model matters far more than the Honcho slot defaults.

## Open questions / follow-ups

- **grilling:** Confirm the documented set: OpenRouter as primary, OpenAI and Gemini as
  alternatives, Anthropic as Rakazo-only (memory off) or steered to OpenRouter. Decide whether
  setup refuses or warns when the key cannot enable memory.
- **grilling:** For OpenAI or Gemini keys, should setup call `models.connect` for the Owner, or
  tell the Self-hoster to connect in the UI? Other users always connect for themselves.
- **prototype:** Run Honcho v3.5.0 with all nine slots on `openai/gpt-6-luna` through
  OpenRouter. Check the dialectic `low` tool loop and a dream. Check whether reasoning tokens
  starve `minimal`'s 250-token cap. Check whether Rakazo's API-written conclusions ever trigger
  a dream.
- **prototype:** Confirm at runtime that `OPENAI_API_KEY` in env does not let a user without a
  stored connection run OpenAI models in Rakazo v0.4.0.
- **research:** OpenRouter's credit-purchase fee, and whether its 1-year credit expiry matters
  for small top-ups.
- **Unverified in this research:** model quality for Honcho's tool loops, all cost figures
  (estimates from assumptions), dream triggering from API conclusions, and env expression of
  OpenRouter `require_parameters`.

## Sources

Rakazo v0.4.0 (https://github.com/elie222/rakazo/tree/v0.4.0):
- `docs/self-host-models.md`
- `packages/adapters/src/deployment-model.ts`, `run-model.ts`, `pi-models.ts`, `executor.ts`,
  `honcho-client.ts`, `honcho-memory-provider.ts`
- `packages/core/src/model-providers.ts`
- `packages/contracts/src/domain.ts` (`ModelConnectInputSchema`) and `packages/contracts/src/rpc.ts` (`models.connect`)
- `infra/compose/docker-compose.images.yml` (provider key pass-through)
- `@earendil-works/pi-ai` 1.3.0 from the npm registry: `dist/env-api-keys.js`, `dist/providers/data/*.json`

Honcho v3.5.0 (https://github.com/plastic-labs/honcho/tree/v3.5.0):
- `docs/v3/contributing/configuration.mdx`
- `src/config.py`, `src/embedding_client.py`, `src/models.py`
- `src/deriver/deriver.py`, `src/utils/summarizer.py`, `src/dialectic/core.py`
- `src/dreamer/specialists.py`, `src/dreamer/dream_due.py`, `src/crud/representation.py`, `src/routers/conclusions.py`
- `src/llm/backends/openai.py`, `anthropic.py`, `gemini.py`
- `src/schemas/api.py`

Providers (read 2026-11-03):
- OpenRouter: `https://openrouter.ai/api/v1/models`, `https://openrouter.ai/api/v1/embeddings/models`,
  https://openrouter.ai/docs/guides/features/structured-outputs.md, https://openrouter.ai/docs/faq.md
- OpenAI: https://developers.openai.com/api/docs/pricing
- Anthropic: https://platform.claude.com/docs/en/about-claude/pricing,
  https://platform.claude.com/docs/en/build-with-claude/embeddings
- Google: https://aistudio.google.com/docs/pricing, https://aistudio.google.com/docs/embeddings

Earlier rakazo-stack findings:
- [honcho-needs](https://github.com/kamui/rakazo-stack/blob/research/honcho-needs/docs/research/honcho-needs.md),
  [rakazo-requirements](https://github.com/kamui/rakazo-stack/blob/research/rakazo-requirements/docs/research/rakazo-requirements.md),
  [seed-db-settings](https://github.com/kamui/rakazo-stack/blob/research/seed-db-settings/docs/research/seed-db-settings.md)
