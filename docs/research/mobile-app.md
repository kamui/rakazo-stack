# Does Rakazo's mobile app work with a self-hosted Instance?

Ticket: kamui/rakazo-stack#14. Pinned to Rakazo v0.4.0 (commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`). The mobile app source is not in the server image, so everything below is read from the `v0.4.0` tag of https://github.com/elie222/rakazo. Paths are relative to that tag.

## Answer

- **Yes, the published iOS and Android apps can point at a self-hosted server, with no custom build.** Sign-in screen -> **Use a custom server** -> enter the server's origin. The app probes `POST {origin}/rpc/health` and must get `{ok:true}` before it saves the address. Upstream says outright that "Self-hosters normally do not need to publish their own mobile app" (`docs/mobile-release.md`, `docs/self-host-clients.md`).
- **Address rule: HTTPS anywhere, plain `http://` only for loopback, `*.local` and private IPs (RFC 1918, 100.64.0.0/10 CGNAT, IPv6 ULA).** No public address is required: a LAN or Tailscale address works. A Tailscale or other VPN *hostname* needs HTTPS with a publicly trusted certificate (upstream's wording), which `tailscale serve` provides (Let's Encrypt on a `*.ts.net` name). `http://<LAN-IP>:<web port>` works but exposes plain-HTTP sign-in to the LAN, and the stack's default loopback-only port binding must be widened (`RAKAZO_WEB_BIND`).
- **Push notifications are the weak spot.** The store apps register Expo push tokens under upstream's EAS project (`extra.eas.projectId` `07f7eb04-45e3-47fe-9374-99a2f991131e`, owner `inbox-zero`, `apps/mobile/app.json`). A self-hosted server sends them through `https://exp.host/--/api/v2/push/send` with no Expo account of its own. If upstream's Expo project has **enhanced push security** on, every send from a self-hosted server is rejected with `UNAUTHORIZED` and the server's own `EXPO_ACCESS_TOKEN` cannot fix it, because the token must come from the account that owns the project (upstream docs), which a Self-hoster does not control. Whether that setting is on for upstream's project is **unverified** (it is not observable from source). If it is off, push works with zero configuration.
- **Fully reliable push for iOS therefore needs the Self-hoster's own build** (own Expo project, own bundle ID, paid Apple Developer account for APNs, `EXPO_PUBLIC_API_URL`, `EXPO_ACCESS_TOKEN` on the server). Out of scope for v1. **Android has a fallback that needs no Expo or Google at all:** Settings -> Notifications -> **Live working status** runs a native foreground poller that calls the server's `/rpc` directly and posts local notifications (works with the published APK; keeps a persistent notification).
- **Speech on mobile is on-device by default, not the server's Voice credential, but the server credential is a fallback and can become the default.** Calls use `expo-speech-recognition` (iOS Speech / Android SpeechRecognizer) for listening and `expo-speech` (the phone's TTS) for the bot's voice. Lemonade is used only when the phone has no usable recognizer, or when the user turns the device voice off. One trap: with no stored choice, the app uses the **hosted (server) voice if a Voice credential is already `ready`**, so once the stack pre-seeds a Lemonade credential, a fresh phone speaks bot replies through Lemonade (`lib/device-voice.ts`) and listens on-device (`lib/voice-call-entry.ts`).
- **Calls on mobile do not need HTTPS or a secure context.** The browser secure-context rule from #6 does not apply to the native app; the microphone is gated by OS permissions. Calls need `PHONE_CALLS_ENABLED=true` on the server only for bot-initiated rings (the Call button on mobile does not read `phoneCallsEnabled`; **unverified** whether the server refuses it when off).
- **Recommendation: support the mobile app in v1 as a "works, with caveats" client, not as a headline feature.** Document the custom-server step, the address rules, the Tailscale HTTPS path, Android live status, and that iOS push is best-effort/unverified. Do not promise push, passkeys, or version lockstep. Verify push on a physical iPhone and Android phone before the spec claims it (see follow-ups).

## Details

### 1. Custom server URL

- UI: `apps/mobile/app/sign-in.tsx` shows **Use a custom server** (or the current host) at the bottom; it routes to `apps/mobile/app/server.tsx`. The screen also accepts optional extra request headers (for a fronting proxy; `lib/server-headers.ts`).
- Validation, `apps/mobile/lib/endpoint.ts`:
  - Missing scheme -> `https://` is prepended. Only `http:` and `https:`.
  - `http:` is refused ("Public servers need https://") unless `isPrivateNetworkHost(hostname)`.
  - The saved value is the origin only (`${protocol}//${host}`), so a path prefix is not supported.
  - `probeApiBase` POSTs `{origin}/rpc/health` with `origin: rakazo://` and requires `json.ok === true`.
