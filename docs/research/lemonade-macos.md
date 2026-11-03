# Lemonade on Apple-silicon macOS

Research for [#5](https://github.com/kamui/rakazo-stack/issues/5): how should Lemonade run on Apple-silicon macOS so speech (Kokoro) and transcription (Whisper) use the GPU via Metal?

Pinned versions: Lemonade `v2026.44.1` (commit `69948a7d`), whisper.cpp backend `v1.8.4` (lemonade-sdk/whisper.cpp-rocm, commit `d92d51e9`), Kokoros backend `b21` (lemonade-sdk/Kokoros, commit `3696f82c`).

## Answer

- **Run Lemonade natively on macOS, not in a container.** The upstream image is `linux/amd64` only, and Lemonade at v2026.44.1 ships no Linux arm64 Kokoro or Vulkan whisper.cpp build. Docker Desktop gives Mac containers no GPU. A Podman `libkrun` machine exposes a Vulkan GPU only to arm64 containers with a patched Mesa, and Lemonade has no backend that could use it there.
- **Whisper uses the GPU through Metal; Kokoro does not.** whisper.cpp's macOS backend (`metal`) is built with `GGML_METAL=ON` and is the only whisper backend Lemonade offers on macOS. Kokoro's macOS backend is also named `metal`, but the Kokoros `b21` build uses ONNX Runtime's **CPU** execution provider, so Kokoro runs on the Apple-silicon CPU. The map note "Lemonade should use the GPU via Metal" holds for Whisper only.
- **Install options.** Upstream offers a `.pkg` (also wrapped by the Homebrew cask `lemonade-server`), which needs admin rights. It installs a root LaunchDaemon (`ai.lemonadeserver.server`, `RunAtLoad`, `KeepAlive=false`) plus a per-user tray LaunchAgent. The release also ships `lemonade-embeddable-<ver>-macos-arm64.tar.gz`, a portable `lemond` + `lemonade` CLI that runs from any directory (`lemond DIR`) and needs no admin rights.
- **Recommendation: rakazo-stack runs the embeddable tarball, pinned, as a per-user LaunchAgent that it owns.** That avoids admin rights, keeps config and backends inside the Instance directory, lets rakazo-stack set `PATH` so `ffmpeg` is found, and gives `KeepAlive`. The tradeoff is that it runs only while the Self-hoster is logged in, the same as Docker Desktop. The `.pkg` stays a documented alternative.
- **Config on macOS:** `whispercpp.backend = "metal"` (`cpu` is not offered on macOS) and `whispercpp.metal_args = "--convert"`. Lemonade reads `<backend>_args` and falls back to `args`. Drop `-t 12`: whisper-server defaults to `min(4, cores)` threads, and with Metal the main compute is on the GPU. Kokoro has no backend or args settings. Leave `host` at the default `localhost`.
- **ffmpeg is still required.** `--convert` makes whisper-server run `ffmpeg` from `PATH` at startup, and the server exits if ffmpeg is not found. On macOS that means installing ffmpeg (for example with Homebrew, into `/opt/homebrew/bin`) and making sure lemond's `PATH` includes it. The `.pkg` LaunchDaemon sets no `PATH`, so Homebrew's ffmpeg is likely invisible to it (**unverified**). This is the strongest practical reason to prefer the LaunchAgent.
- **How containers reach it:** containers use `host.docker.internal:13305`. Docker Desktop and Podman both provide that name. Podman's gvproxy forwards the host-gateway IP to the host's `127.0.0.1`, so a loopback-only Lemonade is reachable. The relay's `UPSTREAM` becomes `http://host.docker.internal:13305` on macOS. Rakazo keeps using the relay on `:13306` as it does today. The loopback reachability on Docker Desktop for Mac is **unverified** on hardware.
- **Latency: no published Metal numbers for Lemonade's Kokoro or Whisper.** The only first-party figure is Kokoros' own CPU benchmark on an M2: 1.44 s to first audio for a long ~120-word passage. Measured macOS latency against the CPU baseline is **unverified**. It needs a hands-on task ticket on real Apple-silicon hardware.

## Details

### Installers and the background service

- The macOS install guide says: "Download and run the macOS `.pkg` from the latest Lemonade release" ([docs/guide/install/macos.md@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/install/macos.md)). The FAQ says the `.pkg` is for Apple Silicon Macs ([docs/guide/faq.md@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/faq.md)). The v2026.44.1 release assets include `Lemonade-2026.44.1-Darwin.pkg` and `lemonade-embeddable-2026.44.1-macos-arm64.tar.gz` ([release v2026.44.1](https://github.com/lemonade-sdk/lemonade/releases/tag/v2026.44.1)).
- The project site also lists `brew install --cask lemonade-server` ([docs/index.html@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/index.html), macOS tab). Homebrew's cask API shows that cask at version 2026.44.1, installing the same `Lemonade-2026.44.1-Darwin.pkg` and requiring arm64 ([formulae.brew.sh/api/cask/lemonade-server.json](https://formulae.brew.sh/api/cask/lemonade-server.json)). A pkg cask still runs the system installer, so it needs admin rights too.
- **What the `.pkg` installs.** The post-install script ([src/cpp/postinst-full-mac@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/postinst-full-mac)) does the following:
  - writes `/Library/LaunchDaemons/ai.lemonadeserver.server.plist` and `/Library/LaunchAgents/ai.lemonadeserver.tray.plist`
  - puts binaries in `/usr/local/bin` and resources in `/Library/Application Support/Lemonade`
  - creates `/var/log/lemonade`
  - bootstraps the daemon into the `system` launchd domain and the tray into the console user's `gui/<uid>` domain

  Writing to those paths needs root, so the `.pkg` requires admin rights.
- **The LaunchDaemon plist.** It runs `/usr/local/bin/lemond` with `RunAtLoad=true` and `KeepAlive=false`. It has no `UserName`, so lemond runs as root. Its only environment variable is `HF_HOME=/Library/Application Support/lemonade`, and it sets no `PATH` ([src/cpp/ai.lemonadeserver.server.plist.in@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/ai.lemonadeserver.server.plist.in)). The tray agent plist, by contrast, sets `PATH=/usr/local/bin:/usr/bin:/bin` ([src/cpp/ai.lemonadeserver.tray.plist.in@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/ai.lemonadeserver.tray.plist.in)). The config docs confirm that on macOS "the LaunchDaemon installed by the macOS package runs `lemond`" ([docs/guide/configuration/README.md@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md), lemonade-tray section).
- **Config and cache locations.** When lemond runs as root and `XDG_*` is unset, it keeps config in `/Library/Application Support/lemonade/.config` and cache in `/Library/Application Support/lemonade/.cache`. Otherwise it uses `~/.config/lemonade` and `~/.cache/lemonade` ([src/cpp/server/utils/platform/path_macos.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/utils/platform/path_macos.cpp)). The docs give the same two config paths ([configuration/README.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md#configjson)).
- **The embeddable tarball.** The embeddable guide describes it as "a portable build of the `lemond` service that you can bundle into your app". The positional `lemond DIR` decides where `config.json` and downloaded backends live, and `lemonade config set` / `lemonade backends install` customize it ([docs/embeddable/README.md@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/embeddable/README.md)). The v2026.44.1 macOS arm64 tarball contains `lemond`, `lemonade`, `LICENSE` and `resources/` (`defaults.json`, `backend_versions.json`, `server_models.json`, …), checked by listing the release asset. Running it as a per-user LaunchAgent is a rakazo-stack design choice, not something the upstream docs cover. Gatekeeper and quarantine behavior for a tarball fetched with `curl` is **unverified**.

### Which backends use Metal

- **whisper.cpp.** The descriptor's macOS support row is `metal` ("Apple Silicon GPU"). The `cpu` and `vulkan` rows are `windows`/`linux`, `x86_64` only ([src/cpp/include/lemon/backends/whispercpp/whispercpp.h@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/backends/whispercpp/whispercpp.h)).
  - The install code maps `metal` to `whisper-<ver>-darwin-metal-arm64.tar.gz`. The `cpu` backend throws "Unsupported platform" off Windows and Linux. Loading with `metal` marks the device as GPU ([src/cpp/server/backends/whispercpp/whispercpp_server.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp)).
  - `backend_versions.json` pins `whispercpp.metal = v1.8.4` ([src/cpp/resources/backend_versions.json@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/backend_versions.json)).
  - The `macos-metal` job that builds that asset configures `-DGGML_METAL=ON` ([.github/workflows/build.yml@d92d51e9, lines 683–703](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/d92d51e9f02af313eaec258be905623775e169c6/.github/workflows/build.yml#L683-L703)).
  - The upstream whisper.cpp README says "On Apple Silicon, the inference runs fully on the GPU via Metal" ([ggml-org/whisper.cpp README@v1.8.4](https://github.com/ggml-org/whisper.cpp/blob/v1.8.4/README.md)).
  - whisper-server defaults to `use_gpu = true` and `flash_attn = true` ([examples/server/server.cpp@d92d51e9](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/d92d51e9f02af313eaec258be905623775e169c6/examples/server/server.cpp)).
  - On macOS, Lemonade always registers an "Apple Metal" device ([src/cpp/server/system_info.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/system_info.cpp), "Metal is always available on macOS"). A Linux Lemonade's `/api/v1/system-info` reports the whisper `metal` backend as `unsupported`, "Requires macOS" (observed on a v2026.44.1 reference install).
- **Kokoro.** On `__APPLE__`, Lemonade hard-codes the `metal` backend and downloads `kokoros-darwin-arm64-metal.tar.gz` from `lemonade-sdk/Kokoros`. The descriptor marks Kokoro `selectable_backend = false`, `takes_args = false`, and Lemonade passes Kokoro only model, voices, IP and port ([src/cpp/server/backends/kokoro/kokoro_server.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/kokoro/kokoro_server.cpp), [kokoro.h](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/backends/kokoro/kokoro.h)). It pins `kokoro.metal = b21`.
  - The Kokoros `b21` workflow builds the `darwin-arm64-metal` archive with plain `cargo build --release` on `macos-14` ([.github/workflows/build-koko.yml@3696f82c](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/.github/workflows/build-koko.yml)).
  - Default features are `["cpu"]`, and only a `cuda` feature exists ([kokoros/Cargo.toml@3696f82c](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/kokoros/Cargo.toml)).
  - Without `cuda`, the ONNX session uses `ep::CPU` and prints "Configured with: CPU execution provider" ([kokoros/src/onn/ort_base.rs@3696f82c](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/kokoros/src/onn/ort_base.rs)).
  - The Kokoros README lists CoreML support as future work: "Attempts to make this work on CoreML … would likely start with converting the ONNX model" ([README.md@3696f82c](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/README.md)).
  - So at these pins, Kokoro on macOS is a native arm64 CPU build. "metal" is only the asset name.

### Config equivalents of the Linux baseline

- Lemonade reads per-backend args as `<section>.<backend>_args` and falls back to `<section>.args` ([src/cpp/server/runtime_config.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/runtime_config.cpp), `recipe_options`). The backend is resolved first ([src/cpp/server/router.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/router.cpp), `resolve_effective_options`).
  - A `<backend>_args` key is valid for any backend in the descriptor's support rows (`is_backend_variant_key`), so `whispercpp.metal_args` is accepted even though the generated example config shows only `cpu_args`/`npu_args`.
  - With `backend: "auto"`, the first supported backend on the host is used, which is `metal` on macOS ([src/cpp/server/recipe_options.cpp@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/recipe_options.cpp), `get_option`). Setting it explicitly is clearer.
- **Proposed macOS `config.json` overrides:** `{"whispercpp": {"backend": "metal", "metal_args": "--convert"}}`. Lemonade reserves `-m`, `--model` and `--port`. Other whisper-server flags pass through ([whispercpp_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp), "optional whisper-server flags like --convert come from whispercpp_args").
- **Thread count.** whisper-server's default is `n_threads = min(4, hardware_concurrency)`, and `-t/--threads` overrides it ([server.cpp@d92d51e9](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/d92d51e9f02af313eaec258be905623775e169c6/examples/server/server.cpp)). The Linux `-t 12` tunes CPU inference. With Metal it is not expected to matter, though its effect is **unverified**. Kokoro exposes no thread setting through Lemonade.
- **ffmpeg.** `--convert` "requires ffmpeg on the server". At startup whisper-server runs `system("ffmpeg -version")`, and if that fails it prints "ffmpeg is not found … included in your system's PATH" and calls `exit(0)`. Each request is converted with `ffmpeg -i … -ar 16000 -ac 1 -c:a pcm_s16le` ([server.cpp@d92d51e9](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/d92d51e9f02af313eaec258be905623775e169c6/examples/server/server.cpp)). Lemonade's packaging notes list ffmpeg as a recommended dependency "for whisper.cpp audio resampling and/or transcoding" ([docs/dev/getting-started.md@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/dev/getting-started.md)), and the `.pkg` does not bundle it. On macOS, ffmpeg must be installed and on lemond's `PATH`. lemond starts whisper-server as a child process with an added library path, and inheriting the parent `PATH` is the expected behavior (**unverified**).

### Why not a container on the Mac

- **Image platform.** `ghcr.io/lemonade-sdk/lemonade-server:v2026.44.1` is published for `linux/amd64` only (checked with `docker buildx imagetools inspect`). On Apple silicon it would run under x86 emulation.
- **Linux arm64 backends.** Kokoro's only non-macOS row is `cpu` on `windows`/`linux` `x86_64`, and its Linux asset is `kokoros-linux-x86_64.tar.gz`. whisper.cpp's `vulkan` and `cpu` rows are also `x86_64`-only ([kokoro.h](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/backends/kokoro/kokoro.h), [whispercpp.h](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/backends/whispercpp/whispercpp.h)). Even a self-built arm64 image would have no Kokoro and no GPU whisper.
- **Docker Desktop.** Its GPU support page says passthrough to Linux containers "is only available on Windows with the WSL 2 backend" with NVIDIA GPUs ([docs.docker.com/desktop/features/gpu](https://docs.docker.com/desktop/features/gpu/)).
- **Podman machine.** `libkrun` is the default macOS provider ([podman-machine(1)](https://docs.podman.io/en/latest/markdown/podman-machine.1.html)). Podman Desktop documents GPU access on Apple silicon as "a virtualized GPU from within the Podman Machine" that translates Vulkan to Metal.
  - The container needs a patched Mesa (Fedora base plus the `slp/mesa-libkrun-vulkan` COPR) and `--device /dev/dri`.
  - The virtualized GPU "only supports vulkan compute shaders" ([podman-desktop.io/docs/podman/gpu](https://podman-desktop.io/docs/podman/gpu)).
  - That would need an arm64 Lemonade image with a patched Mesa and an arm64 Vulkan whisper.cpp build. None of these exist upstream, and rakazo-stack would have to fork or patch to get them, which the map rules out. It would still leave Kokoro on CPU.
- Lemonade's own container-backend spec says "Container backends are hidden on Windows and macOS" ([docs/dev/specs/container-backends.md@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/dev/specs/container-backends.md)).

### How the other containers reach a native Lemonade

- lemond binds `host: "localhost"` and port `13305` by default ([src/cpp/resources/defaults.json@v2026.44.1](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/defaults.json)). The docs warn that `0.0.0.0` exposes the unauthenticated `/internal/*` endpoints unless `LEMONADE_API_KEY` is set ([configuration/README.md, Remote Server Connection](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md#remote-server-connection)). Keep it on loopback.
- **Docker Desktop.** Docker documents `host.docker.internal` as the way for a container to reach a host service ([docs.docker.com networking how-tos](https://docs.docker.com/desktop/features/networking/networking-how-tos/)). The page does not say whether host services bound to loopback are reachable. On the Windows/WSL2 reference install, containers already reach a loopback-published port through `host.docker.internal`. The same check on Docker Desktop for Mac is **unverified**.
- **Podman.** Podman adds `host.containers.internal` and `host.docker.internal`. Under `podman machine`, gvproxy's DNS resolves them ([podman-run(1)](https://docs.podman.io/en/latest/markdown/podman-run.1.html), `--add-host`). gvproxy's default config NATs the host-gateway IP (`192.168.127.254`) to the host's `127.0.0.1` ([containers/gvisor-tap-vsock cmd/gvproxy/config.go@v0.9.0](https://github.com/containers/gvisor-tap-vsock/blob/v0.9.0/cmd/gvproxy/config.go)), so a loopback-only lemond is reachable.
- **Wiring.** On macOS the relay's `UPSTREAM` changes from the in-network `http://lemonade:13305` to `http://host.docker.internal:13305`. Rakazo's speech and transcription base URL keeps pointing at the relay.

### Published latency

- **Lemonade.** Neither the Lemonade docs nor the release notes at v2026.44.1 publish Kokoro or Whisper latency for macOS (searched `docs/` for macOS and Metal).
- **Kokoros.** The README benchmark on "a Mac M2 with 8 cores", CPU build, one instance, reports 1.44 s time to first audio and 19.0 s total for a long multi-sentence passage ([README.md@3696f82c](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/README.md)). That does not compare directly to a short reply.
- **whisper.cpp.** The README makes the qualitative "fully on the GPU via Metal" claim but publishes no server latency numbers.
- **Measured macOS-vs-CPU-baseline latency: unverified.** It was not measured, because no Apple-silicon machine was available.

## Open questions / follow-ups

- **Task:** on a real Apple-silicon Mac, install the pinned embeddable lemond as a LaunchAgent with ffmpeg on `PATH`. Then confirm all of the following:
  - whisper loads with `metal` + `--convert` and the logs show a Metal device
  - Kokoro reports the CPU execution provider
  - `host.docker.internal:13305` works from Docker Desktop and Podman containers with lemond on loopback
  - Rakazo transcription and speech latency (first audio, transcription round-trip) compared with the Linux CPU baseline, using the same phrases
  - whether `-t` changes anything with Metal
- **Grilling:** do we require Homebrew on macOS (for ffmpeg, and possibly Docker), or does setup fetch a pinned ffmpeg binary itself? This touches "self-sufficient" and the hardware/software floor.
- **Grilling:** embeddable LaunchAgent (no admin; runs only while logged in) or the upstream `.pkg` (admin, root daemon, survives logout, `KeepAlive=false`, `PATH` patching needed for ffmpeg)? And what happens if the Self-hoster already runs a `.pkg` Lemonade on port 13305?
- **Grilling:** does the map's "GPU via Metal on macOS" note change, now that Kokoro is CPU-only on Mac at these pins? Should the macOS hardware floor assume CPU TTS?
- **Research (later, version pinning):** watch lemonade-sdk/Kokoros for a CoreML or Metal build. When one ships in Lemonade, macOS TTS moves to the GPU without rakazo-stack changes.
- **Research:** does rakazo-stack on macOS standardize on Docker Desktop or Podman? This affects whether `host.docker.internal` reaches loopback, and is owned by the macOS container-runtime decision.

## Sources

- lemonade-sdk/lemonade v2026.44.1 (commit `69948a7d40afbf91f6ad990d8b863c83ce5cc71d`):
  - [docs/guide/install/macos.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/install/macos.md)
  - [docs/guide/faq.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/faq.md)
  - [docs/guide/configuration/README.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/guide/configuration/README.md)
  - [docs/embeddable/README.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/embeddable/README.md)
  - [docs/dev/getting-started.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/dev/getting-started.md)
  - [docs/dev/specs/container-backends.md](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/dev/specs/container-backends.md)
  - [docs/index.html](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/docs/index.html)
  - [src/cpp/postinst-full-mac](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/postinst-full-mac)
  - [src/cpp/ai.lemonadeserver.server.plist.in](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/ai.lemonadeserver.server.plist.in)
  - [src/cpp/ai.lemonadeserver.tray.plist.in](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/ai.lemonadeserver.tray.plist.in)
  - [src/cpp/server/utils/platform/path_macos.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/utils/platform/path_macos.cpp)
  - [src/cpp/include/lemon/backends/whispercpp/whispercpp.h](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/backends/whispercpp/whispercpp.h)
  - [src/cpp/include/lemon/backends/kokoro/kokoro.h](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/backends/kokoro/kokoro.h)
  - [src/cpp/server/backends/whispercpp/whispercpp_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp)
  - [src/cpp/server/backends/kokoro/kokoro_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/kokoro/kokoro_server.cpp)
  - [src/cpp/server/runtime_config.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/runtime_config.cpp)
  - [src/cpp/server/router.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/router.cpp)
  - [src/cpp/server/recipe_options.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/recipe_options.cpp)
  - [src/cpp/server/system_info.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/system_info.cpp)
  - [src/cpp/resources/backend_versions.json](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/backend_versions.json)
  - [src/cpp/resources/defaults.json](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/defaults.json)
- [Lemonade release v2026.44.1](https://github.com/lemonade-sdk/lemonade/releases/tag/v2026.44.1): asset list and the macOS embeddable tarball contents. Image platforms of `ghcr.io/lemonade-sdk/lemonade-server:v2026.44.1` (OCI index: `linux/amd64` only).
- lemonade-sdk/whisper.cpp-rocm v1.8.4 (commit `d92d51e9f02af313eaec258be905623775e169c6`):
  - [.github/workflows/build.yml](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/d92d51e9f02af313eaec258be905623775e169c6/.github/workflows/build.yml)
  - [examples/server/server.cpp](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/d92d51e9f02af313eaec258be905623775e169c6/examples/server/server.cpp)
  - [release v1.8.4 assets](https://github.com/lemonade-sdk/whisper.cpp-rocm/releases/tag/v1.8.4)
- [ggml-org/whisper.cpp README @ v1.8.4](https://github.com/ggml-org/whisper.cpp/blob/v1.8.4/README.md)
- lemonade-sdk/Kokoros b21 (commit `3696f82cb26363b9f323cb8ad595ee7d9c771811`):
  - [.github/workflows/build-koko.yml](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/.github/workflows/build-koko.yml)
  - [kokoros/Cargo.toml](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/kokoros/Cargo.toml)
  - [kokoros/src/onn/ort_base.rs](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/kokoros/src/onn/ort_base.rs)
  - [README.md](https://github.com/lemonade-sdk/Kokoros/blob/3696f82cb26363b9f323cb8ad595ee7d9c771811/README.md)
  - [release b21](https://github.com/lemonade-sdk/Kokoros/releases/tag/b21)
- [Homebrew cask API: lemonade-server](https://formulae.brew.sh/api/cask/lemonade-server.json)
- Docker docs: [GPU support in Docker Desktop](https://docs.docker.com/desktop/features/gpu/), [Networking how-tos](https://docs.docker.com/desktop/features/networking/networking-how-tos/)
- Podman docs: [podman-machine(1)](https://docs.podman.io/en/latest/markdown/podman-machine.1.html), [podman-run(1)](https://docs.podman.io/en/latest/markdown/podman-run.1.html), [Podman Desktop: GPU container access](https://podman-desktop.io/docs/podman/gpu)
- [containers/gvisor-tap-vsock cmd/gvproxy/config.go @ v0.9.0](https://github.com/containers/gvisor-tap-vsock/blob/v0.9.0/cmd/gvproxy/config.go)
