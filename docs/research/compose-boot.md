# Boot the drafted compose layout end to end (#24)

Throwaway boot of the [compose layout prototype](https://github.com/kamui/rakazo-stack/tree/prototype/compose-layout/prototypes/compose-layout) as its own Compose project on Windows + WSL2 + Docker Desktop (engine 29.9.2, x86-64), profiles `lemonade,honcho`, with generated secrets and a dummy model provider key. Upstream images: Rakazo `v0.4.0`, Honcho `v3.5.0`, `pgvector/pgvector:pg18`, Lemonade `v2026.44.1`, `oven/bun:1.4.4-alpine`. Everything was deleted afterwards. Helper scripts: [`compose-boot/`](compose-boot/), transcripts: [`compose-boot/transcripts/`](compose-boot/transcripts/).

## Answer

- **Boot:** the layout comes up healthy with one `docker compose up -d --wait` once one Dockerfile bug is fixed. `COMPOSE_FILE` and `COMPOSE_PROFILES` from `.env` work as planned.
- **1. Honcho on Postgres 18: pass.** All 31 Honcho v3.5.0 migrations ran on PostgreSQL 18.6 with pgvector 0.8.7, ending at the head `c4d8e2f6a1b3`. With the admin JWT, a workspace, peers, a session, and written messages all return 201, and reading them back (list and session context) returns 200. With the dummy key, message embedding, search and peer chat fail (OpenAI `401 Incorrect API key`, so search and chat return 500), and the deriver never processes its queue. `EMBED_MESSAGES` was left at its default; the round trip did not need it off.
- **2. Lemonade behind the relay: pass after two fixes.** `kokoro-v1` and `Whisper-Large-v3-Turbo` (the large model, not Whisper-Small) were pulled and loaded through the relay. Speech returns 200 with WAV, and an ffmpeg WebM/Opus clip transcribes word for word. Direct calls to `lemonade:13305` return 401 with no key or a wrong key.
- **3. Rakazo settings: pass.** #21's sequence (bootstrap-owner, sign-in, `voice.connect` with `http://lemonade-relay:13306/api/v1`, `memory.connectProvider` with `http://honcho:8000`) returns 200 at every step. "Hear a sample" (`POST /api/voice/speak`) returns 200 `audio/mpeg`, and `POST /api/voice/transcribe` returns the sample's text.
- **4. Bot computer isolation: pass for the new services.** A real computer from the stack's own supervisor can't resolve or reach `lemonade-relay`, `lemonade`, `honcho`, `honcho-db` or `postgres`, by name or by IP on any network (`stack`, `egress`, `honcho-data`, `data`). It *can* reach Rakazo's own published `api:3100`/`web:5173` by container IP, every port published on host loopback via `host.docker.internal` (including another install's), and the internet.
- **5. Linux egress: from source.** `auto` starts open. At supervisor start it runs one probe container on a `rakazo-cprobe` bridge. If TCP to the bridge gateway (ports 22 and 1) times out and `169.254.169.254:80` doesn't connect, it switches to `restricted`. If the host answers, it stays `open` with a warning. The verdict is cached until the supervisor restarts. To turn restricted on, run `sudo bash restrict-computer-egress.sh` (Rakazo's `infra/compose/`) as root on Linux Docker Engine with the iptables backend. It inserts `INPUT` and `DOCKER-USER` drops for `rakazo-c+` bridges, installs itself plus a systemd oneshot so the rules persist across reboots (without systemd they last until reboot), and then needs a supervisor restart.
- **6. Tailscale: not done** (needs a tailnet; see Remaining).
- **Layout changes needed:**
  1. Lemonade's inline Dockerfile needs `USER root` before `apt-get` and `USER lemonade` after, because the upstream image runs as uid 10001 ([patch](compose-boot/lemonade-dockerfile.patch)).
  2. Lemonade needs `whispercpp.args=--convert` (not `cpu_args`: backend `auto` picks `vulkan` on x86-64), or WebM, which browsers record, fails with whisper-server 400. Setup can run `docker compose exec lemonade lemonade config set whispercpp.args=--convert`, which persists in the `lemonade-config` volume.
  3. Setup must pull and load the two models (`POST /api/v1/pull` and `/load` through the relay).
  4. Recommended: stop publishing Rakazo's API port (`api: ports: !reset []`). On Docker Desktop computers reach it, and the web port already proxies `/api` and `/rpc`.