- `isPrivateNetworkHost` (`packages/contracts/src/network-host.ts`): loopback, hostnames ending `.local`, and IPs in `10/8`, `100.64/10` ("shared address space (CGNAT, Tailscale)"), `127/8`, `172.16/12`, `192.168/16`, IPv6 loopback and `fc00::/7`; cloud-metadata addresses excluded. A name like `host.tailnet.ts.net` is **not** private, so it needs `https://`. `http://100.x.y.z:5173` passes the JS check.
- Android: `plugins/with-lan-cleartext.js` sets `android:usesCleartextTraffic="true"` so release builds can reach LAN servers over HTTP; the JS check is the gate. The native notification poller repeats the same rule (`modules/rakazo-notifications/android/.../EndpointAllowlist.kt`).
- iOS: `app.json` sets `NSAppTransportSecurity.NSAllowsLocalNetworking = true` and `NSLocalNetworkUsageDescription` ("Rakazo connects to a self-hosted Rakazo server on your local network."). Whether iOS ATS lets `http://` through to a 100.x Tailscale IP or an RFC 1918 IP literal is **unverified** (Apple's page was not retrievable; Apple documents the key only as relaxing ATS for local networking). Treat HTTPS as the safe path on iOS.
- Changing the server signs the device out of the previous session (`saveApiBase` clears the session token and selected Space on origin change, `lib/api.ts`).
- Auth: the app sends `Authorization: Bearer <session token>` plus `origin: rakazo://`; the API trusts the `rakazo://` origin (`apps/api/src/app.ts`, `packages/auth/src/index.ts` `trustedOrigins`). No server setting is needed for password sign-in. SSO works on mobile through the API callback and the `rakazo://` scheme (`docs/self-host-accounts.md`).
- Passkeys: the store build is associated only with upstream's domain, so passkeys are hidden for a self-hosted server unless `PASSKEY_APPLE_APP_IDS` / `PASSKEY_ANDROID_CERT_SHA256` are set **and** the app was built for that domain (`docs/self-host-accounts.md#passkeys`, `apps/mobile/app.config.ts` `passkeyRpId`). Password (and SSO) sign-in is the self-host path; passkeys on mobile effectively need the Self-hoster's own build.
- Network requirement for the server: the web origin only. The app talks to `{origin}/rpc/*` and `{origin}/api/*` (`/api/voice/*`, auth). `docs/self-host.md` says the Vite preview proxies `/api` and `/rpc` on the web port, so only that port needs exposing.

### 2. Push notifications

- Client: `apps/mobile/lib/push.ts` `registerPushToken` asks OS permission, then `Notifications.getExpoPushTokenAsync({ projectId })` with the `projectId` from `Constants.easConfig` / `expoConfig.extra.eas.projectId`, then calls the server RPC `notifications/registerPush`. It silently returns if no project ID or token ("Expo Go without an EAS project id and Android builds without a Firebase config cannot mint an Expo push token").
- The project ID baked into the store builds is upstream's (`apps/mobile/app.json`: `owner: inbox-zero`, `extra.eas.projectId`). Expo: `projectId` "is used to attribute Expo push token to the specific project" (https://docs.expo.dev/push-notifications/push-notifications-setup/).
- Server: `packages/adapters/src/expo-push.ts` (`ExpoPushProvider`, used by both API `apps/api/src/app.ts` and worker `apps/worker/src/index.ts`) POSTs to `https://exp.host/--/api/v2/push/send` and reads receipts from `.../getReceipts`. Notification title and body (capped 100 and 180 chars) transit Expo, Apple and Google. `Authorization: Bearer` is added only when `EXPO_ACCESS_TOKEN` is set. On `UNAUTHORIZED` the error says "Expo requires an access token for this project; set EXPO_ACCESS_TOKEN."
- `docs/self-host-clients.md#mobile-push-notifications`: "If the Expo project has enhanced security for push notifications enabled, every send needs an access token; without one Expo rejects all pushes. Create an access token in the Expo account that owns the project ... Leave it unset when enhanced security is off." `EXPO_ACCESS_TOKEN` is passed through by the published-images compose (`infra/compose/docker-compose.images.yml`) and `.env.example`.
- Expo (https://docs.expo.dev/push-notifications/sending-notifications/): without enhanced security, anyone holding a token can send to it; with it enabled, requests without a valid access token fail with `UNAUTHORIZED`. Credentials are per Expo project (APNs key, FCM V1 service account).
- Consequence: for the *store* apps the only party able to mint an access token is the owner of project `07f7eb04-...` (upstream). So either (a) upstream leaves enhanced security off and self-hosted push just works, or (b) it is on and only upstream's own servers can push (self-hosted pushes always fail). Upstream's doc is written for people who own the project, i.e. who ship their own build. I could not determine which case applies. **Unverified**; the only decisive test is registering a token from the store app against a self-hosted Instance and triggering a notification.
- Own build path (`docs/mobile-release.md`): `eas project:init` on a project the Self-hoster owns, unique iOS/Android IDs, `EXPO_PUBLIC_API_URL` as an HTTPS URL in the EAS production env (`app.config.ts` throws for non-HTTPS release builds), iOS needs a paid Apple Developer account and APNs credentials, Android needs a Firebase project, `GOOGLE_SERVICES_JSON` as an EAS file env var and an FCM V1 key; then `EXPO_ACCESS_TOKEN` (if enhanced security) on the server. The store apps are published by upstream's Expo account; the published APK has Firebase configured only if upstream added `GOOGLE_SERVICES_JSON` (the docs say builds without it "remain supported", so Android push on the stock APK is also **unverified**).
- Android fallback without Expo/FCM: `lib/live-notifications.ts` and the native module `modules/rakazo-notifications/android` poll `{endpoint}/rpc/runs/list`, `threads/get`, `me` with the session token while **Live working status** (`liveConnection`, default off, `DEFAULT_LIVE_NOTIFICATION_SETTINGS`) is on; the poller stops if the endpoint fails the same HTTPS/private-network allowlist. It is an Android-only foreground service; iOS has no equivalent.
- The app also needs push permission or silently skips registration; foreground notifications for the open thread are suppressed (`configureForegroundNotifications`).
- The same Expo traffic is outbound from the server only; no inbound public URL is needed, and the Instance stays functional when push fails (the failure is logged).

### 3. Speech for calls and dictation

- Docs (`docs/voice-and-calls.md`, "On mobile"): "The mobile app uses your phone's own speech recognition and voices for calls. Pick a **Device voice** in a bot's settings. On Android, a call keeps going with the screen off." `app.json` declares `expo-speech-recognition` and `expo-audio` plugins with call-specific permission strings.
- Listening: `lib/dictation.ts` wraps `expo-speech-recognition` (on-device or OS-vendor recognizer; same 1.2 s silence endpointing as web). On iOS and Android the OS recognizer may itself use the vendor's cloud depending on language and settings; not determined here (**unverified**).
- Fallback listening via the server: if the OS recognizer is unavailable or fails, `lib/call-session.ts` records a clip and `POST {origin}/api/voice/transcribe` (`transcribeClip`). That requires a server Voice credential that can transcribe (Self-hosted/Lemonade qualifies). Without one the call shows "Allow speech recognition in Settings, or connect ElevenLabs, OpenAI, or Fish Audio."
- Speaking: `lib/voice.ts` `speakText` uses `expo-speech` (phone TTS) when `loadDeviceVoiceEnabled()` is true, otherwise `rpc voice/prepare` + `POST {origin}/api/voice/speak` and plays the returned audio. `lib/device-voice.ts`: stored choice wins; with none, "the phone's own is used unless a hosted voice is already set up". So after the stack pre-seeds a ready Lemonade credential, new phones default to **server TTS** for replies and calls, and **on-device** for listening (`lib/voice-call-entry.ts`: plan `device` only when device voice is on; otherwise plan `provider` with `status.transcribe`, which uses Lemonade for listening too). Net effect on a fresh phone with a ready credential: calls use Lemonade for both directions unless the user turns device voice on.
- A bot cannot ring a user with no voice credential (`callRequestRefusal` in #6), so for bot-initiated calls the credential is needed regardless of the phone's device voice.
- Mobile composer dictation: I found no hold-to-talk microphone in the mobile composer code (`components/thread-composer.tsx`, `components/thread/thread-composer-dock.tsx`); `docs/mobile-app.md` says "talk instead of typing" but the code path I found for speech input is the call flow. Treat mobile dictation as OS keyboard dictation plus calls (**unverified** whether the docs line refers to something else).
- The app asks once for consent to share content with the AI services the server uses (`lib/ai-consent.ts`, `docs/mobile-app.md`), including voice routes (`aiDataUsesForProcedure`); self-hosters should expect that prompt.

### 4. Version coupling

- The store app auto-updates through the store and Expo OTA (`docs/mobile-release.md`, `expo-updates` channel `production`), independent of the server. `docs/self-host-upgrading.md` says "Product contracts stay compatible across cloud and self-hosted", but nothing pins the app to a server version. The app sends `x-rakazo-client-build: mobile/<time>` and the server can force an update with `RAKAZO_MIN_CLIENT_BUILD` (HTTP 426); there is no opposite check (app newer than server). v0.4.0 was tagged 2026-11-01 and the app source at the tag is `version` 1.0.12. If the store/OTA app moves ahead of a pinned v0.4.0 server, RPC mismatches are possible; no tested compatibility window exists (**unverified**).
- Clients refused with 426 are told to update (OTA or store). A stack that pins an old server should not set `RAKAZO_MIN_CLIENT_BUILD`.

### 5. Reference install

No reference-install evidence was needed or used; all claims come from the tag source and docs. No live phone test was run.

## Open questions / follow-ups

1. Is enhanced push security enabled on upstream's Expo project, i.e. does the stock store app receive pushes from a self-hosted server? Needs a physical iPhone and Android phone registered against a test Instance. Suggested: prototype.
2. Does iOS (ATS) allow `http://` to a Tailscale 100.x IP or LAN IP, and does the store build accept a Tailscale `serve` HTTPS origin end to end (sign-in, calls, Android live status)? Suggested: prototype.
3. Does the stack default phones to Lemonade for call TTS once a credential is pre-seeded (see `loadDeviceVoiceEnabled`)? Is that what v1 wants, or should the docs tell users to set Device voice for latency? Suggested: grilling.
4. Version policy: how does the stack pin Rakazo versus the auto-updating store app, and what does it say when the app is ahead of the server? Fold into the existing "Version pinning and upgrades" fog. Suggested: grilling.
5. Which HTTPS story is the recommended mobile path: `tailscale serve` (Let's Encrypt on `*.ts.net`) versus LAN HTTP; does v1 keep web port loopback-only and require the Tailscale HTTPS option for phones? Suggested: grilling (shares ticket 3 of #6's follow-ups).
6. Does the server refuse the mobile Call button when `PHONE_CALLS_ENABLED` is off, and does the stock Android APK ship a Firebase config? Suggested: research (small).

## Sources

Upstream, pinned to https://github.com/elie222/rakazo/tree/v0.4.0:
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/mobile-app.md
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/mobile-release.md
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-clients.md
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-accounts.md
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/voice-and-calls.md
- https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-upgrading.md
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/app.json
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/app.config.ts
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/app/server.tsx, `app/sign-in.tsx`
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/lib/endpoint.ts, `lib/api.ts`, `lib/server-headers.ts`, `lib/push.ts`, `lib/live-notifications.ts`, `lib/dictation.ts`, `lib/call-session.ts`, `lib/voice.ts`, `lib/device-voice.ts`, `lib/voice-call-entry.ts`
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/plugins/with-lan-cleartext.js
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/mobile/modules/rakazo-notifications/android/src/main/java/com/rakazo/notifications/EndpointAllowlist.kt, `RakazoNotificationService.kt`
- https://github.com/elie222/rakazo/blob/v0.4.0/packages/contracts/src/network-host.ts
- https://github.com/elie222/rakazo/blob/v0.4.0/packages/adapters/src/expo-push.ts
- https://github.com/elie222/rakazo/blob/v0.4.0/apps/api/src/app.ts, `apps/worker/src/index.ts`, `packages/auth/src/index.ts`
- https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml, `.env.example`

External:
- Expo, sending notifications and enhanced push security: https://docs.expo.dev/push-notifications/sending-notifications/
- Expo, push notification setup (`projectId`, credentials): https://docs.expo.dev/push-notifications/push-notifications-setup/
- Tailscale, enabling HTTPS (Let's Encrypt): https://tailscale.com/kb/1153/enabling-https
- Earlier findings: `docs/research/phone-calls.md` on branch `research/phone-calls` (ticket #6)
