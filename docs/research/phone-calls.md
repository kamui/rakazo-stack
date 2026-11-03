# What do phone calls need?

Ticket: kamui/rakazo-stack#6. Pinned to Rakazo v0.4.0.

## Answer

- **A "phone call" is an in-app voice call, not telephony.** `PHONE_CALLS_ENABLED=true` turns on (1) a **Call** button and a live voice conversation in the Rakazo web, desktop and mobile apps, (2) hold-to-talk dictation in the composer, and (3) bots ringing the user inside the app. v0.4.0 contains no Twilio, SIP, WebRTC or TURN code and needs no telephony account or phone number.
- **Transport is plain HTTPS/RPC plus the browser's own audio APIs.** The browser records the microphone, posts audio to the Rakazo API (`/api/voice/transcribe`), sends the text to the bot as an ordinary chat message, and plays back MP3 fetched from `/api/voice/speak`. Thread updates arrive over the existing event subscription. No separate media server, no websocket-for-audio, no TURN.
- **Set the flag on both the API and the worker** (upstream says "enable on both API and worker"). Optional knobs: `PHONE_CALL_COOLDOWN_MINUTES` (default 2), `PHONE_CALL_RING_SECONDS` (default 45).
- **A bot "calling" the user is not a push.** It writes a `call_request` message and the open app sees it on its roster poll, then rings in the page (tone plus a browser `Notification` if the tab is hidden and permission was granted). Nothing rings if no Rakazo tab is open. Expo push (`EXPO_ACCESS_TOKEN`, only if the Expo project has enhanced security) is a mobile-notification feature documented separately; calls do not depend on it and the call path does not call it (unverified for the mobile app, whose source is not in the image).
- **Browsers need a secure context to use the microphone.** MDN: `navigator.mediaDevices` is `undefined` on insecure origins. Secure means `https://`, or `localhost` / `127.0.0.0/8` / `*.localhost`. Plain `http://` to a LAN IP, a `.local` name or a Tailscale MagicDNS name is **not** secure, so calls and hold-to-talk fail there.
- **Where it works:**
  - `http://localhost:<port>` or `http://127.0.0.1:<port>` in a browser on the same machine (this is also how Windows/WSL2 localhost forwarding is reached): works, no TLS.
  - Another device over a LAN IP or over Tailscale with `http://`: web app loads but the microphone is blocked. Use `tailscale serve` (HTTPS, auto-provisioned certificate on a `*.ts.net` name) and set the public origin variables to that `https://` origin: works from any device on the tailnet. Requires the tailnet admin to enable MagicDNS and HTTPS certificates; machine names are published in public Certificate Transparency logs.
  - Public domain: needs TLS in front (upstream's production guide uses Caddy). Works from anywhere; no extra call-specific requirement.
  - Rakazo's mobile app also wants HTTPS for non-LAN hostnames (it accepts plain `http://` only for a private IP, `localhost` or `.local`; a Tailscale hostname needs HTTPS with a cert the phone trusts).
- **No public address is required for calls.** The speech server is called by the Rakazo API (server side), not by the browser, so Lemonade does not have to be reachable from clients and should not be exposed. Only the Rakazo web origin must be reachable by the client.
- **Voice settings used:** the user's default voice credential (Settings -> Voice; per-space and per-bot overrides exist). A call needs a credential with a chosen voice (`ready`); with no voice connected the Call button opens Voice settings instead, and a bot cannot ring a user who has no voice credential. Speech uses the provider's text-to-speech; transcription uses the provider's speech-to-text when its catalog entry has `transcribe: true`. The **Self-hosted** (OpenAI-compatible) provider has both, so a Lemonade endpoint (`/audio/speech` as MP3, `/audio/transcriptions`) satisfies calls end to end.
- **Self-hosted provider needs an operator opt-in for private addresses.** The Lemonade base URL is fetched by the API container. Loopback, literal private IPs and `host.docker.internal` are refused for voice unless `MCP_ALLOW_PRIVATE_ENDPOINT=true` is set on the API and worker. A Docker-network service name is treated as an ordinary hostname, and an ordinary hostname that resolves to a private address is rejected at connect time; so any path to a container-local Lemonade (service name or host-gateway name) needs `MCP_ALLOW_PRIVATE_ENDPOINT=true`, and a service name also needs `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS` to list it. Without a key, plain `http://` to such a host is fine (HTTPS is only enforced when an API key or custom headers are sent).
- **Caveat for "self-sufficient": desktop Chrome/Edge dictation goes to the browser vendor's cloud, not Lemonade.** `Dictation.listen` prefers the browser's Web Speech API wherever it exists and only uses server transcription (Lemonade) on mobile browsers, in browsers without Web Speech (e.g. Firefox), or after Web Speech fails with `network` / `service-not-allowed`. MDN: in Chrome, audio is "sent to a web service for recognition processing". Text-to-speech always goes through the configured provider.

## Details

### What the flag gates

- `phoneCallSettings` (`packages/core/src/call-request.ts`): `phoneCallsEnabled = PHONE_CALLS_ENABLED === "true"` (exact string), plus `PHONE_CALL_COOLDOWN_MINUTES` and `PHONE_CALL_RING_SECONDS`. Read by the API (`apps/api/src/env.ts`) and the worker (`apps/worker/src/index.ts`).
- API: `callsEnabled` middleware returns `NOT_FOUND` for call procedures when off (`apps/api/src/routes/context.ts`); the flag is exposed to clients as `phoneCallsEnabled` in the bootstrap `me` payload (`apps/api/src/routes/shared.ts`).
- Bot-initiated calls: `requestUserCall` returns "Calls are disabled." when off (`packages/adapters/src/call-request.ts`). `callRequestRefusal` also refuses group chats, users with no `userVoiceCredential`, and calls within the cooldown.
- Web: Call button and "call back" (`pages/Shell.tsx` `callBot`, `pages/shell/chat-header.tsx`), hold-to-talk (`pages/shell/composer.tsx`: `useHoldToTalk` is `disabled` when `!phoneCallsEnabled`, and the control only renders when enabled), incoming-call UI (`components/call/IncomingCall.tsx`), and roster polling that continues while the tab is hidden when calls are enabled (`pages/shell/use-space-roster.ts`, `pages/Shell.tsx`).

### How a call works (client-driven loop)

`apps/web/src/lib/call-session.ts` runs the loop in the page:

1. `startCall` -> `pickUp` (`lib/call-pickup.ts`) plays a synthesized ringback tone (Web Audio oscillator) and calls `getUserMedia({audio:true})` so the permission prompt shows while "ringing". `microphoneReady` silently returns if `navigator.mediaDevices.getUserMedia` is missing, so on an insecure origin the call "connects" and then fails at listen with "Microphone failed" (inferred from code; not exercised live).
2. `dictation.listen({mode:"endpoint"})` (`lib/dictation.ts`) listens until 1.2 s of silence, via Web Speech or, on the server path, `MediaRecorder` + a voice-activity detector (`AudioContext`), then `POST /api/voice/transcribe`.
3. The transcript is sent as a normal message (`rpc.threads.send` / `followUp` / `answer`); the bot's reply is picked up from the thread subscription.
4. `speaker.speak` (`lib/tts.ts`) calls `rpc.voice.prepare` then `POST /api/voice/speak` per utterance and plays the MP3 with `new Audio(...)`. The mic stays open during playback to allow barge-in, with echo guard (600 ms) and echo filtering of the bot's own words.
5. Hang-up calls `rpc.threads.endCall`, which files a marker and a wrap-up run.

Autoplay: the page calls `audio.play()` after a user gesture (the Call click or the answer click), which is what browsers require; a bot-initiated call is answered by a click, so playback is gesture-gated (inferred; not exercised live).

### Mobile and desktop

Upstream docs (`docs/voice-and-calls.md`): "The mobile app uses your phone's own speech recognition and voices for calls. Pick a **Device voice** in a bot's settings. On Android, a call keeps going with the screen off." So on mobile the self-hosted Lemonade path is not used for calls (unverified in code: mobile source is not in the image). The desktop app uses the OS recognizer (`desktopBridge()?.speech`, `lib/desktop-speech.ts`). The macOS "Server (Mac)" provider only exists when the API itself runs on macOS (`docs/self-host-models.md`, Voice -> macOS voices), which matches the map's settled note.

### Voice provider catalog

`VOICE_CATALOG` (`packages/adapters/src/voice-factory.ts`): ElevenLabs, OpenAI, Fish Audio, Self-hosted (all `transcribe: true`), Cartesia (`transcribe: false`). `toVoiceStatus` (`apps/api/src/voice.ts`): `ready = Boolean(cred?.voiceId)`, `transcribe = entry.transcribe && cred`. The Self-hosted entry is hidden from the catalog only when `RAKAZO_OPENAI_COMPAT_ALLOW_PUBLIC=0` and private endpoints are not allowed.

Self-hosted adapter (`packages/adapters/src/openai-compatible-voice.ts`): `POST {base}/audio/speech` with `response_format: "mp3"` (a non-MP3 content type is rejected); `POST {base}/audio/transcriptions` as multipart with `response_format=json`; `GET {base}/models` / `/audio/voices` for verification and the voice list. A `lang_code` field is sent for Kokoro voices (the relay ticket's concern).

Endpoint policy: `voiceMayUsePrivateEndpoint` in `apps/api/src/voice.ts` returns true only if `deps.env.mcpAllowPrivateEndpoint === true` (the actor argument is unused for voice). `isPrivateOpenAiCompatibleHostname` (`openai-compatible-url.ts`) treats loopback, `host.docker.internal` and private IP literals as private, plus names in `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS`. `createOpenAiCompatibleLookup` (`pi-openai-compatible-provider.ts`) resolves DNS at connect time and throws "Public model server hostname resolved to a private address" for an ordinary hostname resolving privately. `docs/self-host-models.md` documents the same rules for model endpoints. The reference v0.4.0 compose passes `MCP_ALLOW_PRIVATE_ENDPOINT`, `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS` and `RAKAZO_OPENAI_COMPAT_ALLOW_PUBLIC` through and gives the API `host.docker.internal:host-gateway`.

### Secure context and origin

- MDN `getUserMedia`: "in insecure contexts, `navigator.mediaDevices` is `undefined`"; secure context is "a page loaded using HTTPS or the `file:///` URL scheme, or a page loaded from `localhost`".
- MDN Secure Contexts: potentially trustworthy origins are `https`/`wss`/`file` schemes, `127.0.0.0/8`, `::1/128`, `localhost`, `*.localhost`. Private-range IPs and private hostnames are not on the list.
- MDN `Notification` is also secure-context only, so the hidden-tab ring notification needs HTTPS or localhost as well. Calls still ring in-page without it.
- Rakazo's own env template says `BETTER_AUTH_URL`, `WEB_ORIGIN`, `API_URL` are the "Public origin the browser uses. For a VPS, put TLS in front of :5173 and use https://..." and `RAKAZO_HOST` must be the public hostname for a TLS proxy (`.env.example`, `docs/self-host.md`, `docs/self-host-production.md`). `docs/self-host-production.md` also says to use one HTTPS origin for web, `/api` and `/rpc`.
- The API and web ports are bound to loopback by default in the published-images compose; `docs/self-host-clients.md` documents `RAKAZO_WEB_BIND=0.0.0.0` for LAN use and says "Tailscale or an HTTPS reverse proxy is the safer option".
- I found no `Permissions-Policy` header or `isSecureContext` check in the API or web source, so nothing in Rakazo itself blocks the microphone beyond the browser rules (a fronting proxy could still add one).

### Tailscale

Tailscale docs: `tailscale serve` shares a local service within the tailnet and "HTTPS traffic uses an automatically provisioned TLS certificate" with the daemon terminating TLS; examples use `https://<node>.<tailnet>.ts.net/`. Enabling HTTPS certificates requires MagicDNS and "Enable HTTPS" on the admin console DNS page, and machine names are published in the public ledger (Certificate Transparency). Combined with the Rakazo rules above, the Self-hoster must set `WEB_ORIGIN`, `BETTER_AUTH_URL`, `API_URL` and `RAKAZO_HOST` to the `https://<node>.<tailnet>.ts.net` origin and proxy that to the web port. Rakazo's published install does not itself run Tailscale (not verified against upstream v0.4.0 beyond the docs above).

### Verification notes

The reference v0.4.0 install's source files used here (`call-session.ts`, `dictation.ts`, `core/src/call-request.ts`) and docs (`voice-and-calls.md`, `self-host-clients.md`, `self-host-models.md`) were diffed against the `v0.4.0` tag on GitHub and are identical. No live browser test was run, so insecure-origin behaviour and gesture/autoplay behaviour are from code and MDN only.

## Open questions / follow-ups

1. Desktop-Chrome dictation uses the vendor's cloud STT by default even when Lemonade is configured. Does rakazo-stack accept that, document it, or recommend a browser/client (Firefox, mobile browser, desktop app with OS recognition) for fully local transcription? Suggested: grilling.
2. Setup must set `MCP_ALLOW_PRIVATE_ENDPOINT=true` (and `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS` for a service-name URL) on both API and worker for the Lemonade/relay URL, and pre-seed the voice credential or guide the Self-hoster through Settings -> Voice. Does v1 pre-create the credential (API/DB) or only document it? Is the broader scope of `MCP_ALLOW_PRIVATE_ENDPOINT` (it also opens MCP/API-connector private targets for every user) acceptable? Suggested: grilling, then task.
3. Which HTTPS story does v1 ship: localhost-only by default plus an opt-in `tailscale serve` step that rewrites `WEB_ORIGIN`/`BETTER_AUTH_URL`/`API_URL`/`RAKAZO_HOST`, and what a public-domain path looks like (Caddy per upstream)? Suggested: grilling.
4. Live check of the unverified claims: behaviour of the call and hold-to-talk on `http://<tailscale-name>` and LAN IP; Safari/iOS behaviour; whether changing `WEB_ORIGIN` after first run invalidates sessions/passkeys. Suggested: prototype.
5. Mobile calls use the phone's own speech and voices (not Lemonade). Is mobile a supported v1 client, and does it need the HTTPS-only custom-server rule satisfied? Suggested: research (mobile source is not in the image; read the tag's `apps/mobile`).
6. Does anything require the worker to reach the voice server (the worker also loads `phoneCallSettings`)? The adapter runs in the API for speak/transcribe; confirm whether bot spoken-summary (`VOICE_SUMMARY_MIN_CHARS`) or other worker paths call voice. Suggested: research.

## Sources

Upstream, pinned to https://github.com/elie222/rakazo/tree/v0.4.0 (also read from the v0.4.0 image):
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/voice-and-calls.md
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md (Voice -> Calls; private endpoint rules)
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-clients.md (mobile clients, LAN, Expo push)
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-production.md (HTTPS origin)
- https://github.com/elie222/rakazo/blob/v0.4.0/.env.example (`PHONE_CALLS_ENABLED`, `WEB_ORIGIN`, `MCP_ALLOW_PRIVATE_ENDPOINT`)
- Source under the same tag: `packages/core/src/call-request.ts`, `packages/adapters/src/call-request.ts`, `packages/adapters/src/voice-factory.ts`, `packages/adapters/src/openai-compatible-voice.ts`, `packages/adapters/src/openai-compatible-url.ts`, `packages/adapters/src/pi-openai-compatible-provider.ts`, `apps/api/src/voice.ts`, `apps/api/src/routes/context.ts`, `apps/web/src/lib/call-session.ts`, `apps/web/src/lib/call-pickup.ts`, `apps/web/src/lib/dictation.ts`, `apps/web/src/lib/tts.ts`, `apps/web/src/pages/Shell.tsx`, `apps/web/src/pages/shell/composer.tsx`
- MDN getUserMedia: https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia
- MDN Secure Contexts: https://developer.mozilla.org/en-US/docs/Web/Security/Secure_Contexts
- MDN SpeechRecognition: https://developer.mozilla.org/en-US/docs/Web/API/SpeechRecognition
- MDN Notification: https://developer.mozilla.org/en-US/docs/Web/API/Notification
- Tailscale Serve: https://tailscale.com/kb/1242/tailscale-serve
- Tailscale enabling HTTPS: https://tailscale.com/kb/1153/enabling-https
