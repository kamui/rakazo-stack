# What Composio setup does the stack need?

Ticket: kamui/rakazo-stack#3. Pinned subject: Rakazo v0.4.0 (commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`), checked both in the published v0.4.0 image of a reference install and against the upstream tag. Composio claims are from docs.composio.dev and composio.dev/pricing as read on 2026-11-02.

## Answer

- **One setting, one secret.** Rakazo reads exactly one Composio setting: `COMPOSIO_API_KEY`. It must be set on both the API and the worker (both read it). Blank means Composio is off (no app catalog; users can still add MCP/OpenAPI sources). There is no other Composio env var (no project ID, no auth-config IDs, no webhook secret).
- **Env is the only way to supply it in v0.4.0.** The in-app "set up an integration provider" path exists in the adapter code, but the shipped API handler for saving it is a stub that throws `FORBIDDEN` (and `canConfigure` is hard-coded `false`). The setup tool must therefore write `COMPOSIO_API_KEY` into the Instance's env file.
- **Composio-managed auth only.** Rakazo creates Composio Tool Router sessions without any `authConfigs`, so every toolkit authenticates through Composio's own shared OAuth apps. The Self-hoster creates no per-toolkit auth configs. Dashboard-created auth configs would be ignored, because Composio only uses a custom auth config when its ID is passed to the session. Toolkits that have no Composio-managed auth (262 OAuth toolkits per Composio's catalog) cannot be connected; Rakazo surfaces an "auth unavailable" error for them. Gmail, Google Calendar/Docs/Drive/Sheets, Slack, Notion, GitHub, Linear, Outlook, Discord, Todoist and Spotify are among the 125 with managed OAuth.
- **Dashboard steps for the Self-hoster (one-time):** (1) create a free Composio account at dashboard.composio.dev; (2) open Settings, API keys, for the project and copy the project API key; (3) give it to the setup command, which puts it in `COMPOSIO_API_KEY`. Nothing else is needed in the dashboard. Per-app connections are made later, inside Rakazo, by each user.
- **Connections are per Rakazo user.** Rakazo passes the Rakazo user ID as the Composio `userId` and stores one connection row per (space, user, connector, provider). Each user signs in to their own accounts. Once connected, every bot in that user's space can use the app.
- **No public URL or webhook is required.** Rakazo calls Composio outbound over HTTPS; Composio never calls the Instance. The only redirect is the user's browser returning from Composio's hosted Connect Link to `<WEB_ORIGIN>/integrations/callback`, so `WEB_ORIGIN` only has to be reachable from the browser doing the connecting (a LAN, localhost or Tailscale address works). Rakazo uses no Composio triggers or webhooks. Whether Composio rejects non-HTTPS or non-public callback URLs is **unverified** (not stated in the docs read).
- **Free tier is enough for one household, with a managed-app cap.** Hobby: $0, no credit card, hard-capped (usage pauses until next month, never billed). Tool calls: 100,000/month, but only 20,000/month on Composio-managed apps, which is all Rakazo uses. Connected accounts: unlimited, first 1,000 free on managed apps; 3 team members; 2,000 requests/minute. 20,000 calls/month is about 650/day, which should cover a household but is the limit to watch. How Composio counts Rakazo's catalog and connection-status lookups against that quota is **unverified**.
- **Setup-tool implication.** Prompt for the key (optional step; skippable), validate it, write `COMPOSIO_API_KEY` for both API and worker, treat it like other secrets, and tell the Self-hoster that Composio is a third-party SaaS that holds their app tokens and sees tool-call payloads (Composio stores connected-account credentials encrypted; Rakazo never sees OAuth tokens).

## Details

### Which settings Rakazo reads

