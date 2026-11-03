# Test Lemonade natively on an Apple-silicon Mac (#16)

Hands-on check of [How should Lemonade run on Apple-silicon macOS?](https://github.com/kamui/rakazo-stack/issues/5) on a real Mac: Apple M3 Pro (11 cores, 18 GB), macOS 27.2, Homebrew ffmpeg 9.0.2, Docker Desktop 4.94.0 (Engine 29.8.2, API 1.56; it updated itself to 4.97.0 / Engine 29.9.2 mid-test). Pins: Lemonade `v2026.44.1` embeddable tarball, whisper.cpp `v1.8.4`, Kokoros `b21`, Rakazo `v0.4.0`, Bun `1.4.4`. Everything ran from a scratch directory and was deleted afterwards. Timing script: [`bench.sh`](lemonade-mac-test/bench.sh). Transcripts: [`lemonade-mac-test/transcripts/`](lemonade-mac-test/transcripts/).

## Answer

- **Native Lemonade works as the research expected.** The embeddable `lemond` runs without admin rights or a Gatekeeper prompt. Its `curl` download has no quarantine flag, and the binary is ad-hoc signed. Whisper loads on the GPU through Metal (`ggml_metal_init: found device: Apple M3 Pro`, `device: gpu`), and `--convert` finds ffmpeg through the `PATH` lemond was given. Kokoro registers only ONNX Runtime's `CPUExecutionProvider` (`device: cpu`).
- **Config:** `whispercpp.backend=metal` and `whispercpp.metal_args=--convert`. Drop `-t`: 4 threads (the default) and 8 threads give the same times, and 1 thread is slower. Add `--no-broadcast`, because lemond's default `broadcast: true` sends a UDP discovery beacon onto the LAN. Models stay loaded when idle (`auto_evict` is off by default).
- **Latency on the M3 Pro, warm, median of 5:**

  | | short (11 words / 3.9 s audio) | medium (34 words / 11.5 s) | long (95 words / 32 s) |
  |---|---|---|---|
  | Kokoro speech (whole WAV) | 0.83 s | 2.95 s | 6.67 s |
  | Whisper transcription, WebM, `language=en` | 0.82 s | 0.95 s | 2.03 s |

  - Through Rakazo, "Hear a sample" (13 words) takes 1.2 s and dictation 0.9 s.
  - Kokoro returns the whole file at once, so time to first byte equals total time.
  - Without a language hint, each transcription costs about 0.8 s more for language detection. Rakazo already sends `language`.
  - The first Whisper load takes 17 s (11 s of it compiling Metal shaders); later loads take 1–2 s.
  - **The Linux CPU baseline was not measured:** no Linux machine was available, and no earlier ticket recorded numbers. Run `bench.sh` against a Linux or WSL2 Lemonade to compare.
- **Containers reach the loopback-only lemond.** On Docker Desktop for Mac, `host.docker.internal` resolves to an IPv6 address that reaches host `127.0.0.1`. Lemonade returns 401 without the key or with a wrong one, and 200 with it.
- **The relay and Rakazo work on the Mac.** With `LEMONADE_UPSTREAM=http://host.docker.internal:13305`, a throwaway Rakazo v0.4.0 stack boots healthy. The supervisor runs with the Docker socket and creates a bot computer. `voice.connect` with `http://lemonade-relay:13306/api/v1` returns 200, and so do speak and transcribe.
- **New problem: `LEMONADE_API_KEY` does not keep bot computers away from Lemonade on macOS.** lemond starts each model as a child server on host loopback (Kokoro on `127.0.0.1:8001`, Whisper on `:8002`, then the next free port), with no authentication. A real Rakazo bot computer can't reach `lemonade-relay`, `postgres`, `api` or `supervisor`. But through `host.docker.internal` it synthesised speech on `:8001` and transcribed audio on `:8002` with no key.
  - whisper-server also has an unauthenticated `POST /load`. From a container, a missing path returns 400 and leaves Whisper unhealthy, which also reveals whether a path exists on the Mac. An existing non-model file (`/etc/hosts`) makes whisper-server exit.
  - lemond restarts Whisper on the next request (4.2 s), so the damage is recoverable, but this breaks the map's rule that bot computers must not reach Lemonade.
  - lemond's WebSocket port `127.0.0.1:9000` is also reachable; it was not probed further.
  - On Linux and Windows, Lemonade runs in a container, so these child ports stay inside its network namespace.
  - Rakazo itself reports `computer egress is open` on Docker Desktop, so computers also reach anything else on the Mac's loopback.
- **Setup binary (from #22): works on the `curl` path.**
  - A `bun-darwin-arm64` hello world cross-compiled in a Linux container is 89 MB. It is ad-hoc and linker-signed (`flags=0x20002`, 21,599 hashes) and passes `codesign --verify --strict`.
  - Fetched with `curl`, it has no `com.apple.quarantine` and runs with no prompt (it reached Docker API 1.56).
  - A copy marked the way a browser marks downloads fails Gatekeeper's assessment (`spctl`: rejected; `syspolicy_check`: "Notary Ticket Missing", fatal). So a browser download would be blocked, as #22 predicted. The dialog itself was not shown, to avoid a prompt on the test Mac.

## Not tested

- **lemond as a real per-user LaunchAgent.** Installing one was out of bounds for this session, so lemond ran as a plain background process with a launchd-like clean environment (`env -i`, explicit `PATH`). `KeepAlive`, login start and launchd logging belong to [How does setup install Lemonade and ffmpeg on macOS?](https://github.com/kamui/rakazo-stack/issues/17).
- **Podman's `host.containers.internal`:** Podman is out of scope for v1.
- **Linux CPU baseline numbers** (see above).
- **Tailscale under Docker Desktop**, still open on [Boot the drafted compose layout end to end](https://github.com/kamui/rakazo-stack/issues/24).

## Open questions / follow-ups

- **Research:** can rakazo-stack stop Rakazo's bot computers on Docker Desktop for Mac from reaching host loopback, or reaching lemond's child ports, without admin rights and without patching upstream? Candidates to check:
  - Docker Desktop settings
  - binding the children elsewhere, or putting them behind a Unix socket
  - Rakazo's `SANDBOX_COMPUTER_DNS` / egress options
  - a macOS `pf` rule, which needs admin
  - Lemonade options to authenticate or proxy its children
- **Grilling (after that research):** what does v1 do about it on macOS: block, accept and document, or change where Lemonade runs?
- **Inputs for "How does setup install Lemonade and ffmpeg on macOS?":**
  - `lemond --no-broadcast <cache> <config>` with `HF_HOME` set
  - `PATH` must include ffmpeg's folder
  - set `whispercpp.backend=metal` and `whispercpp.metal_args=--convert` with `lemonade config set`
  - pull and load both models (1.9 GB) on the first run
  - expect a 17 s first Whisper load

## How to reproduce

1. Download `lemonade-embeddable-2026.44.1-macos-arm64.tar.gz` and check it against the release digest `sha256:204e4364…7017`.
2. Start `lemond --no-broadcast <cache> <config>` with `PATH=/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin`, `HF_HOME=<scratch>` and `LEMONADE_API_KEY=<random>`.
3. Run `lemonade config set whispercpp.backend=metal whispercpp.metal_args=--convert`, then `lemonade pull` and `/api/v1/load` both models.
4. Run `LEMONADE_API_KEY=… bench.sh http://127.0.0.1:13305/api/v1 5 <label>`.
5. For the stack, use the [compose-boot helpers](https://github.com/kamui/rakazo-stack/tree/research/compose-boot/docs/research/compose-boot) with:
   - `COMPOSE_PROFILES=` (empty)
   - `LEMONADE_UPSTREAM=http://host.docker.internal:13305`
   - its own `-p` project name
