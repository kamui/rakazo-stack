# What does Rakazo strictly require?

Research for [#2](https://github.com/kamui/rakazo-stack/issues/2). Pinned to **Rakazo v0.4.0**
(tag commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`, released 2026-11-01). The source in the
published `ghcr.io/elie222/rakazo/app:v0.4.0` image matches that tag: `apps/api/src/env.ts`,
`apps/api/src/email.ts`, `apps/api/src/health.ts` and `apps/api/src/cli/bootstrap-owner.ts` hash
identically in a v0.4.0 reference install and at the tag. Links below use `R` =
`https://github.com/elie222/rakazo/blob/v0.4.0`.

## Answer

- **Rakazo boots with no outside account.** It needs six random secrets, which are all
  "generated internally": `POSTGRES_PASSWORD`, `POSTGRES_APP_PASSWORD`, `BETTER_AUTH_SECRET`,
  `ENCRYPTION_KEY`, `SCREEN_PROXY_SECRET` and `SANDBOX_SUPERVISOR_TOKEN`. The last two must be at
  least 32 characters and differ from each other and from `BETTER_AUTH_SECRET`. Compose refuses to
  start without four of them, and the API exits without the other two. `SANDBOX_SUPERVISOR_TOKEN`
  is required even when `SANDBOX_PROVIDER=none`. Everything else has a working default or is off.
- **Nothing in the published env forces a paid or third-party service.** Email (SMTP or Resend),
  Expo push token, Sentry, Axiom, OpenTelemetry, PostHog analytics, Infisical, AWS KMS, S3 object
  storage, remote computers (E2B, Daytona, Boat and others), Stripe, Crisp, OIDC, Pipedream and
  messaging apps are all off unless their keys are set. The model is the only thing a useful
  install needs. It does not have to be a deployment-wide key: each user can connect any
  provider, including a local OpenAI-compatible server, from the UI. `OPENROUTER_API_KEY` is
  optional.
- **`PHONE_CALLS_ENABLED=true` is the only switch calls need.** It is off by default, and the
  API and worker both read it (`PHONE_CALL_RING_SECONDS` defaults to 45 and
  `PHONE_CALL_COOLDOWN_MINUTES` to 2). Speech in web calls comes only from the server's voice
  provider. With Lemonade as that provider, the instance also needs `MCP_ALLOW_PRIVATE_ENDPOINT=true`,
  and the speech URL must count as private: `host.docker.internal`, a literal private IP, or a
  hostname listed in `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS`. A bare Compose service name such as
  `http://relay:…` is refused. The mobile app uses the phone's own speech for calls.
- **Email is optional.** Without it, forgotten-password reset, sign-in codes, verification links,
  emailed deletion codes and allowlisted signup are unavailable. Rakazo has no email invites to
  depend on. The supported no-email path is `SIGNUPS_ENABLED=false` plus the `bootstrap-owner` CLI,
  which creates a verified account and personal workspace with a password of at least 16
  characters. Keep `SIGNUPS_ENABLED=false` in env: the API re-applies the env signup policy on
  every start.
- **`EXPO_ACCESS_TOKEN` is optional.** It is only needed if the Expo project enables enhanced push
  security. Mobile push always goes through Expo's free hosted push service
  (`exp.host`), and only once a mobile app registers.
- **Encryption and secret storage work with no extra settings.** Content encryption is on by
  default and uses a master key derived from `ENCRYPTION_KEY`. Credentials are encrypted in
  Postgres (`SECRET_STORE=encrypted`). `ENCRYPTION_MASTER_KEYS`, `MASTER_KEY_PROVIDER=aws-kms` and
  Infisical are optional. `ENCRYPTION_KEY` must never change after first start.
- **The default sandbox (`SANDBOX_PROVIDER=docker`) mounts the Docker socket into the
  `supervisor` container** and pulls the `computer` image. No account is needed. Restricted computer
  egress is possible only on Linux hosts where the iptables script can run. On Docker Desktop
  (Windows and macOS) it falls back to `open`, so bot computers can reach the host and LAN.
- **Plain HTTP is allowed.** `BETTER_AUTH_URL`, `WEB_ORIGIN` and `API_URL` must be one identical
  browser origin. Rakazo trusts only that origin, plus its localhost/127.0.0.1 twin. Some features
  still need HTTPS, or a loopback address in the browser: passkeys, the browser microphone used by
  calls and dictation, and the mobile app reaching a hostname that is not a private IP.
- **Pin the version.** The template defaults to the moving `edge` tag. The stack must set
  `RAKAZO_IMAGE_TAG` and `RAKAZO_COMPUTER_IMAGE_TAG` to `v0.4.0`. Both tags exist on GHCR.

## Details

### How settings reach services

`docker-compose.images.yml` passes each container only the keys it names. Shared API and worker
keys sit in the `x-app-environment` anchor. API-only keys (auth, email, signup, OIDC, Stripe) sit
under `api`. `web` and `supervisor` get small explicit lists
([R/infra/compose/docker-compose.images.yml#L12-L15](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L12-L15)).
`infra/updater/src/compose-env.test.ts` fails CI when the code reads a key the compose file does
not pass, or when the compose file passes an unused key (same file, line 15). A key in `.env` that
no service lists never reaches a container. `install-images.sh` names any such key
([R/docs/self-host-configuration.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-configuration.md)).

### Classification of every `.env.images.example` key

Source: [R/infra/compose/.env.images.example](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/.env.images.example).
"Readers" are the services that receive the key in `docker-compose.images.yml`.

| Key | Class | Default | Readers | Notes |
| --- | --- | --- | --- | --- |
| `POSTGRES_USER` | default ok | `rakazo` | postgres, migrate | Owner login. |
| `POSTGRES_PASSWORD` | **required, generated** | — (`:?` fails compose) | postgres, migrate | `openssl rand -hex 16`. Must be URI-safe. |
| `POSTGRES_DB` | default ok | `rakazo` | postgres, migrate, api, worker | |
| `POSTGRES_APP_PASSWORD` | **required, generated** | — (`:?`) | migrate, api, worker (in `DATABASE_URL`) | `rakazo_app` login bound by row-level security. |
| `BETTER_AUTH_SECRET` | **required, generated** | — (API throws in production) | api | `openssl rand -hex 32`. Signs sessions. |
| `ENCRYPTION_KEY` | **required, generated** | — (API and worker throw) | api, worker | `openssl rand -hex 32`. Encrypts stored credentials and derives the content master key. **Never rotate.** |
| `ENCRYPTION_MASTER_KEYS` | optional | empty (derived key used) | api, worker | Only for content-key rotation. |
| `MASTER_KEY_PROVIDER` | optional | `local` | api, worker | `aws-kms` makes the install depend on AWS KMS (paid). |
| `AWS_KMS_KEY_ARN`, `AWS_KMS_ACCESS_KEY_ID`, `AWS_KMS_SECRET_ACCESS_KEY` | optional | empty | api, worker | Only with `aws-kms`. |
| `CONTENT_ENCRYPTION` | default ok | `on` | api, worker | `off` stores new content readable. Existing ciphertext still reads. |
| `OBJECT_STORAGE_*` (7 keys) | optional, off | empty | api, worker | S3-compatible bot homes. Off unless the bucket is set. |
| `SECRET_STORE` | default ok | `encrypted` | api, worker | `infisical` requires every `INFISICAL_*` key. |
| `INFISICAL_URL`, `_CLIENT_ID`, `_CLIENT_SECRET`, `_PROJECT_ID`, `_ENVIRONMENT`, `_FOLDER` | optional, off | empty | api, worker | Infisical can be self-hosted or cloud. Needs HTTPS unless loopback or `INFISICAL_ALLOW_INSECURE_HTTP=true`. |
| `SCREEN_PROXY_SECRET` | **required, generated** | — (`:?`) | api, web | At least 32 characters, distinct. Signs `/novnc/*` screen access. |
| `SANDBOX_SUPERVISOR_TOKEN` | **required, generated** | — (`:?`) | api, worker, supervisor | At least 32 characters, distinct. Required by compose for every provider. |
| `BETTER_AUTH_URL` | must match origin | `http://127.0.0.1:5173` | api | Must be an absolute http(s) URL. |
| `WEB_ORIGIN` | must match origin | `http://127.0.0.1:5173` | api | Trusted origin, email links, passkey RP ID. |
| `RAKAZO_WEB_BIND` | default ok | `127.0.0.1` | compose port binding | `0.0.0.0` publishes the web port on the LAN. |
| `API_URL` | must match origin | `http://127.0.0.1:5173` | api, worker | |
| `RAKAZO_HOST` | must match hostname | `localhost` | web | Vite preview `allowedHosts`. |
| `RAKAZO_CSP_SCRIPT_SRC`, `RAKAZO_CSP_CONNECT_SRC` | optional | unset | web | Extra CSP origins. |
| `SIGNUPS_ENABLED` | default ok, but set `false` | `true` (unset also means open) | api | Re-applied from env on every API start. |
| `MAX_UPLOAD_SIZE_MB` | default ok | `10` | api, worker, supervisor | |
| `SIGNUP_ALLOWLIST` | optional | empty | api | Non-empty **requires email delivery**, even for the first account. |
| `EXPO_ACCESS_TOKEN` | optional | empty | api, worker | See *Push*. |
| `SMTP_URL`, `EMAIL_FROM`, `EMAIL_REPLY_TO`, `WELCOME_EMAIL_ENABLED` | optional, off | empty | api | See *Email*. |
| `RAKAZO_IMAGE`, `RAKAZO_COMPUTER_IMAGE` | default ok | `ghcr.io/elie222/rakazo/{app,computer}` | compose | |
| `RAKAZO_IMAGE_TAG`, `RAKAZO_COMPUTER_IMAGE_TAG` | **set to `v0.4.0`** | `edge` (moving) | compose, web | |
| `POSTGRES_IMAGE`, `BUSYBOX_IMAGE` | optional | digest-pinned `postgres:16`, `busybox:1` | compose | Mirror overrides. |
| `SANDBOX_PROVIDER` | default ok | `docker` | api, worker | `none` boots without computers. A remote provider without its key resolves to `none`. |
| `SANDBOX_COMPUTER_EGRESS` | default ok | `auto` | supervisor | See *Sandbox*. |
| `SANDBOX_COMPUTER_DNS`, `SANDBOX_COMPUTER_MOUNTS` | optional | unset | supervisor | |
| `E2B_API_KEY`, `DAYTONA_API_KEY`, `BOAT_API_KEY` | optional, off | unset | api, worker | Paid remote computers. Not needed with `docker`. |
| `OPENROUTER_API_KEY` | optional | empty | api, worker | Deployment-wide default model. See *Models*. |
| `COMPOSIO_API_KEY` | optional, off | empty | api, worker | Plugins catalog (hosted Composio). |
| `RAKAZO_AUTO_REVIEW_PROVIDER`, `TYPESAFE_API_KEY` | optional, off | unset | api, worker | Hosted approval checker. "Core runs without either." |
| `RAKAZO_LOCAL_VISION_MODELS`, `RAKAZO_OPENAI_COMPATIBLE_VISION_MODELS` | optional | empty | api, worker | |
| `RAKAZO_OPENAI_COMPAT_ALLOW_PUBLIC` | optional | allowed (only `0` blocks) | api, worker | |
| `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS` | optional | empty | api, worker | See *Private endpoints*. |
| `MCP_ALLOW_PRIVATE_ENDPOINT` | optional, off — **needed by this stack** | unset (`false`) | api, worker | See *Private endpoints*. |
| `INTEGRATIONS_CATALOG_URL` | optional | unset | api | |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | optional | unset | api, worker | |
| `LOG_LEVEL`, `LOG_FORMAT` | default ok | `info`, JSON in production | api, worker, supervisor | |
| `AXIOM_TOKEN`, `AXIOM_DATASET`, `AXIOM_EDGE`, `AXIOM_EDGE_URL` | optional, off | empty | api, worker, supervisor | Off unless token **and** dataset are set. |
| `SENTRY_DSN`, `SENTRY_BROWSER_DSN`, `SENTRY_ENVIRONMENT`, `SENTRY_RELEASE` | optional, off | empty | api, worker, supervisor (browser DSN: api) | Off unless a DSN is set. |

Evidence for the "required" rows:

- Compose `:?` guards on `POSTGRES_PASSWORD`, `POSTGRES_APP_PASSWORD`, `SCREEN_PROXY_SECRET` and
  `SANDBOX_SUPERVISOR_TOKEN`:
  [R/infra/compose/docker-compose.images.yml#L20](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L20),
  [L180](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L180),
  [L279](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L279),
  [L331](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L331),
  [L428](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L428).
- `resolveAuthSecret` and `resolveEncryptionKey` throw outside development or test. Dedicated
  secrets must be at least 32 characters and differ from each other:
  [R/packages/core/src/secrets-guard.ts#L43-L128](https://github.com/elie222/rakazo/blob/v0.4.0/packages/core/src/secrets-guard.ts#L43-L128).
- The installer fills exactly these six secrets with `openssl rand`, and a rerun preserves them:
  [R/infra/compose/install-images.sh#L237-L253](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/install-images.sh#L237-L253),
  [R/docs/self-host-secrets.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-secrets.md).
  That doc also states the distinctness rules, and that rotating `ENCRYPTION_KEY` strands stored
  credentials and content.
- `DATABASE_URL` is the only other hard requirement in `loadEnv`, and compose builds it:
  [R/apps/api/src/env.ts#L229](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/env.ts#L229).

### Compose-only keys relevant to v1 (not in the template)

These keys are passed through by `docker-compose.images.yml` but are absent from
`.env.images.example`, so the stack's own template must add any it sets.

| Key | Default | Readers | Source |
| --- | --- | --- | --- |
| `PHONE_CALLS_ENABLED` | off (only `"true"` enables) | api, worker | [R/packages/core/src/call-request.ts#L5-L15](https://github.com/elie222/rakazo/blob/v0.4.0/packages/core/src/call-request.ts#L5-L15) |
| `PHONE_CALL_RING_SECONDS`, `PHONE_CALL_COOLDOWN_MINUTES` | 45, 2 | api, worker | same |
| `VOICE_SUMMARY_MIN_CHARS` | off | api | [R/docs/self-host-models.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md) |
| `PI_DEFAULT_PROVIDER`, `PI_DEFAULT_MODEL` | `openrouter`, `openai/gpt-6-luna` | api, worker | [R/packages/adapters/src/deployment-model.ts#L34-L59](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/deployment-model.ts#L34-L59) |
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, and about 40 other provider keys | unset | api, worker | compose L95-L160 |
| `EMAIL_PROVIDER`, `EMAIL_API_KEY` | `none` unless `SMTP_URL` is set | api | [R/apps/api/src/email.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/email.ts) |
| `AUTH_BREACHED_PASSWORD_CHECK_ENABLED` | `false` (calls HIBP when on) | api | [R/docs/self-host-accounts.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-accounts.md) |
| `OIDC_*`, `AUTH_PASSWORD_ENABLED` | off, `true` | api | [R/apps/api/src/env.ts#L176-L218](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/env.ts#L176-L218) |
| `ANALYTICS_PROVIDER` (PostHog) | off | api, worker | [R/packages/adapters/src/product-analytics.ts#L11](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/product-analytics.ts#L11) |
| `OTEL_EXPORTER_OTLP_*` | off | api, worker | [R/packages/adapters/src/pi-telemetry.ts#L13](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/pi-telemetry.ts#L13) |
| `WEB_PROVIDER` | `keyless-http` (DuckDuckGo HTML, no account) | api, worker | [R/docs/self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md) |
| `PIPEDREAM_*`, `STRIPE_*`, `CRISP_WEBSITE_ID`, messaging tokens | off | api (and worker) | env.ts L284-L342 |
| `RAKAZO_WEB_PORT`, `RAKAZO_API_PORT` | 5173, 3100 (loopback) | compose ports | compose L454, L550 |

### Calls (`PHONE_CALLS_ENABLED`)

- When the flag is off, the `answerCall`, `declineCall` and `endCall` RPCs return `NOT_FOUND`, the
  `call_user` tool is withheld, and `requestUserCall` returns "Calls are disabled."
  ([R/apps/api/src/routes/context.ts#L356-L359](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/context.ts#L356-L359),
  [R/packages/adapters/src/run-tool-selection.ts#L129-L134](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/run-tool-selection.ts#L129-L134),
  [R/packages/adapters/src/call-request.ts#L164](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/call-request.ts#L164)).
  The web UI hides Call and hold-to-talk when the flag is off (`apps/web/src/pages/Shell.tsx`).
  Compose passes the flag to both API and worker through `x-app-environment` (compose L51-L53).
- Speech in web calls comes only from the server. `apps/web/src/lib/call-session.ts` speaks through
  `apps/web/src/lib/tts.ts`, which calls `/api/voice/speak` and has no browser `speechSynthesis`
  fallback. A web call therefore needs a voice provider in **Settings → Voice**. For this stack
  that is the **Self-hosted** (OpenAI-compatible) provider pointed at the relay
  ([R/docs/voice-and-calls.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/voice-and-calls.md)).
- **Web transcription may skip Lemonade.** Mobile browsers and the Electron desktop app send
  audio to the server's transcription provider. A desktop browser that has the Web Speech API
  (Chrome, Edge, Safari) uses it even when a server transcription provider is configured
  ([R/apps/web/src/lib/dictation.ts#L247-L272](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/lib/dictation.ts#L247-L272)).
  In Chrome, Web Speech recognition is server-based, so the audio goes to the browser vendor
  ([MDN: Web Speech API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Speech_API/Using_the_Web_Speech_API#speech_recognition)).
  This happens in the browser, and no Rakazo setting changes it.
- Calls and dictation in the browser use `getUserMedia`, which only works in a secure context:
  HTTPS or loopback ([MDN: getUserMedia](https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia#security)).
  An instance reached over plain `http://<LAN or tailnet host>:5173` cannot use the microphone
  in a browser.
- The mobile app uses the phone's own speech recognition and voices for calls
  ([R/docs/voice-and-calls.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/voice-and-calls.md)).

### Private endpoints: `MCP_ALLOW_PRIVATE_ENDPOINT` and `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS`

- `MCP_ALLOW_PRIVATE_ENDPOINT=true` (api and worker) is the single instance-wide switch for private
  endpoints. It covers self-hosted voice
  ([R/apps/api/src/voice.ts#L58-L63](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/voice.ts#L58-L63)),
  local memory such as Honcho in `local` mode
  ([R/packages/adapters/src/honcho-memory-provider.ts#L57-L59](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/honcho-memory-provider.ts#L57-L59),
  [R/apps/api/src/memory-provider-config.ts#L52-L57](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/memory-provider-config.ts#L52-L57)),
  OpenAI-compatible models
  ([R/apps/api/src/model-credentials.ts#L339](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/model-credentials.ts#L339)),
  and MCP servers and connectors. No per-user bypass exists. Upstream advises leaving it unset
  on public installs. **The stack needs it on for Lemonade and Honcho.**
- For voice and models, a URL counts as private only if it uses loopback, `host.docker.internal`,
  a literal non-public IP, or a hostname listed in `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS`. Any other
  hostname is treated as public, and the request is refused when DNS resolves it to a private
  address. A bare Compose service name like `relay` falls in that case and fails with "Public
  model server hostname resolved to a private address"
  ([R/packages/adapters/src/openai-compatible-url.ts#L20-L28](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/openai-compatible-url.ts#L20-L28),
  [R/packages/adapters/src/pi-openai-compatible-provider.ts#L165-L197](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/pi-openai-compatible-provider.ts#L165-L197)).
  The stack has three options for the relay URL:
  1. `http://host.docker.internal:<port>`. API and worker already map this name to `host-gateway`
     (compose L402-L403, L498-L499).
  2. A literal private IP.
  3. List the relay's service name in `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS`.
- Plain HTTP with an API key is allowed only to private hosts
  ([R/packages/adapters/src/openai-compatible-url.ts#L60-L74](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/openai-compatible-url.ts#L60-L74)).
- The memory path accepts Compose service names when they resolve privately, per the docs
  ([R/docs/self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md)
  "Self-hosted memory"). Honcho is outside this ticket. This research did not check the exact
  memory classifier code.

### Email (SMTP or Resend)

- With no email provider configured, `configuredEmailProvider` returns `undefined`. The API still
  boots. An unknown `EMAIL_PROVIDER` or an incomplete provider fails at startup
  ([R/apps/api/src/email.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/email.ts)).
- Without email, these are unavailable: password reset (`sendResetPassword` is undefined), the
  verification email on signup, six-digit sign-in codes and emailed deletion codes
  ([R/packages/auth/src/index.ts#L197-L222](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/index.ts#L197-L222),
  [R/docs/self-host-accounts.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-accounts.md)).
  Password changes for a signed-in user still work.
- Signup with a non-empty `SIGNUP_ALLOWLIST` and no email fails with "Registration requires email
  delivery"
  ([R/packages/auth/src/index.ts#L404-L408](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/index.ts#L404-L408)).
- Rakazo has no email-based workspace or member invites. The only "invite" is a billing referral
  link (`apps/api/src/referrals.ts`).
- The no-email path is `SIGNUPS_ENABLED=false`, then
  `docker compose … exec -e OWNER_EMAIL -e OWNER_PASSWORD api node dist/cli/bootstrap-owner.js`.
  The CLI creates a verified credential account and a personal workspace, refuses passwords
  shorter than 16 characters, grants no server privileges, and is idempotent for an
  already-verified account
  ([R/apps/api/src/cli/bootstrap-owner.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/cli/bootstrap-owner.ts),
  [R/docs/self-host-accounts.md#installations-without-email](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-accounts.md#installations-without-email)).
  It writes `signupsEnabled=false` to the database. However, every API start overwrites that
  value from the env policy, and the sign-up hook reads the env too
  ([R/apps/api/src/app.ts#L334-L349](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts#L334-L349),
  [R/packages/core/src/signup-policy.ts#L31-L34](https://github.com/elie222/rakazo/blob/v0.4.0/packages/core/src/signup-policy.ts#L31-L34)).
  An unset `SIGNUPS_ENABLED` therefore means open signup.
- `RAKAZO_ADMIN_EMAILS` is unrelated to ownership. It only lets verified listed accounts review
  plugins, add stdio MCP servers and grant credits. "There is no app-admin role, first-user
  privilege" (same doc).

### Push (`EXPO_ACCESS_TOKEN`)

- API and worker always construct `ExpoPushProvider`, and there is no switch to turn it off
  ([R/apps/api/src/app.ts#L511-L518](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts#L511-L518)).
  It posts to `https://exp.host/--/api/v2/push/send`
  ([R/packages/adapters/src/expo-push.ts#L17-L18](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/expo-push.ts#L17-L18)),
  but only for devices whose mobile app registered a push token.
- The token is needed only "when the Expo project enables enhanced push security"
  ([R/docs/self-host-clients.md#mobile-push-notifications](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-clients.md#mobile-push-notifications)).
  The store app's Expo project belongs to the upstream team (`apps/mobile/app.json`, `owner`),
  so a Self-hoster cannot mint a token for it. Whether that project has enhanced security
  enabled is **unverified**. If it does, push to store-app users of a self-hosted server would
  fail.

### Encryption, secret store, Infisical

- `CONTENT_ENCRYPTION` defaults to on in production, and only `on`/`off` are valid
  ([R/packages/core/src/secrets-guard.ts#L35-L41](https://github.com/elie222/rakazo/blob/v0.4.0/packages/core/src/secrets-guard.ts#L35-L41)).
  The default master key is an HKDF of `ENCRYPTION_KEY`, and `ENCRYPTION_MASTER_KEYS` only adds
  keys for rotation
  ([R/packages/db/src/content-keys.ts#L32-L57](https://github.com/elie222/rakazo/blob/v0.4.0/packages/db/src/content-keys.ts#L32-L57)).
- `SECRET_STORE` accepts `encrypted` (the default, credentials in Postgres under `ENCRYPTION_KEY`)
  or `infisical`, which needs all six `INFISICAL_*` keys. Any other value throws
  ([R/packages/adapters/src/secret-store-factory.ts#L16-L58](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/secret-store-factory.ts#L16-L58)).
- `MASTER_KEY_PROVIDER` accepts `local` (the default) or `aws-kms`
  ([R/packages/adapters/src/key-provider-factory.ts#L20-L26](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/key-provider-factory.ts#L20-L26)).

### Sandbox and computers

- `resolveSandboxProvider` behaves as follows
  ([R/packages/adapters/src/sandbox-provider-env.ts#L4-L41](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/sandbox-provider-env.ts#L4-L41)):
  - Unset means `docker`, and an explicitly empty value means `none`.
  - A remote provider without its key becomes `none`.
  - `docker` in production without a supervisor token also becomes `none`. Compose blocks that
    case earlier with `:?`.
- The `supervisor` service runs from the app image as root. It mounts `/var/run/docker.sock` and
  the `appdata` volume and is not published on the host. A `computer` stub service pulls the
  desktop image, and `data-init` (busybox) runs `chown` on `/data` (compose L291-L363). The
  socket is equivalent to control of the Docker host.
- Docker Engine 26 or newer with the Compose plugin is required (API 1.45+, for bot-home volume
  subpaths), plus curl and OpenSSL
  ([R/docs/self-host.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host.md)).
- `SANDBOX_COMPUTER_EGRESS` takes `auto`, `restricted` or `open`. `restricted` needs the host
  iptables ruleset from `restrict-computer-egress.sh`. The installer writes `open` on macOS,
  Windows, Docker Desktop, rootless Docker or a host without iptables
  ([R/docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)).
  With `open`, a bot computer can reach host and LAN services. That includes any
  unauthenticated Lemonade or relay port bound on the host, and Docker Desktop's
  `host.docker.internal` path to host loopback services
  ([R/docs/self-host-models.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md),
  "Local models").

### Origins and HTTPS

- `BETTER_AUTH_URL` and `WEB_ORIGIN` must be absolute `http:` or `https:` URLs. Plain HTTP is
  accepted in production
  ([R/apps/api/src/env.ts#L351-L360](https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/env.ts#L351-L360)).
- Better Auth's trusted origins are `WEB_ORIGIN`, `BETTER_AUTH_URL`, `rakazo://` and the
  localhost/127.0.0.1 twins of loopback origins
  ([R/packages/auth/src/index.ts#L737-L742](https://github.com/elie222/rakazo/blob/v0.4.0/packages/auth/src/index.ts#L737-L742)).
  An instance used both at `localhost` and at a tailnet hostname therefore has to pick one origin.
- The web `vite preview` allows `RAKAZO_HOST` plus `RAKAZO_ALLOWED_HOSTS`
  ([R/apps/web/vite.config.ts#L466-L473](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/vite.config.ts#L466-L473)).
  The images compose does not pass `RAKAZO_ALLOWED_HOSTS` to `web`, so only one extra hostname
  can be configured without an override file.
- HTTPS is required for:
  - Passkeys: only when `WEB_ORIGIN` is an HTTPS domain or localhost
    ([R/docs/self-host-accounts.md#passkeys](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-accounts.md#passkeys)).
  - The browser microphone (see *Calls*).
  - The mobile app's custom server. Plain `http://` is accepted only for loopback, `.local`
    names, or private, CGNAT or ULA IPs
    ([R/apps/mobile/lib/endpoint.ts#L16-L35](https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/lib/endpoint.ts#L16-L35),
    [R/packages/contracts/src/network-host.ts#L133-L177](https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/network-host.ts#L133-L177)).
    Upstream says a hostname reached over Tailscale needs HTTPS with a certificate the phone
    trusts
    ([R/docs/self-host-clients.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-clients.md)).

### Models

- The deployment default model is set by `PI_DEFAULT_PROVIDER`, which defaults to `openrouter`
  (key `OPENROUTER_API_KEY`, model `openai/gpt-6-luna`). The other option with a deployment key
  is `anthropic` (`ANTHROPIC_API_KEY`). Bedrock and Vertex can use host credentials
  ([R/packages/adapters/src/deployment-model.ts](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/deployment-model.ts)).
  Without a key, the deployment model is reported as not configured, and the API still boots.
- Without any deployment key, each user connects a model in **Settings → Models**: a hosted
  provider, Ollama Cloud, or any OpenAI-compatible endpoint
  ([R/docs/self-host.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host.md) "What you need",
  [R/docs/self-host-models.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md)).
  The setup command can therefore take any provider key, and does not have to collect an
  OpenRouter key.

### Composio and Pipedream

Each one turns on only when its keys are set. Composio needs `COMPOSIO_API_KEY`
([R/packages/adapters/src/composio-connector.ts#L41-L43](https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/composio-connector.ts#L41-L43)).
Pipedream needs all four `PIPEDREAM_*` keys, which are not in the images template
([R/docs/self-host-integrations.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md)).
Both are hosted third-party services.

### Default outbound contacts with nothing configured

None of these needs an account:

- Image pulls from GHCR and Docker Hub.
- DuckDuckGo HTML for `web_search` (default `WEB_PROVIDER`).
- Expo's push service, once a mobile app registers.
- Live model-catalog probes of configured providers (`RAKAZO_LIVE_MODEL_CATALOG`, on by default).
- Link-icon fetches. **Unverified:** whether these go to the linked site or to an icon service.

## Open questions / follow-ups

1. **Relay addressing.** Which URL does the setup command give Rakazo for Lemonade/relay
   (`host.docker.internal`, a pinned private IP, or a service name listed in
   `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS`)? Does that work identically on Linux, Docker Desktop on
   WSL2, and macOS? Prototype.
2. **Browser HTTPS for calls.** Remote browser calls need a secure context. Is Tailscale Serve
   HTTPS (MagicDNS name plus certificate) the v1 answer for remote access? Then
   `WEB_ORIGIN`/`BETTER_AUTH_URL`/`API_URL`/`RAKAZO_HOST` become that name, and loopback-only
   use is the no-Tailscale default. Grilling, depends on the Tailscale ticket.
3. **Desktop Chrome sends call audio to the browser vendor.** Is that acceptable for v1, or
   should the docs steer users to Firefox, the Electron app or mobile? Grilling.
4. **Open egress on Docker Desktop.** On Windows and macOS, bot computers can reach host ports.
   Should Lemonade, the relay and Honcho bind only to a Docker network, or require auth, so bots
   can't reach them? Research.
5. **Expo enhanced push security.** Does the upstream store app's Expo project enable it? If so,
   self-hosted push to store apps silently fails. Research (ask upstream).
6. **Owner bootstrap.** Should the setup command default to `SIGNUPS_ENABLED=false` plus
   `bootstrap-owner` (password of at least 16 characters, no SMTP), and pin both image tags to
   `v0.4.0`? Task.

## Sources

- Rakazo v0.4.0 tag: <https://github.com/elie222/rakazo/tree/v0.4.0> (commit
  `b9c652f8898eb970b5c6d2d235d3f78b66502068`). All `R/…` links above point into it.
- `ghcr.io/elie222/rakazo/app:v0.4.0` and `ghcr.io/elie222/rakazo/computer:v0.4.0`: manifests
  checked via the GHCR registry API (HTTP 200).
- Source inside a v0.4.0 reference image (`/app/apps/api/src`, `/app/packages/*/src`), hash-checked
  against the tag.
- MDN, Using the Web Speech API:
  <https://developer.mozilla.org/en-US/docs/Web/API/Web_Speech_API/Using_the_Web_Speech_API>
- MDN, `MediaDevices.getUserMedia()` security:
  <https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia#security>
- Expo push security:
  <https://docs.expo.dev/push-notifications/sending-notifications/#additional-security>