- `COMPOSIO_API_KEY` is the only Composio env var. API: `apps/api/src/env.ts` maps it to `composioApiKey` (`source.COMPOSIO_API_KEY`); `apps/api/src/app.ts` constructs `new ComposioConnector({ apiKey })` when `isComposioEnabled(env.composioApiKey)`. Worker: `apps/worker/src/index.ts` does the same from `process.env.COMPOSIO_API_KEY`. `isComposioEnabled` is true when the key is non-empty (and not in a test runner), in `packages/adapters/src/composio-connector.ts`.
- Docs: `docs/self-host-integrations.md` ("Plugins use Composio when `COMPOSIO_API_KEY` is set ... Set them on the API and worker"), `docs/self-host-secrets.md` (table: `COMPOSIO_API_KEY`, "Composio managed catalog"), `README.md` ("Managed app catalogs are optional"), and `infra/compose/.env.images.example` (`COMPOSIO_API_KEY=` under "Optional model / integration keys"). The images compose file lists `COMPOSIO_API_KEY:` in the shared environment block.
- Rakazo also supports Pipedream as an alternative provider (out of scope for v1 per the map).
- Rakazo depends on `@composio/core` 0.18.1 (`packages/adapters/package.json`).
- In-app configuration: `packages/adapters/src/integration-provider-settings.ts` can load an encrypted provider key from the database, and `packages/contracts/src/integration-settings.ts` defines `{provider: "composio", apiKey}`. But the router in `apps/api/src/routes/mcp.ts` implements `integrationSetup.get` as `canConfigure: false` and `integrationSetup.save` as `throw new ORPCError("FORBIDDEN")`. Net: env var is the only supported route in v0.4.0. Env var also acts as the fallback provider when no database row exists.

### Managed auth versus auth configs

