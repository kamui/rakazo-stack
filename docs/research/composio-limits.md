# How far does Composio's free tier go?

Ticket: kamui/rakazo-stack#34. Builds on #3 (`docs/research/composio-setup.md` on `research/composio-setup`). Subject: Rakazo v0.4.0 (tag, read via the GitHub API) against the Composio Hobby plan. Composio claims are from composio.dev/pricing, docs.composio.dev and the Rakazo-bundled Composio agent guide, read 2026-11-03. Pricing and limits can change; link rather than hard-code them in setup docs.

## Answer

- **What Composio meters.** Its pricing table says "Each tool execution counts once. Meta tools such as tool search are free." Creating sessions is free. Connected accounts are not tool calls (first 1,000 on managed apps are free on Hobby). Listing toolkits or connected accounts is not a tool execution, so by the pricing page it is not metered as a tool call; it only counts toward the 2,000 requests/minute API budget. The page does not spell out the listing case, so treat it as **unverified** (inferred).
- **What Rakazo calls.** Catalog, "is it connected" and connection-ready checks use `session.toolkits(...)` and `connectedAccounts.list` (listing, not tool executions). The toolkit directory is cached for 1 hour per process. Tool discovery uses `session.tools()` (meta-tool definitions, not an execution). Only `session.execute(...)` runs are tool-call candidates: the agent's `COMPOSIO_MULTI_EXECUTE_TOOL` calls (the real app actions) and its `COMPOSIO_SEARCH_TOOLS` calls (free meta tool). Rakazo v0.4.0 uses no Composio triggers.
- **Open metering question.** Rakazo sends several app actions in one `COMPOSIO_MULTI_EXECUTE_TOOL` call. Whether Composio bills that as one call or one per inner tool is not documented. **Unverified.** Planning with one per inner tool is the safe assumption.
- **Biggest quota risk: routine "watch" polling.** A routine can watch a read-only integration tool. The checker calls that tool itself on a timer, with no model, every `everyMinutes` (minimum 5, maximum 1,440). One 5-minute watch is about 288 executions/day, roughly 8,640/month, so two such watches use most of the 20K managed cap. A 15-minute watch is about 2,900/month; an hourly one about 720/month. This is Rakazo polling, not Composio triggers, so Composio's 10K managed-trigger cap does not apply.
- **Estimate for one household.** Assumption, not measured: 3 users, 5 to 30 app actions per user per day from chat and routines, giving about 450 to 2,700 executions/month, well under 20,000. Each watch adds the figures above. Plain chat use fits comfortably; a few frequent watches are what could hit the cap. Measure on a real install to confirm.
- **At the limit.** Hobby is hard-capped: "When you reach a cap, usage pauses until the next month or until you upgrade", with no charge. Composio does not document the error a paused call returns, so the status code and message are **unverified**. Rakazo has no quota-specific handling: a failed execution becomes a generic error event with the Composio message (credentials redacted) that the agent reads and relays; a routine watch records it as a failed check. Reset timing: "next month"; whether that is the calendar month (UTC) or the signup anniversary is **unverified**. The 2,000 requests/minute limit returns HTTP 429 with `Retry-After`, and a household will not reach it.
- **Cap is the managed-app one.** Rakazo always uses Composio-managed auth, so the 20K/month managed cap applies, not the 100K total. Provider quotas are separate: managed apps share Composio's provider quota, so a busy shared app (the guide names Slack 429s) can fail even with Composio allowance left.
- **No managed sign-in (Bring-your-own OAuth app).** Of the apps you named: Spotify, Twitter/X and Shopify are bring-your-own. Also bring-your-own: Google Chat, Google Contacts, Google Forms, Microsoft To Do, Word and PowerPoint, Pipedrive, Klaviyo, Xero, Vercel, Webflow, WordPress.com, Threads, TikTok, Vimeo, Snowflake, and every `... MCP` variant of an app (Notion MCP, Linear MCP and so on; the plain Notion and Linear toolkits are managed). Telegram, Apple and Home Assistant appear in neither list (not OAuth toolkits; their handling in Rakazo is **unverified**).
- **Managed sign-in exists for** Gmail, Google Calendar, Drive, Docs, Sheets, Slides, Tasks, Meet, Photos, Slack, GitHub, GitLab, Notion, Linear, Outlook, OneDrive, SharePoint, Microsoft Teams, WhatsApp, Discord, Dropbox, Box, Zoom, HubSpot, Asana, Jira, Confluence, Trello, Todoist, Airtable, Figma, Canva, LinkedIn, Facebook, Instagram, Reddit, YouTube, Stripe, Salesforce, Zendesk, ClickUp and Monday (125 OAuth toolkits in all).
- **Own OAuth app does not help in v0.4.0.** Rakazo never passes an auth config to Composio sessions (from #3), so a bring-your-own-only app cannot be connected at all; Rakazo shows "auth unavailable" for it. Using one would need a Rakazo change, or the user adding that app as an MCP/OpenAPI source.
- **Correction to #3.** That note listed Spotify among the managed-auth apps. Composio's current list has Spotify under bring-your-own.

## Details

### Composio: what counts

From https://composio.dev/pricing (Hobby, as rendered on the page):

- Plan: $0, no credit card, hard-capped. "100,000 tool calls a month (up to 20K on Composio-managed apps)"; "20K of that 100K free, shared across all managed apps."
- Definition: "Each tool execution counts once. Meta tools such as tool search are free."
- Sessions: free; "the recommended path for running tools". Add-ons priced on top (direct execution outside a session, proxy execute, sandbox) are not used by Rakazo (sandbox disabled, session execution only).
- Connected accounts: first 1,000 on managed apps free on Hobby. Triggers: 50K/month (10K managed); unused by Rakazo.
- Pro (not needed): $0.0003 per call after 100K, overage instead of pause.
- API rate limit (https://docs.composio.dev/reference/rate-limits.md): per organisation, fixed one-minute window; Hobby 2,000 requests; all authenticated endpoints share one budget; 429 with `Retry-After`; body example `{"message": "Rate limit exceeded. Limit: 10000 requests per 1 minutes"}`. Plan usage (tool-call allowance) is a separate limit from the rate limit; the page gives no exhaustion error and mentions no monthly reset (its window is one minute). Provider quota is a third, separate limit.
- Session docs (https://docs.composio.dev/docs/how-composio-works.md, https://docs.composio.dev/docs/tools-and-toolkits): a session exposes "a small fixed set" of meta tools (`COMPOSIO_SEARCH_TOOLS`, `COMPOSIO_MANAGE_CONNECTIONS`, others); `session.execute()` routes meta-tools through the session. The docs do not say how `COMPOSIO_MULTI_EXECUTE_TOOL` is counted, and do not mention `instantCharge` beyond an opt-in result field.

### Rakazo v0.4.0: call by call

All in `packages/adapters/src/composio-connector.ts` unless noted.

| Rakazo path | Composio call | Metered as tool call? |
| --- | --- | --- |
| Integration catalog page (`connections.catalog`, `apps/api/src/routes/connections.ts`) | `session.toolkits({limit: 50, cursor})` paged, cached 1 h (`composio-catalog-cache.ts`, `COMPOSIO_DIRECTORY_TTL_MS`), plus per-user `session.toolkits({isConnected: true})` and `connectedAccounts.list` for reconnect refs, every visit | No (listing); inferred, **unverified** |
| Connection-ready check after OAuth (`connectionReady`) | `session.toolkits({search, limit: 50})` | No; inferred |
| Start connection (`begin`) | `session.authorize(provider, {callbackUrl})` | No (auth flow) |
| Session create/reuse (`sessionFor`, `sessionForExecute`) | `composio.create(...)`, `sessions.use(...)`; session IDs cached in memory | No (sessions free) |
| Tool discovery (`discoverTools`) | `session.tools()` returns meta-tool definitions | No (definitions, not an execution) |
| Agent search for an app action | `session.execute("COMPOSIO_SEARCH_TOOLS", ...)` | No ("meta tools such as tool search are free") |
| Agent runs an app action | `session.execute("COMPOSIO_MULTI_EXECUTE_TOOL", {tools: [...]})`; one call per planned item (Dropbox folder listings are split out as direct calls) | Yes; per call or per inner tool **unverified** |
| Routine "tool" watch (`packages/adapters/src/routine-watch.ts`) | `discoverTools` then `connector.execute` on each check, every `everyMinutes` (min 5, `ROUTINE_WATCH_MIN_MINUTES` in `packages/contracts/src/routine-watch.ts`) | Yes, per check |

Rakazo itself counts nothing and enforces no budget. On failure `execute` yields `{type: "error", message: sanitizeComposioError(...)}`; multi-execute batch failures include the per-tool errors (`composioResultError`). No retry, no quota detection. A catalog or discovery failure is logged ("connector discovery failed") and that provider's tools are skipped for the run.

### Managed versus bring-your-own

From https://docs.composio.dev/toolkits/managed-auth (125 managed OAuth toolkits, 262 bring-your-own OAuth toolkits; list covers OAuth toolkits only). The membership above is from that page as read on 2026-11-03. "Own OAuth app" there means a custom auth config in the Composio dashboard. Rakazo cannot use one: `ComposioConnector` never sets `authConfigs`, and `ToolRouterV2_NoManagedAuth` is mapped to `ConnectorAuthUnavailableError` (see #3). Managed apps show Composio branding on consent and share provider quotas (https://docs.composio.dev/docs/authentication/custom-app-vs-managed-app; the bundled guide notes managed-app Slack 429s).

## Open questions / follow-ups

- research or task: Measure metering on a throwaway project: run one `COMPOSIO_MULTI_EXECUTE_TOOL` with several inner tools, then compare the dashboard usage counters; also check that catalog and connection-status listing do not move the counter.
- research: What exactly does a paused (cap reached) call return, and does the cap reset on the calendar month or the signup anniversary? Test needs hitting the cap, or ask Composio support.
- grilling: Should the stack warn about, or cap, tool-watch routines (each 5-minute watch is about 43% of the managed cap)?
- spec: Document that apps without managed sign-in (Spotify, X, Shopify and others) are not connectable through Composio in v0.4.0; MCP/OpenAPI sources are the workaround.

## Sources

- Rakazo v0.4.0 (https://github.com/elie222/rakazo/tree/v0.4.0): `packages/adapters/src/composio-connector.ts`, `packages/adapters/src/composio-catalog-cache.ts`, `packages/adapters/src/composio-batch.ts`, `packages/adapters/src/routine-watch.ts`, `packages/contracts/src/routine-watch.ts`, `apps/api/src/routes/connections.ts`, `apps/worker/src/index.ts`, `.agents/skills/composio/references/errors.md`.
- Composio: https://composio.dev/pricing, https://docs.composio.dev/reference/rate-limits.md, https://docs.composio.dev/docs/how-composio-works.md, https://docs.composio.dev/docs/tools-and-toolkits, https://docs.composio.dev/toolkits/managed-auth, https://docs.composio.dev/docs/authentication/custom-app-vs-managed-app
