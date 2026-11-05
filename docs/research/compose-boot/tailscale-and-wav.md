# Tailscale container and the relay's WAV path (#24, follow-up)

The two checks left open after [compose-boot.md](../compose-boot.md). Both ran on Windows + WSL2 + Docker Desktop (Engine 29.9.2, x86-64) as throwaway Compose projects (`rkz-tstest`, `rkz-wavtest`), deleted afterwards. The tailnet name and node ids are redacted as `<tailnet>` and `<id>`.

## Tailscale container

- Layout: the [prototype](https://github.com/kamui/rakazo-stack/tree/prototype/compose-layout/prototypes/compose-layout) with profile `tailscale` only, Rakazo v0.4.0, and `tailscale/tailscale:v1.104.4` (v1.104.5 was GitHub's latest release but had no Docker Hub tag yet). `TS_USERSPACE=true`, `TS_AUTH_ONCE=true`, the prototype's `tailscale-serve.json`, `TS_HOSTNAME=rkz-test`.
- Order: start `tailscale` first, read `Self.DNSName`, write `.env` with that name ([`make-env.sh`](tailscale/make-env.sh)), then start Rakazo. Every command goes through [`dc.sh`](tailscale/dc.sh) (`-p rkz-tstest`).
- The auth key was single-use, so a second tailnet client couldn't join ([`03-client-up.log`](tailscale/transcripts/03-client-up.log)). Instead, the server machine was recreated with a test-only outbound HTTP proxy ([`compose.test.yaml`](tailscale/compose.test.yaml)), and curl requests went through that machine's own tailnet address ([`https-checks.sh`](tailscale/https-checks.sh)). The requests still passed through TLS, Serve and the Host header. A browser on another tailnet device then signed in and used a passkey.
- Transcripts: [boot](tailscale/transcripts/01-tailscale-up.log), [recreate without a key](tailscale/transcripts/04-ts-recreate.log), [checks with every URL on `ts.net`](tailscale/transcripts/05-https-checks.log), [checks with `BETTER_AUTH_URL` on localhost](tailscale/transcripts/06-split-origin.log), [teardown](tailscale/transcripts/07-teardown.log).
- Sources (Rakazo v0.4.0):
  - `packages/auth/src/index.ts` `buildTrustedOrigins`: `WEB_ORIGIN`, `BETTER_AUTH_URL`, mobile origins, and loopback twins.
  - `packages/auth/src/passkey.ts` `passkeyRelyingParty(webOrigin)`.
  - `apps/api/src/app.ts`: `/rpc` allows `webOrigin`, `apiUrl` and `authUrl`; emails use `authUrl` as their origin.
  - better-auth 1.7.12: `useSecureCookies` follows the base URL's protocol.
- Onboarding: with no Instance-wide model, Rakazo's onboarding stops at "Connect a model" and has no skip (`needsModel` in `apps/api/src/routes/shared.ts`). For the browser check, the test stack got a placeholder `OPENROUTER_API_KEY`.

## WAV path

- Lemonade `ghcr.io/lemonade-sdk/lemonade-server:v2026.45.3` (latest stable; v2026.46.x were RCs), unmodified: no ffmpeg on `PATH`, `whispercpp.args` empty, backend `auto`. Models `kokoro-v1` and `Whisper-Large-v3-Turbo`.
- The converter stands in for the planned relay: `oven/bun:1.4.4-alpine` plus `apk add ffmpeg` (Alpine 3.24.3, `ffmpeg-8.1.3-r0`).
- The test clip is kokoro speech encoded as WebM/Opus at 48 kHz mono, like a browser upload, then converted with `ffmpeg -i in.webm -ar 16000 -ac 1 -c:a pcm_s16le out.wav`.
- Scripts: [`wav-path/scripts/`](wav-path/scripts/). Transcripts: [`wav-path/transcripts/`](wav-path/transcripts/). `04a` is a first run in which speech with `lang_code:"a"` came back as 0.475 s of near-silence, the known Kokoros bug the relay's `lang_code` fix covers. The later runs use `en-us`.
