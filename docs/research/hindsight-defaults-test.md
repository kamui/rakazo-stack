# Run Hindsight on the default models through Rakazo

Task for [#45](https://github.com/kamui/rakazo-stack/issues/45). Checks the defaults that
[What does Hindsight need?](https://github.com/kamui/rakazo-stack/issues/44) proposed, on a real
stack with a real OpenRouter key.

Everything below was **run** on 2026-11-25, on Docker Desktop for Windows (WSL 2 backend, 24 CPUs).
The stack ran as its own Compose project (`rs45`) on internal networks with no published Hindsight
port, and was deleted afterwards. The OpenRouter key belonged to an account with ZDR only turned on.

- **Rakazo v0.16.0** (commit `6e24f7e6`), upstream `infra/compose/docker-compose.images.yml` at that
  tag, unchanged (sha256 `732f4e4f…de02`), plus
  [`compose.override.yml`](hindsight-defaults-test/compose.override.yml).
  - **`ghcr.io/elie222/rakazo/app:v0.16.0` doesn't exist** (GHCR returns 404 for the tag; `v0.15.0`
    and `latest` exist). The app image was built locally from the unmodified v0.16.0 tag with the
    repo's own `infra/compose/Dockerfile`. `computer:v0.16.0` is also missing; bot computers were off
    (`SANDBOX_PROVIDER=none`), so `computer:v0.15.0` only filled the variable.
- **Hindsight** `ghcr.io/vectorize-io/hindsight-api:1.3.0@sha256:c054765e9196fb1fe63497be06bcb55256c9024cc9314f951dc59858985ab242`
  on `pgvector/pgvector:pg18@sha256:38681dc6eaf55308e2a4c6de392d131652dcfac70e1263e35cd9b5ea3fe39782`
  with `shm_size: 1g`.
  - `ApiKeyTenantExtension` with a generated tenant key; Rakazo has `MCP_ALLOW_PRIVATE_ENDPOINT=true`.
  - `HINDSIGHT_API_LLM_PROVIDER=openrouter`, `HINDSIGHT_API_LLM_MODEL=~z-ai/glm-flash-latest`,
    `HINDSIGHT_API_LLM_REASONING_EFFORT=low`,
    `HINDSIGHT_API_LLM_EXTRA_BODY={"provider":{"zdr":true,"data_collection":"deny"}}`,
    `HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100`.
  - Pinned local models: `BAAI/bge-small-en-v1.5` (embedder) and `hindsight-ai/hs-rerank-1`
    (reranker).
- **Rakazo's own model:** OpenRouter, `~z-ai/glm-flash-latest` (served as `z-ai/glm-5.3-flash`).

**How calls were seen.** As in [#37](glm-defaults-test.md):
[`hook/hook.mjs`](hindsight-defaults-test/hook/hook.mjs) is preloaded into Rakazo's `api` and
`worker`, and Hindsight's `HINDSIGHT_API_LLM_BASE_URL` points at
[`hook/proxy.mjs`](hindsight-defaults-test/hook/proxy.mjs), a pass-through proxy. Both log the shape
and usage of every OpenRouter call, without keys or message text. For the `openrouter` provider,
Hindsight's cache affinity resolves to `none` whatever the base URL, so the proxy doesn't change the
requests. Hindsight's own `llm_requests` trace table was also exported. The per-call table is
[`transcripts/calls.tsv`](hindsight-defaults-test/transcripts/calls.tsv) (289 calls). The logged
cost, **$0.2483**, matches OpenRouter's meter for the key (`GET /api/v1/key`, usage before and
after) to the tenth of a cent.

## Answer

- **Spend for the whole ticket: $0.2483** (under the ~$1 budget). It covers two simulated days of 23
  turns plus 7 follow-up turns, 289 calls.
- **Memory costs about 2× the estimate.**
  - Measured: **$0.0026–0.0041 per turn**, or **$2.4–3.7 a month** at 30 turns a day. #44 estimated
    $0.0010–0.0018 per turn, or $1–2 a month.
  - Rakazo's own chat on GLM costs $0.0014 per turn (~$1.3 a month), with 80% of input tokens cached.
  - The Instance comes to **about $3.7–5 a month**, inside #44's $3–11 range.
  - Two causes:
    - Consolidation makes 1.3–2.2 calls per turn, not one. They average 8k input tokens and 1k output
      (a third of it reasoning), and inputs grow to 16.7k by the end of a day.
    - Hindsight's calls get almost no cache hits: 5% on consolidation and 16% on extraction. OpenRouter
      spread them over 17 providers.
- **Append re-extraction does re-consolidate.**
  - Each turn, Hindsight deletes the facts from the conversation's last one or two chunks and
    extracts them again under new IDs. Every new copy is consolidated again.
  - Over 23 turns, 96 facts were consolidated to end with 39. 57 of the 96 replaced copies of facts
    that had already been consolidated.
- **Extraction and recall quality on GLM are good**, with one recurring fault.
  - With Rakazo's recall settings (observations only, budget `mid`, 4,096 tokens), all 7 questions
    after a day of turns found every expected fact, with a relevant result at rank 1, in about 1.0 s
    each.
  - The fault: the bot's own guesses and mistakes get extracted as facts about the user. A
    hallucinated "you (Inês) live in Porto" was stored, merged into an observation and repeated
    later.
- **No retain went to an endpoint capped under 64,000 output tokens.**
  - All 81 extraction calls asked for `max_tokens: 64000` and finished with `stop`, on 16 providers,
    each capped at 131,072 or more.
  - The only GLM endpoint with a lower cap (Darkbloom, 29,491) isn't ZDR, so ZDR-only already
    excludes it.
- **`POST /rpc/memory/connectProvider` with `provider:"hindsight"` works on v0.16.0 as #44 wrote it.**
  - It returned 200 with `connectionLabel "Self-hosted (http://hindsight:8888)"` and
    `defaultMemoryScope "isolated"`.
  - Read-only detection SQL showed `hindsight: false` before the call and `true` after. A re-run of
    setup found nothing missing.
- **v0.16.0 no longer needs the Owner's `aiConsent` for chat or memory.**
  - Chat, retain and recall all ran with no consent rows. `aiConsent/status` lists OpenRouter and
    Hindsight as not allowed.
  - In v0.16.0, consent gates only voice calls and page review comments.
- **New findings that change setup or docs:**
  - Rakazo v0.16.0 was released with no `app` or `computer` image. Setup that resolves "newest
    release" would have failed.
  - Clearing a conversation also deletes everything Hindsight learned from it, in both banks.
  - Hindsight needs an egress network to reach its model provider. #44 put it on the internal `stack`
    network only.
  - New memories show up in recall seconds to minutes after the turn: retain takes a median 2.9 s,
    consolidation a median 5 s and up to 115 s.

## Details

### 1. What was run

[`setup-connect.sh`](hindsight-defaults-test/setup-connect.sh) set up the Instance the way setup
would (section 4). Then [`chat.sh`](hindsight-defaults-test/chat.sh) drove the default bot,
"Helper", through [`day.txt`](hindsight-defaults-test/day.txt). That is 23 turns of household
assistant chat in one thread: names, a dog, diet, a changed running schedule, a trip, a long 2,662-character
planning turn, a sister's visit. Then the thread was cleared, and 7 questions tested recall (run 1).

Clearing the thread turned out to delete what Hindsight had learned from it (section 7). So
[`replay.sh`](hindsight-defaults-test/replay.sh) replayed turns 1–23 to a second bot, "Helper Two",
in a fresh thread. After each turn it waited for Hindsight's queue to empty and snapshotted the bank.
[`recall.py`](hindsight-defaults-test/recall.py) then replayed Rakazo's recall request against that
bank for the 7 questions.

Transcripts: [`day.log`](hindsight-defaults-test/transcripts/day.log) (run 1),
[`replay.log`](hindsight-defaults-test/transcripts/replay.log) (replay with per-turn usage),
[`recall.log`](hindsight-defaults-test/transcripts/recall.log),
[`memories-bot2.tsv`](hindsight-defaults-test/transcripts/memories-bot2.tsv) (Helper Two's final
bank) and [`hindsight-llm-requests.tsv`](hindsight-defaults-test/transcripts/hindsight-llm-requests.tsv)
(Hindsight's own trace).

### 2. Tokens and cost per call

From [`totals.jsonl`](hindsight-defaults-test/transcripts/totals.jsonl), built by
[`analyze.sh`](hindsight-defaults-test/analyze.sh). Hindsight calls are told apart by the first
words of their system prompt.

| Part | Calls | Input tokens (cached) | Output (reasoning) | Largest input | Cost |
| --- | --- | --- | --- | --- | --- |
| Hindsight consolidation | 91 | 723,626 (39,203, 5.4%) | 91,798 (30,717) | 16,705 | $0.1257 |
| Hindsight fact extraction (retain) | 81 | 315,578 (50,200, 15.9%) | 28,259 (1,686) | 4,319 | $0.0473 |
| Hindsight startup check | 1 | 16 | 14 (11) | 16 | $0.0000066 |
| Rakazo chat | 116 | 1,153,844 (918,784, 79.6%) | 38,236 (24,402) | 17,775 | $0.0753 |
| **Total** | 289 | | | | **$0.2483** |

By phase:

| Phase | Turns | Consolidation | Extraction | Rakazo chat | Memory per turn |
| --- | --- | --- | --- | --- | --- |
| Run 1, day (Helper) | 24 (greeting + 23) | 52 calls, $0.0748 | 43 calls, $0.0237 | 56 calls, $0.0335 | $0.0041 |
| Run 1, after clearing | 7 | 8 calls, $0.0096 | 8 calls, $0.0045 | 13 calls, $0.0083 | $0.0020 |
| Replay (Helper Two) | 23 | 31 calls, $0.0413 | 30 calls, $0.0191 | 47 calls, $0.0335 | $0.0026 |

- **Consolidation is the expensive part**: 72% of memory spend.
  - Calls average 7,950 input and 1,010 output tokens, of which 340 are reasoning, even at
    `REASONING_EFFORT=low`. One call reasoned for 2,952 tokens.
  - Inputs grow with the bank, from about 4.6–5.3k on the first turns to 16.7k by turn 23. Each call carries
    the related observations it might update.
  - Calls per turn ranged from 1.3 (replay, one turn at a time with the queue drained) to 2.2 (run 1,
    turns a few seconds apart).
- **Extraction** averages 3,900 input and 350 output tokens per call, at 1.3–1.8 calls per turn.
  Each turn re-extracts the last one or two chunks (section 3).
- **Caching barely helps Hindsight.** Its 172 calls went to 17 providers:
  - Together 24, OpenInference 21, Wafer 19, InferenceNet 13, Particle AI 13, Venice 12, DeepInfra 11,
    BaseTen 10, Makora 8, Parasail 8, River 8, CoreWeave 7, Mistral 6, Relace 6, Fireworks 3, Modal 3
    and Cohere 1.
  - A prompt prefix cached at one provider is cold at the next.
  - Rakazo's chat went to three providers (InferenceNet 69, Modal 46, Relace 1) and was 80% cached,
    against 6.5% in #37 on v0.4.0.
- **Per turn and per month**, at #37's 30 turns a day:
  - Memory: $0.0026–0.0041 per turn, or $2.4–3.7 a month. #44 estimated $0.0010–0.0018 per turn,
    assuming one extraction and one consolidation call of 5–8k tokens with no reasoning.
  - Chat: about $0.0014 per turn, or $1.3 a month.
  - Instance: **about $3.7–5 a month**. A Space on the Shared scope writes each turn to two banks,
    which roughly doubles the memory part to $5–7.5.
- **Request shape.**
  - Hindsight's retain sent `response_format: json_object`, `max_tokens: 64000`, `temperature: 0.1`,
    `reasoning_effort: low` and
    `provider: {require_parameters: true, zdr: true, data_collection: "deny"}`. The `EXTRA_BODY`
    keys were merged with Hindsight's own.
  - Consolidation sent no `max_tokens`.
  - Rakazo v0.16.0 still sends GLM `reasoning: {effort: "high"}`, `max_completion_tokens: 32768`,
    `store: false`, streaming, and the raw Rakazo user ID as `user`, with no provider preferences.
    These are the same as #37 found on v0.4.0.
- **Errors: none.** All 289 calls finished with `stop` or `tool_calls`.

### 3. Does append-mode re-extraction re-consolidate?

**Yes.** Rakazo writes each conversation to Hindsight as one document,
`rakazo-<thread>-<generation>-<bot>`, in append mode. Snapshots after each replay turn
([`reextract-bot2.tsv`](hindsight-defaults-test/transcripts/reextract-bot2.tsv), from
[`reextract.py`](hindsight-defaults-test/reextract.py)) show what happens:

- **Each append drops the facts from the document's last one or two chunks and extracts them again.**
  - The new copies get new IDs and are reworded, so only 1 of 57 matched the deleted text.
  - Examples: turn 7 deleted 4 facts and added 4; turn 18 deleted 5 and added 7; turn 23 deleted 3
    and added 13.
- **Every new copy goes through consolidation again.** The "consolidated this turn" count equals the
  facts added every turn.
  - Over the day, 96 facts were consolidated and 57 of them replaced copies that had already been
    consolidated.
  - 59% of consolidation inputs were repeats.
  - This is why consolidation runs more than once a turn and why memory costs more than estimated.
- **Observations aren't duplicated.** The bank ended with 36 observations from 39 conversation
  facts. Consolidation recognised the repeats and updated existing observations rather than adding
  new ones. The cost is in tokens, not in a growing bank.
- The document grew to 9 chunks by turn 23.
- Snapshot attribution sometimes lags one turn, because Rakazo posts the retain after the run
  finishes.

### 4. Setup path: `connectProvider` and detection

[`setup-connect.sh`](hindsight-defaults-test/setup-connect.sh) runs the steps setup would. Output is
in [`setup-connect.log`](hindsight-defaults-test/transcripts/setup-connect.log) (secrets redacted):

1. Read-only detection SQL on Rakazo's database: `{"owner": false, "defaultSpace": false, "hindsight": false, "aiConsents": []}`.
2. `node dist/cli/bootstrap-owner.js` → `Account verified`.
3. `POST /api/auth/sign-in/email` → 200.
4. `POST /rpc/memory/connectProvider` with
   `{"provider":"hindsight","settings":{"mode":"local","baseUrl":"http://hindsight:8888"},"credentials":{"apiKey":<tenant key>}}`
   → 200, `{"provider":"hindsight","settings":{…},"defaultMemoryScope":"isolated","connectionLabel":"Self-hosted (http://hindsight:8888)"}`.
5. `POST /api/auth/sign-out` → 200.
6. Detection again: `{"owner": true, "defaultSpace": true, "hindsight": true, "aiConsents": []}`.

A second run ([`setup-rerun.log`](hindsight-defaults-test/transcripts/setup-rerun.log)) detected
everything present and did nothing. The stored row is
`hindsight | {"mode":"local","baseUrl":"http://hindsight:8888"} | isolated`. No `trust` value is
stored when setup doesn't send one. The first chat turn created the bot's `rakazo-<sha256>` bank in
Hindsight.

### 5. Owner `aiConsent`

- `aiConsent/status` returned two recipients, both `allowed: false`:
  - OpenRouter, as the model provider.
  - "Hindsight (http://hindsight:8888)", as the memory provider.
- Every chat turn, retain and recall in this run worked with **no consent rows**.
- In v0.16.0's source, consent is checked only by voice calls (`apps/web/src/lib/calls/voice-consent.ts`)
  and page review comments (`apps/api/src/routes/pages.ts`). Setup doesn't need to grant it for chat or memory. It would for a voice-call feature.

### 6. Endpoint output caps

- **Retain asks for 64,000 output tokens on every call.** All 81 extraction calls finished with
  `stop`, with no errors.
- They went to BaseTen, CoreWeave, DeepInfra, Fireworks, InferenceNet, Makora, Mistral, Modal,
  OpenInference, Parasail, Particle AI, Relace, River, Together, Venice and Wafer. OpenRouter lists
  each of them with `max_completion_tokens` of 131,072 or more for GLM 5.3 Flash.
- Of the 42 GLM 5.3 Flash endpoints OpenRouter listed, only Darkbloom is under 64,000 (29,491). It
  isn't in OpenRouter's ZDR endpoint list, so ZDR-only accounts never reach it.
- Not tested: whether `require_parameters` would steer around Darkbloom on an account without ZDR.

### 7. Quality

**Recall, measured cleanly** ([`recall.log`](hindsight-defaults-test/transcripts/recall.log)).
`recall.py` sends Rakazo's request against Helper Two's settled bank: observations only, budget
`mid`, `max_tokens: 4096`, query capped at 800 characters. With that request:

- All 7 questions found every expected fact, with a relevant result at rank 1, in 0.98–1.05 s.
  - The questions covered the dog's name and vaccinations, diets, the current running days, the
    birthday and gift style, the visiting sister's allergy, the October trip, and work and Rosa.
- The **changed running schedule** came back correctly as Tue/Thu/Sat.
- With world and experience facts added, recall took 1.6–2.3 s. The results included re-extracted
  duplicates and the stale "Tue/Thu/Sun" schedule at ranks 4–5. Rakazo's observations-only default
  is the better choice.

**Extraction.** Helper Two's bank ended with 36 observations, 34 world facts and 8 experience facts
([`memories-bot2.tsv`](hindsight-defaults-test/transcripts/memories-bot2.tsv)). Most are accurate and
well dated. The faults:

- **The bot's own words become user facts.** "The household's favourite dish so far is chickpea &
  spinach curry" and "user uses a Portuguese oven and bakes pastéis de nata" came from the bot's
  suggestions, not from anything the user said.
- **A hallucination was stored.** In run 1, after the thread was cleared, turn 24 asked for the dog's
  name.
  - Recall at that moment returned only the "Inês lives in Porto" observation; the bot's explicit
    saves were still being consolidated.
  - The bot replied "the only personal detail I have on file is that you (Inês) live in Porto".
  - That reply was extracted as a world fact and merged into an observation.
  - Four turns later the bot corrected itself ("I mistakenly took Inês to be you").
- **Observations collect unrelated notes.** For example, "Alex loves jazz and bouldering" picked up
  the Japan wishlist in a parenthesis.
- **Meta facts.** "Helper Two set itself up…" became an observation. Run 1's bank labelled Alex
  "(user's acquaintance)", although Alex is the user's partner.

**Latency.** From Hindsight's `async_operations`:

| Operation | Median | Max |
| --- | --- | --- |
| Retain | 2.9 s | 19.7 s |
| Consolidation | 5 s | 114.5 s |

So a fact can take up to about two minutes after the turn to reach the observations Rakazo recalls.
The turn 24 error above happened inside that window.

### 8. Clearing a conversation deletes its memories

- `threads/clear` calls the Hindsight provider's `purgeHistory`. That deletes the conversation's
  documents from both the bot's bank and the shared bank, along with the facts extracted from them.
- Rakazo's source states the intent: "Bots may have written either bank, so both lose the cleared
  conversation".
- After clearing in run 1:
  - The dog, the October trip and Rosa were gone.
  - Facts the bot had saved explicitly with its memory tool survived: 9 separate documents, plus
    Rakazo's own bot memory. So diet, running days, the birthday, Inês's visit and work still came
    back.
- This contradicts the natural reading of "learns from every conversation". Clearing a chat is
  also how a user forgets things on purpose.

### 9. Startup and networks

- Hindsight logged `reasoning_effort=low` and "Connection verified" (one $0.0000066 call with
  `max_tokens: 512`). It loaded the local embedder (dim 384) and reranker, and was healthy after
  about 60 s. Its log had no WARNING or ERROR lines.
- **Hindsight needs internet egress for its LLM.** Here it reached OpenRouter through the proxy on a
  non-internal network. #44's layout puts Hindsight on the internal `stack` network only, which would
  make every retain fail. It needs an egress network, the way Lemonade has one. `hindsight-db` stays
  internal.
- **Missing v0.16.0 images.** Rakazo tagged v0.16.0 on GitHub but published no
  `app:v0.16.0` or `computer:v0.16.0` to GHCR (404 on 2026-11-25). Setup that picks the newest GitHub
  release would fail to pull. Either resolve the newest release that has an image, or pin.

## Open questions / follow-ups

- **grilling:** Clearing a conversation also deletes what Hindsight learned from it, in both banks.
  Only facts the bot saved explicitly survive. Is that the behaviour v1 wants? Either way, the
  README's memory FAQ should say it.
- **task:** Setup must handle a Rakazo release with no published images (v0.16.0 has none). Resolve
  the newest tag whose `app` and `computer` manifests exist, or pin, and add a check that warns when
  a pinned tag has no image.
- **task:** Correct #44's network layout: Hindsight joins an egress network for its model provider.
  `hindsight-db` stays internal, and bot computers stay off both.
- **research:** Memory spend is about 2× the estimate, mainly from re-consolidating re-extracted
  facts and low cache hits. Look for Hindsight 1.3.0 settings that reduce this without hurting
  recall:
  - retain extraction mode or chunk size for appended documents
  - consolidation batching or delay
  - pinning one or two OpenRouter providers for Hindsight's calls to get cache hits
  - a cheaper model only for consolidation
- **research:** The bot's own guesses and errors are extracted as user facts. Can Rakazo retain only
  user turns, or does Hindsight have an extraction instruction that limits facts to what the user
  said? How much would that cost in recall?
- **grilling:** New memories take up to ~2 minutes to reach recall. Is that acceptable for v1, or
  should the README say "memory catches up after a minute or two"?
- **Unverified:** whether OpenRouter would route a 64,000-token retain to Darkbloom on an account
  without ZDR; cost on the Shared scope (estimated by doubling, not run); Rakazo's `aiConsent`
  behaviour for voice calls.

## Sources

Run artifacts in [`hindsight-defaults-test/`](hindsight-defaults-test/): the Compose override, env
generator (no provider key), hooks, drivers, analysis scripts and redacted transcripts listed above.

Rakazo v0.16.0 (https://github.com/elie222/rakazo/tree/v0.16.0):
- `infra/compose/docker-compose.images.yml`, `infra/compose/Dockerfile`
- `packages/adapters/src/providers/memory/hindsight/hindsight-memory-provider.ts` (recall request, `purgeHistory`)
- `apps/api/src/integrations/memory-provider-config.ts`, `packages/contracts/src/bots/bots-rpc.ts`
- `packages/core/src/account/ai-consent.ts`, `apps/api/src/account/ai-consent.ts`, `apps/api/src/routes/pages.ts`, `apps/web/src/lib/calls/voice-consent.ts`
- GHCR manifests for `ghcr.io/elie222/rakazo/app` and `computer` (anonymous registry API, 2026-11-25)

Hindsight 1.3.0 (https://github.com/vectorize-io/hindsight/tree/v1.3.0):
- `hindsight-api-slim/hindsight_api/engine/providers/openai_compatible_llm.py`
- `engine/retain/`, `engine/consolidation/consolidator.py`
- The running server's `llm_requests`, `async_operations`, `memory_units`, `chunks` and `documents`
  tables

OpenRouter (queried 2026-11-25):
- `GET https://openrouter.ai/api/v1/key` (usage before and after)
- https://openrouter.ai/api/v1/models/z-ai/glm-5.3-flash/endpoints
- https://openrouter.ai/api/v1/endpoints/zdr

Earlier rakazo-stack research: `research/hindsight-needs` (#44) and `research/glm-defaults-test` (#37).
