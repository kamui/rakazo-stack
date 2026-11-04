# Run Honcho on the default models through Rakazo

Task for [#37](https://github.com/kamui/rakazo-stack/issues/37). Checks the OpenRouter defaults from
[#36](https://github.com/kamui/rakazo-stack/issues/36) on a real stack.

Everything below was **run** on 2026-11-04. The stack used Rakazo's published `app:v0.4.0` images
with the upstream `infra/compose/docker-compose.images.yml` at v0.4.0, unchanged, plus
[`compose.override.yml`](glm-defaults-test/compose.override.yml). Honcho was
`ghcr.io/plastic-labs/honcho:v3.5.0` (API and deriver) on `pgvector/pgvector:pg15`. It ran as its
own Compose project (`rs-glm-test`) and was deleted afterwards. The OpenRouter key was a throwaway
key with a $10 limit, on an account with ZDR only turned on.

- Model: `~z-ai/glm-flash-latest` (served as `z-ai/glm-5.3-flash`) for Rakazo and on all nine
  Honcho slots, each with `THINKING_EFFORT=low`.
- Embeddings: `openai/text-embedding-3-small`.

`H` = `https://github.com/plastic-labs/honcho/blob/v3.5.0` and
`R` = `https://github.com/elie222/rakazo/blob/v0.4.0`.

**How calls were seen.** [`hook/hook.mjs`](glm-defaults-test/hook/hook.mjs) is preloaded into
Rakazo's `api` and `worker` with `NODE_OPTIONS=--import`. It logs the shape and usage of every
OpenRouter call, without keys or message text. Honcho's `LLM_OPENAI_BASE_URL` and embeddings base
URL point at [`hook/proxy.mjs`](glm-defaults-test/hook/proxy.mjs), a pass-through proxy that logs
the same fields. The per-call table is
[`transcripts/calls.tsv`](glm-defaults-test/transcripts/calls.tsv). The logged cost
($0.0648) matches OpenRouter's own meter for the key ($0.0636, read from `/api/v1/key`). The
difference is a few probe calls that were made directly.

## Answer

- **`low` fits dialectic `minimal`'s 250-token cap with lots of room.** Five questions at
  `minimal` took 6 calls. The largest output was 77 tokens, with at most 6 reasoning tokens. Every
  answer was correct, including "no information" for a fact that wasn't stored. `low` behaved the
  same. **Don't raise `MAX_OUTPUT_TOKENS`.**
- **`EXTRA_BODY` parses from env only in one spelling.**
  - `…__OVERRIDES__PROVIDER_PARAMS__EXTRA_BODY='{"provider":…}'` (the spelling #36 names) arrives
    as a **string**. Honcho boots, but every LLM call on that slot then fails with
    `ValidationException: provider_params.extra_body must be a mapping, got str`.
  - `…__OVERRIDES__PROVIDER_PARAMS='{"extra_body":{"provider":{"zdr":true,"data_collection":"deny","require_parameters":true}}}'`
    parses into a mapping. Each request then carries the `provider` block next to
    `reasoning_effort: "low"`. **Use this spelling in env**, so no `config.toml` is needed.
- **Everything routes under ZDR only.**
  - With ZDR only on, a request pinned to OpenAI's non-ZDR embeddings endpoint was refused:
    `404 … ZDR violation (guardrail)`, `zdr-violation-by-guardrail`. Honcho's embeddings went
    to Azure, and its chat calls went to ZDR endpoints (BaseTen, CoreWeave, DeepInfra,
    InferenceNet, Makora, Relace, Wafer). Rakazo's chat went to BaseTen.
  - OpenRouter now files the setting as a **workspace guardrail**
    (`https://openrouter.ai/workspaces/default/guardrails`).
  - **Setup can check it**, which #36 said it couldn't. The pinned request above fails with
    `zdr-violation-by-guardrail` when ZDR only is on. When it's off, the request succeeds for
    about $0.00000002.
- **Rakazo v0.4.0 sends `reasoning: {"effort": "high"}` for GLM.** It sent that on all 41 calls,
  for a bot with no model or thinking level of its own, so the env default model was used. Its
  default `medium` is clamped to the nearest level GLM has, rounding up. Reasoning stayed small:
  1,812 of 10,917 output tokens (17%) over the day, and at most 315 in one call.
  - Rakazo sends no OpenRouter provider preferences. Requests carry `store: false`, a hashed
    `user`, `max_completion_tokens: 32768` and streaming.
- **Chat works.** All 30 turns completed with no errors. Tool calls (`save_memory`,
  `recall_memory`, `remember`, `update_bot`, `ask_user`) were well formed, and the replies made
  sense.
- **Dreams never fire from conclusions Rakazo writes.** There are two independent reasons:
  1. **Rakazo sends no `session_id`** when it writes a conclusion
     ([R/packages/adapters/src/honcho-client.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/honcho-client.ts),
     `createHonchoConclusions`). Honcho stores the conclusion anyway, as an explicit document with
     `session_name` null. Its due-dream query then skips any pair whose newest explicit
     conclusion has no session
     ([H/src/dreamer/dream_due.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/dreamer/dream_due.py),
     `if newest_session_name is None: continue`).
  2. **With Honcho's default `DERIVER_SCHEDULER=deriver`, dreams are only checked after the
     deriver saves memory derived from messages**
     ([H/src/crud/representation.py#L236-L240](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/crud/representation.py#L236-L240)).
     Rakazo never posts messages. Only `DERIVER_SCHEDULER=api` runs the due-dream poll
     ([H/src/backlog.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/src/backlog.py)).

  **Proof.** The dream threshold was lowered to 1 conclusion and the idle time to 1 minute.
  Rakazo's three conclusions were never due, and nothing was queued. One control conclusion with a
  `session_id`, in the same pair, became due. With `DERIVER_SCHEDULER=api` it was queued within
  seconds, and the dream ran.
  - **Dreams work on GLM when they run.** Deduction and induction made 9 tool calls in 14 s and
    wrote 2 deductive and 3 inductive conclusions, all sensible. A dream cost **$0.0008**: 25k
    input tokens, 75% of them cached, and 839 output tokens.
  - **A manual `POST /v3/workspaces/{id}/schedule_dream`** with no session also ran a full dream
    (8 tool calls). That is the one way to dream over Rakazo's conclusions without changing
    Rakazo.
- **Memory recall returns sensible results.** The bot's one `recall_memory` call ran a two-call
  dialectic `low` and answered correctly. Direct dialectic queries at `minimal` and `low` were
  all right (above).
  - **Rakazo only recalls automatically after a thread's history has been compacted**
    (`thread.historyCompactedUpToSeq != null`,
    [R/packages/adapters/src/executor.ts#L731-L745](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/executor.ts#L731-L745)).
    Before that, Honcho is used only when the bot calls `recall_memory`. So in a young thread,
    the dialectic runs far less than the 15 recalls a day assumed in #33.
  - Only 3 of the 6 facts reached Honcho. The bot saved the others with Rakazo's own `remember`
    tool. Two of the three conclusions are near-duplicates: API writes bypass Honcho's dedup.
- **A day of typical use cost $0.058, about $1.75 a month.** That is under #36's ~$6 estimate.
  See [Cost](#cost).

## Cost

The workload was 30 turns of household-assistant use in one thread
([`day.txt`](glm-defaults-test/day.txt) plus a first message). It mixed facts to remember,
everyday tasks and explicit memory questions. Prices are what OpenRouter billed.

| Part | Calls | Input tokens (cached) | Output (reasoning) | Cost |
| --- | --- | --- | --- | --- |
| Rakazo chat (GLM, effort `high`) | 41 | 365,406 (23,808, 6.5%) | 10,917 (1,812) | $0.0574 |
| Honcho dialectic `low` (1 recall) | 2 | 8,281 (3,840) | 108 (17) | $0.0006 |
| Embeddings (Azure) | 7 | 143 | – | $0.000003 |
| **Day** | 50 | | | **$0.0581** |

- **Per month: about $1.75** (×30). Reasoning tokens added about $0.001 a day. A daily dream
  would add about $0.02 a month.
- **Why the day cost less than estimated.** The estimate assumed 3 model calls per turn with
  20k-token prompts and 800 output tokens. The day averaged 1.4 calls per turn, 8.9k-token
  prompts and 266 output tokens. That is one young thread, with no web search or long tool loops.
- **Caching worked against the estimate.** The estimate assumed 80% cache hits. Rakazo's calls
  all went to BaseTen and got 6.5%, and Rakazo can't steer routing. At #33's heavier workload
  with this cache rate, Rakazo would cost about **$9 a month**: 2,700 calls of 20k input at
  $0.142 per million (blended) and 800 output at $0.50 per million. So the range is about
  **$2–9 a month** for one active user, with the low end measured.

## Other things seen

- **Honcho's deriver needs outbound access once.** It downloads tiktoken's `o200k_base` file on
  first start and crashes without it. This matches #13's "tokenizer files cached in a volume".
- **Rakazo asks the Owner for AI consent.** Before the first run, Rakazo lists OpenRouter and
  Honcho as data recipients (`aiConsent.status`) and needs `aiConsent.allow`. This is a UI step,
  and the checklist should mention it.
- **A new Owner has no bots.** `bots.list` is empty until one is created.

## Scripts

- [`make-env.sh`](glm-defaults-test/make-env.sh) writes the Rakazo `.env` and `honcho.env` (the
  nine slots) from a key file. Secrets are random and never printed.
- [`rpc.sh`](glm-defaults-test/rpc.sh) signs in and calls `/rpc`.
  [`chat.sh`](glm-defaults-test/chat.sh) sends a message and waits for the run.
- [`analyze.sh`](glm-defaults-test/analyze.sh) totals the call logs. Its output is in
  [`transcripts/totals.jsonl`](glm-defaults-test/transcripts/totals.jsonl), and the replies are in
  [`transcripts/day.log`](glm-defaults-test/transcripts/day.log).
- Dream settings used for the proof: `DREAM_DOCUMENT_THRESHOLD=1`,
  `DREAM_IDLE_TIMEOUT_MINUTES=1`, `DREAM_DUE_POLL_INTERVAL_SECONDS=15`, then
  `DERIVER_SCHEDULER=api`.
