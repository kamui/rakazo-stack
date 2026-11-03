# Prove setup's sign-in, voice and memory calls on a throwaway v0.4.0 stack

Research for [#21](https://github.com/kamui/rakazo-stack/issues/21). Follows
[#11](https://github.com/kamui/rakazo-stack/issues/11) (code paths) and
[#9](https://github.com/kamui/rakazo-stack/issues/9) ("fill in only when missing").

Everything below was **run**, not just read. The target was a fresh stack from Rakazo's published
images `ghcr.io/elie222/rakazo/app:v0.4.0` and `computer:v0.4.0`, using the upstream image compose
file at tag v0.4.0 (commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`). Honcho was
`ghcr.io/plastic-labs/honcho:v3.5.0` (tag commit `0cd37ddbd88f51cdac7c660d7372f4b2fd40a4b3`) with
its own `pgvector/pgvector:pg15`. The stack had its own Compose project, ports and volumes, and was
deleted afterwards. Voice went to an already-running Lemonade Server and relay on the same host.
`R` = `https://github.com/elie222/rakazo/blob/v0.4.0`.

The scripts are in [`setup-rpc-proof/`](setup-rpc-proof/). The redacted transcripts are in
[`setup-rpc-proof/transcripts/`](setup-rpc-proof/transcripts/).

## Answer

- **All three fill-ins work on a fresh v0.4.0, and the first run needed no retries.**
  - `bootstrap-owner` created the Owner.
  - `POST /api/auth/sign-in/email` returned a bearer token that `/rpc` accepted.
  - `POST /rpc/voice/connect` saved the Self-hosted voice. It used the relay, `kokoro-v1` and
    `Whisper-Large-v3-Turbo`, and the server picked `af_heart` as the default voice.
  - `POST /rpc/memory/connectProvider` connected the default Space to local Honcho.
  - All replies were `200`.
- **Honcho connects without a working LLM key.** Honcho's API started and passed its health check
  with a dummy `LLM_OPENAI_API_KEY`. `connectProvider` only probes the API with the admin JWT.
  Whether memory then works (deriver, dialectic) with a dummy key was not tested; it presumably
  needs a real key.
- **Call sequence for setup:**
  1. Wait for the Rakazo API, Honcho and the relay to be healthy.
  2. **Detect** what exists. This needs no password (see below).
  3. If anything is missing, ask for the Owner password.
  4. If the Owner is missing, run
     `docker compose exec -T -e OWNER_EMAIL -e OWNER_PASSWORD api node dist/cli/bootstrap-owner.js`.
  5. `POST {api}/api/auth/sign-in/email` with `{"email","password"}`, and read `.token` from the
     reply.
  6. If voice is missing, `POST /rpc/voice/connect` with `Authorization: Bearer <token>` and the
     body
     `{"json":{"provider":"openai-compatible","baseUrl":"http://host.docker.internal:<relay>/api/v1","ttsModel":"kokoro-v1","sttModel":"Whisper-Large-v3-Turbo"}}`.
  7. If memory is missing, mint an admin JWT with
     `docker compose exec -T honcho-api python scripts/generate_jwt.py --admin --print-only`. Then
     `POST /rpc/memory/connectProvider` with
     `{"json":{"provider":"honcho","settings":{"mode":"local","baseUrl":"http://<honcho>:8000"},"credentials":{"apiKey":<jwt>}}}`.
  8. `POST /api/auth/sign-out`.
  9. A non-2xx reply becomes a checklist item. Errors come back as
     `{"json":{"code","status","message"}}` with a readable message.
- **A re-run can detect all three without the Owner password**, using read-only SQL as the
  database owner role. That role bypasses row-level security (RLS).
  - **Owner:** a verified `user` with a `credential` account for the Owner's email.
  - **Voice:** any row in `user_voice_credentials` for that user.
  - **Memory:** a row in `space_memory_configs` for the Owner's default Space, which is the first
    by `isDefault desc, createdAt asc`.

  With a session, the same facts come from `voice.credentials`, `voice.status` and
  `memory.providerConfig`. Those calls need the password, which #9 says a re-run asks for only
  when something is missing.
- **Detection is required, not optional.** The second run saw everything present, asked for no
  password and wrote nothing. After a voice change made through the UI's own call
  (`voice.setVoice`, `af_heart` → `af_bella`), a re-run left it alone. A re-run that skips
  detection and calls both RPCs again caused damage:
  - it reset the voice to `af_heart`;
  - it replaced both stored secrets (new `secretId`s).
- **Surprises:**
  - `bootstrap-owner` ignores argv. `--help` without env fails (`OWNER_EMAIL is required`). With
    `OWNER_EMAIL` set it runs and writes. For an existing verified Owner it needs **no password**,
    and still rewrites `deployment_settings` (signups off).
  - The API re-applies `SIGNUPS_ENABLED` from env to the database on every start, so setup should
    own signups through env.
  - If a voice is removed in the UI (`voice.disconnect`), the next re-run sees "missing" and adds
    it back. Detection cannot tell "removed on purpose" from "never set".
  - Sign-in also sets a cookie and a `set-auth-token` header. The body token is enough.
- **Blockers: none.**

## Details

### The throwaway stack

- **Compose files:** the upstream `infra/compose/docker-compose.images.yml` at v0.4.0, unchanged,
  plus [`compose.override.yml`](setup-rpc-proof/compose.override.yml). Run as
  `docker compose -p rs-rpc-proof`, so every container, network and volume is named
  `rs-rpc-proof_*`.
- **Env:** [`make-env.sh`](setup-rpc-proof/make-env.sh) writes fresh random secrets in the formats
  upstream `install-images.sh` uses (`openssl rand -hex`). It also sets:
  - `RAKAZO_IMAGE_TAG=v0.4.0` and `RAKAZO_COMPUTER_IMAGE_TAG=v0.4.0`;
  - `MCP_ALLOW_PRIVATE_ENDPOINT=true`, which both RPCs need for private endpoints;
  - `SANDBOX_PROVIDER=none` and `SIGNUPS_ENABLED=false`.
- **Supervisor:** the override removes the Docker socket mount. The proof needs no bot computers,
  and the Docker host also runs another Rakazo install. The supervisor's `/health` is static
  ([R/infra/sandboxes/supervisor/src/index.ts#L201](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L201)),
  so it still went healthy. It only logged failed image and egress checks. Afterwards the database
  had 0 bots, 0 computers and 0 threads.
- **Honcho:** `honcho-api` runs the image's own `docker/entrypoint.sh` with
  `AUTH_USE_AUTH=true`, a random `AUTH_JWT_SECRET`, `CACHE_ENABLED=false` and
  `LLM_OPENAI_API_KEY=sk-dummy-not-a-real-key`. It joins Rakazo's `app` network, so the API reaches
  it at `http://honcho-api:8000`. It went healthy with the dummy key. Redis and the deriver were
  not run. Honcho's `.env.template` says the server "will fail to start without a provider
  configured"
  ([honcho v3.5.0 `.env.template`](https://github.com/plastic-labs/honcho/blob/v3.5.0/.env.template)).
  A syntactically present key was enough for the API.
- **Voice server:** the relay was reachable from the `api` container at
  `http://host.docker.internal:<relay port>/api/v1`. The upstream compose gives `api` and `worker`
  `host.docker.internal:host-gateway`. `GET /models` listed `Whisper-Large-v3-Turbo`,
  `Whisper-Small` and `kokoro-v1`.

### First run (transcript: [`run1.log`](setup-rpc-proof/transcripts/run1.log))

Script: [`fill-in.sh`](setup-rpc-proof/fill-in.sh). It reads `OWNER_EMAIL`/`OWNER_PASSWORD` from
env; a real setup would prompt. Redacted exchanges:

```
detect: {"owner":false,"defaultSpace":false,"voiceCredentials":[],"memoryProvider":null}
bootstrap-owner -> "Account verified", exit 0

-> POST /api/auth/sign-in/email {"email":"owner@example.com","password":"<redacted>"}
<- 200 {"redirect":false,"token":"<redacted>","user":{"name":"Account","email":"owner@example.com","emailVerified":true,…,"id":"a230c169-…"}}

-> POST /rpc/voice/connect  (Authorization: Bearer <token>)
   {"json":{"provider":"openai-compatible","baseUrl":"http://host.docker.internal:13306/api/v1","ttsModel":"kokoro-v1","sttModel":"Whisper-Large-v3-Turbo"}}
<- 200 {"json":{"id":"cmw0562rk…","provider":"openai-compatible","hasKey":true,"isDefault":true,"voiceId":"af_heart","speechModel":"","transcribe":true,
        "endpoint":"http://host.docker.internal:13306/api/v1","ttsModel":"kokoro-v1","sttModel":"Whisper-Large-v3-Turbo"}}

-> POST /rpc/memory/connectProvider  (Authorization: Bearer <token>)
   {"json":{"provider":"honcho","settings":{"mode":"local","baseUrl":"http://honcho-api:8000"},"credentials":{"apiKey":"<redacted admin JWT>"}}}
<- 200 {"json":{"provider":"honcho","settings":{"mode":"local","baseUrl":"http://honcho-api:8000","reasoningLevel":"low"},"defaultMemoryScope":"isolated","updatedAt":"…"}}

-> POST /api/auth/sign-out
<- 200 {"success":true}

detect: {"owner":true,"defaultSpace":true,"voiceCredentials":["openai-compatible"],"memoryProvider":"honcho"}
```

- **Wire format.** Every RPC request is `POST /rpc/<router>/<procedure>` with
  `Content-Type: application/json`. The input is wrapped as `{"json": …}`, and `meta` can be left
  out. Every reply is wrapped the same way. Procedures with no input accept `{}`.
- **Token.** The sign-in body's `token` is the signed session value, a 77-character string with
  one `.`. It worked as `Authorization: Bearer` with no `Origin` header and no cookie, as #11
  predicted. The reply also carries `set-auth-token` and `set-cookie:
  better-auth.session_token=…; Max-Age=604800; HttpOnly; SameSite=Lax`, so a session lasts 7 days
  unless setup signs out.
- **Voice.** No `voiceId` was sent, so the server took the first voice the relay lists,
  `af_heart`
  ([R/apps/api/src/voice.ts#L302-L306](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/voice.ts#L302-L306)).
  The reply shows `hasKey: true` even though no key was sent.
- **Memory.** `reasoningLevel: "low"` and `defaultMemoryScope: "isolated"` were filled in by the
  server.
- **Storage.** Two `secrets` rows were written, of kinds `voice` and `memory-provider`, as #11
  described.

### Detecting existing settings on a re-run

- **Without the password: read-only SQL.** This is the `detect()` function in `fill-in.sh`. It
  runs `psql` inside the stack's `postgres` container as `POSTGRES_USER`, which is a superuser
  and so bypasses RLS. (`user_voice_credentials`, for one, has RLS enabled.) The
  queries:
  - **Owner:** `user` (`email`, `emailVerified`) joined to `account`, where
    `providerId = 'credential'`.
  - **Default Space:** `space_members` (`role = 'owner'`) joined to `spaces`, where
    `deletingAt is null`, ordered by `isDefault desc, createdAt asc`. This mirrors
    `requireMembership`'s default
    ([R/packages/db/src/scope.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/src/scope.ts)).
  - **Voice:** `user_voice_credentials.provider` for the Owner. Any provider counts, so a voice
    the Self-hoster switched to in the UI also counts as present.
  - **Memory:** `space_memory_configs.provider` for that Space.

  These tables and columns come from Prisma models `User`, `Account`, `Space`, `SpaceMember`,
  `UserVoiceCredential` and `SpaceMemoryConfig`
  ([R/packages/db/prisma/schema.prisma](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/prisma/schema.prisma)).
  The schema is internal, so pin these queries to the Rakazo tag, the same way as the RPC wire
  format.
- **With a session: the UI's own reads.** These are in
  [`ui.log`](setup-rpc-proof/transcripts/ui.log):
  - `voice.credentials` returns the list above;
  - `voice.status` returns
    `{"configured":true,"ready":true,"transcribe":true,"provider":"openai-compatible","voiceId":"af_heart"}`;
  - `memory.providerConfig` returns the config, or `null`.

  Contracts:
  [R/packages/contracts/src/rpc.ts#L1271-L1272](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/rpc.ts#L1271-L1272)
  and [#L726](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/rpc.ts#L726).
  Without a token, every `/rpc` call returns
  `401 {"json":{"defined":false,"code":"UNAUTHORIZED","status":401,"message":"Unauthorized"}}`.
- **Owner without a password through the CLI.** `bootstrap-owner` reads `OWNER_PASSWORD` only
  when the account is missing or unverified
  ([R/apps/api/src/cli/bootstrap-owner.ts#L26](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/cli/bootstrap-owner.ts#L26)).
  - With only `OWNER_EMAIL` and an existing verified Owner, it exits 0 and prints nothing.
  - With an unknown email, it fails with `OWNER_PASSWORD is required` and writes nothing.

  It still writes even when the Owner exists: `deployment_settings.updatedAt` moved, because the
  CLI always sets signups off (`#L74`). So it is not a read-only check. Use SQL.

### Re-runs and a change made in the UI

| Run | State before | Detection | Password asked | Writes |
| --- | --- | --- | --- | --- |
| 2 ([`run2.log`](setup-rpc-proof/transcripts/run2.log)) | as after run 1 | all present | no | none |
| UI change ([`ui.log`](setup-rpc-proof/transcripts/ui.log)) | voice `af_heart` | – | – | `voice.setVoice {"scope":"account","voiceId":"af_bella","provider":"openai-compatible"}` → `200 …"voiceId":"af_bella"` |
| 3 ([`run3.log`](setup-rpc-proof/transcripts/run3.log)) | voice `af_bella` | all present | no | none; `af_bella` kept |
| naive ([`naive.log`](setup-rpc-proof/transcripts/naive.log)) | voice `af_bella` | skipped | yes | voice back to `af_heart`; voice and memory secrets replaced (new `secretId`s) |
| 4 ([`run4.log`](setup-rpc-proof/transcripts/run4.log)) | voice removed in the UI (`voice.disconnect` → `200 {"ok":true}`) | voice missing | yes | voice re-added (`af_heart`) |

- The UI change used `voice.setVoice`, the call Settings → Voice makes when picking a voice
  ([R/apps/web/src/pages/VoiceSettingsOverlay.tsx#L286](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/pages/VoiceSettingsOverlay.tsx#L286)).
  The disconnect used `voice.disconnect`, the call its Disconnect button makes (`#L269`).
- The naive run shows why setup must never call `voice.connect` when a voice credential already
  exists. Reconnecting without a `voiceId` resets the voice choice to the server's first voice
  (`voice.ts#L302-L306`). `connectProvider` upserts and swaps the secret.

### Errors setup should expect

From [`errors.log`](setup-rpc-proof/transcripts/errors.log). Neither error wrote anything:

- **Voice server unreachable:**
  `400 {"json":{"defined":false,"code":"BAD_REQUEST","status":400,"message":"Couldn't reach the speech server at http://host.docker.internal:1."}}`
- **Honcho key rejected:**
  `400 {"json":{…,"code":"BAD_REQUEST","status":400,"message":"Honcho request failed: 401. A key scoped to one workspace needs that workspace entered."}}`

### Signups

`bootstrap-owner` writes `signupsEnabled=false` on every run. But the API re-applies the env
policy (`SIGNUPS_ENABLED`, `SIGNUP_ALLOWLIST`) to `deployment_settings` on every start
([R/apps/api/src/app.ts#L334-L349](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts#L334-L349)).
So env is what decides. Setup should write `SIGNUPS_ENABLED=false` to config rather than rely on
the CLI.

### Reproduce

1. `make-env.sh .env <api-port> <web-port>`.
2. `docker compose -p rs-rpc-proof --env-file .env -f docker-compose.images.yml -f compose.override.yml up -d --wait`.
3. Run `fill-in.sh` with the env it documents. Then `ui-change.sh`, `fill-in.sh` again,
   `naive-rerun.sh`, `errors-and-disconnect.sh`, and `fill-in.sh` again.
4. `docker compose -p rs-rpc-proof down -v --remove-orphans`.

Sign-in is limited to 10 tries per 15 minutes (#11). This sequence uses 6.

## Open questions / follow-ups

- **grilling:** If the Self-hoster removes a setting in the UI (voice disconnected, memory
  disconnected), should a re-run add it back?
  - Detection alone re-adds it (run 4).
  - Alternative: setup records in its settings file that it filled a setting once, and never
    fills it again unless asked.
- **task:** Spec the detection queries and the call sequence above as setup's "database
  fill-in" step. Pin both the SQL and the `/rpc` wire format to the Rakazo tag, and re-check them
  on every Rakazo upgrade.
- **research:** With a dummy LLM key, what does Honcho's deriver or dialectic do once Rakazo
  actually writes memory? Does it fail softly or block? This decides whether setup may connect
  memory before the Self-hoster has a working model key. #9 currently says memory is on only when
  a key is given.
- **task:** Setup should never use `bootstrap-owner` as an existence check, because it writes
  `deployment_settings` every time. Use detection, and own `SIGNUPS_ENABLED` in env.

**Unverified:** memory actually working end to end (deriver, recall) with any key. Real
speech/transcription through the saved voice (only the server's verify step ran). Behaviour with
`SECRET_STORE=infisical`, and with `NODE_ENV` other than `production`.

## Sources

Rakazo v0.4.0, tag commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`, run as the published
`ghcr.io/elie222/rakazo/app:v0.4.0` image:

- [R/infra/compose/docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml), [R/infra/compose/.env.images.example](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/.env.images.example), [R/infra/compose/install-images.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/install-images.sh)
- [R/apps/api/src/cli/bootstrap-owner.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/cli/bootstrap-owner.ts)
- [R/apps/api/src/app.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts) (signup policy upsert, `/rpc` mount)
- [R/apps/api/src/voice.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/voice.ts), [R/apps/api/src/routes/memory.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/memory.ts)
- [R/packages/contracts/src/rpc.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/rpc.ts) (`voice.*`, `memory.*`)
- [R/packages/db/prisma/schema.prisma](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/prisma/schema.prisma), [R/packages/db/src/scope.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/src/scope.ts)
- [R/apps/web/src/pages/VoiceSettingsOverlay.tsx](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/pages/VoiceSettingsOverlay.tsx)
- [R/infra/sandboxes/supervisor/src/index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts)

Honcho v3.5.0, tag commit `0cd37ddbd88f51cdac7c660d7372f4b2fd40a4b3`, run as
`ghcr.io/plastic-labs/honcho:v3.5.0`:

- [docker/entrypoint.sh](https://github.com/plastic-labs/honcho/blob/v3.5.0/docker/entrypoint.sh), [scripts/generate_jwt.py](https://github.com/plastic-labs/honcho/blob/v3.5.0/scripts/generate_jwt.py), [.env.template](https://github.com/plastic-labs/honcho/blob/v3.5.0/.env.template), [docker-compose.yml.example](https://github.com/plastic-labs/honcho/blob/v3.5.0/docker-compose.yml.example)

Earlier rakazo-stack findings:

- [seed-db-settings (#11)](https://github.com/kamui/rakazo-stack/blob/research/seed-db-settings/docs/research/seed-db-settings.md)