- `ComposioConnector.sessionFor` and `sessionForExecute` call `composio.create(userId, { manageConnections: false, sandbox: { enable: false }, toolkits?, connectedAccounts?, multiAccount? })`. There is no `authConfigs` option anywhere in `composio-connector.ts` (zero occurrences in the file).
- Composio docs: "A session uses your config only when you pass its ID to `authConfigs`, keyed by toolkit. Toolkits you leave out keep using Composio managed auth" (https://docs.composio.dev/docs/authentication/custom-app-vs-managed-app). Hence dashboard-made auth configs are not picked up by Rakazo.
- When a toolkit has no managed auth, Composio returns `ToolRouter_V2_NoManagedAuth`-style errors; Rakazo maps `ToolRouterV2_NoManagedAuth` to `ConnectorAuthUnavailableError` (`isNoManagedAuthError`). Toolkits that need no auth are handled (`isNoAuthToolkitError`).
- Managed-OAuth catalog (125 toolkits) versus bring-your-own-OAuth (262): https://docs.composio.dev/toolkits/managed-auth. The list covers OAuth toolkits only; API-key toolkits are a separate case. Composio notes managed apps use Composio's branding on consent screens, shared provider quotas, and a 2-minute minimum trigger polling interval (triggers unused by Rakazo).
- Rakazo sets `manageConnections: false`, so Composio's in-chat connection prompts are off; Rakazo drives connections itself via `session.authorize(provider, { callbackUrl })`.

### Per-user connections

- `apps/api/src/routes/connections.ts` (`connections.begin`) creates or reuses a `connection` row keyed by `spaceId`, `userId` (the actor), `connectorId`, `provider`, then calls the provider's `begin` with the actor's context. The connector uses `context.userId` as the Composio user ID (`sessionFor(context.userId)`, `connectedAccounts.list({ userIds: [userId], ... })`).
- Composio: "Authentication is always per user. Each user connects their own accounts" (https://docs.composio.dev/docs/authentication). One Composio project (one API key) holds all Rakazo users' connections, keyed by Rakazo user ID. A multi-account-per-toolkit case (work and personal Gmail) is supported by Rakazo.
- A separate catalog session uses a synthetic user ID (`__rakazo_catalog__`) just to list toolkits; it needs no connection.

### Callback and reachability

- Redirect URL given to Composio: `${deps.env.webOrigin}/integrations/callback` (`apps/api/src/routes/connections.ts`). `WEB_ORIGIN` defaults to `http://127.0.0.1:5173` in `apps/api/src/env.ts` and is configured per Instance.
- Flow per Composio docs: Rakazo gets a Connect Link (`https://connect.composio.dev/link/...`) from `session.authorize()`, the user's browser completes OAuth there, then returns to the callback URL (https://docs.composio.dev/docs/authentication/manually-authenticating). Token exchange happens at Composio, not at the Instance.
- Rakazo's code references no Composio triggers or webhook subscriptions (searched `composio-*.ts` and API routes). The Instance therefore needs outbound HTTPS to Composio (`backend.composio.dev`, per the SDK docs) and nothing inbound from it. Composio's webhook features (connected-account expiry events, triggers) are optional and unused.

### Free tier

From https://composio.dev/pricing (Hobby plan; read from the page's own FAQ and plan table):

- Price $0, "No credit card required"; "The Hobby plan is hard-capped, needs no credit card, and never produces a surprise bill."
- 100,000 tool calls/month, of which "20K free, then stops" on Composio-managed apps ("shared across all managed apps"); with your own OAuth app the full 100K applies. Rakazo always uses managed apps, so 20,000/month is the effective cap.
- Connected accounts unlimited; first 1,000 free on Composio-managed apps. 3 team members. Rate limit 2,000 requests/minute on Hobby. Triggers 50,000/month (10,000 on managed). Sandbox LLM tokens 1M/month (Rakazo disables the sandbox). $2 starting credit for pay-per-use items (Instant Tools, some partner APIs).
- At the cap: "usage pauses until the next month or until you upgrade." Pro adds metered overage; not needed.
- Pricing and limits can change; the setup docs should link to the pricing page rather than hard-code numbers.

### Reference install

A v0.4.0 reference install uses this same `COMPOSIO_API_KEY` path, and its Composio integrations work. The reference install's key was not read.

## Open questions / follow-ups

- research: How does Composio meter Rakazo's calls (do catalog/toolkit listing and connection-status checks count as "tool calls" against the 20K managed cap)? Measure on a reference install over a few days, or ask Composio support; decide whether to recommend bringing own OAuth apps (not supported by Rakazo v0.4.0 anyway).
- research: Which Composio toolkits do typical household Self-hosters want that lack managed auth (so cannot be connected in v1)? Possibly document the limitation in the spec.
- grilling: Should Composio be a prompt-or-skip optional step in the setup command, with a clear data-sharing notice (third-party SaaS holds tokens), given the "no outside account except model provider and Composio" note?
- task/research: Does Composio accept a non-HTTPS or private/LAN/Tailscale `WEB_ORIGIN` as `callbackUrl`? Test on a throwaway project after the stack exists (unverified here).
- task: Setup command should validate the key (a catalog or connected-accounts call) before writing it; Rakazo's own validation path is disabled in v0.4.0.
- fog: Behavior when a Self-hoster later rotates `COMPOSIO_API_KEY`: connected accounts live in the Composio project, not in Rakazo, so a new project orphans all user connections (users must reconnect). Worth a note in backup/restore design (backup cannot capture Composio-side state).

## Sources

- Rakazo v0.4.0 (tag commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`), paths also read inside the v0.4.0 image of a reference install:
  - https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/composio-connector.ts
  - https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/integration-provider-settings.ts
  - https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/env.ts
  - https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts
  - https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/connections.ts
  - https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/routes/mcp.ts (`integrationSetup` stub)
  - https://github.com/elie222/rakazo/blob/v0.4.0/apps/worker/src/index.ts
  - https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-integrations.md
  - https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-secrets.md
  - https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/.env.images.example
- Composio:
  - https://docs.composio.dev/docs/how-composio-works
  - https://docs.composio.dev/docs/authentication
  - https://docs.composio.dev/docs/authentication/custom-app-vs-managed-app
  - https://docs.composio.dev/docs/authentication/manually-authenticating
  - https://docs.composio.dev/toolkits/managed-auth
  - https://docs.composio.dev/docs/quickstart (API key location: dashboard Settings, API keys)
  - https://composio.dev/pricing