## Details

### How the test ran

- Stack directory: the prototype files with the Dockerfile patch, a generated `.env` ([`make-env.sh`](compose-boot/make-env.sh)), and every command through [`dc.sh`](compose-boot/dc.sh), which always passes `-p rs-compose-boot`. That matters because upstream's file sets `name: rakazo` ([`docker-compose.images.yml`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L10)).
- Host ports came from `RAKAZO_WEB_PORT`/`RAKAZO_API_PORT`, chosen to avoid a reference install's defaults. `MODEL_PROVIDER_API_KEY=sk-dummy-not-a-real-key`.
- `docker compose config --services` with only `COMPOSE_FILE`/`COMPOSE_PROFILES` in `.env` listed the 13 expected services, and `up -d --wait` reported every long-running service healthy.
- The host restarted mid-test (outside the test). The first stack was removed, and the full run was repeated on a fresh stack with new secrets. The transcripts come from that second run; the first run gave the same results.

### 1. Honcho v3.5.0 on `pgvector/pgvector:pg18` ([transcript](compose-boot/transcripts/01-honcho.log))

- Honcho's entrypoint ran Alembic from empty to head: 31 `Running upgrade` steps, including the concurrent `pg_trgm` and queue index builds, then `Database initialized successfully`. `alembic_version` is `c4d8e2f6a1b3`, which equals `alembic heads` in the image.
- Extensions from the prototype's `honcho/init.sql`: `vector 0.8.7`, `pg_trgm 1.6`, `btree_gin 1.3`. The pg18 image keeps data in `/var/lib/postgresql/18/docker`, so the prototype's parent mount (`honcho-pgdata:/var/lib/postgresql`) is right.
- Round trip ([`honcho-roundtrip.py`](compose-boot/honcho-roundtrip.py)): the JWT comes from Honcho's own `scripts/generate_jwt.py --admin --print-only` inside the container. The routes and request schemas come from the running server's `/openapi.json`. A request without a token returns `401 No access token provided`, so `AUTH_USE_AUTH=true` is active.
- Without a real key:
  - Message writes still succeed: the embedding happens in the background and fails there. `message_embeddings` held 2 rows, 0 with a vector.
  - `sessions/{id}/search` and `peers/{id}/chat` return 500. The log shows `openai.AuthenticationError: Error code: 401` from `src/embedding_client.py`.
  - The deriver kept 2 `representation` tasks unprocessed for about 10 minutes.
  - What memory does with a real key is out of scope here; embeddings are a separate ticket.
- `CACHE_ENABLED=false` works for the API and the deriver: both log `Cache disabled, using in-memory cache`.

### 2. Lemonade + relay with `LEMONADE_API_KEY` ([transcript](compose-boot/transcripts/02-lemonade.log))

