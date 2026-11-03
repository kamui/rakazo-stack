# Can setup fill in Rakazo's database-only settings?

Research for [#11](https://github.com/kamui/rakazo-stack/issues/11). Pinned to **Rakazo v0.4.0**
(tag commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`). Every source file cited below hashes
identically in the published `ghcr.io/elie222/rakazo/app:v0.4.0` image (checked in a v0.4.0
reference install) and at the tag. `R` = `https://github.com/elie222/rakazo/blob/v0.4.0`.
Library code was read from the versions bundled in the image: `@orpc/server`/`@orpc/client`
1.15.7 and `better-auth` 1.7.12.

## Answer

- **Yes for all three, but only through Rakazo's own code paths, never raw SQL.** Rakazo v0.4.0
  has no seed file and no env variable for any of these settings, and no CLI other than
  `bootstrap-owner`. The other two are scriptable through the same HTTP API the web and mobile
  apps use: sign in as the Owner, get a bearer token, then call two RPCs.
- **Owner account: the documented CLI.** `docker compose exec -e OWNER_EMAIL -e OWNER_PASSWORD api
  node dist/cli/bootstrap-owner.js`. It takes no arguments, and `--help` is ignored, so even a
  help call runs the bootstrap and writes. It needs a password of at least 16 characters. It
  creates a verified account, a personal Space (role `owner`, `isDefault`) and sets signups off.
  Re-running it is safe: a verified account is left alone, and the Space step is idempotent.
- **Voice credential (Self-hosted → relay): RPC `voice.connect`.** It is a per-user setting:
  `POST /rpc/voice/connect` with the body
  `{"json":{"provider":"openai-compatible","baseUrl":…,"ttsModel":…,"sttModel":…}}` and the
  Owner's bearer token. It needs `MCP_ALLOW_PRIVATE_ENDPOINT=true`. The server calls the voice
  server (`GET …/v1/models`, then lists voices) before saving, so Lemonade and the relay must be
  up first. Pass Lemonade's model ids explicitly: the defaults are MLX names.
- **Per-Space Honcho connection: RPC `memory.connectProvider`.** It needs the Owner (role `owner`
  in that Space). The body is
  `{"json":{"provider":"honcho","settings":{"mode":"local","baseUrl":"http://honcho:8000"},"credentials":{"apiKey":<admin JWT>}}}`.
  It also needs `MCP_ALLOW_PRIVATE_ENDPOINT=true`, and Honcho must already be up because the
  server probes it with the admin key. It applies to the caller's default Space, or to the Space
  named in `x-rakazo-space-id`.
- **Auth for the RPCs is an ordinary session, not an admin token.** `POST /api/auth/sign-in/email`
  returns a signed session token. Send it as `Authorization: Bearer …`, the way the mobile app
  does. Neither RPC needs identity re-confirmation. A script that sends no `Origin`, no cookie and
  no `Sec-Fetch-*` headers passes Better Auth's CSRF checks. Sign-in is rate-limited to 10 tries
  per 15 minutes in production.
- **Both values are encrypted before storage, so a raw SQL insert would be wrong.** Each secret is
  sealed with AES-256-GCM. The key is derived from `ENCRYPTION_KEY` with a per-record salt, and
  the record id is bound in as AAD. The secret store can also be Infisical. Writes also maintain
  voice-preference rows and run under row-level security. Only the RPCs get all of this right.
- **Composio is env-only in v0.4.0.** `COMPOSIO_API_KEY` is read from env (`apps/api/src/env.ts`).
  A database table for it exists, but no route or CLI writes to it. So setup handles it entirely
  in config.
- **Setup can safely:** run `bootstrap-owner`, then sign in once and call both RPCs for the
  Owner's personal Space, after health checks pass. It should discard the token afterwards and
  treat a non-2xx reply as "finish in the UI". **Leave to the UI:** other users' Voice credentials
  (each user signs in for themselves), Honcho for Spaces created later, and any change after
  first run.
  The `/rpc` protocol is internal, not a documented public API, so pin the wire format to the
  Rakazo tag and re-check it on upgrade.

## Details

### What exists for scripting

- **CLIs.** The image ships five built CLIs in `apps/api/dist/cli`: `bootstrap-owner`,
  `migrate-database`, `diagnostics`, `shared-storage` and `wrap-env`. The source tree also has
  `home-storage`, `content-encryption` and `secrets-infisical`
  ([R/apps/api/src/cli](https://github.com/elie222/rakazo/tree/v0.4.0/apps/api/src/cli)). Only
  `bootstrap-owner` creates application data. The others migrate schema or storage, print a
  redacted report, or wrap env values for a KMS. None of them touches voice, memory or
  integration settings.
- **No env or seed path.** Voice and memory providers are not in the API env
  ([R/apps/api/src/env.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/env.ts)),
  and the upstream image compose has no such variable
  ([R/infra/compose/docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml)).
  The earlier [Honcho findings](https://github.com/kamui/rakazo-stack/blob/research/honcho-needs/docs/research/honcho-needs.md)
  reached the same conclusion for Honcho.
- **No documented public API.** The docs mention `/rpc` only as what the web and mobile clients
  call. They say that "requests that name no build, such as webhooks and API tools, are never
  refused" by the client-build check
  ([R/docs/self-host-clients.md#client-updates](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-clients.md#client-updates)).
  So a script that sends no `x-rakazo-client-build` header is not blocked
  ([R/apps/api/src/client-build.ts#L29-L55](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/client-build.ts#L29-L55)).
  It is still the app's internal oRPC contract, not a versioned public API.

### Owner account

- Source: [R/apps/api/src/cli/bootstrap-owner.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/cli/bootstrap-owner.ts).
  Inputs are env only: `OWNER_EMAIL` and `OWNER_PASSWORD` (at least 16 characters), plus
  `DATABASE_URL`, `BETTER_AUTH_URL` and `BETTER_AUTH_SECRET`, which the `api` container already
  has. The CLI never reads `process.argv`.
- Behaviour:
  - New email: creates a verified user and a `credential` account, with the password hashed by
    Better Auth.
  - Existing unverified account: verifies it only if the password matches.
  - Existing verified account: left unchanged.
  - In every case it then runs `bootstrapUserSpace` and sets `signupsEnabled=false`.
- `bootstrapUserSpace` creates the "Personal" organization and a Space with `isDefault: true`,
  and adds the user with role `owner`. Every step tolerates unique-key conflicts, so re-running
  it is idempotent
  ([R/packages/db/src/bootstrap-user.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/src/bootstrap-user.ts)).
- Documented invocation, which reads the password with `read -s` and unsets it afterwards:
  [R/docs/self-host-accounts.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-accounts.md)
  (lines 50–58).

### Authenticating a script

- `/api/auth/*` is served by Better Auth; only `/organization*` is blocked
  ([R/apps/api/src/app.ts#L901-L910](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts#L901-L910),
  [R/packages/auth/src/index.ts#L771-L773](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/index.ts#L771-L773)).
  Email and password sign-in is on unless the deployment is SSO-only.
- Bearer tokens: Rakazo installs Better Auth's `bearer` plugin with `requireSignature: true`
  ([R/packages/auth/src/account-security.ts#L132](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/account-security.ts#L132)).
  It also rewrites the sign-in response body so that its `token` is the signed cookie value, "so
  a native client that keeps the body token stores a bearer that works"
  ([R/packages/auth/src/index.ts#L700-L733](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/index.ts#L700-L733)).
- The `/rpc/*` middleware turns the bearer into a session. It then resolves the actor's Space
  with `requireMembership(userId, x-rakazo-space-id header)`. Without the header, the Space is
  the user's default Space (`isDefault desc`, then oldest)
  ([R/apps/api/src/app.ts#L916-L975](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts#L916-L975),
  [R/packages/db/src/scope.ts#L19-L45](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/src/scope.ts#L19-L45)).
- CSRF/origin: in `better-auth@1.7.12` (`dist/api/middlewares/origin-check.mjs` in the image),
  `validateOrigin` returns early when the request has no cookie and is not forced. The form-CSRF
  check forces it only when `Sec-Fetch-*`, `Origin` or `Referer` headers are present. So a plain
  server-side request is accepted. If a script does send `Origin`, it must be a trusted origin
  (`WEB_ORIGIN`/`BETTER_AUTH_URL`).
- Rate limit: `/sign-in/email` allows 10 requests per 15 minutes when `NODE_ENV=production`,
  stored in the database
  ([R/packages/auth/src/index.ts#L84-L141](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/index.ts#L84-L141)).
- Identity re-confirmation (`requireIdentityConfirmation`) guards only some routes in
  `routes/bots.ts` and `routes/content.ts`. It does not guard voice or memory
  ([R/apps/api/src/identity-confirmation.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/identity-confirmation.ts)).
- Wire format: `RPCHandler` is mounted with prefix `/rpc`
  ([R/apps/api/src/app.ts#L814-L816](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts#L814-L816)),
  so procedure `voice.connect` is `POST /rpc/voice/connect`. The oRPC 1.15.7 RPC serializer sends
  `{"json": <input>, "meta": [...]}`, and `meta` may be omitted for plain JSON input
  (`@orpc/client` `dist/shared/client.DXk6DV1Y.mjs`, the `#serialize`/`#deserialize` methods).
- Reachability: the upstream image compose publishes the API on host loopback at
  `127.0.0.1:${RAKAZO_API_PORT:-3100}`
  ([R/infra/compose/docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml),
  `api.ports`). A setup tool on the host can call it there without going through the web
  server.

### Voice credential (Self-hosted provider → relay)

- Contract `voice.connect` takes these fields
  ([R/packages/contracts/src/rpc.ts#L1269-L1293](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/rpc.ts#L1269-L1293)):
  - `provider`;
  - `apiKey`, which defaults to empty and is optional for self-hosted;
  - `voiceId?`, `speechModel?`;
  - `baseUrl?`, which is required when the provider is `openai-compatible`;
  - `ttsModel?`, `sttModel?`;
  - `scope?`.
- Handler `persistVoiceCredential`
  ([R/apps/api/src/routes/account.ts#L296-L308](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/account.ts#L296-L308),
  [R/apps/api/src/voice.ts#L258-L380](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/voice.ts#L258-L380)):
  - Private endpoints are allowed only for `openai-compatible` and only when
    `MCP_ALLOW_PRIVATE_ENDPOINT=true` (`voice.ts#L59-L63`).
  - The plaintext is a JSON blob `{kind:"openai_compatible", baseUrl, apiKey?, ttsModel,
    sttModel}`. `baseUrl` is normalized to end in `/v1`, and sending a key over plain HTTP is
    allowed only to private hosts
    ([R/packages/adapters/src/openai-compatible-voice.ts#L74-L109](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/openai-compatible-voice.ts#L74-L109),
    [R/packages/adapters/src/openai-compatible-url.ts#L60-L74](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/openai-compatible-url.ts#L60-L74)).
    The relay needs no key.
  - Default model ids are `mlx-community/Kokoro-82M-bf16` and
    `mlx-community/whisper-large-v3-turbo-asr-fp16`
    ([R/packages/contracts/src/openai-compatible-ui.ts#L64-L65](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/openai-compatible-ui.ts#L64-L65)).
    Lemonade names its models differently, so setup must send `ttsModel` and `sttModel`.
  - `provider.verify` calls `GET {baseUrl}/models`, or `/audio/voices` on a 404 or 405, before
    anything is stored. If no `voiceId` is given, the first voice from `listVoices` becomes the
    default (`voice.ts#L298-L308`;
    `openai-compatible-voice.ts#L303-L337`).
  - It then calls `secrets.put(plaintext)` and, in one serializable transaction, writes:
    - a `secret` row of kind `voice`;
    - an upsert of `userVoiceCredential` (per user, account-wide);
    - the user's or Space's voice preference (`voice.ts#L309-L380`).
- **Per user, not per Instance.** The row is keyed by `userId`, so setup can only configure the
  Owner. Every other user must connect voice themselves in **Settings → Voice**
  ([R/docs/voice-and-calls.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/voice-and-calls.md)).

### Per-Space Honcho connection

- Contract `memory.connectProvider`
  ([R/packages/contracts/src/rpc.ts#L727-L736](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/rpc.ts#L727-L736)):
  `{provider, settings: Record<string,string>, credentials: Record<string,string>,
  defaultMemoryScope: "isolated"|"shared" (default "isolated")}`.
- Handler `persistMemoryProviderConfig`
  ([R/apps/api/src/routes/memory.ts#L124-L126](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/memory.ts#L124-L126),
  [R/apps/api/src/memory-provider-config.ts#L30-L136](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/memory-provider-config.ts#L30-L136)):
  - The caller must have role `owner` in the actor's Space; otherwise the handler returns
    `FORBIDDEN`.
  - Local mode requires `MCP_ALLOW_PRIVATE_ENDPOINT=true`. The handler checks this before and
    after DNS classification, and returns `FORBIDDEN` otherwise.
  - `prepareHonchoConnection` validates the settings:
    - `mode` is `cloud` or `local`, and a non-empty `baseUrl` is required for local;
    - `apiKey` must be 8–4096 characters;
    - `reasoningLevel`, `workspaceId` and `userPeerId` are optional.
  - It then probes Honcho. With no workspace it calls `probeHonchoAdminKey`, which lists
    workspaces and so needs an admin JWT; with a workspace it does get-or-create on it
    ([R/packages/adapters/src/honcho-memory-provider.ts#L76-L176](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/honcho-memory-provider.ts#L76-L176)).
  - Finally it stores `secrets.put(JSON.stringify({apiKey}))` as a `secret` row of kind
    `memory-provider` and upserts `spaceMemoryConfig` for the Space. Settings are stored as JSON;
    the key exists only as the sealed secret.
- Rakazo's own instructions for this setting are UI steps: **Settings → Memory → Local**, the
  base URL, and an admin JWT from Honcho's `scripts/generate_jwt.py --admin`
  ([R/docs/self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md),
  lines 32–44).
- **Per Space.** Setup can fill the Owner's personal Space, which is the default when no
  `x-rakazo-space-id` header is sent. A Space created later needs the same RPC called with that
  header, or the UI.

### Why raw SQL is wrong

- `EncryptedSecretStore.put` seals each value as `v2:` followed by
  base64(salt ‖ iv ‖ tag ‖ ciphertext). It uses AES-256-GCM with a key derived from
  `ENCRYPTION_KEY` and a fresh salt, and binds the record id as AAD
  ([R/packages/adapters/src/secrets.ts#L66-L102](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/secrets.ts#L66-L102)).
  A row inserted outside this code cannot be decrypted. Copying a sealed value to another row id
  fails too, because the record id is part of the AAD.
- With `SECRET_STORE=infisical`, the ciphertext column holds a reference into Infisical, not the
  secret ([R/docs/self-host-secrets.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-secrets.md),
  [R/docs/infisical-secrets.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/infisical-secrets.md)).
- RPC procedures run under row-level security scoped to the actor's Space
  ([R/apps/api/src/routes/context.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/context.ts),
  `runAsTenant`). The voice path also maintains preference rows, and the memory path deletes the
  replaced secret. A hand-written insert would have to replicate all of this and track every
  upstream change.

### Composio

- `COMPOSIO_API_KEY` comes from env
  ([R/apps/api/src/env.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/env.ts),
  `composioApiKey: source.COMPOSIO_API_KEY`). It is the env fallback in
  `IntegrationProviderSettings`, which also reads an `integrationProviderConfig` table
  ([R/packages/adapters/src/integration-provider-settings.ts#L16-L108](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/integration-provider-settings.ts#L16-L108)).
- No route, worker or CLI in v0.4.0 calls `IntegrationProviderSettings.save`. Searching
  `apps/api/src`, `apps/worker/src` and `packages` in the image finds only the class itself and
  its construction. So the env key is the only path, and nothing about Composio needs a database
  write.

### Suggested setup sequence (for the spec)

1. Start everything and wait for the health checks of the Rakazo API, Lemonade, the relay and
   Honcho.
2. Run `bootstrap-owner` with `OWNER_EMAIL` and `OWNER_PASSWORD` passed through the process
   environment, never argv.
3. Call `POST http://127.0.0.1:<api port>/api/auth/sign-in/email` with `{email, password}` and
   keep the returned `token` in memory only.
4. Call `POST /rpc/voice/connect` with `provider:"openai-compatible"`, the relay's URL on the
   Compose network, and Lemonade's TTS and STT model ids.
5. Call `POST /rpc/memory/connectProvider` with `provider:"honcho"`,
   `settings:{mode:"local", baseUrl:"http://<honcho service>:8000"}` and
   `credentials:{apiKey:<admin JWT>}`.
6. Optionally call `POST /api/auth/sign-out` so the session does not outlive setup. On any
   failure, print which settings remain and point to the UI path. Do not retry sign-in in a loop,
   because of the rate limit.

## Open questions / follow-ups

- **prototype:** Run steps 3–5 above against a throwaway v0.4.0 stack. Confirm the exact
  sign-in response, the oRPC envelope and status codes, and the relay/Lemonade model ids that
  `voice.connect` accepts. None of these calls was made here: the brief forbids data-changing
  calls on the reference install.
- **grilling:** Should setup do steps 3–5 at all, or print UI instructions? Automating them
  holds the Owner password in memory for one sign-in and depends on Rakazo's internal `/rpc`
  contract, which could change between tags. One option is automatic setup with a UI fallback.
- **grilling:** On backup/restore and re-runs, is setup idempotent for these settings? Re-calling
  `voice.connect` replaces the secret, and `connectProvider` upserts. What should happen if the
  Self-hoster already changed them in the UI?
- **research:** Rakazo `x-rakazo-space-id` and multiple Spaces. Does the stack promise Honcho
  for Spaces created after setup? If so, how: documentation, or a `rakazo-stack` subcommand that
  signs in and loops over Spaces?
- **task:** The setup tool must wait for Honcho and the relay to be healthy before steps 4–5,
  because both RPCs probe their target and fail closed.

**Unverified:** the end-to-end HTTP calls, which were read from source but not executed; the
exact Lemonade model ids to pass; behaviour when `NODE_ENV` is not `production` (the rate limit
is off then).

## Sources

Rakazo v0.4.0 (`https://github.com/elie222/rakazo/tree/v0.4.0`; identical in the published image):

- `apps/api/src/cli/bootstrap-owner.ts`, `apps/api/src/cli/*` (CLI inventory), `apps/api/dist/cli/` (built CLIs in the image)
- `apps/api/src/app.ts`, `apps/api/src/router.ts`, `apps/api/src/client-build.ts`, `apps/api/src/identity-confirmation.ts`, `apps/api/src/env.ts`
- `apps/api/src/routes/account.ts`, `apps/api/src/voice.ts`
- `apps/api/src/routes/memory.ts`, `apps/api/src/memory-provider-config.ts`, `apps/api/src/routes/context.ts`
- `packages/contracts/src/rpc.ts`, `packages/contracts/src/openai-compatible-ui.ts`, `packages/contracts/src/domain.ts`
- `packages/auth/src/index.ts`, `packages/auth/src/account-security.ts`
- `packages/adapters/src/secrets.ts`, `honcho-memory-provider.ts`, `openai-compatible-voice.ts`, `openai-compatible-url.ts`, `integration-provider-settings.ts`
- `packages/db/src/scope.ts`, `packages/db/src/bootstrap-user.ts`
- `infra/compose/docker-compose.images.yml`
- `docs/self-host-accounts.md`, `docs/self-host-clients.md`, `docs/self-host-integrations.md`, `docs/self-host-secrets.md`, `docs/infisical-secrets.md`, `docs/voice-and-calls.md`

Libraries as bundled in the v0.4.0 image:

- `better-auth` 1.7.12: `dist/api/middlewares/origin-check.mjs`
- `@orpc/client` 1.15.7: `dist/shared/client.DXk6DV1Y.mjs` (RPC serializer); `@orpc/server` 1.15.7 (`RPCHandler`)

Earlier rakazo-stack findings:

- [rakazo-requirements](https://github.com/kamui/rakazo-stack/blob/research/rakazo-requirements/docs/research/rakazo-requirements.md), [honcho-needs](https://github.com/kamui/rakazo-stack/blob/research/honcho-needs/docs/research/honcho-needs.md)