- **Dockerfile bug:** `apt-get update` fails with `List directory /var/lib/apt/lists/partial is missing. - Acquire (13: Permission denied)`, because `ghcr.io/lemonade-sdk/lemonade-server:v2026.44.1` runs as `uid=10001(lemonade)`. Switching to `USER root` for the install and back to `USER lemonade` fixes it; a v2026.44.1 reference install uses the same wrapper.
- **Models:** [`lemonade-pull.sh`](compose-boot/lemonade-pull.sh) runs from Rakazo's `api` container, the path Rakazo itself uses, so the relay adds the key. Results: `pull kokoro-v1` took 16 s, `pull Whisper-Large-v3-Turbo` 77 s, each `load` 7 s or less, all with `status: success`.
- **Relay end to end** ([`voice-roundtrip.sh`](compose-boot/voice-roundtrip.sh), in a one-off container of the stack's own Lemonade image on `stack`, sending no key):
  - `POST /audio/speech` (`lang_code: "a"`, rewritten by the relay) returns 200 with 3.3 s of WAV.
  - ffmpeg re-encodes it to WebM/Opus, and `POST /audio/transcriptions` returns `"The quick brown fox jumps over the lazy dog."`.
- **Key enforcement:** `GET /api/v1/models` and `POST /api/v1/audio/speech` straight to `lemonade:13305` return 401 without a key and with `Bearer wrong`. Through the relay, `/models` returns 200.
- **WebM needs `--convert`:** with Lemonade's defaults the WebM transcription fails with `whisper-server returned status 400: Invalid request`.
  - Setting `whispercpp.cpu_args=--convert` changed nothing. The log showed `whispercpp_backend=vulkan, whispercpp_args=(none)`, because `auto` selects `vulkan`, which Lemonade lists for "cpu (x86_64)" ([backends reference](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/dev/backends-reference.md#L73-L77)).
  - `whispercpp.args` is the backend-agnostic fallback, "unused when backend-specific args defined" ([configuration guide](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md#L246-L252)). After `lemonade config set whispercpp.cpu_args= whispercpp.args=--convert` and a model reload, the log shows `whispercpp_args=--convert` and WebM works.
  - The value is written to `/opt/lemonade/.config/lemonade/config.json`, inside the prototype's `lemonade-config` volume, so it survives restarts.
  - Lemonade also merges a `defaults.json` named by `LEMONADE_DEFAULTS_PATH` ([precedence](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md#L26-L34)), which would let the compose file carry the setting. **Unverified:** not tried.
- `whisper-server` ran on the `vulkan` binary in a GPU-less container and still transcribed. The reference install pins `backend: cpu` instead; which is better was not measured.

### 3. Rakazo accepts the relay and Honcho URLs ([transcript](compose-boot/transcripts/03-rakazo.log))

- Settings came only from `.env`: `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS=lemonade-relay` and `MCP_ALLOW_PRIVATE_ENDPOINT=true`. Upstream's file passes both through to `api` and `worker` ([`docker-compose.images.yml`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml)).
- [`fill-in.sh`](compose-boot/fill-in.sh) is #21's script with the Honcho service name as a variable:
  - Detection found nothing, then `bootstrap-owner` reported `Account verified` and sign-in returned 200.
  - `voice.connect` returned 200 with `endpoint: http://lemonade-relay:13306/api/v1`, `kokoro-v1`, `Whisper-Large-v3-Turbo`, and `voiceId: af_heart`.
  - `memory.connectProvider` returned 200 with `baseUrl: http://honcho:8000` and `reasoningLevel: low`.
  - Sign-out returned 200, and detection afterwards shows the Owner, the voice and Honcho.
- "Hear a sample" calls `speaker.speak(...)` ([`VoiceSettingsOverlay.tsx`](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/pages/VoiceSettingsOverlay.tsx#L324-L343)), which posts to `/api/voice/speak` ([`tts.ts`](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/lib/tts.ts#L180-L190)). Dictation posts base64 audio to `/api/voice/transcribe` ([`dictation.ts`](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/lib/dictation.ts#L577-L587)).
  - [`rakazo-voice-check.sh`](compose-boot/rakazo-voice-check.sh) makes both calls with the Owner's bearer token. Speak returned 200 `audio/mpeg` (81,796 bytes). The same clip, re-encoded to WebM/Opus, transcribed to `"Hi, this is how I will sound when I read replies out loud."`.
- The web port proxies `/api` and `/rpc`: `POST /api/auth/sign-in/email` with a bad password and an unauthenticated `POST /rpc/voice/status` both return 401 on the web port, as on the API port. Setup can therefore use the web port, which leaves the published API port unnecessary.

### 4. Bot computer isolation ([transcript](compose-boot/transcripts/04-computer.log))

- **Creating the computer.** [`computer-probe.sh`](compose-boot/computer-probe.sh) calls the stack's own supervisor (`POST /computers`, token read inside the supervisor container) with a unique botId.
  - The supervisor wants the home at `<DATA_DIR>/homes/<botId>` and won't create it while running as root ([`index.ts`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L218-L249), [`assertBotHomePath`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1510-L1515)), so the script creates it first as uid 1000.
  - The computer got its own non-internal bridge, `rakazo-computer-<botId>-<hash>`, labeled with the botId and this install's owner hash ([`computerNetworkCreateOptions`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L431-L455)), with no extra hosts and no custom DNS.
- **Sharing a Docker host.**
  - Startup cleanup touches only networks carrying this install's owner label ([`reclaimIdleComputerNetworks`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1780-L1798)), and computer names derive from the botId ([`containerNameFor`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L635-L645)). Two stacks on one host don't touch each other's computers as long as botIds differ.
  - The `auto` probe uses fixed global names (`rakazo-egress-probe` network, `rakazo-cprobe` bridge) and removes both afterwards ([`egress-probe.ts`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/egress-probe.ts#L20-L22)). Two supervisors probing at the same moment could collide (**unverified**, read from source).
- **Probe from inside the computer** (TCP connect with a 3 s timeout, run through the supervisor's `exec` endpoint):

  | Target | Result |
  |---|---|
  | `lemonade-relay`, `lemonade`, `honcho`, `honcho-db`, `postgres`, `supervisor`, `web`, `api` by name | not resolvable |
  | Same services by container IP on `stack`, `egress`, `honcho-data`, `data` | timeout |
  | `supervisor` by IP on `app` (unpublished) | timeout |
  | `api:3100`, `web:5173` by IP on `app` | **open**: both ports are published to host `127.0.0.1` |
  | `host.docker.internal`: this stack's web/API ports and another install's `5173`/`3100` | **open** |
  | `host.docker.internal:13305`/`13306` (nothing published) | refused |
  | Per-bot gateway | refused |
  | `1.1.1.1:443` | open (`auto` settled on open, see below) |

- **What the probe shows:**
  - The new services are isolated on Docker Desktop as the README claims, and `LEMONADE_API_KEY` is defense in depth on top.
  - The open paths are Rakazo's own sign-in-protected web/API and whatever else listens on host loopback. Not publishing the API port removes one of them.
  - Docker Desktop also let the computer reach the two published container ports across bridges by container IP; unpublished ports on the same container network did not answer.
  - Supervisor and `web` join a computer's network only when a screen is used, so they weren't on it during the probe.

### 5. Linux Docker Engine: `SANDBOX_COMPUTER_EGRESS` (from Rakazo v0.4.0 source, not run)

- **Default.** Upstream's compose file sets `SANDBOX_COMPUTER_EGRESS: ${SANDBOX_COMPUTER_EGRESS:-auto}` with `SANDBOX_SCREEN_NETWORK: isolated` ([L332-L333](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L332-L333)). Unset outside Compose means `open` ([`resolveComputerEgressMode`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L199-L222)).
- **How `auto` decides.**
  - It starts as `open` ([index.ts L122-L126](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L122-L126)). At supervisor start, after pulling the computer image, it runs `verifyRestrictedEgress` once ([L1169-L1173](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1169-L1173)) and caches the answer in `autoEgressSettled` ([L1226-L1250](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1226-L1250)).
  - The probe ([`egress-probe.ts`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/egress-probe.ts#L4-L153)) creates the network `rakazo-egress-probe` with bridge name `rakazo-cprobe`, which matches the ruleset's `rakazo-c+`. On it, it runs a cap-dropped, read-only container from the computer image that:
    - opens TCP to the bridge's IPv4 gateway (the Docker host) on ports 22 and 1, 2 s each,
    - tries `169.254.169.254:80`,
    - and, if `SANDBOX_COMPUTER_DNS` is set, sends a DNS query to those servers.
  - Every gateway attempt timing out with no metadata connection means `restricted`. A refused or accepted connection means `EgressNotEnforced`, so it stays `open` and logs `computer egress is open: the host firewall rules are not installed` (seen on Docker Desktop). A Docker error or timeout isn't cached: that computer's start fails and the next start probes again.
  - In `restricted`, each per-bot network gets the bridge name `rakazo-c<first 7 hex of sha256(botId)>` ([computer-spec.ts L421-L455](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L421-L455)). Computers created while open are replaced on their next provision ([docs](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L71-L82)).
- **Turning restricted on** ([`restrict-computer-egress.sh`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh)):
  - **Command:** `sudo bash infra/compose/restrict-computer-egress.sh`. The default mode is `--install`; without root it re-runs itself through `sudo` ([L310-L318](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L310-L318)). `--print` shows the rules and changes nothing; `--remove` uninstalls.
  - **Requires** Linux Docker Engine with the iptables backend (the `DOCKER-USER` chain). Docker Desktop, rootless Docker and `firewall-backend: nftables` are not supported ([L27-L29](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L27-L29)). It fails if the host has global IPv6 but no usable `ip6tables` ([L265-L290](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L265-L290)).
  - **Rules,** inserted at the head of each chain ([L56-L109](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L56-L109)):
    - `INPUT -i rakazo-c+ -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT`
    - `INPUT -i rakazo-c+ -j DROP`
    - `DOCKER-USER -i rakazo-c+ -o rakazo-c+ -j RETURN`
    - `DOCKER-USER -i rakazo-c+ -d <cidr> -j DROP` for `0.0.0.0/8 10.0.0.0/8 100.64.0.0/10 127.0.0.0/8 169.254.0.0/16 172.16.0.0/12 192.0.0.0/24 192.168.0.0/16 198.18.0.0/15 224.0.0.0/4 240.0.0.0/4`
    - the same with `ip6tables` for `::1/128 fc00::/7 fe80::/10 ff00::/8`.
  - **Persistence** ([L320-L348](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L320-L348)): the script copies itself to `/usr/local/sbin/rakazo-computer-egress` and writes and enables `/etc/systemd/system/rakazo-computer-egress.service` (oneshot, `After=docker.service`, `ExecStart=… --apply`). The rules therefore survive a reboot.
    - Without systemd, or when run from a pipe instead of a saved file, it applies the rules and warns that they last until reboot.
    - For Docker inside a Linux VM, the docs give a host-network container with `NET_ADMIN` that re-applies the rules every 60 s ([docs L111-L142](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L111-L142)).
  - **After installing,** restart (recreate) the supervisor so `auto` probes again ([docs L65-L77](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L65-L77)). With an explicit `restricted`, no restart is needed: the check runs before each first restricted computer until it passes.
- **Upstream's installer** (`install-images.sh`, [`configure_computer_egress`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/install-images.sh#L337-L370)) runs the script itself where supported and writes `SANDBOX_COMPUTER_EGRESS=restricted`, or `open` with the reason (Docker Desktop, rootless, no iptables, or a failed run).
- **For rakazo-stack setup:** setup can detect the state without root from the supervisor's log (`restricted computer egress verified` vs `computer egress is open`). When open on Linux, it prints the sudo command plus `docker compose up -d --force-recreate supervisor`. This matches #10's plan of never running sudo itself.

## Open questions / follow-ups

- **Tailscale container** (userspace, Docker Desktop): does Serve keep the ts.net `Host` header Rakazo's allowed-hosts check sees, do HTTPS certs work on the free plan, and do Secure cookies and Composio callbacks work? Needs a tailnet auth key (HITL, on the Mac).
- **macOS variant:** native Lemonade on host loopback with `LEMONADE_API_KEY`, the relay via `LEMONADE_UPSTREAM=http://host.docker.internal:13305`, and a bot computer getting 401 from native Lemonade.
- **Linux Docker Engine live check** of `restrict-computer-egress.sh` plus the `auto` switch (only read from source here).
- Should `compose.stack.yaml` carry `whispercpp.args=--convert` declaratively (`LEMONADE_DEFAULTS_PATH` plus a mounted `defaults.json`), or should setup run `lemonade config set`? Should the backend be pinned to `cpu`?
- Should the layout drop Rakazo's published API port (`ports: !reset []`)? Setup and the browser only need the web port.

## Sources

- Prototype under test: <https://github.com/kamui/rakazo-stack/tree/prototype/compose-layout/prototypes/compose-layout>
- Rakazo v0.4.0 (tag commit `b9c652f`, the same revision as `ghcr.io/elie222/rakazo/app:v0.4.0`):
  - [infra/compose/docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml)
  - [infra/sandboxes/supervisor/src/index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts)
  - [computer-spec.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts)
  - [egress-probe.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/egress-probe.ts)
  - [infra/compose/restrict-computer-egress.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh)
  - [infra/compose/install-images.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/install-images.sh)
  - [docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)
  - [apps/web/src/pages/VoiceSettingsOverlay.tsx](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/pages/VoiceSettingsOverlay.tsx), [apps/web/src/lib/tts.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/lib/tts.ts), [apps/web/src/lib/dictation.ts](https://github.com/elie222/rakazo/blob/v0.4.0/apps/web/src/lib/dictation.ts)
- Lemonade v2026.44.1:
  - [docs/guide/configuration/README.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md)
  - [docs/dev/backends-reference.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/dev/backends-reference.md)
  - in-image `lemonade config` output and `lemond` logs (transcript 02)
- Honcho v3.5.0 image `ghcr.io/plastic-labs/honcho:v3.5.0`: entrypoint migration log, `/openapi.json`, `scripts/generate_jwt.py` (transcript 01).
- #21 call sequence: <https://github.com/kamui/rakazo-stack/blob/research/setup-rpc-proof/docs/research/setup-rpc-proof.md>
