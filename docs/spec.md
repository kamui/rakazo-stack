# rakazo-stack v1 spec

This is the build spec for rakazo-stack v1: what it installs, what it needs, and how each command works on Linux, Windows (WSL 2) and Apple-silicon macOS. It was written from the decisions on [Map: rakazo-stack v1 spec](https://github.com/kamui/rakazo-stack/issues/1) and the tickets it links. Each rule links the ticket that decided it. Where an older ticket and a newer one disagree, the newer one wins, and this spec states only the result. Decisions made while reviewing this spec, with no ticket of their own, cite the map ([#1](https://github.com/kamui/rakazo-stack/issues/1)).

Terms such as Self-hoster, Instance, Instance URL, Instance folder, Component, Backup, Update, Hold, Model provider and Checklist are defined in [GLOSSARY.md](../GLOSSARY.md). Three of Rakazo's own terms are used as Rakazo uses them:

- **Owner:** the Rakazo account setup creates for the Self-hoster. It owns the default Space.
- **Space:** Rakazo's workspace. Memory and some settings are per Space. The Owner's first Space is the **default Space**.
- **Bot computer:** a container Rakazo's `supervisor` starts for an AI agent to run code in.

"Not tested" in this spec means the behaviour was decided from source or docs and still needs proving during implementation. Those items are collected in [Acceptance checks](#acceptance-checks-not-yet-run).

## Contents

1. [Scope](#1-scope)
2. [Supported platforms](#2-supported-platforms)
3. [Requirements and preflight](#3-requirements-and-preflight)
4. [Components and versions](#4-components-and-versions)
5. [Compose layout](#5-compose-layout)
6. [Files on disk](#6-files-on-disk)
7. [`rakazo-stack setup`](#7-rakazo-stack-setup)
8. [Model provider](#8-model-provider)
9. [Voice](#9-voice)
10. [Memory](#10-memory)
11. [Access: localhost, Tailscale and phones](#11-access-localhost-tailscale-and-phones)
12. [Bot-computer isolation](#12-bot-computer-isolation)
13. [Integrations (Composio)](#13-integrations-composio)
14. [`rakazo-stack update`](#14-rakazo-stack-update)
15. [`rakazo-stack backup`](#15-rakazo-stack-backup)
16. [`rakazo-stack restore`](#16-rakazo-stack-restore)
17. [Running the Instance day to day](#17-running-the-instance-day-to-day)
18. [Distribution of the program](#18-distribution-of-the-program)
19. [Testing and CI](#19-testing-and-ci)
20. [Known limitations to document](#20-known-limitations-to-document)
21. [Left to the implementation](#21-left-to-the-implementation)

## 1. Scope

### What v1 is

- One Self-hoster, one Instance, one machine ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- One program, `rakazo-stack`, with the subcommands `setup`, `update`, `backup` and `restore`, installed by a `curl … | sh` one-liner ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#30](https://github.com/kamui/rakazo-stack/issues/30), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- It installs the newest stable upstream releases of its Components, unmodified, and moves them only when the Self-hoster runs `rakazo-stack update` ([#32](https://github.com/kamui/rakazo-stack/issues/32)). See [ADR 0002](adr/0002-latest-stable-not-pinned.md).
- It never forks or patches an upstream project. The stack's own code is the program, a compose layer file, the Lemonade relay and the computer-egress sidecar ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- No outside account is required except the Model provider the Self-hoster brings. Composio and Tailscale are optional ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#3](https://github.com/kamui/rakazo-stack/issues/3), [#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Voice runs on the machine through Lemonade. Phone calls are on (`PHONE_CALLS_ENABLED=true`) ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#6](https://github.com/kamui/rakazo-stack/issues/6)).
- Memory is Hindsight, on only when the Self-hoster gives a Model provider key ([#44](https://github.com/kamui/rakazo-stack/issues/44)). See [ADR 0001](adr/0001-hindsight-for-v1-memory.md).
- Lifecycle covered: fresh install, re-runs of setup, Update, Backup and restore ([#1](https://github.com/kamui/rakazo-stack/issues/1)).

### Non-goals for v1

From the map's "Out of scope" list ([#1](https://github.com/kamui/rakazo-stack/issues/1)):

- A multi-tenant or hosted service.
- Migrating an existing hand-built Rakazo install into rakazo-stack.
- Pipedream integrations (planned after v1).
- An out-of-the-box local LLM mode.
- GPU acceleration on Linux or Windows.
- Intel Macs, and Linux on ARM (Lemonade's container image is x86-64 only).
- Podman: Rakazo's bot computers need engine API ≥ 1.45, and Podman reports 1.44 at best ([#4](https://github.com/kamui/rakazo-stack/issues/4)).
- Officially supported alternative engines: Docker Engine inside WSL 2, Colima, OrbStack ([#18](https://github.com/kamui/rakazo-stack/issues/18)).
- An always-on mode that runs without a signed-in user on Windows or macOS.
- A plain-LAN access mode: browsers block the microphone on non-HTTPS origins ([#6](https://github.com/kamui/rakazo-stack/issues/6), [#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Setup-managed public exposure (own domain, certificates, Tailscale Funnel). The unsupported bring-your-own Instance URL option exists instead ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Paid voice providers (ElevenLabs, Fish Audio and others) out of the box.
- Verifying Rakazo's Android app on a real phone: Android is documented as untested ([#28](https://github.com/kamui/rakazo-stack/issues/28)).
- Alpine/musl Linux ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Honcho as a memory option (planned after v1) ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- Scheduled or automatic Backups. v1 backs up only when run; the docs show a cron example ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- An uninstall command. Removal is documented as manual steps ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
- `rakazo-stack start|stop|status|logs` commands. The Self-hoster uses `docker compose` directly ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- A `rakazo-stack memory connect` command ([#49](https://github.com/kamui/rakazo-stack/issues/49)).

## 2. Supported platforms

| Platform | Supported | Notes |
|---|---|---|
| Linux, x86-64, glibc | Yes | Docker Engine with the Compose plugin from Docker's own repository. Voice on the CPU ([#18](https://github.com/kamui/rakazo-stack/issues/18), [#22](https://github.com/kamui/rakazo-stack/issues/22)). |
| Windows 11 23H2+ or Windows 10 22H2, through WSL 2 (2.1.5+) | Yes | Docker Desktop with the WSL 2 backend. The program runs inside a glibc WSL distro and drives Windows' `docker.exe` ([#18](https://github.com/kamui/rakazo-stack/issues/18), [#20](https://github.com/kamui/rakazo-stack/issues/20), [#41](https://github.com/kamui/rakazo-stack/issues/41)). Voice on the CPU. |
| Windows Home | Yes, with a warning | Works through WSL 2, but Docker no longer lists Home as supported. Preflight warns and continues ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#20](https://github.com/kamui/rakazo-stack/issues/20)). |
| macOS on Apple silicon (M1 or later), macOS 13.0+ | Yes | Docker Desktop. Lemonade runs natively, Whisper on Metal, Kokoro on the CPU ([#5](https://github.com/kamui/rakazo-stack/issues/5), [#16](https://github.com/kamui/rakazo-stack/issues/16), [#22](https://github.com/kamui/rakazo-stack/issues/22)). |
| Intel Mac, Linux on ARM, Alpine/musl | No | ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#35](https://github.com/kamui/rakazo-stack/issues/35)) |
| Docker Desktop for Linux | No | Own VM, needs a GUI session, no restricted egress ([#18](https://github.com/kamui/rakazo-stack/issues/18)). |
| Podman, OrbStack, Colima, Engine inside WSL 2 | No | ([#4](https://github.com/kamui/rakazo-stack/issues/4), [#18](https://github.com/kamui/rakazo-stack/issues/18)) |

On Windows and macOS the Instance runs only while the Self-hoster is signed in, because Docker Desktop stops at sign-out. This is documented, not worked around ([#1](https://github.com/kamui/rakazo-stack/issues/1)). Setup doesn't check whether Docker Desktop starts at sign-in; the README says that if it doesn't, the Instance stays down until Docker Desktop is opened ([#1](https://github.com/kamui/rakazo-stack/issues/1)).

## 3. Requirements and preflight

### Requirements

- **Docker** ([#4](https://github.com/kamui/rakazo-stack/issues/4), [#18](https://github.com/kamui/rakazo-stack/issues/18)):
  - Linux: Docker Engine and the Compose plugin, installed from Docker's apt/dnf repository (`get.docker.com` is the shortcut to the same repository). Distro packages are fine if they pass the version checks.
  - Windows and macOS: Docker Desktop. Docker Desktop's WSL integration is optional on Windows ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
  - Minimum Docker Engine API 1.45 (Engine 26+) and Docker Compose 2.24.4.
  - Linux: the Self-hoster's user runs `docker` without `sudo`, through Docker's `docker` group. Setup never runs `sudo` ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **Docker Desktop licence**: free for personal use, education, non-commercial open source, and businesses with fewer than 250 employees and under $10M revenue. Others, and all government entities, need a paid subscription. Docker Engine is free everywhere. The docs say this ([#4](https://github.com/kamui/rakazo-stack/issues/4), [#18](https://github.com/kamui/rakazo-stack/issues/18)).
- **A Model provider key** (optional): OpenRouter, OpenAI, Gemini (billing-enabled key) or Anthropic ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Optional:** a Composio project API key ([#3](https://github.com/kamui/rakazo-stack/issues/3)) and a Tailscale account ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
- **Hardware** ([#42](https://github.com/kamui/rakazo-stack/issues/42), [#46](https://github.com/kamui/rakazo-stack/issues/46)):

  | | Minimum | Recommended |
  |---|---|---|
  | CPU | 4 threads | 8 threads |
  | RAM | 16 GB on every OS | |
  | Free disk where Docker stores its data | 35 GB | 50 GB |

  - A fresh install uses about 15.6–15.7 GB: about 9 GB of Rakazo images, 6.4 GB for `hindsight-api`, about 2 GB of voice models and 0.2 GB of volumes. An Update peaks near 21 GB when only Rakazo moves, and near 27.5 GB when Hindsight moves too, because old images stay until the Update succeeds ([#46](https://github.com/kamui/rakazo-stack/issues/46)).
  - The whole Instance (memory, Tailscale and one idle bot computer on) uses 5.2–5.8 GiB of RAM, and about 7.1–7.7 GiB with a bot computer at its 2 GiB cap ([#46](https://github.com/kamui/rakazo-stack/issues/46)).
  - On Windows, WSL gives Docker Desktop's VM half of the host's RAM by default, which is why 16 GB is the Windows minimum ([#42](https://github.com/kamui/rakazo-stack/issues/42)).
  - On macOS, Docker Desktop's memory limit is left at its default. Setup and the docs don't suggest lowering it ([#46](https://github.com/kamui/rakazo-stack/issues/46)).

### Preflight

Every `setup`, `update` and `restore` run starts with preflight; `backup` takes only the lock ([§15](#15-rakazo-stack-backup)). A **stop** ends the run before anything on the machine changes. A **warn** prints a warning and carries on. Nothing in preflight is fixed automatically.

| Check | Result | Ticket |
|---|---|---|
| Another `rakazo-stack` command holds the lock in the Instance folder (every command creates the folder to hold it; see [§16](#16-rakazo-stack-restore) for `restore`) | stop: "another rakazo-stack command is running" | [#32](https://github.com/kamui/rakazo-stack/issues/32) |
| WSL: the home directory is under `/mnt/` | stop | [#29](https://github.com/kamui/rakazo-stack/issues/29) |
| Docker server API < 1.45, or `docker compose version` < 2.24.4 | stop, with install instructions | [#18](https://github.com/kamui/rakazo-stack/issues/18) |
| Linux: Docker Desktop for Linux is the active engine | stop | [#9, #18] |
| Linux: `docker` doesn't work without `sudo` | stop, with instructions; when it works, warn that the `docker` group is root-equivalent | [#1](https://github.com/kamui/rakazo-stack/issues/1) |
| Windows: WSL interop off, or no `docker.exe` found | stop, with instructions | [#41](https://github.com/kamui/rakazo-stack/issues/41) |
| Windows: Docker Desktop not in Linux containers mode | stop | [#41](https://github.com/kamui/rakazo-stack/issues/41) |
| Windows Home edition | warn | [#9, #20] |
| `docker info` reports firewall backend `nftables` | warn; checklist says bot computers won't start until `firewall-backend` is removed from `daemon.json` and Docker is restarted | [#40](https://github.com/kamui/rakazo-stack/issues/40) |
| Fewer than 4 CPU threads, less than 16 GB host RAM (under 15 GiB reported, since a 16 GB machine reports a little less), or less than 35 GB free where Docker stores its data. Docker Engine: its `DockerRootDir`. Docker Desktop keeps its data in a disk image, so the host disk holding it stands in: `/mnt/c` on Windows, the home volume on macOS. Windows host RAM comes from Windows, not WSL | warn | [#42, #46] |
| Windows: Docker Desktop's VM memory (`docker info` `MemTotal`) is below 7.5 GiB (a 16 GB host gives it about 7.8 GiB; a busy Instance needs 7.1–7.6 GiB) | warn | [#42](https://github.com/kamui/rakazo-stack/issues/42), [#46](https://github.com/kamui/rakazo-stack/issues/46) |
| Rakazo's web port is already taken: a container other than the Instance's own `web` publishes it, or something listens on `127.0.0.1:5173` (on WSL, also a Windows listener in `netstat.exe`) | stop | [#29](https://github.com/kamui/rakazo-stack/issues/29) |
| macOS: lemond's stack port (`127.0.0.1:13315`) is taken by anything but the Instance's own lemond (the LaunchAgent's process, from `launchctl print`; listeners from `lsof`) | stop, naming what uses it | [#17](https://github.com/kamui/rakazo-stack/issues/17), [#70](https://github.com/kamui/rakazo-stack/issues/70) |
| Recorded Component versions are below the program's floor | stop: "run `rakazo-stack update`" (`setup` only) | [#32](https://github.com/kamui/rakazo-stack/issues/32) |
| GitHub unreachable | stop on a first `setup`, on `update` and on `restore`, which fetches the compose file of the Backup's Rakazo release; re-runs of `setup` don't need GitHub | [#32](https://github.com/kamui/rakazo-stack/issues/32), [#72](https://github.com/kamui/rakazo-stack/issues/72) |

`update` adds a free-disk check sized to the images it will pull, once it knows which: each new image not on the machine counts as big as the image of the same repository the Instance runs now, plus 2 GB left for the running Instance. Too little stops the run ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#73](https://github.com/kamui/rakazo-stack/issues/73)). Setup checks free disk again before it pulls voice models, and only when a model is still missing: below 3 GB free, it skips voice and adds a checklist item ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#64](https://github.com/kamui/rakazo-stack/issues/64)).

On Windows every Docker check runs against `docker.exe` (see [Windows](#windows-docker-through-dockerexe)).

## 4. Components and versions

### Components

| Component | What runs | Source | On |
|---|---|---|---|
| Rakazo | `postgres`, `data-init`, `migrate`, `supervisor`, `api`, `worker`, `web`, and bot computers | `ghcr.io/elie222/rakazo/app` and `ghcr.io/elie222/rakazo/computer`, Rakazo's own `infra/compose/docker-compose.images.yml` | Always |
| Lemonade | Speech (`kokoro-v1`) and transcription (Whisper) | Linux/Windows: `ghcr.io/lemonade-sdk/lemonade-server`, unmodified. macOS: the `lemonade-embeddable-<ver>-macos-arm64.tar.gz` release asset | Always ([#5](https://github.com/kamui/rakazo-stack/issues/5), [#17](https://github.com/kamui/rakazo-stack/issues/17)) |
| Lemonade relay (`lemonade-relay`) | Converts uploads to WAV, fixes `lang_code`, adds Lemonade's key | Built locally from a Bun Alpine base fixed in the program | Always ([#17](https://github.com/kamui/rakazo-stack/issues/17)) |
| Hindsight | `hindsight` and `hindsight-db` | `ghcr.io/vectorize-io/hindsight-api`, `pgvector/pgvector:pg18` | Only with a Model provider key ([#44](https://github.com/kamui/rakazo-stack/issues/44)) |
| Tailscale | `tailscale` | `tailscale/tailscale` | Only when chosen ([#8](https://github.com/kamui/rakazo-stack/issues/8)) |
| Computer-egress sidecar (`computer-egress`) | Firewall rules for bot computers | Built locally from an Alpine base fixed in the program, with Rakazo's `restrict-computer-egress.sh` built in | Always ([#40](https://github.com/kamui/rakazo-stack/issues/40)) |

### How versions are resolved

rakazo-stack installs the latest stable release of each Component, best effort, rather than a tested set ([#32](https://github.com/kamui/rakazo-stack/issues/32)). See [ADR 0002](adr/0002-latest-stable-not-pinned.md).

- **Rakazo:** GitHub's "latest release" of `elie222/rakazo` (prereleases skipped). Setup fetches `infra/compose/docker-compose.images.yml` and `infra/compose/restrict-computer-egress.sh` from that same tag, unmodified ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
  - **Releases without images.** If the tag has no `app` or no `computer` image on GHCR, setup and update walk back to the newest release that has both, and say which release they skipped and why. A later `update` picks up the skipped release once its images appear. The program never builds Rakazo images locally ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#45](https://github.com/kamui/rakazo-stack/issues/45)). (Rakazo v0.16.0 was tagged a day before its images were published.)
  - The same rule covers every Component resolved from GitHub releases: a Hindsight or Lemonade release whose image (or, on macOS, whose embeddable tarball) isn't published yet is skipped the same way ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
  - Rakazo's Postgres is whatever its compose file uses (today `postgres:16`, pinned by digest upstream) ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Hindsight:** GitHub's latest release of `vectorize-io/hindsight`. Release `vX.Y.Z` maps to image tag `X.Y.Z` (Hindsight's tags have no `v`) ([#44](https://github.com/kamui/rakazo-stack/issues/44)). Tested on 1.3.0. Resolved only when memory runs ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- **`hindsight-db`:** `pgvector/pgvector:pg18`, from Docker Hub. It floats within PostgreSQL 18 (minors and security fixes), never to a new major ([#32](https://github.com/kamui/rakazo-stack/issues/32)). Its digest is read when Hindsight is resolved and recorded with it.
- **Lemonade:** GitHub's latest release of `lemonade-sdk/lemonade`. Linux/Windows use the image of that tag (release `vX.Y.Z` is image tag `vX.Y.Z`; tested on v2026.48.3, [#64](https://github.com/kamui/rakazo-stack/issues/64)); macOS downloads the embeddable tarball and checks it against GitHub's published SHA-256 for that asset ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Tailscale:** the version behind the `stable` tag in the image registry, not GitHub releases, because images lag releases ([#24](https://github.com/kamui/rakazo-stack/issues/24)). On Docker Hub, `tailscale/tailscale:stable` gives the digest, and its image index's `org.opencontainers.image.version` annotation gives the version: `1.104.7` is recorded as `v1.104.7` ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- **Relay and sidecar bases:** fixed in each program release (for example `oven/bun:1.4.4-alpine` and `alpine:3.24.4`). The relay's ffmpeg comes from Alpine and is pinned loosely (`~=8.1`), because Alpine deletes old package versions ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Recording.** `settings.json` records each Component's running version and image digest ([#32](https://github.com/kamui/rakazo-stack/issues/32)). Because no upstream tag stays put, the stack runs images by recorded digest (for example `RAKAZO_IMAGE_TAG=v0.15.0@sha256:…`), never by a moving tag ([#31](https://github.com/kamui/rakazo-stack/issues/31)). On macOS Lemonade's recorded digest is the embeddable tarball's SHA-256, so a re-run that has to download it again checks it against the recorded value ([#70](https://github.com/kamui/rakazo-stack/issues/70)).
- **Lemonade models** change only with Lemonade's version: Lemonade runs with `auto_check_model_updates: false` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **GitHub's API limit.** Without a token GitHub allows 60 API requests an hour per address. When `GITHUB_TOKEN`, or else `GH_TOKEN`, is set, its value is sent with every request to `api.github.com` and nowhere else, and is never printed. A refused token stops the run and names the variable.
- **Never enabled:** Rakazo's `updater` sidecar. Nothing updates on its own ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Who moves versions

- The first `setup` resolves and records the latest versions ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Re-runs of `setup` keep the recorded versions. The one exception: a Component switched on later (memory, Tailscale) starts at its latest version ([#32](https://github.com/kamui/rakazo-stack/issues/32)). Its record stays when it is switched off, so switching it on again runs the version that last migrated its data ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- Only `rakazo-stack update` moves recorded versions ([§14](#14-rakazo-stack-update)).
- `restore` brings back the versions recorded in the Backup ([§16](#16-rakazo-stack-restore)).

### Floors and Holds

- Each program release knows the lowest version of each Component it can drive. "Latest" has no ceiling ([#32](https://github.com/kamui/rakazo-stack/issues/32)). v1's floors are **Rakazo v0.16.0**, where setup's memory connect call was proven ([#45](https://github.com/kamui/rakazo-stack/issues/45)), and **Hindsight 1.3.0**, the version tested ([#1](https://github.com/kamui/rakazo-stack/issues/1)). If no release at or above the floor has published images, setup stops and says so.
- `rakazo-stack update --hold <component>=<version>` keeps a Component at that version; `--unhold <component>` releases it. Holds live in `settings.json` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
  - The components are `rakazo`, `hindsight`, `lemonade` and `tailscale`, and a version is `X.Y.Z`, with or without its `v` ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
  - A Hold at a newer version than the recorded one moves the Component there and keeps it there. That version must be a stable release with its images published, checked as in [How versions are resolved](#how-versions-are-resolved) ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
  - A Hold below the recorded version is refused: `update` never moves a Component back, because its data has already migrated forward. An older Backup is the way back ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
- A Hold below the floor is refused. If a newer program raises the floor above an existing Hold, `update` moves that Component to latest, removes the Hold and says so in its confirmation; with `--yes` it stops instead, so a Hold is never dropped silently ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

## 5. Compose layout

### Shape

- **One Compose project** named `rakazo-stack` (`COMPOSE_PROJECT_NAME=rakazo-stack` in `.env`; only tests change it, [§19](#test-instances)). Without it, upstream's `name: rakazo` would let a hand-built Rakazo on the same machine share the Instance's volumes ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- **Two compose files**, both in the Instance folder ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#32](https://github.com/kamui/rakazo-stack/issues/32)):
  - `rakazo.<tag>.images.yml`: Rakazo's `infra/compose/docker-compose.images.yml` from the recorded tag, byte for byte.
  - `compose.stack.yaml`: rakazo-stack's layer, bundled in the program.
- Setup writes `COMPOSE_FILE=rakazo.<tag>.images.yml:compose.stack.yaml`, `COMPOSE_PATH_SEPARATOR=:` and `COMPOSE_PROFILES` to `.env`, so plain `docker compose up -d` works from the Instance folder ([#10](https://github.com/kamui/rakazo-stack/issues/10)). `COMPOSE_PROFILES` lists the profiles that are on, and is empty when none is ([#68](https://github.com/kamui/rakazo-stack/issues/68)). The separator is spelled out because Windows' `docker.exe` otherwise splits `COMPOSE_FILE` on `;`. Compose takes a shell variable over the same name in `.env`, so setup runs Compose without the names `.env` sets, the names the compose files interpolate and any `COMPOSE_*` setting. A Self-hoster who runs `docker compose` by hand with such a variable exported gets the shell's value; the docs say so.
- **Upstream's file is never edited.** Everything the stack changes goes through `.env` (which upstream's file already passes through) or the layer file (`!reset`, extra networks, extra services) ([#10](https://github.com/kamui/rakazo-stack/issues/10)).

### Profiles

| Profile | Services | On when | Ticket |
|---|---|---|---|
| (none) | Rakazo's services, `lemonade-relay`, `computer-egress` | Always | [#10, #40] |
| `lemonade` | `lemonade` | Linux and Windows. On macOS Lemonade runs natively | [#10](https://github.com/kamui/rakazo-stack/issues/10) |
| `hindsight` | `hindsight`, `hindsight-db` | A Model provider key is set | [#44](https://github.com/kamui/rakazo-stack/issues/44) |
| `tailscale` | `tailscale` | Tailscale access is chosen | [#8, #10] |

### Services the layer adds or changes

| Service | Image | Networks | Key settings | Ticket |
|---|---|---|---|---|
| `api` | upstream | upstream's + `stack` | `ports: !reset []`: the API port (3100) is not published; the web port proxies `/api` and `/rpc` | [#10, #24] |
| `worker` | upstream | upstream's + `stack` | | [#10](https://github.com/kamui/rakazo-stack/issues/10) |
| `lemonade` | `ghcr.io/lemonade-sdk/lemonade-server:<tag>@<digest>`, unmodified | `stack`, `egress` | `LEMONADE_API_KEY`; volumes `lemonade-models:/opt/lemonade/.cache/huggingface`, `lemonade-llama:/opt/lemonade/llama`, `lemonade-data:/opt/lemonade/.cache/lemonade`, `lemonade-config:/opt/lemonade/.config/lemonade`; healthcheck on `/live` that starts checking every 2 s, because the image's own waits 30 s; not published | [#10, #17, #64] |
| `lemonade-relay` | built locally as `<project>-lemonade-relay`, Compose's own name: `rakazo-stack-lemonade-relay` ([§9](#the-lemonade-relay)) | `stack`, `egress` | listens on `13306`; `UPSTREAM=${LEMONADE_UPSTREAM:-http://lemonade:13305}`; `LEMONADE_API_KEY`; not published | [#10, #17, #64] |
| `hindsight` | `ghcr.io/vectorize-io/hindsight-api:<X.Y.Z>@<digest>` (the full image, not `-slim`) | `stack`, `hindsight-data`, `egress` | listens on `8888`; `stop_grace_period: 75s`; healthcheck on `/health`; env in [§10](#hindsight-settings); not published | [#44, #45, #47] |
| `hindsight-db` | `pgvector/pgvector:pg18@<digest>` | `hindsight-data` | `shm_size: 1g`; `POSTGRES_USER=hindsight`, `POSTGRES_DB=hindsight`, generated password; volume `hindsight-pgdata:/var/lib/postgresql`; healthcheck `pg_isready` | [#44, #48] |
| `tailscale` | `tailscale/tailscale:<version>@<digest>` | `app` | `TS_USERSPACE=true`, `TS_AUTH_ONCE=true`, `TS_STATE_DIR=/var/lib/tailscale`, `TS_HOSTNAME`, `TS_AUTHKEY`, `TS_SERVE_CONFIG=/config/serve.json`; volume `tailscale-state`; not published. Upstream's compose file has its own opt-in `tailscale-ingress` service, with the variable `TAILSCALE_IMAGE`; the stack enables neither and uses other names | [#8, #10, #24, #68] |
| `supervisor` | upstream | upstream's | `depends_on: computer-egress` (`service_started`), so on `up` its start-up probe runs after the sidecar starts. Not `service_healthy`: with the rules missing the Instance still starts and computers fail closed | [#40](https://github.com/kamui/rakazo-stack/issues/40) |
| `computer-egress` | built locally as `rakazo-stack-computer-egress` ([§12](#the-computer-egress-sidecar)) | `network_mode: host` | `cap_add: [NET_ADMIN]`, `restart: unless-stopped` | [#40](https://github.com/kamui/rakazo-stack/issues/40) |

All long-running services use `restart: unless-stopped`.

**No Instance-folder bind mounts** in long-running services, on any OS ([#41](https://github.com/kamui/rakazo-stack/issues/41)):

- The relay's code is copied into its image.
- The Tailscale Serve config is an inline `configs: content:` in `compose.stack.yaml`. Its `${TS_CERT_DOMAIN}` is written `$${TS_CERT_DOMAIN}` so Compose leaves it for Tailscale:

  ```json
  {
    "TCP": { "443": { "HTTPS": true } },
    "Web": { "${TS_CERT_DOMAIN}:443": { "Handlers": { "/": { "Proxy": "http://web:5173" } } } }
  }
  ```

- `hindsight-db` needs no `init.sql`: Hindsight creates the `vector` extension itself as the database owner ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- Upstream's own `/var/run/docker.sock` mount on `supervisor` stays, because upstream's file is used unmodified.
- Short-lived containers that `setup`, `backup` or `restore` run may bind-mount files.

### Networks

| Network | Internal | Members | Purpose |
|---|---|---|---|
| `app` | no (upstream) | Rakazo's services, `tailscale` | Rakazo's own |
| `data` | yes (upstream) | `postgres` and Rakazo's services | Rakazo's database |
| `stack` | yes | `api`, `worker`, `lemonade-relay`, `lemonade`, `hindsight` | How Rakazo reaches voice and memory, by service name ([#10](https://github.com/kamui/rakazo-stack/issues/10)) |
| `hindsight-data` | yes | `hindsight`, `hindsight-db` | Keeps the database off `stack` ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#45](https://github.com/kamui/rakazo-stack/issues/45)) |
| `egress` | no | `lemonade` (model downloads), `lemonade-relay` (reaches macOS host loopback), `hindsight` (Model provider API) | Outbound access ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#45](https://github.com/kamui/rakazo-stack/issues/45)) |
| per-bot bridges (`rakazo-c…`) | created by `supervisor` | one bot computer, plus `supervisor` and `web` when a screen is open | Rakazo's own isolation |

Bot computers never join `stack`, `hindsight-data` or `data`, and the computer-egress sidecar blocks them from the host, the LAN and the tailnet ([§12](#12-bot-computer-isolation)) ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#40](https://github.com/kamui/rakazo-stack/issues/40)).

### Published ports

Only Rakazo's `web`, on `127.0.0.1:5173` (upstream's `RAKAZO_WEB_BIND` and `RAKAZO_WEB_PORT` defaults). Nothing the stack adds is published ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#24](https://github.com/kamui/rakazo-stack/issues/24)). Tailscale serves through its own userspace networking, not a published port ([#24](https://github.com/kamui/rakazo-stack/issues/24)).

### Volumes

Rakazo's `pgdata`, `appdata` and `webassets` (upstream), plus `lemonade-models`, `lemonade-llama`, `lemonade-data`, `lemonade-config` (Linux/Windows), `hindsight-pgdata` and `tailscale-state`. With the project name they appear as `rakazo-stack_<name>`. All Instance data lives in named volumes; nothing is bind-mounted from the Instance folder ([#29](https://github.com/kamui/rakazo-stack/issues/29)).

### Rakazo settings setup writes to `.env`

| Variable | Value | Ticket |
|---|---|---|
| `COMPOSE_PROJECT_NAME` | `rakazo-stack` | [#29](https://github.com/kamui/rakazo-stack/issues/29) |
| `COMPOSE_FILE`, `COMPOSE_PATH_SEPARATOR`, `COMPOSE_PROFILES` | see above | [#10](https://github.com/kamui/rakazo-stack/issues/10) |
| `RAKAZO_IMAGE_TAG`, `RAKAZO_COMPUTER_IMAGE_TAG` | recorded tag and digest | [#2, #32] |
| `POSTGRES_PASSWORD`, `POSTGRES_APP_PASSWORD`, `BETTER_AUTH_SECRET`, `ENCRYPTION_KEY`, `SCREEN_PROXY_SECRET`, `SANDBOX_SUPERVISOR_TOKEN` | generated ([§6](#generated-secrets)) | [#2](https://github.com/kamui/rakazo-stack/issues/2) |
| `SIGNUPS_ENABLED` | `false` (the API re-applies it from env on every start, so setup owns it) | [#2, #21] |
| `PHONE_CALLS_ENABLED` | `true` | [#1, #6] |
| `MCP_ALLOW_PRIVATE_ENDPOINT` | `true` (lets Rakazo reach `lemonade-relay` and `hindsight`; broader than voice) | [#6, #10] |
| `HINDSIGHT_IMAGE_TAG`, `HINDSIGHT_DB_IMAGE_TAG` | memory on only: recorded tag and digest (`1.3.0@sha256:…`, `pg18@sha256:…`) | [#66](https://github.com/kamui/rakazo-stack/issues/66) |
| `HINDSIGHT_DB_PASSWORD`, `HINDSIGHT_API_TENANT_API_KEY` | memory on only: generated ([§6](#generated-secrets)) | [#44, #66] |
| `HINDSIGHT_API_LLM_PROVIDER`, `HINDSIGHT_API_LLM_API_KEY`, `HINDSIGHT_API_LLM_MODEL`, and on OpenRouter `HINDSIGHT_API_LLM_REASONING_EFFORT` and `HINDSIGHT_API_LLM_EXTRA_BODY` | memory on only: per provider ([§8](#8-model-provider)). The layer passes them to `hindsight` alone, so Rakazo's services never see them; the JSON is single-quoted | [#44, #66] |
| `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS` | `lemonade-relay` | [#10](https://github.com/kamui/rakazo-stack/issues/10) |
| `SANDBOX_COMPUTER_EGRESS` | `restricted` (never `auto`, never `open`) | [#25, #40] |
| `BETTER_AUTH_URL`, `WEB_ORIGIN`, `API_URL`, `RAKAZO_HOST` | from the access mode ([§11](#11-access-localhost-tailscale-and-phones)) | [#8, #24] |
| `TAILSCALE_IMAGE_TAG`, `TS_HOSTNAME` | Tailscale on: the recorded tag and digest, and the device name | ([#68](https://github.com/kamui/rakazo-stack/issues/68)) |
| `TS_AUTHKEY` | Tailscale on, only until the Instance has joined ([§11](#tailscale)) | ([#68](https://github.com/kamui/rakazo-stack/issues/68)) |
| `OPENROUTER_API_KEY` or `ANTHROPIC_API_KEY`, `PI_DEFAULT_PROVIDER`, `PI_DEFAULT_MODEL` | OpenRouter or Anthropic only ([§8](#8-model-provider)) | [#36](https://github.com/kamui/rakazo-stack/issues/36) |
| `COMPOSIO_API_KEY` | the pasted key, or blank | [#3](https://github.com/kamui/rakazo-stack/issues/3) |
| `LEMONADE_UPSTREAM` | macOS only: `http://host.docker.internal:13315` | [#17](https://github.com/kamui/rakazo-stack/issues/17), [#70](https://github.com/kamui/rakazo-stack/issues/70) |

Never set: `RAKAZO_MIN_CLIENT_BUILD` ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)), `OPENAI_API_KEY` or `GEMINI_API_KEY` for Rakazo ([#36](https://github.com/kamui/rakazo-stack/issues/36)), and `EXPO_ACCESS_TOKEN` (it can't help a self-hosted server) ([#14](https://github.com/kamui/rakazo-stack/issues/14), [#19](https://github.com/kamui/rakazo-stack/issues/19)). Everything else in upstream's env (SMTP, Sentry, S3, remote sandboxes and so on) stays at its default, which is off ([#2](https://github.com/kamui/rakazo-stack/issues/2)).

## 6. Files on disk

### Instance folder

Fixed at `~/rakazo-stack` on every OS, with no option to move it. On Windows that is the Linux home inside the WSL distro, not the C: drive ([#29](https://github.com/kamui/rakazo-stack/issues/29)).

```
~/rakazo-stack/                    mode 700
  settings.json                    the Self-hoster's choices and recorded state, non-secret
  secrets.env                      generated secrets and pasted keys (mode 600), source of truth
  .env                             generated for Compose from the two files above (mode 600)
  rakazo.<tag>.images.yml          Rakazo's compose file, unmodified
  compose.stack.yaml               rakazo-stack's layer
  relay/                           build context for lemonade-relay
  computer-egress/                 build context for the sidecar: Dockerfile, the wrapper, and Rakazo's restrict-computer-egress.sh
  lemonade/                        macOS only: lemond's working folder (config.json, backends), versions/<v>/, models/ (HF_HOME)
  logs/                            setup run logs, one `setup-<UTC time>.log` per run (no secrets); `lemond.log` on macOS
  .lock                            the lock setup, update, backup and restore take (an OS file lock, released if the program dies)
  .update-check                    when setup or backup last checked for updates, in Unix seconds
  .update.env                      only while update fetches: the new versions' image lines
```

- Setup re-applies the modes on every run ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- Setup rewrites `.env`, both compose files and the build folders on every run. `.env` starts with a "generated, edits are overwritten" line. The docs say not to edit these files ([#29](https://github.com/kamui/rakazo-stack/issues/29), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Compose files sit beside `.env` because Compose reads `.env` and resolves relative paths from the project folder ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- The only data inside the folder is native lemond's files on macOS (about 2 GB of models) ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- Outside the folder: the macOS LaunchAgent plist, `~/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist` (mode 600) ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#70](https://github.com/kamui/rakazo-stack/issues/70)), Backups in `~/rakazo-stack-backups/` ([#30](https://github.com/kamui/rakazo-stack/issues/30)), and the program in `~/.local/bin/rakazo-stack` ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- The lock that `setup`, `update`, `backup` and `restore` take, and the timestamp of the last update check (`.update-check`), also live in the Instance folder ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#73](https://github.com/kamui/rakazo-stack/issues/73)).

### `settings.json`

JSON, rewritten by setup on every run ([#29](https://github.com/kamui/rakazo-stack/issues/29)). It holds:

- The Self-hoster's choices: Model provider (no key), access mode (`access`: `localhost`, `tailscale` or `bring-your-own`), Tailscale device name, bring-your-own Instance URL, the Owner's email, whether memory is on ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Optional overrides: one Rakazo model and one memory model. Only overrides are stored; defaults belong to the program version ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#51](https://github.com/kamui/rakazo-stack/issues/51)).
- Recorded versions and image digests for each Component, and the program version that wrote it ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Once the Instance has joined a tailnet, the `ts.net` name Tailscale assigned (`tailscale_dns_name`). Without it, joining needs an auth key ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- Holds, as `holds`: each held component's version, for example `"holds": { "lemonade": "v2026.48.3" }`, absent when there are none ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#73](https://github.com/kamui/rakazo-stack/issues/73)).
- Which database-only settings setup filled in (the Owner's voice, the Owner's model connection, the default Space's memory), so a re-run can tell "removed in Rakazo" from "never set" ([#23](https://github.com/kamui/rakazo-stack/issues/23), [#36](https://github.com/kamui/rakazo-stack/issues/36)).
- A failed Update, if the last one was rolled back ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#74](https://github.com/kamui/rakazo-stack/issues/74)).

The schema so far; later slices add the fields above as they land. A bring-your-own Instance URL is `bring_your_own_url`. Switching to another access mode keeps the other modes' values, to pre-fill a switch back:

```json
{
  "schema": 1,
  "program_version": "0.1.0",
  "owner_email": "owner@example.com",
  "model_provider": "openrouter",
  "memory": "on",
  "filled_by_setup": ["owner", "voice", "memory"],
  "access": "tailscale",
  "tailscale_device_name": "rakazo",
  "tailscale_dns_name": "rakazo.example.ts.net",
  "components": {
    "rakazo": {
      "version": "v0.16.0",
      "images": { "app": "sha256:…", "computer": "sha256:…" }
    },
    "tailscale": { "version": "v1.104.7", "digest": "sha256:…" },
    "lemonade": { "version": "v2026.48.3", "digest": "sha256:…" },
    "hindsight": {
      "version": "1.3.0",
      "images": { "hindsight": "sha256:…", "hindsight-db": "sha256:…" }
    }
  }
}
```

- `model_provider` is the answer to the Model provider question: `openrouter`, `openai`, `gemini`, `anthropic` or `none`. It is recorded even when the key was left blank.
- `rakazo_model` is the Rakazo model override ([§8](#8-model-provider)). It is absent unless the Self-hoster adds it, and setup keeps it.
- `memory` is the answer to the Memory question, `on` or `off`. Like `model_provider`, it is kept while the key is blank, so adding a key later brings the choice back. Memory runs only when it is `on` and a key is set ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- `memory_model` is the memory model override ([§8](#8-model-provider)), under the same rules as `rakazo_model` ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- `components.hindsight` appears the first time memory runs: the image tag, and the digests of `hindsight` and `hindsight-db` ([§4](#how-versions-are-resolved)). It stays while memory is off ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- In `filled_by_setup`, the Owner's model connection is `model_connection`. It is dropped when the Model provider changes ([#67](https://github.com/kamui/rakazo-stack/issues/67)).
- `model_connection_due` is present while the Owner's OpenAI or Gemini connection still has to take the saved key: `new_provider` after a switch to that provider, otherwise `new_key` ([§7](#database-only-settings-in-rakazo), [#67](https://github.com/kamui/rakazo-stack/issues/67)).
- `model_providers_left` lists `openai` or `gemini` after a switch away, until a run finds no Owner connection to it or the Self-hoster switches back. Changing the Owner email clears it and `model_connection_due` ([#67](https://github.com/kamui/rakazo-stack/issues/67)).
- `failed_update` is written after a rollback: each Component the failed Update moved, with the version it moved to, and the UTC day it failed, for example `"failed_update": { "to": { "rakazo": "v0.17.0" }, "on": "2026-11-27" }`. The next Update's switch drops it ([§14](#failures), [#74](https://github.com/kamui/rakazo-stack/issues/74)).

### `secrets.env`

Holds every generated secret and every pasted key. It is the source of truth: `.env` is regenerated from it each run, and a Backup needs only `settings.json` and `secrets.env` from the folder ([#29](https://github.com/kamui/rakazo-stack/issues/29)). Protection is file permissions only, with no OS keychain. The docs say the Self-hoster's account and anyone in the `docker` group can read these secrets ([#29](https://github.com/kamui/rakazo-stack/issues/29)).

#### Generated secrets

Generated once, on the run that first needs them, and never regenerated ([#9](https://github.com/kamui/rakazo-stack/issues/9)).

| Secret | Used by | Ticket |
|---|---|---|
| `POSTGRES_PASSWORD`, `POSTGRES_APP_PASSWORD` | Rakazo's Postgres | [#2](https://github.com/kamui/rakazo-stack/issues/2) |
| `BETTER_AUTH_SECRET` | Rakazo sign-in | [#2](https://github.com/kamui/rakazo-stack/issues/2) |
| `ENCRYPTION_KEY` | Rakazo's content encryption. Must never change after first start | [#2](https://github.com/kamui/rakazo-stack/issues/2) |
| `SCREEN_PROXY_SECRET`, `SANDBOX_SUPERVISOR_TOKEN` | Rakazo's supervisor and screens. At least 32 characters, different from each other and from `BETTER_AUTH_SECRET` | [#2](https://github.com/kamui/rakazo-stack/issues/2) |
| `LEMONADE_API_KEY` | Lemonade, on every OS; only the relay sends it | [#10, #17] |
| `HINDSIGHT_DB_PASSWORD` | `hindsight-db` and `HINDSIGHT_API_DATABASE_URL`; hex, because it sits in a URL | [#44, #66] |
| `HINDSIGHT_API_TENANT_API_KEY` | Hindsight's API key, 8–4096 characters; also stored by Rakazo for each connected Space | [#44, #49] |

#### Pasted keys

The Model provider key, `COMPOSIO_API_KEY` and the Tailscale auth key ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#8](https://github.com/kamui/rakazo-stack/issues/8), [#36](https://github.com/kamui/rakazo-stack/issues/36)). A re-run shows a pasted key as "set, press Enter to keep, or paste a new one" and never prints it ([#9](https://github.com/kamui/rakazo-stack/issues/9)). In no-questions mode each pasted key comes from `RAKAZO_STACK_` followed by its name in `secrets.env`, for example `RAKAZO_STACK_COMPOSIO_API_KEY` ([#61](https://github.com/kamui/rakazo-stack/issues/61)).

- The Model provider key is `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY` or `ANTHROPIC_API_KEY`, after its provider. `secrets.env` holds only the chosen provider's: choosing another provider or `none` removes the old line ([#65](https://github.com/kamui/rakazo-stack/issues/65)).
- Surrounding spaces are dropped from a pasted key. A key with a space, quote, backslash, backtick, `$`, `#` or non-ASCII character is invalid input, because it would change the meaning of its `.env` line ([#65](https://github.com/kamui/rakazo-stack/issues/65)).
- The Composio key is `COMPOSIO_API_KEY`. While integrations are off, `secrets.env` has no such line ([#69](https://github.com/kamui/rakazo-stack/issues/69)).
- The Tailscale auth key is `TS_AUTHKEY`. It is the one pasted key that isn't kept: once the Instance has joined, setup removes it from `secrets.env` and `.env`, because `tailscale-state` holds the device's identity ([#68](https://github.com/kamui/rakazo-stack/issues/68)).

## 7. `rakazo-stack setup`

One re-runnable command. It is an interactive wizard on a first run and safe to re-run at any time to change things. A no-questions mode, `rakazo-stack setup --no-questions`, is for scripted runs and for `restore`'s hand-over ([#9](https://github.com/kamui/rakazo-stack/issues/9)). The wizard and the no-questions mode are two front ends to one set of answers ([#61](https://github.com/kamui/rakazo-stack/issues/61)):

- **No-questions mode** takes each answer from its environment variable, then from `settings.json`, then the wizard's default. An answer with no default stops the run when neither gives it. So a first scripted run, with no `settings.json` yet, gives its choices as environment variables, and a scripted re-run can change a choice the same way.
- **Environment variables** are `RAKAZO_STACK_` followed by the answer's name: `RAKAZO_STACK_OWNER_EMAIL`, `RAKAZO_STACK_OWNER_PASSWORD`, `RAKAZO_STACK_MODEL_PROVIDER`, `RAKAZO_STACK_MEMORY` (`on` or `off`), and each pasted key under its `secrets.env` name ([§6](#pasted-keys)). Of the Model provider keys, only the chosen provider's variable is read. The access questions are `RAKAZO_STACK_ACCESS` (`localhost`, `tailscale` or `bring-your-own`), `RAKAZO_STACK_TAILSCALE_DEVICE_NAME`, `RAKAZO_STACK_TS_AUTHKEY` and `RAKAZO_STACK_INSTANCE_URL` ([#68](https://github.com/kamui/rakazo-stack/issues/68)). Later questions add theirs under the same scheme. The program removes every `RAKAZO_STACK_*` variable from the environment of the Docker commands it runs.
- **The wizard** needs a terminal. Without one, and without `--no-questions`, setup stops before changing anything.

### Questions

| Question | First run | Re-run | Ticket |
|---|---|---|---|
| Owner email and password | Asked; the password twice, without echo. Password 16–128 characters, Rakazo's own bounds | Email pre-filled. Password asked only when a database-only setting will be filled in; never saved | [#9, #19, #21, #61] |
| Model provider | `openrouter` (default), `openai`, `gemini`, `anthropic` or `none`, typed | Pre-filled | [#36, #65] |
| Model provider key | Not asked for `none`. Without echo; blank allowed. Checked with the provider before saving ([§8](#key-check)); a rejected key is never saved | "set, press Enter to keep" | [#9, #36, #65] |
| Composio project API key | Without echo; blank allowed. Checked with Composio before saving ([§13](#13-integrations-composio)); a rejected key is never saved | "set, press Enter to keep" | [#3, #9, #69] |
| Access | localhost only (default), Tailscale, or bring-your-own (advanced, unsupported) | Pre-filled; switching warns first ([§11](#switching-the-instance-url)) | [#8, #68] |
| Tailscale auth key and device name | Only with Tailscale access. A key that isn't ephemeral, typed without echo; device name default `rakazo` | Device name pre-filled with the name Tailscale actually assigned. Once joined, Enter skips the key; a key saved before a join succeeded is "set, press Enter to keep" | [#8, #24, #68] |
| Bring-your-own Instance URL | Only with bring-your-own access. Advanced, unsupported; mutually exclusive with Tailscale | Pre-filled | [#8, #68] |
| Memory | Asked only when a key is given. On by default; Enter keeps it on, `off` stops Hindsight but keeps its data | Pre-filled | [#9, #66] |

The wizard asks no model questions. The model overrides are edited in `settings.json` ([#36](https://github.com/kamui/rakazo-stack/issues/36)).

Setup **stops before changing anything** on a failed preflight or invalid input: a short Owner password, no Tailscale auth key on a first join, an auth key that setup can tell is ephemeral or isn't an auth key ([§11](#tailscale)), a malformed bring-your-own URL, `RAKAZO_STACK_INSTANCE_URL` set while the access mode isn't `bring-your-own`, a declined switch of the Instance URL, or a rejected key ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#68](https://github.com/kamui/rakazo-stack/issues/68)).

### First run, step by step

1. **Lock and preflight** ([§3](#preflight)).
2. **Questions** (wizard) or environment variables plus `settings.json` (no-questions mode). A first run always takes the Owner password here, which no-questions mode reads from `RAKAZO_STACK_OWNER_PASSWORD`; a scripted re-run reads it here only if that variable is set.
3. **Check keys.** Model provider key ([§8](#key-check)), then the Composio key ([§13](#13-integrations-composio)), then on OpenRouter the ZDR only probe ([§8](#zdr-only-probe)). Both key checks finish before setup writes anything, the run log included, so a rejected key changes nothing ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#36](https://github.com/kamui/rakazo-stack/issues/36), [#39](https://github.com/kamui/rakazo-stack/issues/39), [#69](https://github.com/kamui/rakazo-stack/issues/69)).
4. **Resolve versions** for every Component that will run, skipping releases without published images ([§4](#how-versions-are-resolved)). If Tailscale's version can't be looked up, the run carries on without Tailscale and adds a checklist item ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
5. **Write the Instance folder.** Generate missing secrets; write `secrets.env`, `settings.json`, the fetched Rakazo compose file and egress script, the bundled layer file and build folders, and `.env`; apply modes ([#29](https://github.com/kamui/rakazo-stack/issues/29), [#32](https://github.com/kamui/rakazo-stack/issues/32)). The egress script is fetched whenever `rakazo.<tag>.images.yml` is, and written before it, so while that file is there the script beside it is from the same tag and a re-run doesn't fetch either.
6. **macOS only: native Lemonade** ([§9](#macos-native-lemond)): download and verify `lemond`, write the plist, load the LaunchAgent and check its health. A failure is a checklist item and skips step 9 ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#70](https://github.com/kamui/rakazo-stack/issues/70)).
7. **Build and pull.** Pull the other images by digest (`compose pull --ignore-buildable`) and build `computer-egress` (`compose build computer-egress`). The relay is built and started with Lemonade (`up -d --build --wait lemonade lemonade-relay`, which also pulls Lemonade's image) before Rakazo's services, so a failed pull, build or start becomes a checklist item and Rakazo starts anyway ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
8. **Start.** `docker compose up -d --wait` ([#32](https://github.com/kamui/rakazo-stack/issues/32)). This step and step 7's pull name every service but voice's and `tailscale`, so their failure can't stop Rakazo; step 10 starts Tailscale ([#64](https://github.com/kamui/rakazo-stack/issues/64), [#68](https://github.com/kamui/rakazo-stack/issues/68)). On Windows, first force-recreate any of the Instance's containers that WSL's `docker` made ([below](#windows-docker-through-dockerexe)) ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
9. **Voice models.** macOS: set `whispercpp.backend=metal` against the running lemond ([§9](#macos-native-lemond)). Linux and Windows: set `whispercpp.backend=cpu` against the running Lemonade, never `auto`, which picked Vulkan on a software renderer in [#24](https://github.com/kamui/rakazo-stack/issues/24) ([#1](https://github.com/kamui/rakazo-stack/issues/1)). After a free-disk check, pull and load `kokoro-v1` and that OS's Whisper model through the relay, then run one speech and one transcription so the first-load cost (17 s for Whisper on Metal) happens during setup ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#24](https://github.com/kamui/rakazo-stack/issues/24), [#42](https://github.com/kamui/rakazo-stack/issues/42)).
   - The settings go through `docker compose exec -T lemonade lemonade config set whispercpp.backend=cpu auto_evict=false auto_check_model_updates=false broadcast=false`. `broadcast=false` stops lemond's UDP discovery beacon on the Instance's networks, as `--no-broadcast` does on macOS ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
   - Everything else runs as a Node script in `api` (`docker compose exec -T api node -e …`), so it takes the path Rakazo's own calls take: `http://lemonade-relay:13306/api/v1`. Pulls use `stream: true`; the progress events keep the relay's 255 s idle timeout from ending a download, and setup prints them. A model already downloaded isn't pulled again ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
   - Any failure becomes a checklist item, and the voice fill-in is skipped on that run ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
10. **Tailscale** (if on): start `tailscale`, then read the device name Tailscale assigned (for example `rakazo-1` on a clash), derive the Instance URL, and if it differs from what `.env` holds, rewrite `.env` and recreate Rakazo's services ([#8](https://github.com/kamui/rakazo-stack/issues/8)). Details are in [§11](#tailscale).
11. **Egress check.** Look for `restricted computer egress verified` in the supervisor log since the supervisor last started, waiting up to 90 s for its probe (the probe's own deadline is 60 s). If it's missing, add a checklist item that bot computers are off until it's fixed, with the cause. Never fall back to open egress ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
    - The supervisor probes only when it starts and before a computer starts. If its start-up probe failed but the sidecar's latest pass applied the rules (a Docker restart starts both at once), setup restarts the supervisor once and looks again.
    - The cause is the first that applies: Docker's firewall backend is `nftables`; the sidecar isn't running; the sidecar can't apply its rules (its last log line); or the reason the supervisor's probe gave. Upstream's error ends with advice to set `SANDBOX_COMPUTER_EGRESS=open`, so setup quotes only its reason.
12. **Database-only settings** ([below](#database-only-settings-in-rakazo)).
13. **Checklist and update nudge** ([below](#the-checklist)).

### Re-runs

- Pre-fill every answer from `settings.json` and the running Instance ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Keep recorded versions; only a Component switched on later starts at its latest version ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Turning Tailscale **off**: Compose leaves a service whose profile is off running, so setup runs `docker compose --profile tailscale rm --stop --force tailscale`. `tailscale-state` is kept, so turning it on again rejoins as the same device with no key. A failed removal is a checklist item ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- Never regenerate a secret; never print a pasted key ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Rewrite the stack files and `.env`, re-apply file modes, and re-run steps 6–13 idempotently.
- Turning memory **on** later: generate Hindsight's secrets, resolve and start Hindsight (or the recorded version, if memory ran before), connect the default Space ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#66](https://github.com/kamui/rakazo-stack/issues/66)).
- Turning memory **off** (`off`, or a blank key): drop the `hindsight` profile and remove both containers (`docker compose --profile hindsight rm --stop --force hindsight hindsight-db`, because `up` alone leaves a running container of an inactive profile up); keep `hindsight-pgdata`. Nothing is deleted unless separately asked ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#66](https://github.com/kamui/rakazo-stack/issues/66)). Setup doesn't disconnect memory in Rakazo; it finds the Spaces still connected (`space_memory_configs` rows with `provider = 'hindsight'`, with each Space's name and the email of who connected it, read-only) and adds a checklist item for each ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- A re-run started by the install one-liner with a newer program behaves the same: it keeps recorded versions and changes nothing a newer program needs except stack files. If recorded versions are below the new floor, it stops with "run `rakazo-stack update`" ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#35](https://github.com/kamui/rakazo-stack/issues/35)).

### Database-only settings in Rakazo

Rakazo keeps the Owner, each user's voice credential, each user's model connections and each Space's memory connection in its database, encrypted. Setup fills them only through Rakazo's own code paths, never raw SQL writes ([#11](https://github.com/kamui/rakazo-stack/issues/11)). The calls are Rakazo's internal, undocumented `/rpc`. They were proven on v0.4.0 (Owner, voice, memory with Honcho) and v0.16.0 (Owner, voice, memory with Hindsight, and the model connection with placeholder keys) ([#64](https://github.com/kamui/rakazo-stack/issues/64), [#67](https://github.com/kamui/rakazo-stack/issues/67)); on other versions they are best effort, and any failure becomes a checklist item with the UI steps ([#21](https://github.com/kamui/rakazo-stack/issues/21), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#45](https://github.com/kamui/rakazo-stack/issues/45)).

Sequence ([#21](https://github.com/kamui/rakazo-stack/issues/21), [#23](https://github.com/kamui/rakazo-stack/issues/23), [#36](https://github.com/kamui/rakazo-stack/issues/36), [#44](https://github.com/kamui/rakazo-stack/issues/44)):

1. Wait for health checks (Rakazo, the relay, Lemonade, and Hindsight when memory is on; Hindsight takes about 60–70 s).
2. **Detect** what exists with read-only SQL as the database owner role in Rakazo's Postgres (no password needed):
   - Owner: a verified `user` with the Owner email and a `credential` account. `docker compose exec -T postgres psql -U rakazo -d rakazo` with the query on stdin, after `SET default_transaction_read_only = on`. The email is trimmed and lowercased first, as `bootstrap-owner` stores it ([#61](https://github.com/kamui/rakazo-stack/issues/61)).
   - Voice: any `user_voice_credentials` row for the Owner (`JOIN "user" u ON u.id = v."userId"` on the Owner email). Its row-level security isn't forced, so the owner role sees every row.
   - Memory: any `space_memory_configs` row for the Owner's default Space. The default Space is the one Rakazo opens first: the Owner's earliest `space_members` row whose Space has `isDefault` and no `deletingAt`. A connection to another memory provider counts as present, so setup leaves it alone ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
   - Model connection (OpenAI/Gemini only): any `user_model_credentials` row for the Owner with the provider's Rakazo id, `openai` or `google`. Setup reads the id of its own connection, or else the newest: `SELECT c.id FROM user_model_credentials c JOIN "user" u ON u.id = c."userId" WHERE u.email = '<email>' AND c.provider = '<id>' ORDER BY c.label = '<OpenAI or Google> (rakazo-stack)' DESC, c."updatedAt" DESC, c."createdAt" DESC, c.id DESC LIMIT 1`. Its row-level security isn't forced either ([#67](https://github.com/kamui/rakazo-stack/issues/67)).
3. **Decide** what to fill:
   - Missing and never filled by setup: fill it.
   - Missing but `settings.json` says setup filled it before: treat it as removed in Rakazo's UI. The wizard asks "Your voice settings were removed in Rakazo. Add them back? [y/N]" (same for the Owner, memory and the model connection); no-questions mode leaves it removed ([#23](https://github.com/kamui/rakazo-stack/issues/23)). Changing the Owner email clears `filled_by_setup`, because everything setup fills belongs to the Owner, so the new address and its settings count as never set. An Owner created on this run counts its voice and its default Space's memory as never set too ([#61](https://github.com/kamui/rakazo-stack/issues/61), [#64](https://github.com/kamui/rakazo-stack/issues/64), [#66](https://github.com/kamui/rakazo-stack/issues/66)).
   - Present: leave it alone, even if it differs from what setup would write ([#21](https://github.com/kamui/rakazo-stack/issues/21)).
   - The model connection differs in two ways ([#67](https://github.com/kamui/rakazo-stack/issues/67)). A present one is updated while `model_connection_due` is set: the key differs from the saved one, or the Model provider changed. And choosing another Model provider drops the record that setup filled the connection, so the new provider's counts as never set. An Owner created on this run counts it as never set too.
4. Only if something will be filled: ask for the Owner password, unless step 2 already took it. No-questions mode never asks: without `RAKAZO_STACK_OWNER_PASSWORD` nothing is filled, and a checklist item says to run again with it set ([#61](https://github.com/kamui/rakazo-stack/issues/61)).
5. Owner missing: run Rakazo's `dist/cli/bootstrap-owner.js` in `api` with `OWNER_EMAIL` and `OWNER_PASSWORD` set. The password must not reach an argument list, and WSL's environment doesn't reach `docker.exe`, so setup runs `docker compose exec -T api node -e <script>` and writes `{"email","password"}` to its stdin; the script sets both variables and imports `bootstrap-owner.js` ([#61](https://github.com/kamui/rakazo-stack/issues/61)). Never run `bootstrap-owner` as a check: it ignores arguments (including `--help`) and rewrites `deployment_settings` on every run ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#21](https://github.com/kamui/rakazo-stack/issues/21)).
6. `POST /api/auth/sign-in/email` with `{email, password}`; read `.token` (on v0.16.0 already the signed value the bearer plugin requires); send it as `Authorization: Bearer …`. Send no `Origin`, cookie or `Sec-Fetch-*` headers. Sign-in is rate-limited to 10 tries per 15 minutes ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#21](https://github.com/kamui/rakazo-stack/issues/21)).
7. Voice missing: `POST /rpc/voice/connect` with `{"json":{"provider":"openai-compatible","baseUrl":"http://lemonade-relay:13306/api/v1","ttsModel":"kokoro-v1","sttModel":"<Whisper model>"}}`. The server lists models and voices before saving and picks the voice (`af_heart`) ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#21](https://github.com/kamui/rakazo-stack/issues/21), [#24](https://github.com/kamui/rakazo-stack/issues/24)). Lemonade has no `/audio/voices`, so Rakazo falls back to its own Kokoro list, which starts with `af_heart`. Only after step 9 of the first run made voice ready; on v0.16.0 it runs after `bootstrap-owner` in the same signed-in session ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
8. OpenAI or Gemini, and the connection is missing or due: `POST /rpc/models/connect` with `{"json":{"provider":"<openai or google>","apiKey":"<key>","modelId":"<model>","label":"<OpenAI or Google> (rakazo-stack)","credentialId":null,"makeDefault":true,"scope":"account"}}` ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#67](https://github.com/kamui/rakazo-stack/issues/67)).
   - `credentialId: null` adds a connection (omitting it would overwrite the provider's newest). An update sends the id step 2 read, and `makeDefault` is `true` only after a switch to this provider: a new key alone leaves the Owner's choice of model as it is. Rakazo keeps an updated connection's name.
   - `makeDefault: true` with `scope: account` makes the model the Owner's account-wide main model (`user_model_role_choices`, role `main`, position 0). A Space the Owner gave its own models keeps them.
   - Before it saves, Rakazo v0.16.0 asks the provider about the key (Gemini only, up to 30 s) and sends one small request to the model (up to 15 s). It answers `400` with a message when the provider refuses the key, the account or the model, and saves anyway when it can't reach the provider or is rate limited. Setup waits up to 60 s.
   - Success clears `model_connection_due`. A failure keeps it, so a later run tries again.
   - Setup withholds Rakazo's message if it quotes any four characters of the key.
9. Memory on and missing: `POST /rpc/memory/connectProvider` with `x-rakazo-space-id: <default Space id>`, so the call can't land on another Space the Owner's session opens, and `{"json":{"provider":"hindsight","settings":{"mode":"local","baseUrl":"http://hindsight:8888"},"credentials":{"apiKey":"<HINDSIGHT_API_TENANT_API_KEY>"}}}`. A new Owner's default Space is read after `bootstrap-owner` creates it. Rakazo checks the key against Hindsight before saving, which needs no LLM. Pass nothing for `retainConversations` or other memory toggles: Rakazo's defaults stay ([#44](https://github.com/kamui/rakazo-stack/issues/44), [#45](https://github.com/kamui/rakazo-stack/issues/45), [#50](https://github.com/kamui/rakazo-stack/issues/50)).
10. `POST /api/auth/sign-out`; discard the token ([#21](https://github.com/kamui/rakazo-stack/issues/21)).
11. Record in `settings.json` what was filled.
12. A non-2xx reply (`{"json":{"code","status","message"}}` from `/rpc`, `{"code","message"}` from `/api/auth`) becomes a checklist item with its message and the UI steps; setup carries on. So does a failed `bootstrap-owner`, with its error message; with no Owner, nothing after it is filled ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#21](https://github.com/kamui/rakazo-stack/issues/21), [#61](https://github.com/kamui/rakazo-stack/issues/61)).

Setup never touches other users' settings or other Spaces ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#49](https://github.com/kamui/rakazo-stack/issues/49)).

### The checklist

Every run ends with a checklist of what is left to do by hand ([#9](https://github.com/kamui/rakazo-stack/issues/9)). Items, each only when it applies:

- Any failed database-only fill-in, with the UI steps ([#9](https://github.com/kamui/rakazo-stack/issues/9)). For voice: **Settings → Voice → Self-hosted**, the relay's Server URL and the two model names ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
- Voice isn't set up (Lemonade or the relay didn't start, a model didn't download or load, low disk, or the warm-up failed), with the cause and where its logs are ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
- On the run that connects voice: the first time voice is used, Rakazo asks "Share voice data with Self-hosted (http://lemonade-relay:13306)?"; choose Allow, because that server is the Instance's own Lemonade ([§9](#9-voice), [#64](https://github.com/kamui/rakazo-stack/issues/64)).
- Bot computers are off because restricted egress isn't verified, with the cause ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- Docker's firewall backend is `nftables` ([§3](#preflight)) ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
- macOS: the lemond LaunchAgent is off in Login Items or keeps failing, with the fix or the last log lines ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
- Turn on **ZDR only** in OpenRouter (only when the probe found it off or couldn't check) ([#39](https://github.com/kamui/rakazo-stack/issues/39)).
- No Model provider key: each person connects a provider in **Settings → Models**; the Owner connects one before creating a bot, because Rakazo's onboarding stops at "Connect a model" ([#24](https://github.com/kamui/rakazo-stack/issues/24), [#36](https://github.com/kamui/rakazo-stack/issues/36)).
- OpenAI or Gemini with a key: chat on it is the Owner's alone, and other people connect their own provider in Rakazo ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- Switched away from OpenAI or Gemini, while the Owner still has a connection to it: remove the old connection in **Settings → Models**. After a switch to OpenRouter, Anthropic or none the item says why: until then it stays the Owner's default. A switch to the other of OpenAI and Gemini makes the new connection the default. Setup never removes a connection. If it can't read the database, the item says to check ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#67](https://github.com/kamui/rakazo-stack/issues/67)).
- Memory off but a Space still connected to Hindsight: that Space's owner disconnects it under **Settings → Memory**, because until then each reply first tries a recall that fails ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- Memory on: how to turn memory on in another Space: **Settings → Memory → Connect memory provider → Hindsight → Advanced → Self-hosted**, `http://hindsight:8888`, and the key from the `HINDSIGHT_API_TENANT_API_KEY` line in `~/rakazo-stack/secrets.env` ([#49](https://github.com/kamui/rakazo-stack/issues/49)).
- Tailscale on: enable MagicDNS and HTTPS certificates in the Tailscale admin console, and turn off key expiry for the device ([#8](https://github.com/kamui/rakazo-stack/issues/8), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- Tailscale didn't join, couldn't be looked up or couldn't be removed, with what to do; or it joined but the tailnet has MagicDNS or HTTPS certificates off, with `docker compose restart tailscale` to run after enabling them ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- "For private dictation, use Firefox or the Rakazo desktop app" ([#15](https://github.com/kamui/rakazo-stack/issues/15)).
- After an Update: any Component that became a checklist item ([§14](#failures)) ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### The update nudge

`setup` and `backup` check GitHub at most once a day. When the program or any un-held Component has a newer version, they print one line: "Updates available: run `rakazo-stack update`". A failed check is silent. `RAKAZO_STACK_NO_UPDATE_CHECK=1` turns it off. `restore` doesn't check ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

- The check runs at the end of a successful run. It writes the time to `.update-check` before it looks, so a failed check also waits a day, and the line prints only on the run that checks ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
- "Newer" is what `update` would move to: the program release step 2 would install ([§14](#steps)), and each running Component's newest release with published images. A Hold, or a release without images, doesn't count ([#73](https://github.com/kamui/rakazo-stack/issues/73), [#76](https://github.com/kamui/rakazo-stack/issues/76)).

### Per-OS differences

| | Linux | Windows (WSL 2) | macOS |
|---|---|---|---|
| Docker CLI the program uses | `docker` | `docker.exe` via WSL interop, `--context desktop-linux` | `docker` |
| Lemonade | `lemonade` container | `lemonade` container | native `lemond` LaunchAgent |
| Whisper model | `Whisper-Small` | `Whisper-Small` | `Whisper-Large-v3-Turbo` on Metal |
| `LEMONADE_UPSTREAM` | default (`http://lemonade:13305`) | default | `http://host.docker.internal:13315` |
| Computer-egress sidecar | host namespace; auto-detects legacy iptables | Docker Desktop's engine namespace | Docker Desktop's VM |
| Extra preflight | Docker Desktop for Linux refused; `docker` without sudo | `/mnt/` home refused; `docker.exe`, interop, Linux containers mode, Windows Home warning, VM memory | lemond port (13315) free |
| Runs without a signed-in user | Yes (Docker Engine starts at boot) | No | No |

Sources: ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#18](https://github.com/kamui/rakazo-stack/issues/18), [#40](https://github.com/kamui/rakazo-stack/issues/40), [#41](https://github.com/kamui/rakazo-stack/issues/41), [#42](https://github.com/kamui/rakazo-stack/issues/42)).

#### Windows: Docker through `docker.exe`

Windows setup stays a Linux program inside WSL, and the Instance folder stays in WSL's Linux home, but every Docker and Compose call that `setup`, `update`, `backup` and `restore` make goes through Windows' `docker.exe` ([#41](https://github.com/kamui/rakazo-stack/issues/41)):

- **Why:** a WSL-run Compose gives containers bind mounts and a `docker.sock` served from the WSL distro. After `wsl --shutdown` or a reboot, if Docker Desktop starts containers before the distro is back, those mounts fail (exit 127) and Docker never retries. The sidecar and the supervisor stayed down in testing. Through `docker.exe`, containers get Docker Desktop's own socket (`/host-services/docker.proxy.sock`) and nothing they need at start depends on the distro ([#27](https://github.com/kamui/rakazo-stack/issues/27), [#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Finding `docker.exe`:** on `PATH`, then `/Docker/host/bin/docker.exe`, then `/mnt/c/Program Files/Docker/Docker/resources/bin/docker.exe` ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Every call** passes `--context desktop-linux`, so a context changed on the Windows side can't send the Instance to another engine ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Everything goes through files and flags.** Environment variables set in WSL don't reach `docker.exe`. Compose reads the compose files and `.env` through `\\wsl.localhost\<distro>\…`; absolute Linux paths given to `-f` work ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Repair:** each `setup` and `update` force-recreates any of the Instance's containers whose `com.docker.compose.project.working_dir` label is a Linux path, which means WSL's `docker` made it ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Docs:** on Windows, type `docker.exe compose …` instead of `docker compose …`, from `~/rakazo-stack` in WSL ([#41](https://github.com/kamui/rakazo-stack/issues/41)).

## 8. Model provider

One key runs chat and memory. Setup asks which provider, then the key, and checks it with the provider before saving ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#44](https://github.com/kamui/rakazo-stack/issues/44)).

| Provider | Rakazo (chat) | Hindsight (memory) | Who gets chat |
|---|---|---|---|
| OpenRouter (default) | env: `OPENROUTER_API_KEY`, `PI_DEFAULT_PROVIDER`, `PI_DEFAULT_MODEL=~z-ai/glm-flash-latest` | `HINDSIGHT_API_LLM_PROVIDER=openrouter`, `HINDSIGHT_API_LLM_MODEL=~z-ai/glm-flash-latest`, `HINDSIGHT_API_LLM_REASONING_EFFORT=low`, `HINDSIGHT_API_LLM_EXTRA_BODY={"provider":{"zdr":true,"data_collection":"deny"}}` | Everyone on the Instance |
| OpenAI | per user: Owner connected through `models.connect`, model `gpt-6-luna` | `openai`, `gpt-6-luna` | Owner; others connect their own |
| Gemini (billing-enabled key; setup says so) | per user: Owner connected through `models.connect`, model `gemini-3.5-flash-lite` | `gemini`, `gemini-3.5-flash-lite` | Owner; others connect their own |
| Anthropic | env: `PI_DEFAULT_PROVIDER=anthropic`, `ANTHROPIC_API_KEY`, `PI_DEFAULT_MODEL=claude-haiku-5-5` | `anthropic`, `claude-haiku-5-5` | Everyone on the Instance |
| None (blank key) | nothing; each person connects a provider in Rakazo | off | Nobody until they connect |

Sources: ([#33](https://github.com/kamui/rakazo-stack/issues/33), [#36](https://github.com/kamui/rakazo-stack/issues/36), [#44](https://github.com/kamui/rakazo-stack/issues/44), [#45](https://github.com/kamui/rakazo-stack/issues/45)).

- A blank key means the same as `none` for the Instance, whichever provider was chosen.
- The key goes into `HINDSIGHT_API_LLM_API_KEY` for every provider, including OpenAI and Gemini, which Rakazo itself connects per user ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- With OpenAI or Gemini, memory runs on the Owner's key for everyone, so other people's conversations are learned through the Owner's account. The README FAQ says so ([#50](https://github.com/kamui/rakazo-stack/issues/50)).
- `OPENAI_API_KEY` and `GEMINI_API_KEY` are not written to Rakazo's env: Rakazo doesn't use them for runs ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- Rakazo's id for Gemini is `google`, and its Settings → Models calls it Google. Setup always names the model to `models.connect`, because v0.16.0's own default for `google` is another model ([#67](https://github.com/kamui/rakazo-stack/issues/67)).
- **Overrides.** `settings.json` may hold one Rakazo model (`rakazo_model`, written as `PI_DEFAULT_MODEL`) and one memory model (`memory_model`, written as `HINDSIGHT_API_LLM_MODEL`). An override that is empty or holds anything but ASCII letters, digits and `~ - _ . / : @` is invalid input. With OpenAI or Gemini the Rakazo model override is the `modelId` setup sends to `models.connect`: it takes effect when setup adds the connection or switches to the provider, and a connected model is otherwise changed in Rakazo ([#67](https://github.com/kamui/rakazo-stack/issues/67)). Choosing another provider removes both overrides, because they name models at the old one, and setup says so ([#65](https://github.com/kamui/rakazo-stack/issues/65), [#66](https://github.com/kamui/rakazo-stack/issues/66)). No per-operation Hindsight models (`HINDSIGHT_API_RETAIN_LLM_*`, `…_CONSOLIDATION_LLM_*`, `…_REFLECT_LLM_*`) ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#51](https://github.com/kamui/rakazo-stack/issues/51)).
- **Defaults belong to the program version.** `rakazo-stack update` moves to a newer program's defaults; overrides always win ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Switching provider** never touches stored memories: Hindsight's embeddings run locally and don't depend on the provider ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **`~z-ai/glm-flash-latest` is an OpenRouter alias.** It moves when Z.ai ships a new Flash model, not on an Update. A Self-hoster who wants a fixed model sets the override (for example `z-ai/glm-5.3-flash`) ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Cost.** Measured on OpenRouter defaults with Rakazo v0.16.0 and Hindsight 1.3.0: chat about $1.3 a month and memory about $2.4–3.7 a month for one user at 30 turns a day, so about $3.7–5 for the Instance. A Space on the Shared memory scope roughly doubles the memory part ([#45](https://github.com/kamui/rakazo-stack/issues/45)).

### Key check

Every run with a key checks it at step 3, with a request that costs nothing ([#65](https://github.com/kamui/rakazo-stack/issues/65)):

| Provider | Request | Rejected |
|---|---|---|
| OpenRouter | `GET https://openrouter.ai/api/v1/key`, `Authorization: Bearer` (its model list needs no key) | `401`, `403` |
| OpenAI | `GET https://api.openai.com/v1/models`, `Authorization: Bearer` | `401`, `403`, unless the reply says `Missing scopes`: a restricted key that can't list models is still accepted |
| Gemini | `GET https://generativelanguage.googleapis.com/v1beta/models`, `x-goog-api-key` | `400` with `API_KEY_INVALID`, `401`, `403` |
| Anthropic | `GET https://api.anthropic.com/v1/models`, `x-api-key`, `anthropic-version: 2023-06-01` | `401`, `403` |

- `200` accepts the key. Billing isn't checked.
- A rejected key stops the run before anything changes, whether it is new or saved.
- Any other reply, or none, is no verdict. A new key (one that differs from the saved one) stops the run before anything changes. A saved key was accepted before, so setup warns, keeps it and carries on.
- Setup never prints a provider's reply: OpenAI's refusal quotes part of the key.
- Redirects aren't followed.

### ZDR only probe

On OpenRouter, after every key check that lets setup carry on, including no-questions runs ([#39](https://github.com/kamui/rakazo-stack/issues/39), [#65](https://github.com/kamui/rakazo-stack/issues/65)):

- Request: one embeddings call for `openai/text-embedding-3-small` with `provider: {only: ["openai"], allow_fallbacks: false}` and the input `zdr check`. OpenAI's endpoint isn't ZDR.
- `404` with `zdr-violation-by-guardrail`: ZDR only is on. No checklist item.
- `200`: it's off (the call costs about $0.00000002). The wizard explains that Rakazo's chat can then go to providers that keep data, links `openrouter.ai/workspaces/default/guardrails`, and offers **Check again** (`c`; repeats the probe) or **Continue without it** (Enter, the default; adds the checklist item). No-questions mode warns and adds the item.
- Anything else: couldn't check. No warning, no question; add the checklist item.
- The key is saved whatever the probe returns. Setup never blocks on it. No probe for other providers.

Hindsight's own calls carry `provider: {zdr: true, data_collection: "deny"}` (Hindsight adds `require_parameters: true` itself), so they stay on ZDR endpoints whether or not the guardrail is on; Rakazo's chat is covered only by the guardrail ([#39](https://github.com/kamui/rakazo-stack/issues/39), [#45](https://github.com/kamui/rakazo-stack/issues/45)).

## 9. Voice

Voice is Lemonade only, out of the box: `kokoro-v1` for speech, Whisper for transcription ([#1](https://github.com/kamui/rakazo-stack/issues/1)). Phone calls are in-app voice calls, not telephony: no provider, public address, TURN server or push is needed, but the browser microphone needs HTTPS or localhost ([#6](https://github.com/kamui/rakazo-stack/issues/6)).

| | Linux, Windows | macOS |
|---|---|---|
| Where Lemonade runs | `lemonade` container, unmodified upstream image | native `lemond` LaunchAgent |
| Whisper model | `Whisper-Small` (6.2% WER; 1.9 s / 4.5 s for 6 s / 29 s clips on 4 Zen 4 cores) | `Whisper-Large-v3-Turbo` on Metal (about 0.9 s / 1.3 s) |
| Kokoro | CPU | CPU (its Mac build has no GPU path) |
| Whisper backend | `cpu` ([#42](https://github.com/kamui/rakazo-stack/issues/42), [#1](https://github.com/kamui/rakazo-stack/issues/1)) | `metal` |
| Threads | Lemonade's default (4); no `-t` | default; no `-t` |

Sources: ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#17](https://github.com/kamui/rakazo-stack/issues/17), [#42](https://github.com/kamui/rakazo-stack/issues/42)).

- Setup pulls and warms `kokoro-v1` and that OS's Whisper model, through the relay ([#42](https://github.com/kamui/rakazo-stack/issues/42)).
- Models stay loaded while idle (`auto_evict` is `false`) ([#42](https://github.com/kamui/rakazo-stack/issues/42)).
- Lemonade runs with `auto_check_model_updates: false` ([#32](https://github.com/kamui/rakazo-stack/issues/32)) and `broadcast: false` ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
- **AI-recipient consent.** Rakazo v0.16.0 counts the relay as an AI-data recipient ("Self-hosted (http://lemonade-relay:13306)"). Its web app asks each person once, before their first speech or transcription, and saves the answer per Space; its server doesn't check it for a person's own voice credential, so a call fails only if the person refuses. Setup doesn't answer for the Owner. It adds a checklist item on the run that connects voice ([#45](https://github.com/kamui/rakazo-stack/issues/45), [#64](https://github.com/kamui/rakazo-stack/issues/64)).
- **Measured** on Docker Desktop for Windows (a 12-core Zen 4 desktop, Lemonade's default 4 threads) with Lemonade v2026.48.3: the relay's conversion (ffmpeg to 16 kHz WAV) took 48 ms for a 6 s WebM/Opus clip and 98 ms for a 29 s clip, medians of 5. Whisper itself took 3.5 s and 8.7 s for the same clips; speech about 0.9 s for a short reply. First downloads: `kokoro-v1` (354 MB) in 16 s and `Whisper-Small` in 23 s; first loads 5.8 s and 1.6 s ([#64](https://github.com/kamui/rakazo-stack/issues/64)).
- Lemonade runs without `--convert` and without ffmpeg on every OS: the relay converts audio ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
- Lemonade re-downloads its backends after every version change, so voice comes back last after an Update ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).

### The Lemonade relay

`lemonade-relay` sits between Rakazo and Lemonade on every OS ([#17](https://github.com/kamui/rakazo-stack/issues/17)). Rakazo's voice Server URL is `http://lemonade-relay:13306/api/v1`.

- **Image:** built locally from a Bun Alpine base fixed in the program (`oven/bun:1.4.4-alpine` today), plus Alpine's ffmpeg (`~=8.1`), with the relay's code copied in. It runs as user `bun` ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#31](https://github.com/kamui/rakazo-stack/issues/31), [#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Converts every transcription upload** to 16 kHz mono WAV (`ffmpeg -i <in> -ar 16000 -ac 1 -c:a pcm_s16le <out>.wav`) before forwarding it. Browsers upload WebM/Opus (and probably MP4/AAC on Safari), which whisper-server can't decode without ffmpeg ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- **Rewrites `lang_code`** on `/audio/speech`: Rakazo sends Kokoro's one-letter codes (`a`, `b`, `e`, `f`, `h`, `i`, `j`, `p`, `z`), which Kokoros passes to espeak as-is. The relay maps them to `en-us`, `en-gb-x-rp`, `es`, `fr-fr`, `hi`, `it`, `ja`, `pt-br`, `cmn`. This part goes away once [lucasjinreal/Kokoros#144](https://github.com/lucasjinreal/Kokoros/issues/144) ships in Lemonade; the relay stays for the conversion ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#17](https://github.com/kamui/rakazo-stack/issues/17)).
- **Joins Whisper's line breaks** in transcription text: newlines become single spaces, because Whisper breaks lines at its segment boundaries, even mid-sentence. `srt` and `vtt` replies are left as they are, because their line breaks are part of the format ([#24](https://github.com/kamui/rakazo-stack/issues/24), [#1](https://github.com/kamui/rakazo-stack/issues/1)).
- **Adds `Authorization: Bearer $LEMONADE_API_KEY`** to every request. Only the relay holds the key ([#10](https://github.com/kamui/rakazo-stack/issues/10)).
- Forwards to `UPSTREAM`: `http://lemonade:13305` on Linux/Windows, `http://host.docker.internal:13315` on macOS. Idle timeout 255 s, because transcription can take tens of seconds before the first byte.
- Rakazo accepts the relay as a private endpoint because of `MCP_ALLOW_PRIVATE_ENDPOINT=true` and `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS=lemonade-relay`. A bare service name is refused without the second setting ([#2](https://github.com/kamui/rakazo-stack/issues/2), [#10](https://github.com/kamui/rakazo-stack/issues/10)).
- The relay must follow Bun onto newer Alpine releases as Alpine's community packages lose support ([#31](https://github.com/kamui/rakazo-stack/issues/31)).

### macOS: native `lemond`

Containerised Lemonade is ruled out on the Mac: the image is amd64 only and Docker Desktop gives containers no GPU ([#5](https://github.com/kamui/rakazo-stack/issues/5)). Setup runs its own `lemond` ([#17](https://github.com/kamui/rakazo-stack/issues/17)):

- **Never reuses** a Lemonade from the upstream `.pkg` or the Homebrew cask `lemonade-server`; never touches one.
- **Release:** the newest stable release of `lemonade-sdk/lemonade` whose `lemonade-embeddable-<X.Y.Z>-macos-arm64.tar.gz` asset is published with a `sha256:` digest; newer ones without it are skipped and named ([§4](#how-versions-are-resolved)). Lemonade's `candidate-v…` prereleases carry assets with the same file names but other digests, so only the release's own asset counts ([#70](https://github.com/kamui/rakazo-stack/issues/70)).
- **Install:** download `lemonade-embeddable-<ver>-macos-arm64.tar.gz` with the program's HTTP client (so no quarantine flag), check it against GitHub's published SHA-256, unpack into `~/rakazo-stack/lemonade/versions/<ver>/` (for example `versions/v2026.48.3/`), dropping the tarball's one top folder. It unpacks into a hidden folder beside it and is renamed into place, so a version folder that exists is complete; a present one isn't downloaded again. A mismatched SHA-256 refuses the download. An Update unpacks the new version beside it and swaps ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#17](https://github.com/kamui/rakazo-stack/issues/17), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Port:** `13315` on `127.0.0.1`, a fixed loopback port of the stack's own, not 13305, so it can't collide with an upstream Lemonade. lemond exits rather than pick another port, so preflight stops if the port is taken by anything but the Instance's own lemond ([#70](https://github.com/kamui/rakazo-stack/issues/70)).
- **LaunchAgent** with one fixed label, `com.github.kamui.rakazo-stack.lemond` (`com.github.kamui.<project>.lemond`, so a test Instance's is its own, [§19](#test-instances)), `RunAtLoad` and `KeepAlive` ([#70](https://github.com/kamui/rakazo-stack/issues/70)):
  - Command: `lemond --no-broadcast --host 127.0.0.1 --port 13315 ~/rakazo-stack/lemonade`, with absolute paths, from `versions/<ver>/`. `--no-broadcast` stops the UDP discovery beacon on the LAN ([#16](https://github.com/kamui/rakazo-stack/issues/16)). `--host 127.0.0.1` because lemond's default `localhost` could land on `::1`, which Docker Desktop's `host.docker.internal` doesn't reach. The positional folder is lemond's working folder: its `config.json` and backends.
  - Environment: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HF_HOME=~/rakazo-stack/lemonade/models`, `LEMONADE_API_KEY` (lemond reads the key from its environment only).
  - Plist `~/Library/LaunchAgents/<label>.plist`, mode 600, rewritten from `secrets.env` on every run.
  - Stdout and stderr go to `~/rakazo-stack/logs/lemond.log`.
  - Load with `launchctl bootstrap gui/$UID <plist>` when `launchctl print` doesn't show it loaded; reload (`bootout` then `bootstrap`, which is retried for a few seconds while launchd tears the old job down) only when the plist changed. Never `launchctl unload -w`, and never `launchctl enable`: when `launchctl print-disabled gui/$UID` lists the label as disabled, as turning it off in Login Items does, setup leaves it off and adds the checklist item.
- **Config:** `lemonade --host 127.0.0.1 --port 13315 config set whispercpp.backend=metal auto_evict=false auto_check_model_updates=false` against the running lemond, with the `lemonade` from the same version folder and the key in its environment, not by writing `config.json`. No `-t`, no `metal_args` (gone in Lemonade v2026.45.3) ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#42](https://github.com/kamui/rakazo-stack/issues/42)). This is step 9's settings call on macOS; the models then go through the relay as on Linux.
- **Health on every run:** `launchctl print gui/$UID/<label>` shows it loaded, and `/api/v1/health` answers 200 with the key, within 60 s. Otherwise a checklist item with the fix (for example re-enabling it in Login Items) or the last 10 lines of `lemond.log`, with the key replaced by `<LEMONADE_API_KEY>` if it appears. A failed download, check or unpack is a checklist item too. Setup never falls back to another voice setup.
- **Docs:** mention the "Background Items Added" notice macOS 13+ shows on first install, and the manual removal steps: `launchctl bootout`, delete the plist, delete the Instance folder.
- Measured on an M3 Pro: speech 0.8 s for a short reply, transcription 0.8–2.0 s, first Whisper load 17 s ([#16](https://github.com/kamui/rakazo-stack/issues/16)).

### Speech that bypasses Lemonade

Accepted and documented, not worked around ([#15](https://github.com/kamui/rakazo-stack/issues/15)):

- Desktop Chrome, Edge and Safari always dictate through the browser maker's cloud recognizer. Rakazo has no setting for this.
- Firefox, phone browsers and Rakazo's desktop app send audio to the server, so Lemonade transcribes it.
- Rakazo's phone app listens with the phone's own recognizer during calls; replies use the server voice unless the user picks the device voice ([#14](https://github.com/kamui/rakazo-stack/issues/14)).
- An optional upstream feature request ("Prefer server transcription") is drafted on [#15](https://github.com/kamui/rakazo-stack/issues/15) for the Self-hoster to file.

## 10. Memory

v1's memory is Hindsight, replacing Honcho, which is planned as a later option ([#44](https://github.com/kamui/rakazo-stack/issues/44)). See [ADR 0001](adr/0001-hindsight-for-v1-memory.md).

- **On** only when the Self-hoster gives a Model provider key, any of the four. Hindsight refuses to start without an LLM key, so a blank key means memory is off ([#44](https://github.com/kamui/rakazo-stack/issues/44)). A key its startup check can't use only gives a warning (1.3.0's check fails open), which is how the integration test runs memory with a placeholder key ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- **Setup connects memory only in the default Space** ([§7](#database-only-settings-in-rakazo)). Rakazo lets only a Space's owner connect memory, so other Spaces, including other people's, are connected by hand from the checklist and README instructions ([#49](https://github.com/kamui/rakazo-stack/issues/49)).
- **Learn from conversations stays on** (Rakazo's default). Setup passes nothing for `retainConversations` and doesn't ask. It can be turned off per Space under **Settings → Memory → Edit** ([#50](https://github.com/kamui/rakazo-stack/issues/50)).
- **One memory model** for all of Hindsight's work ([#51](https://github.com/kamui/rakazo-stack/issues/51)).
- **Output limit:** Hindsight's fact extraction asks for 64,000 output tokens. OpenAI documents 128,000 for [`gpt-6-luna`](https://developers.openai.com/api/docs/models/gpt-6-luna) and Google 65,536 for [`gemini-3.5-flash-lite`](https://ai.google.dev/gemini-api/docs/models/gemini-3.5-flash-lite), so setup leaves `HINDSIGHT_API_RETAIN_MAX_COMPLETION_TOKENS` at its default for every provider ([#67](https://github.com/kamui/rakazo-stack/issues/67)).
- **Recall settings:** Rakazo's defaults stay (`recallBudget` `mid`, observations only, **Recall automatically** on) ([#47](https://github.com/kamui/rakazo-stack/issues/47)).
- Rakazo creates its `rakazo-` banks in Hindsight itself on first write ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- The Honcho decisions ([#7](https://github.com/kamui/rakazo-stack/issues/7), [#12](https://github.com/kamui/rakazo-stack/issues/12), [#13](https://github.com/kamui/rakazo-stack/issues/13), [#37](https://github.com/kamui/rakazo-stack/issues/37), [#38](https://github.com/kamui/rakazo-stack/issues/38)) don't apply to v1. They stay on record for the later Honcho option. Because Hindsight embeds locally, the Honcho-era rules about a fixed embeddings model per Instance, the tokenizer-cache volume and its ownership fix, and Anthropic turning memory off are all gone ([#44](https://github.com/kamui/rakazo-stack/issues/44)).

### Hindsight settings

`hindsight` gets this environment ([#44](https://github.com/kamui/rakazo-stack/issues/44), [#45](https://github.com/kamui/rakazo-stack/issues/45), [#47](https://github.com/kamui/rakazo-stack/issues/47)):

| Variable | Value |
|---|---|
| `HINDSIGHT_API_DATABASE_URL` | `postgresql://hindsight:<password>@hindsight-db:5432/hindsight` |
| `HINDSIGHT_API_TENANT_EXTENSION` | `hindsight_api.extensions.builtin.tenant:ApiKeyTenantExtension` |
| `HINDSIGHT_API_TENANT_API_KEY` | generated |
| `HINDSIGHT_API_LLM_PROVIDER`, `HINDSIGHT_API_LLM_API_KEY`, `HINDSIGHT_API_LLM_MODEL` | per provider ([§8](#8-model-provider)) |
| `HINDSIGHT_API_LLM_REASONING_EFFORT` | `low` (OpenRouter; otherwise GLM reasons at `max`) |
| `HINDSIGHT_API_LLM_EXTRA_BODY` | `{"provider":{"zdr":true,"data_collection":"deny"}}` (OpenRouter; parsed as JSON) |
| `HINDSIGHT_API_EMBEDDINGS_LOCAL_MODEL` | `BAAI/bge-small-en-v1.5` (pinned, so an upstream default change can't break boot) |
| `HINDSIGHT_API_RERANKER_LOCAL_MODEL` | `hindsight-ai/hs-rerank-1` (pinned) |
| `HINDSIGHT_API_RERANKER_MAX_CANDIDATES` | `100` |
| `HINDSIGHT_API_WORKER_ID` | `hindsight` (stable, so in-flight tasks survive a restart) |
| `HINDSIGHT_API_MCP_ENABLED` | `false` (Rakazo doesn't use Hindsight's MCP endpoint) ([#44](https://github.com/kamui/rakazo-stack/issues/44), [#1](https://github.com/kamui/rakazo-stack/issues/1)) |

- **Why `RERANKER_MAX_CANDIDATES=100`:** the CPU reranker is over 90% of each recall. With Hindsight's default of 300 candidates, two recalls at once on a slow 4-thread CPU reached 14–16 s, past Rakazo's 15 s recall timeout, after which Rakazo replies without memory. With 100: about 3× faster, 89–93% of the top 5 unchanged, same top result. On an M3 Pro limited to 4 CPUs, two at once take 1.9 s. FlashRank and `recallBudget: low` were rejected ([#47](https://github.com/kamui/rakazo-stack/issues/47)).
- **Why `shm_size: 1g` on `hindsight-db`:** every new bank builds three partial HNSW indexes on the shared `memory_units` table. Once that table passes 8 MB, Postgres builds them in parallel, and pgvector's segment takes 61 MB of Docker's 64 MB default `/dev/shm`. Without the setting, every new bank after the first failed (HTTP 500 `could not resize shared memory segment`), so a bot added later, or a Space switched to the Shared scope, never got memory. The `hindsight` container doesn't need it ([#48](https://github.com/kamui/rakazo-stack/issues/48)).
- **Embeddings and reranking** run on the CPU inside the full image; the models are baked in, so first start downloads nothing ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **Egress:** `hindsight` is on `egress` to reach the Model provider. `hindsight-db` stays on internal `hindsight-data` only ([#45](https://github.com/kamui/rakazo-stack/issues/45)).
- **The one tenant key reaches every Space's banks.** Sharing it is acceptable because Hindsight is reachable only on the internal networks, which bot computers are firewalled from, and through Rakazo the key opens only that Space's `rakazo-` banks. Anyone with shell access to the host already has everything ([#49](https://github.com/kamui/rakazo-stack/issues/49)).
- **Upgrades:** Alembic migrations run forward on every start. An older image on a newer schema only logs a warning and keeps running, so rollback is always "restore the Backup", never an image swap ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **Backup:** a `pg_dump` of `hindsight-db` is complete (files are stored in Postgres). The tenant key comes back with `secrets.env` ([#44](https://github.com/kamui/rakazo-stack/issues/44)).

### Memory behaviours to document

Documented in one README FAQ; the stack changes nothing ([#45](https://github.com/kamui/rakazo-stack/issues/45)):

- Memory learns from what bots say as well as what people say, so a bot's guess can be saved as a fact about the user.
- New memories can take up to about 2 minutes to reach recall.
- Clearing a conversation deletes everything memory learned from it, in both banks. Facts a bot saved on purpose are kept.
- Turns go to the Model provider a second time, for memory ([#50](https://github.com/kamui/rakazo-stack/issues/50)).

## 11. Access: localhost, Tailscale and phones

### Modes

| Mode | Instance URL | Rakazo address settings | Ticket |
|---|---|---|---|
| localhost (default) | `http://127.0.0.1:5173` (Rakazo also trusts the `localhost` twin) | `BETTER_AUTH_URL`, `WEB_ORIGIN`, `API_URL` on `http://127.0.0.1:5173`; `RAKAZO_HOST=127.0.0.1` (a hostname, not a URL) | [#2, #8] |
| Tailscale | `https://<device>.<tailnet>.ts.net` (port 443) | `WEB_ORIGIN`, `API_URL`, `RAKAZO_HOST` on the `ts.net` address; `BETTER_AUTH_URL=http://127.0.0.1:5173` | [#8, #24] |
| Bring-your-own Instance URL (advanced, unsupported) | the given HTTPS URL | all from the given URL; nothing else changes | [#8](https://github.com/kamui/rakazo-stack/issues/8) |

- localhost works fully, microphone included, because browsers treat localhost as secure ([#6](https://github.com/kamui/rakazo-stack/issues/6), [#8](https://github.com/kamui/rakazo-stack/issues/8)).
- **Why `BETTER_AUTH_URL` stays on loopback with Tailscale:** Rakazo trusts only `WEB_ORIGIN`, `BETTER_AUTH_URL`, its mobile app origins and their loopback twins. With all four on `ts.net`, sign-in at localhost fails with 403 `INVALID_ORIGIN`. With `BETTER_AUTH_URL` on `127.0.0.1`, password sign-in works at both addresses. Costs: the session cookie loses `Secure` and its `__Secure-` prefix (it still travels only over HTTPS on the tailnet or over loopback), and passkeys work only at the `ts.net` address ([#24](https://github.com/kamui/rakazo-stack/issues/24)).
- Bring-your-own: the proxy, certificates and firewall are the Self-hoster's. Mutually exclusive with Tailscale ([#8](https://github.com/kamui/rakazo-stack/issues/8)). `web` stays on `127.0.0.1:5173`, so the proxy must run on the same machine; setup doesn't offer a wider bind, and the docs say so ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- A bring-your-own Instance URL is `https://<host>`, with an optional port and no path, query or credentials. The host is an ASCII hostname or IPv4 address (an international domain in its `xn--` form; no IPv6 address). Setup lowercases it and drops a trailing `/` and the port `443`, as browsers write an origin. `RAKAZO_HOST` is the host without the port ([#68](https://github.com/kamui/rakazo-stack/issues/68)).

### Tailscale

- A stack-managed `tailscale` container joins the Self-hoster's tailnet as its own device, in userspace mode. The host's own Tailscale, if any, isn't used ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Setup asks for a plain auth key that isn't ephemeral (Tailscale removes ephemeral machines after they go offline) and a device name, default `rakazo`. No tags, OAuth or ephemeral nodes. `TS_AUTH_ONCE` plus the `tailscale-state` volume keep the identity, so later restarts need no key ([#8](https://github.com/kamui/rakazo-stack/issues/8), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- **What setup checks.** Whether an auth key is ephemeral is stored only on Tailscale's servers: neither the key nor the joined device shows it. Setup refuses an OAuth client secret (`tskey-client-…`, ephemeral by default), a key carrying `ephemeral=true`, and anything not of the form `tskey-auth-…`. An ephemeral `tskey-auth-` key is not detected, so the question and the docs say not to use one. A device name is one DNS label (up to 63 letters, digits and hyphens, not starting or ending with a hyphen), lowercased ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- **Joining** (step 10). Setup runs `docker compose up -d --no-deps tailscale` and polls `docker compose exec -T tailscale tailscale status --json --peers=false`, 45 times 2 s apart, until the state is `Running` under the requested name or a numbered variant of it. After a change of name it keeps waiting while the old name is reported, and takes whatever the device reports when the time is up, so a name set in the admin console wins. The Instance URL is `https://` plus `Self.DNSName` without its trailing dot ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- **After a join**, setup records the name, removes the auth key from `secrets.env` and `.env`, and, if `.env` changed, recreates Rakazo's services and `tailscale` and waits for the device again, now with no key. Until the first join, `.env` holds the localhost address settings ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- **A failure** to start or join is a checklist item and setup carries on. The auth key stays saved for the next run. Setup never prints the container's log or the raw status reply ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- Serve gets a Let's Encrypt certificate and proxies HTTPS 443 to `http://web:5173`, keeping the `ts.net` Host header that Rakazo's `RAKAZO_HOST` check needs. Only Rakazo's web app is served; nothing else answers on the device ([#24](https://github.com/kamui/rakazo-stack/issues/24)).
- One-time manual steps (checklist): enable MagicDNS and HTTPS certificates in the admin console (device names then appear in public Certificate Transparency logs), and turn off key expiry for the device ([#8](https://github.com/kamui/rakazo-stack/issues/8), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- Anyone on the tailnet can open the Instance URL; Rakazo sign-in (sign-ups off) decides who uses it. Tailscale ACLs are documented as optional hardening ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Tested on Docker Desktop for Windows with `tailscale/tailscale:v1.104.4`; untested on a Mac ([#24](https://github.com/kamui/rakazo-stack/issues/24)).

### Switching the Instance URL

A re-run can turn Tailscale on or off, change a joined device's name, or change the bring-your-own URL. Before it does, the wizard warns that this signs everyone out, breaks passkeys made on the old address, makes phones sign in again and leaves old email links stale, and asks to confirm. No data is lost ([#8](https://github.com/kamui/rakazo-stack/issues/8)). MCP servers connected with OAuth may need re-adding (unverified) ([#30](https://github.com/kamui/rakazo-stack/issues/30)). The default answer is no, and declining stops the run before anything changes. No-questions mode prints the warning and carries on, because a changed variable is already a deliberate choice. A name that Tailscale changes by itself (a clash, or a rename in the admin console) is applied without asking ([#68](https://github.com/kamui/rakazo-stack/issues/68)).

### Phone apps

- Rakazo's store apps take a custom server URL (sign-in → "Use a custom server") ([#14](https://github.com/kamui/rakazo-stack/issues/14)).
- **iPhone:** works over the Tailscale HTTPS address, including sign-in and push notifications. Plain `http://` to a Tailscale `100.x` address is blocked by iOS for the app, so the docs say to use the `https://….ts.net` address ([#19](https://github.com/kamui/rakazo-stack/issues/19)).
- **Push** works today with no `EXPO_ACCESS_TOKEN`, because upstream's Expo project has enhanced push security off. Upstream can change that at any time, and a Self-hoster couldn't fix it: documented as best-effort ([#19](https://github.com/kamui/rakazo-stack/issues/19)).
- **Android:** untested (custom server, `http://` rules, push, Live working status) ([#28](https://github.com/kamui/rakazo-stack/issues/28)).
- Passkeys are hidden for self-hosted servers in the stock app; people sign in with a password ([#14](https://github.com/kamui/rakazo-stack/issues/14)).
- Store apps update themselves and degrade feature by feature against an older server; there's no version check. The docs say to run `rakazo-stack update` when the app asks ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).

## 12. Bot-computer isolation

Bot computers run code that AI agents write. On every OS they must not reach Lemonade, the relay, Hindsight, any database, the host, the LAN or the tailnet. They can still reach the internet ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#26](https://github.com/kamui/rakazo-stack/issues/26), [#40](https://github.com/kamui/rakazo-stack/issues/40)).

Two layers:

1. **Networks.** Each bot computer sits on its own bridge, joined only by `supervisor` and `web`, so it can't reach anything on `stack`, `hindsight-data` or `data` ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
2. **The computer-egress sidecar** with `SANDBOX_COMPUTER_EGRESS=restricted`, on every OS, always on, with no opt-out ([#40](https://github.com/kamui/rakazo-stack/issues/40)).

### The computer-egress sidecar

- **Image:** `rakazo-stack-computer-egress`, Compose's own name for the built service (`<project>-computer-egress`, so a test Instance's image is its own). Built locally from an Alpine base fixed in the program, with `bash` (the script needs it), `iptables`, `ip6tables` and Alpine's `iptables-legacy` baked in, and Rakazo's `infra/compose/restrict-computer-egress.sh` from the recorded Rakazo tag built into the image, unmodified. Nothing is downloaded at boot, so computers aren't refused when the internet is down ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#27](https://github.com/kamui/rakazo-stack/issues/27), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **Runs** with `network_mode: host`, `cap_add: [NET_ADMIN]` and `restart: unless-stopped`, re-applying the rules every 60 s. Its rules match only `rakazo-c+` bridges, so the relay and screens are unaffected ([#25](https://github.com/kamui/rakazo-stack/issues/25), [#27](https://github.com/kamui/rakazo-stack/issues/27)).
- **Legacy iptables (Linux):** on every pass, a wrapper checks which backend holds `DOCKER-USER`. If `iptables-legacy -L DOCKER-USER` succeeds and `iptables -L DOCKER-USER` doesn't, it sets `RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy` before running the script ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **The wrapper** runs the script with `--apply`, which never calls `sudo` or systemd. It logs `computer egress rules applied with <iptables>` on its first success and after a failure, and the script's errors otherwise. With arguments it runs the script once with them, after the same backend check, so `docker compose run --rm computer-egress --print` shows the rules and `--remove` deletes them.
- **When the sidecar goes away,** its rules stay: stopping or removing the container, or `docker compose down`, never runs `--remove`. That fails closed. A Docker restart can start the supervisor before the sidecar, and with the rules in place its probe passes and no computer starts open. The rules match only `rakazo-c+` bridges, and only a Rakazo supervisor in `restricted` mode makes those, so rules left behind change nothing else. They last until the engine's firewall is reset: a reboot on Linux, unless something on the host saves and restores the firewall, and a Docker Desktop restart or `wsl --shutdown` on Windows and macOS, which is expected but untested ([§19](#acceptance-checks-not-yet-run)). Setup never removes them. The manual steps for removing an Instance ([§20](#20-known-limitations-to-document)) run `docker compose run --rm computer-egress --remove` before `docker compose down`, unless another Instance uses the same engine.
- **Two Instances on one engine.** v1 supports one Instance per machine ([§1](#1-scope)), but a test Instance can run beside a real one, and so can a hand-built Rakazo in `restricted` mode ([§5](#5-compose-layout)). Rakazo's rules match `rakazo-c+` on the whole engine, so every Instance on it shares one set of rules. A second sidecar's pass finds the rules already at the head of each chain and changes nothing. Computers don't clash, because Rakazo names bridges and computers after the bot, not the Instance. The Instances' supervisors share one probe network (`rakazo-egress-probe`, bridge `rakazo-cprobe`). Two probes at once can fail one, and setup's single supervisor restart repeats it. When one of two Instances is removed, the rules have to stay. Running `--remove` then opens the other Instance's computers, running and new alike, because its supervisor checked the rules once and cached the result. That lasts until its sidecar's next pass, up to 60 s.
- **Where the rules land:** the host's namespace on Linux; Docker Desktop's engine namespace or VM on Windows and macOS ([#25](https://github.com/kamui/rakazo-stack/issues/25), [#27](https://github.com/kamui/rakazo-stack/issues/27)).
- **Why `restricted`, not `auto`:** `auto` caches its first probe and can settle on `open` if the supervisor starts before the sidecar. `restricted` fails closed: with the rules missing, computer creation returns 500 naming the script, and works again once the sidecar is back, without a supervisor restart ([#25](https://github.com/kamui/rakazo-stack/issues/25), [#27](https://github.com/kamui/rakazo-stack/issues/27)).
- **On Linux** it replaces upstream's `sudo bash restrict-computer-egress.sh` step and its systemd unit, so setup needs no sudo ([#27](https://github.com/kamui/rakazo-stack/issues/27), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **On macOS** it is what keeps bot computers off lemond's unauthenticated child model servers (Kokoro on `:8001`, Whisper on `:8002`, then the next free ports), which `LEMONADE_API_KEY` doesn't cover and which no Lemonade setting can protect ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#25](https://github.com/kamui/rakazo-stack/issues/25), [#26](https://github.com/kamui/rakazo-stack/issues/26)).
- **Setup's check:** `restricted computer egress verified` in the supervisor log; if missing, a checklist item. Never a fallback to open ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- Tested with ufw and firewalld on Linux (rules survive reloads) ([#27](https://github.com/kamui/rakazo-stack/issues/27)). Docker's `firewall-backend: nftables` has no `DOCKER-USER` chain, so computers fail closed; preflight warns ([§3](#preflight)) ([#40](https://github.com/kamui/rakazo-stack/issues/40)).

### Costs and gaps (documented)

- Bot computers can't reach the machine itself, the LAN or the tailnet; on Windows they also lose Docker Desktop's HTTP proxy, which may cut them off the internet on a PC with a system proxy (untested) ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
- The sidecar blocks private addresses only. A computer given Tailscale, VPN or tunnel credentials gets past it, so never give an agent those unless that's the intent. The docs describe one unsupported, untested way to give an agent one home or tailnet service on purpose: a tagged Tailscale auth key with ACLs that let that tag reach only that service ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
- `restricted` checks the rules once per supervisor process. If something wipes the firewall without restarting Docker, new computers can get out until the sidecar re-applies the rules (up to 60 s) ([#27](https://github.com/kamui/rakazo-stack/issues/27), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- Cloud endpoints in public ranges (for example Azure's `168.63.129.16`) stay reachable; this matters only on a cloud VM ([#27](https://github.com/kamui/rakazo-stack/issues/27), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- On Docker Desktop it relies on undocumented internals (`DOCKER-USER` in the engine namespace). If an update removes the chain, computers are refused rather than run open ([#25](https://github.com/kamui/rakazo-stack/issues/25), [#27](https://github.com/kamui/rakazo-stack/issues/27)).
- Optional upstream requests are drafted for the Self-hoster to file: Rakazo re-checking egress before each computer start ([#40](https://github.com/kamui/rakazo-stack/issues/40)), and Lemonade protecting its child servers ([#26](https://github.com/kamui/rakazo-stack/issues/26)).

## 13. Integrations (Composio)

Optional ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#3](https://github.com/kamui/rakazo-stack/issues/3)):

- Rakazo reads one setting, `COMPOSIO_API_KEY`, on `api` and `worker`. Blank means off. It is env-only; Rakazo has no in-app path to save it ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#11](https://github.com/kamui/rakazo-stack/issues/11)).
- Setup asks for the Composio **project** API key, checks it with Composio before saving, and never saves a rejected key ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- **Key check.** Every run with a key sends `GET https://backend.composio.dev/api/v3.1/auth_configs?limit=1` with `x-api-key`: a read that runs no tool, which Composio documents for project keys and Rakazo makes itself. Composio's OpenAPI document lists only user keys for `/auth/session/info`, so setup doesn't use it ([#69](https://github.com/kamui/rakazo-stack/issues/69)).
  - `200` accepts the key. `401` or `403` rejects it. Any other reply, or none, is no verdict. Each result does what it does for the [Model provider key](#key-check). Redirects aren't followed. Setup never reads the reply's body: Composio's refusal quotes part of the key ([#69](https://github.com/kamui/rakazo-stack/issues/69)).
  - A project API key has full access unless it was created as a scoped one. Composio documents that a scoped key can be refused with `401` or `403`. One that can't read auth configs fails this check; one that can, but lacks other access Rakazo uses, passes it and fails later in Rakazo ([#69](https://github.com/kamui/rakazo-stack/issues/69)).
- A blank answer keeps a saved key. To turn integrations off, remove the `COMPOSIO_API_KEY` line from `secrets.env` and run setup again ([#69](https://github.com/kamui/rakazo-stack/issues/69)).
- No per-app auth configs, public URL or webhook. Each Rakazo user connects their own apps in Rakazo; the browser returns to `<WEB_ORIGIN>/integrations/callback` ([#3](https://github.com/kamui/rakazo-stack/issues/3)).
- **Callback (not tested).** Rakazo v0.16.0 passes that address to Composio's SDK as `callbackUrl`. Composio's OpenAPI document types `callback_url` as a URI, and the authentication pages read for [#69](https://github.com/kamui/rakazo-stack/issues/69) on 2026-11-26 state no rule about `http`, `127.0.0.1` or `ts.net` hosts and no step that registers a callback address with Composio. A Composio project can also set a callback verifier URL, which must be public `https`; setup never sets one ([#69](https://github.com/kamui/rakazo-stack/issues/69)).
- Only Composio-managed sign-in works: Spotify, X/Twitter, Shopify and other bring-your-own-OAuth apps can't be connected through Composio ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#34](https://github.com/kamui/rakazo-stack/issues/34)).
- The free Hobby tier (no card, hard-capped, pauses rather than bills) gives 20,000 managed-app tool calls a month. A household's chat use is about 0.5–3K, but a routine watch polls every 5+ minutes and two 5-minute watches nearly use the cap alone. The docs say so ([#34](https://github.com/kamui/rakazo-stack/issues/34)).
- Restoring with a different Composio key orphans existing connections; restore keeps the backed-up key ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

## 14. `rakazo-stack update`

The only command that moves recorded versions ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Steps

1. **Lock.**
2. **Program first.** If a newer rakazo-stack exists, ask "rakazo-stack vX is available (you have vY). Install it? [Y/n]", run the latest `install.sh --no-setup` (same SHA-256 and attestation checks as a fresh install), and hand over to the new program, which keeps the lock. Newer Components may need a newer layer file or floor.
   - **Which release.** The newest in GitHub's list of `kamui/rakazo-stack` releases, never `releases/latest`, which answers 404 while only pre-releases exist. Drafts never count. A program from a release (`vX.Y.Z`) moves only to a newer release. A program from a pre-release (`vX.Y.Z-<suffix>`) also moves to a newer pre-release, in SemVer order (`v0.1.0-rc.2` < `v0.1.0-rc.10` < `v0.1.0`), so a pre-release tester stays on pre-releases until a release passes them, and a release never offers a pre-release. The update nudge counts the same release ([#76](https://github.com/kamui/rakazo-stack/issues/76)).
   - **When.** Right after the lock, before preflight, and only when `settings.json` exists. `--yes` installs without asking. Without a terminal and without `--yes`, `update` stops with nothing changed, as at step 5. A No carries on with the running program. If GitHub can't be reached, `update` stops with nothing changed ([#76](https://github.com/kamui/rakazo-stack/issues/76)).
   - **Install.** `update` downloads the `install.sh` of the release it offered (`releases/download/vX/install.sh`, not `latest`) and runs it with `sh … --no-setup`, so the checks are the release's own. Its "Next: run … setup" line isn't printed. If it fails, the older program stays installed, because `install.sh` replaces it only after both checks pass, and `update` stops with nothing in the Instance changed ([#76](https://github.com/kamui/rakazo-stack/issues/76)).
   - **Hand-over.** The program `exec`s `~/.local/bin/rakazo-stack` with its own arguments, so the newer program runs in the same process. The lock's file descriptor stays open across `exec`, and its number goes in `RAKAZO_STACK_HANDOVER_LOCK_FD`. The newer program takes the lock over only if that descriptor is the Instance folder's `.lock`, so the lock is never free between the two programs. It skips step 2 and goes on from step 3, so an Update installs at most one program ([#76](https://github.com/kamui/rakazo-stack/issues/76)).
3. **Preflight**, plus a free-disk check sized to the images to pull ([§3](#preflight)), made once step 4 knows them.
4. **Resolve** the latest versions of the Components that run (skipping Rakazo releases without images) and fetch the new Rakazo tag's compose file and egress script. Detect any Postgres major move. The new compose file is written beside the old one, which nothing reads yet, and the new versions' image lines go into `.update.env`, which Compose reads over `.env` (`--env-file .env --env-file .update.env`) until the switch. The egress script replaces the old one only for step 6's build. A No, or a failure before the switch, deletes both files and puts the old script back ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
5. **Confirm.** List each Component's old → new version, held Components ("held at v0.5.1, latest is v0.5.3"), skipped releases, any Postgres move ("Postgres 16 → 18: a Backup is required"), any failed earlier Update, and links to each release's notes (Rakazo's "Self-host upgrade notes" can need manual steps). Ask "Update? [Y/n]"; without a terminal, and without `--yes`, `update` stops. With nothing newer: "Everything is up to date", and nothing changes beyond a Hold given in the same command.
6. **Fetch while running:** pull every image, build the relay and sidecar, and on macOS download and verify `lemond`. A failed pull or build here, Lemonade's included, is a failure before the switch.
7. **Backup**, if anything it holds changes (Rakazo, Hindsight, either Postgres image, Tailscale). A Lemonade-only Update skips it. Rakazo's `api`, `worker`, `supervisor` and bot computers stay stopped afterwards, so the Backup holds every write. A failed Backup starts them again. The switch starts the services; bot computers stay stopped until Rakazo starts them.
8. **Postgres major move**, if needed ([below](#postgres-major-moves)).
9. **Switch:** rewrite the stack files and `.env`, swap `lemond` on macOS, `docker compose up -d --wait`. Rakazo's `migrate` and Hindsight's startup migrations run. As soon as the new containers start, record the new versions in `settings.json`: the data now belongs to them.
   - The switch is setup's no-questions run on the new versions, so a `RAKAZO_STACK_*` answer in the environment applies as it does there. It records the new versions and the Holds when it rewrites the stack files, just before `up`, and setup's own steps that follow are steps 10 and 11 ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
   - If the run stops before that write, nothing is recorded and anything the Backup stopped starts again on the old versions.
10. `ALTER EXTENSION vector UPDATE` in Hindsight's database (every Update; cheap and safe to repeat) ([#31](https://github.com/kamui/rakazo-stack/issues/31)).
11. **Health checks**, then the database-only fill-ins as in setup. Voice comes back last, because Lemonade re-downloads its backends.
12. **Clean up:** remove the images of replaced versions. A rollback or an older restore pulls them again by digest ([#42](https://github.com/kamui/rakazo-stack/issues/42)).
    - Replaced means an image one of the Instance's containers ran before the switch and none runs after it. The program pulls by digest, so an image that also has a `name:tag` belongs to something else as well and stays. Docker's containerd image store lists a pull by digest as `name@sha256:…`, which isn't such a tag ([#73](https://github.com/kamui/rakazo-stack/issues/73)).
    - On macOS the replaced `lemonade/versions/<ver>/` folder is deleted.

Sources: ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#42](https://github.com/kamui/rakazo-stack/issues/42), [#44](https://github.com/kamui/rakazo-stack/issues/44), [#45](https://github.com/kamui/rakazo-stack/issues/45)).

### Options

- `--yes` skips both questions. With `--yes` and no Backup passphrase in the environment (`RAKAZO_STACK_BACKUP_PASSPHRASE`, [§15](#file-and-encryption)), an Update that takes a Backup stops before changing anything. Without `--yes`, the passphrase is asked after "Update?" unless that variable is set.
- `--no-backup` skips the Backup; refused when Postgres moves.
- `--hold <component>=<version>`, `--unhold <component>` ([§4](#floors-and-holds)).
- If GitHub can't be reached, `update` stops with nothing changed.

Sources: ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Postgres major moves

`update` compares each database volume's major version (`PG_VERSION`) with its image's. On a mismatch the Backup is mandatory: `update` removes the old volume and restores that Backup's dump into a fresh one under the new image, reusing restore's database step. This covers Rakazo's compose file moving from `postgres:16` to `18`, and `hindsight-db` if the stack ever leaves `pg18`. Postgres 18+ refuses to start on an older cluster, so a missed move fails loudly rather than starting empty ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).

- Step 4 reads `PG_VERSION` in the running cluster, and the new image's major from its tag in the new compose file (`postgres:16@sha256:…` is 16, `pgvector/pgvector:pg18` is 18). Step 5 lists the move as "Rakazo's Postgres 16 → 18: a Backup is required", and `--no-backup` stops the Update there, before anything changes ([#74](https://github.com/kamui/rakazo-stack/issues/74)).
- Step 8 first records the new versions in `settings.json`: once the old volume goes, the old versions have nothing to run on. Then, for each moving database, it removes the container (with Hindsight's API, when `hindsight-db` moves) and the volume, starts the service alone under the new image on a fresh volume (`up -d --no-deps --wait`, with `.update.env`), and restores the dump from the Backup just taken. A Backup without that dump fails the move ([#74](https://github.com/kamui/rakazo-stack/issues/74)).

### Failures

- **Steps 1–7** (up to and including the Backup): the Instance stays as it was. Anything stopped restarts on the old versions, nothing is recorded, and `update` exits with the error.
- **A failed Postgres move, or Rakazo, Hindsight or either Postgres failing to start or pass health checks**, is a failed Update. `update` asks "Update failed at <step>. Roll back to the previous versions from the Backup taken at <time>? [Y/n]". Yes runs `restore --replace` on that Backup inside update's lock, without restore's type-the-name confirmation, losing only what the new versions did since they started. With `--yes`, it exits non-zero and prints the exact `restore --replace <file>` command. No leaves the new versions in place; re-running `setup` retries starting them.
  - `<step>` is "the Postgres major move" or "the switch", and `<time>` the Backup's, as `2026-11-27 12:01 UTC`. A failure counts as a failed Update once the new versions are recorded; before that, the old versions start again as for steps 1–7 ([#74](https://github.com/kamui/rakazo-stack/issues/74)).
  - Every failed Update exits non-zero, a rolled-back one included. No prints the same `restore --replace <file>` command, to roll back later ([#74](https://github.com/kamui/rakazo-stack/issues/74)).
- **Lemonade (including backend downloads), Tailscale, the relay, the sidecar and the fill-ins** become checklist items. The Instance stays on the new versions, and re-running `setup` retries them.
- After a rollback, no Hold is added. `settings.json` records the failed Update, and the next `update` shows "The last update to Rakazo v0.5.2 failed on <date> and was rolled back", with the date as `2026-11-27`, before its plan. It shows it on every `update` until one switches to new versions ([§6](#settingsjson), [#74](https://github.com/kamui/rakazo-stack/issues/74)).
- With no pre-update Backup (`--no-backup`), no rollback is offered: `update` explains the data may already belong to the new versions, and the options are fixing forward or restoring an older Backup. It never swaps images back.

Sources: ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

## 15. `rakazo-stack backup`

Asks nothing but the passphrase, and only when `RAKAZO_STACK_BACKUP_PASSPHRASE` is unset, so `update` and cron can run it. Manual only: the program installs no scheduler; the docs show a cron example for Linux ([#30](https://github.com/kamui/rakazo-stack/issues/30)). It runs no preflight: it takes the lock and works on the Instance as it runs ([#71](https://github.com/kamui/rakazo-stack/issues/71)).

### What a Backup holds

| In | Out |
|---|---|
| `settings.json`, `secrets.env` | `.env`, compose files, build folders, `logs/` (setup rewrites them) |
| `pg_dump` of Rakazo's database | Docker images |
| `pg_dump` of `hindsight-db` whenever `hindsight-pgdata` exists, even with memory off | Lemonade volumes and macOS's `~/rakazo-stack/lemonade/` (setup re-downloads them) |
| `appdata` volume (bot computer homes, home revisions, artifacts, attachments), minus `appdata/staging/` and `appdata/mcp-packages/` | `webassets` volume |
| `tailscale-state` volume | |
| The program version, and each Component's recorded version and digest | |

Sources: ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#44](https://github.com/kamui/rakazo-stack/issues/44)).

Inside, the Backup is one tar archive holding these files, in this order ([#71](https://github.com/kamui/rakazo-stack/issues/71)):

| File | What it is |
|---|---|
| `manifest.json` | `format` (1), the `program_version` that wrote it, `created` (UTC, `YYYYMMDDTHHMMSSZ`), `platform` (`linux`, `wsl` or `macos`, so a restore on another OS knows to set Lemonade up afresh, [#72](https://github.com/kamui/rakazo-stack/issues/72)) and `components`, a copy of `settings.json`'s recorded versions and digests |
| `settings.json`, `secrets.env` | The two files, byte for byte |
| `rakazo.dump` | `pg_dump -Fc` of Rakazo's database |
| `hindsight.dump` | `pg_dump -Fc` of `hindsight-db`, when `hindsight-pgdata` exists |
| `appdata.tar` | GNU tar of `appdata`, with numeric owners |
| `tailscale-state.tar` | GNU tar of `tailscale-state`, when the volume exists |

### How it runs

1. Lock, then the passphrase: from `RAKAZO_STACK_BACKUP_PASSPHRASE`, or typed twice on a terminal. With neither, the run stops before anything changes.
2. Stop whichever of Rakazo's `api`, `worker` and `supervisor` are running, and the running bot computers (not Compose services; found by the `rakazo.managed=true` label and a mount of the Instance's `appdata` volume, so another Instance's computers on the same engine are left alone). Postgres, `web`, Hindsight's API and Tailscale keep running.
3. With memory off but `hindsight-pgdata` kept, start `hindsight-db` alone for its dump and remove its container afterwards, as setup leaves it ([#1](https://github.com/kamui/rakazo-stack/issues/1)). `.env` has no `hindsight-db` image while memory is off, so Compose gets a second env file, `.backup.env` in the Instance folder, holding only `HINDSIGHT_DB_IMAGE_TAG` from `settings.json`, and deleted afterwards. Dump each database inside its container: `docker compose exec -T <service> pg_dump -Fc --no-owner --no-privileges …` (without `-T`, terminal allocation corrupts the dump). Postgres volumes are never copied, so dumps survive Postgres major moves and moves between OSes.
4. Archive `appdata` (and `tailscale-state`) in a throwaway container of Rakazo's own `postgres` image, which is Debian's and has GNU tar, on no network, with the volume mounted read-only. `staging/` and `mcp-packages/` are left out only at the top of `appdata`: `--anchored --exclude=./staging --exclude=./mcp-packages`. Dumping first means every row has its file.
5. Start again what step 2 stopped, unless `update` asked to keep Rakazo stopped: `docker compose up -d --no-recreate --wait` for the services, then `docker start` for the bot computers. The run prints how long Rakazo was stopped. A failure in steps 2–4 still starts them again. So does Ctrl-C (or SIGTERM) from step 2 until the file is renamed in step 7: the program notes it and carries on, a terminal's Ctrl-C also stops the Docker command running at that moment, and the run starts again what it stopped, writes no file and exits with an error. If starting them again fails too, the error says so.
6. Write the file. Each part waits in a spool folder in the output folder until then, because a tar header needs its size first. The spool is encrypted to an `age` key that only the running program holds, so a run that dies leaves nothing readable, even on an `--output` drive.
7. **Self-check:** decrypt the file and read every part back: `pg_restore --list` on each dump, in a throwaway container of the image that made it, `tar --list` on each archive, and a byte comparison for the three small files. Every pipe step's exit code is checked, a file cut short fails `age`'s last-chunk check, and every part written must be there. The file is written as `.<name>.partial` and renamed only once it passes; on any failure it is deleted and the run exits with an error.

On WSL every call goes through `docker.exe`, and secrets never go in arguments or the environment: dumps and archives come back on standard output, and the self-check sends parts on standard input.

**Measured** on WSL 2 with Docker Desktop, on a test Instance with memory on, one bot computer, 20 MB of random data in its home and a 10,000-row table ([#71](https://github.com/kamui/rakazo-stack/issues/71)): the Backup was 20.7 MB, so beyond its data an Instance adds well under 1 MB, and Rakazo was stopped for 35 s. With memory off, starting `hindsight-db` alone made it 42 s. The stop lasts from stopping the writers until `up --wait` reports them healthy and the bot computers have started; writing and checking the file come after it.

Sources: ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#71](https://github.com/kamui/rakazo-stack/issues/71)).

### File and encryption

- One file per Backup, named `rakazo-stack-backup-<UTC time>.tar.age` (for example `rakazo-stack-backup-20261127T120101Z.tar.age`), mode 600, in `~/rakazo-stack-backups/` (mode 700, outside the Instance folder), or `--output <dir>`, which is created at mode 700 when missing and otherwise keeps its mode ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#71](https://github.com/kamui/rakazo-stack/issues/71)). The extension names both tools that open it by hand: `age --decrypt -o backup.tar <file>`, then `tar -xf backup.tar`.
- Always encrypted with a passphrase the Self-hoster picks, in the `age` format (its scrypt passphrase recipient), implemented inside the program with the `age` crate. The `age` CLI can still decrypt it by hand. No unencrypted option: bot computer homes hold logged-in browser profiles and CLI tokens. In no-questions use the passphrase comes from `RAKAZO_STACK_BACKUP_PASSPHRASE`; an empty value counts as unset. The passphrase is never printed or logged. A lost passphrase makes the Backup unusable, and the docs say so ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#71](https://github.com/kamui/rakazo-stack/issues/71)).
- The program never deletes or rotates Backups ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

## 16. `rakazo-stack restore`

Brings an Instance back from a Backup, on the same machine or a new one, on any supported OS ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

### Rules

- **Lock:** `restore` takes the lock in the Instance folder like every other command, so `setup`, `update` and `backup` can't run beside it. On a machine with no Instance folder it creates the folder (mode 700) to hold `.lock`. A folder holding only `.lock` counts as no Instance, and when `restore` stops before step 4 it removes a folder it created, so a refused restore leaves the machine as it was ([#72](https://github.com/kamui/rakazo-stack/issues/72)).
- **Existing Instance:** refuses if `~/rakazo-stack` holds anything but `.lock`, or if any `rakazo-stack_*` volume exists that no other Compose project claims (its `com.docker.compose.project` label is `rakazo-stack` or missing). `--replace` wipes the current Instance's data after the Self-hoster types its name, the Compose project name `rakazo-stack`, on a terminal; it suggests a Backup first but doesn't take one ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#72](https://github.com/kamui/rakazo-stack/issues/72)).
  - The wipe removes the Instance's containers and its bot computers (`rakazo.managed=true` with its `appdata` mounted), those volumes, and every file in the Instance folder but `.lock`. On macOS it first unloads the lemond LaunchAgent, which runs from that folder. Networks and the sidecar's firewall rules stay; setup reuses them.
  - `--replace` refuses a Backup file inside the Instance folder, which the wipe would delete.
  - `update`'s rollback runs the same `--replace` inside its own lock, without the typed name ([§14](#failures)).
- **Versions:** restore brings back exactly the Component versions and digests recorded in the Backup, so a pre-update Backup is a real way back. If Rakazo or Hindsight is below the program's floor, it says so and restores onto that Component's latest version instead, migrating forward. A Backup made by a newer rakazo-stack (`program_version` above this one's), or in another `format`, is refused, pointing to `rakazo-stack update` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Other OS:** restore keeps the Self-hoster's choices and re-derives the OS-specific parts (native Lemonade on macOS, the `lemonade` container elsewhere). Lemonade's record is an image digest on Linux and Windows but the tarball's SHA-256 on macOS, so when the manifest's `platform` runs Lemonade the other way, restore drops `components.lemonade` and setup resolves Lemonade afresh; it holds no data in the Backup. Archives unpack inside a Linux container, so a case-insensitive Mac disk doesn't matter, and Rakazo's `data-init` resets ownership on every start ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#72](https://github.com/kamui/rakazo-stack/issues/72)).
- **Secrets:** `secrets.env` comes back as-is, byte for byte. `ENCRYPTION_KEY`, `BETTER_AUTH_SECRET` and `HINDSIGHT_API_TENANT_API_KEY` match the data, and pasted keys come back, so Composio connections keep working. A Backup missing one of Rakazo's generated secrets, or Hindsight's database password beside a `hindsight.dump`, is refused rather than given new ones ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **Instance URL:** kept, apart from the Tailscale case below. Changing it afterwards is a normal setup re-run ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- **Tailscale:** with `tailscale-state` restored, the Instance comes back as the same device and Instance URL. A running old copy would cause a "Duplicate node key", so when the Backup holds `tailscale-state` restore asks "Is the machine this Backup came from switched off for good? [y/N]", or reads `RAKAZO_STACK_OLD_MACHINE_OFF` (`yes` or `no`); with neither it stops before changing anything. If not, it leaves the state out and clears `tailscale_dns_name`, and with Tailscale access the Instance rejoins with a new auth key (`RAKAZO_STACK_TS_AUTHKEY`, or typed), which gives a new device name (for example `rakazo-1`), so the Instance URL changes and restore prints the sign-out warning. `--replace` on the same machine skips the question: the replaced Instance's `tailscale_dns_name` is the Backup's, so the copy being wiped is the old one ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#72](https://github.com/kamui/rakazo-stack/issues/72)).
- `restore` doesn't run the update check ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Steps

1. Lock and setup's preflight (Docker versions, ports, the WSL home check, GitHub).
2. Decrypt and verify the Backup and apply the version rule. The passphrase comes from `RAKAZO_STACK_BACKUP_PASSPHRASE`, or is typed once on a terminal. Every part is read through `age`, whose chunks are authenticated, and the tar must hold every required part in order, as in backup's self-check. The dumps and archives were read back by `pg_restore --list` and `tar --list` when the Backup was written, and `age` proves they haven't changed since, so this step runs no container. Latest versions for Components below the floor are looked up here. The machine is untouched until this passes ([#72](https://github.com/kamui/rakazo-stack/issues/72)).
3. Check for an existing Instance; ask the Tailscale question.
4. With `--replace`, wipe the Instance. Write `settings.json` and `secrets.env` into the Instance folder with the right modes.
5. Write the stack files as setup's step 5 does, and a provisional `.env` that setup rewrites. Start only `postgres` and `hindsight-db` on fresh volumes (`up -d --no-deps --wait`), wait until each answers over TCP (`pg_isready -h 127.0.0.1`, since the image's first start serves only its socket until initialization ends), create the `rakazo_app` role Rakazo's dump expects (`NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION`, as Rakazo's row-level security migration makes it), and run `pg_restore --no-owner --no-privileges --single-transaction --exit-on-error` for each, with the dump on standard input. This runs before Rakazo's `migrate` and Hindsight's API, which both migrate on start; `migrate` then gives `rakazo_app` its login, password and grants. `update`'s Postgres major move reuses this step ([§14](#postgres-major-moves)).
6. Unpack `appdata`, and `tailscale-state` if kept, in a throwaway container of Rakazo's own `postgres` image on no network (GNU tar, `--numeric-owner`). Each volume is created first with Compose's `com.docker.compose.project` and `com.docker.compose.volume` labels, so Compose takes it as the Instance's own.
7. Hand over to setup's no-questions run. It rewrites the stack files and `.env`, adds the OS-specific parts, pulls models and starts everything, finds the Owner, voice and memory settings already in the database (so it never needs the Owner password), and ends with the usual checklist.

**Failure:** restore doesn't resume. If it fails at step 4 or later, it names the failed step and leaves the half-restored Instance for inspection. The fix is `restore --replace` with the same file; the Backup file is never modified.

Sources: ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#44](https://github.com/kamui/rakazo-stack/issues/44), [#72](https://github.com/kamui/rakazo-stack/issues/72)).

## 17. Running the Instance day to day

There are no `start`, `stop`, `status` or `logs` subcommands. From `~/rakazo-stack` ([#41](https://github.com/kamui/rakazo-stack/issues/41)):

| Task | Linux, macOS | Windows (in WSL) |
|---|---|---|
| Start | `docker compose up -d` | `docker.exe compose up -d` |
| Stop (data kept) | `docker compose down` | `docker.exe compose down` |
| Status, logs | `docker compose ps`, `docker compose logs` | `docker.exe compose ps`, `docker.exe compose logs` |

On Windows, plain `docker` in WSL can leave parts of the Instance unable to start after a reboot; the next `setup` or `update` repairs it ([§7](#windows-docker-through-dockerexe)) ([#41](https://github.com/kamui/rakazo-stack/issues/41)).

On macOS, the voice engine runs outside Docker as the LaunchAgent, so `docker compose down` doesn't stop it.

## 18. Distribution of the program

### The program

- One Rust program, built with a stable toolchain pinned in `rust-toolchain.toml`, into a single native executable per platform: `x86_64-unknown-linux-gnu` (glibc ≥ 2.17; also serves WSL) and `aarch64-apple-darwin` (macOS 13.0+). Linux builds on `ubuntu-latest` with `cargo zigbuild --target x86_64-unknown-linux-gnu.2.17`, and CI fails if the executable uses a glibc symbol newer than 2.17. macOS builds natively on `macos-latest`, because building a Mac executable on Linux needs Apple's SDK ([ADR 0003](adr/0003-rust-for-the-program.md), [#22](https://github.com/kamui/rakazo-stack/issues/22)).
- Apple's linker writes an ad-hoc signature into every arm64 executable, and `curl` downloads aren't quarantined, so no Apple Developer ID or notarization is needed in v1. No browser download of the macOS asset is offered (a browser-quarantined copy fails Gatekeeper) ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#22](https://github.com/kamui/rakazo-stack/issues/22)).
- Bundled in the program: the layer file `compose.stack.yaml`, the relay and sidecar build folders, the relay and sidecar base image references, and the default models. Fetched per Rakazo tag at run time: Rakazo's compose file and `restrict-computer-egress.sh` ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- `rakazo-stack --version` prints the program version (used by the release smoke test) ([#22](https://github.com/kamui/rakazo-stack/issues/22)).
- The build bakes the release tag into the program (`RAKAZO_STACK_RELEASE_TAG` at compile time), because `v0.1.0-rc.1` and `v0.1.0-rc.2` both have version `0.1.0` and `update` must tell them apart ([§14](#steps)). A build without a tag, or with one that isn't of `Cargo.toml`'s version (CI's `v0.0.0-ci`), counts as the release `vX.Y.Z`. `v0.1.0-rc.1` was built before this, so it counts as `v0.1.0` and never moves to another pre-release ([#76](https://github.com/kamui/rakazo-stack/issues/76)).

### `install.sh` and the one-liner

v1 installs only through a `curl … | sh` one-liner that runs `install.sh` from the GitHub release ([#35](https://github.com/kamui/rakazo-stack/issues/35)):

- `https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh` installs the newest program; `https://github.com/kamui/rakazo-stack/releases/download/vX.Y.Z/install.sh` installs an exact version, which is also how to go back a version ([#35](https://github.com/kamui/rakazo-stack/issues/35)). v1 has no shorter address for the one-liner ([#75](https://github.com/kamui/rakazo-stack/issues/75)).
- Detects with `uname -s` / `uname -m`. Supports exactly `Linux x86_64` and `Darwin arm64`, handles Rosetta, and refuses everything else. Detects musl/Alpine and stops, because v1 ships only the glibc executable: "Alpine/musl Linux isn't supported; use a glibc distro such as Ubuntu or Debian" ([#22](https://github.com/kamui/rakazo-stack/issues/22), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Downloads the matching program to a temp file, checks it against SHA-256 digests baked into `install.sh`, then renames it into place at `~/.local/bin/rakazo-stack`. No `sudo` ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- The release workflow fills the release tag and the digests into `install.sh`, and the script downloads from that tag. A copy without them, such as the one in the repo, installs nothing. The shell reads the whole script before running any of it, so a download of `install.sh` cut off part way does nothing ([#75](https://github.com/kamui/rakazo-stack/issues/75)).
- If `gh` is installed and signed in, runs `gh attestation verify`; a failure stops the install and installs nothing. Without `gh`, prints one line ("build origin not checked; to check it yourself: …") and continues ([#35](https://github.com/kamui/rakazo-stack/issues/35)). The check is `gh attestation verify <download> --repo kamui/rakazo-stack`, run before anything is installed. A `gh` too old to have `gh attestation verify` counts as no `gh` ([#75](https://github.com/kamui/rakazo-stack/issues/75)).
- If `~/.local/bin` isn't on `PATH`, prints the exact line to add. Never edits shell profiles ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- When a terminal is available, ends by running `~/.local/bin/rakazo-stack setup` with input from `/dev/tty`. With no terminal, or `sh -s -- --no-setup`, it only installs and prints the next command ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Installing a newer program changes nothing in the running Instance until setup or update runs ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- The docs also show how to download `install.sh`, read it, then run it ([#35](https://github.com/kamui/rakazo-stack/issues/35)).

### Releases

- Immutable GitHub releases holding the two binaries, named `rakazo-stack-x86_64-unknown-linux-gnu` and `rakazo-stack-aarch64-apple-darwin`, and `install.sh` (with their digests baked in), plus build attestations for all three ([#22](https://github.com/kamui/rakazo-stack/issues/22), [#35](https://github.com/kamui/rakazo-stack/issues/35), [#75](https://github.com/kamui/rakazo-stack/issues/75)). Immutability is a repository setting that must be on; the release workflow fails if what it published isn't immutable.
- Pushing a tag publishes: `vX.Y.Z` a release, `vX.Y.Z-<suffix>` a pre-release, which `releases/latest` never serves. `X.Y.Z` must be the program's version in `Cargo.toml`, so a pre-release's program reports `X.Y.Z` ([#75](https://github.com/kamui/rakazo-stack/issues/75)).
- Each release is gated on a `macos-latest` (arm64) smoke test: `codesign --verify --strict`, then `rakazo-stack --version`. Apple silicon kills an executable whose signature is invalid, so a broken build must not reach a release ([#22](https://github.com/kamui/rakazo-stack/issues/22)).
- Before publishing, the baked `install.sh` also installs the built programs on Linux and macOS, including from a shell under Rosetta. After publishing, the workflow runs the release's one-liner on Linux and macOS with the build origin checked ([#75](https://github.com/kamui/rakazo-stack/issues/75)).
- No Homebrew, `.pkg`, `.exe`, zip or `git clone` path in v1 ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- No self-update command beyond `rakazo-stack update`, which updates the program first ([§14](#14-rakazo-stack-update)). A periodic self-offer like `omz update` is for after v1 ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#35](https://github.com/kamui/rakazo-stack/issues/35)).

## 19. Testing and CI

### Weekly canary

Nobody tracks upstream releases by hand. A weekly GitHub Actions job on Linux, [`canary.yml`](../.github/workflows/canary.yml) (Mondays 05:23 UTC, or by hand), runs four parts. Each is one integration test in [`tests/setup.rs`](../tests/setup.rs), on a runner of its own, and one failing doesn't stop the others ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#48](https://github.com/kamui/rakazo-stack/issues/48), [#77](https://github.com/kamui/rakazo-stack/issues/77)).

| Part | What runs | Test |
|---|---|---|
| `fresh-install` | A fresh install against the latest versions, with voice. Memory's fresh install is the `memory` part | `a_scripted_first_run_creates_the_owner_and_a_rerun_changes_nothing` |
| `update` | An Instance on the versions of the last passing run, with memory on, runs `update`. The Owner still signs in, the memories are kept, Hindsight is healthy, voice works and bot computers' egress is verified | `canary_update_starts_from_the_last_passing_versions` |
| `memory` | Creates a **second** Hindsight bank on a populated database, so a missing `shm_size` shows up | `memory_runs_hindsight_connects_the_default_space_and_turns_off_and_on` |
| `end-to-end` | With a real key, a bot is told a few things on the default model, and another bot in the same Space recalls one of them | `canary_a_real_key_learns_and_recalls_through_rakazo` |

- **The `memory` part needs no Model provider key:** it seeds memories without the LLM, as [#48](https://github.com/kamui/rakazo-stack/issues/48)'s `repro.sh` did. Hindsight runs on a placeholder key, a bank is seeded in chunks mode, `memory_units` is grown past 8 MB by copying rows in SQL (seconds, where `repro.sh` took 8 minutes of writes), and two new banks are created at once. Its test also runs on every pull request ([#66](https://github.com/kamui/rakazo-stack/issues/66)).
- **The `update` part** installs the recorded versions and digests, with a placeholder key, then updates behind a Backup. Before any run has passed, or once the record has expired, it starts from Rakazo's and Hindsight's floors, with the latest Lemonade. Its test also runs on every pull request, from the floors. A program release that raises Hindsight's floor raises the test's starting version with it, and a record holding an older Hindsight, which `setup` would refuse, is then deleted by hand.
- **The `end-to-end` part** runs when the repo secret `OPENROUTER_API_KEY` is set, on the default model (`~z-ai/glm-flash-latest`). Both bots use the shared memory scope. A bot created with a model set up starts a hidden intro run that asks the Owner what it is for and waits on that card, so the test stops each one first, as the chat's Stop button does; any later run waiting on a question or approval card fails the part at once. The test waits up to 10 minutes for an observation about the fact in the Space's team bank, then asks the second bot, which never saw the first one's conversation. The key is a separate OpenRouter key with a spending limit of about $1 a month. Only this part's run step receives it, and it is never printed in the log. A failure prints the runs, each bot's last messages, the Hindsight banks' memory counts and the last lines of the `api`, `worker`, `supervisor` and `hindsight` logs, with the key, any eight characters of it and anything shaped like an OpenRouter key taken out. Without the secret, as on a fork, the part is skipped and the run says so ([#77](https://github.com/kamui/rakazo-stack/issues/77)).
- **The record** of the last passing versions is the Actions artifact `canary-last-passing`: `settings.json`'s `components` as the `update` part left them. Every run uploads it again, with the new versions when no part failed and the old ones otherwise, so its 90-day retention doesn't run out while the canary runs. Only runs of `main` in this repository are read ([#77](https://github.com/kamui/rakazo-stack/issues/77)).
- **Issues.** Each failed part has one open issue, titled `Canary: <part> failed` and labelled `canary`. It names the run, the versions the part's Instance recorded (for `update`, also the versions it started from) and the latest upstream releases. While that issue is open, a later failure of the same part comments on it. The failed part is what counts as the cause. Closing the issue is by hand ([#77](https://github.com/kamui/rakazo-stack/issues/77)).
- **By hand:** `gh workflow run canary.yml -f fail=<part>` forces that part to fail after it has run. `--ref <branch>` tries a branch, which reads `main`'s record, never replaces it, and opens issues like any run.
- **Permissions:** the parts have `contents: read` and `actions: read`; the reporting job has `contents: read` and `issues: write`.

macOS isn't covered (GitHub's Mac runners can't run Docker). There is no canary for the ZDR only probe, because its result depends on the privacy settings of the account that owns the key ([#39](https://github.com/kamui/rakazo-stack/issues/39)).

### Test Instances

Tests on a machine that also runs a real Instance need their own. The Instance folder already follows `$HOME`, and four environment variables, for tests only and not documented for Self-hosters, change what the spec otherwise fixes: `RAKAZO_STACK_TEST_PROJECT` (the Compose project name), `RAKAZO_STACK_TEST_WEB_PORT` (the web port), `RAKAZO_STACK_TEST_RAKAZO_FLOOR` (a lower Rakazo floor, so that a test can install a release below it and update it, [#73](https://github.com/kamui/rakazo-stack/issues/73)) and `RAKAZO_STACK_TEST_NEXT_RAKAZO` (`<tag>=<compose file>`: an unpublished Rakazo release, which `update` takes as the latest, running the latest release's images under `<tag>` with that compose file and the current egress script, so a test can make the next release move Postgres or fail to start without editing Rakazo's file, [#74](https://github.com/kamui/rakazo-stack/issues/74)). The integration tests run `setup` with a temporary `$HOME`, their own project and a free port, then remove the project with its volumes and networks. CI runs them on `ubuntu-latest`; the dev machine runs them on WSL.

Every test Instance's sidecar applies the engine-wide rules ([§12](#the-computer-egress-sidecar)), so each test also removes them when it ends, even after a failed assertion, before `compose down`. Before its first `setup`, a test reads the `rakazo-c` rules in the engine's `DOCKER-USER` chain, on both iptables backends, and checks for other running `computer-egress` sidecars. If rules were there with no sidecar keeping them, they belong to someone else, such as a stopped Instance or upstream's own install, and the test leaves them. Otherwise, when the test ends, it stops its own sidecar, looks again for other running sidecars, and runs `--remove` only if there are none. Stopping first means that when two test Instances end together, the last one to stop still removes the rules. The egress test removes the rules part-way through, so it fails at the start on an engine where another Instance has rules.

On a Mac the LaunchAgent's label follows `RAKAZO_STACK_TEST_PROJECT` ([§9](#macos-native-lemond)), and its plist follows `$HOME`, so a test never replaces the real Instance's. lemond's port doesn't move: a test Instance can't run beside a real one on the same Mac, and its preflight stops naming the real lemond ([#70](https://github.com/kamui/rakazo-stack/issues/70)).

`install.sh` has one such variable too: `RAKAZO_STACK_TEST_DOWNLOAD_BASE` replaces `https://github.com/kamui/rakazo-stack/releases/download` with a fake release, over `http` or `file`. The SHA-256 and build origin checks still apply ([#75](https://github.com/kamui/rakazo-stack/issues/75)). Its tests run with a temporary `$HOME` and never start the real program's `setup`. The program reads the same variable, over `http` only: `update`'s step 2 and the update nudge then read the list of releases from `<base>/releases.json`, in the shape of GitHub's reply, and `install.sh` from `<base>/<tag>/install.sh` ([#76](https://github.com/kamui/rakazo-stack/issues/76)).

### Release gate

The `macos-latest` smoke test and the `install.sh` runs in [§18](#releases) ([#22](https://github.com/kamui/rakazo-stack/issues/22), [#75](https://github.com/kamui/rakazo-stack/issues/75)). CI runs the same builds and checks on every pull request, and `shellcheck` on the shell scripts.

### Acceptance checks not yet run

Each of these was decided from source, docs or a partial run and must pass during implementation:

- **Windows:** a reboot and `wsl --shutdown` with the `docker.exe` layout: the sidecar's rules come back, the supervisor verifies, and the Instance runs without a manual start. Also a Docker Desktop restart. And whether rules left by a removed sidecar are gone after each of these ([§12](#the-computer-egress-sidecar)) ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **macOS:** a real LaunchAgent (mode-600 plist, `KeepAlive`, start at login, the name shown in Login Items for an ad-hoc-signed binary, logging) ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#17](https://github.com/kamui/rakazo-stack/issues/17)).
- **macOS:** the Tailscale container ([#24](https://github.com/kamui/rakazo-stack/issues/24)); real lemond children behind the sidecar's rules ([#25](https://github.com/kamui/rakazo-stack/issues/25)); whether Docker Desktop for Mac's default `/dev/shm` is also 64 MB ([#48](https://github.com/kamui/rakazo-stack/issues/48)).
- **Linux:** a real reboot with the sidecar; a home LAN, tailnet and global IPv6; the legacy-iptables wrapper on a real legacy host (it passed in Docker-in-Docker on both backends, `computer-egress/test/image.sh`) ([#27](https://github.com/kamui/rakazo-stack/issues/27), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **Updates:** `install.sh --no-setup` driven from the running program, against two real published pre-releases that both have the baked tag: `update` on the older one installs the newer one, hands over, and the Update finishes under the newer program with the lock held throughout ([#32](https://github.com/kamui/rakazo-stack/issues/32)). Against fake releases served on localhost through `RAKAZO_STACK_TEST_DOWNLOAD_BASE`, with each release's own baked `install.sh`, it passed on WSL 2: the newer program ran in the same process with the same arguments and the lock still held, a failed SHA-256 or build origin check kept the older program and changed nothing, a No carried on to preflight with the running program, and the newer program took the lock over and skipped step 2 (`tests/install.rs`, [#76](https://github.com/kamui/rakazo-stack/issues/76)). A Postgres 16 → 18 move on Rakazo's images compose layout and the rollback path passed on WSL 2 with Docker Desktop, on v0.16.0 moved to fake newer tags whose test copies of its compose file run `postgres:18` (`update_moves_postgres_16_to_18_and_rolls_a_failed_update_back`, [#74](https://github.com/kamui/rakazo-stack/issues/74)): a failed Update, after its move to 18, was rolled back on a terminal to v0.16.0 and Postgres 16 with every row kept and no Hold added; the next Update showed the failure, moved to 18 and kept every row and the Owner; under `--yes` a failure printed its `restore --replace` command, and with `--no-backup` none was offered and the new versions stayed. An Update across Rakazo releases with the layer file passed on WSL 2 with Docker Desktop: v0.15.0, installed with the test floor, moved to v0.16.0 behind a Backup with the Owner kept, and its egress check passed on the new tag's script (`update_moves_rakazo_across_releases_behind_a_backup_and_honours_holds`, [#73](https://github.com/kamui/rakazo-stack/issues/73)).
- **Backup and restore:** a Linux-to-macOS round trip, which must also check voice: the restored voice connection names the Whisper model of the OS it came from, and setup leaves a present connection alone ([#30](https://github.com/kamui/rakazo-stack/issues/30)). Restoring `tailscale-state` on a real tailnet: the same device when the old machine is off, and a `-1` name and new Instance URL when it isn't; the integration test only shows the volume restored or left out, since no test joins a tailnet ([#72](https://github.com/kamui/rakazo-stack/issues/72)). The `rakazo_app` role step against Rakazo's images compose file is proven by the integration test `restore_brings_the_instance_back_on_its_recorded_versions`, a backup-then-restore round trip into a fresh HOME and project ([#72](https://github.com/kamui/rakazo-stack/issues/72)).
- **Hardware:** a real slow 4-thread x86-64 CPU; Linux on bare metal (Linux figures are derived from Docker Desktop's VM) ([#42](https://github.com/kamui/rakazo-stack/issues/42), [#46](https://github.com/kamui/rakazo-stack/issues/46), [#47](https://github.com/kamui/rakazo-stack/issues/47)).
- **Rakazo calls:** a voice call with a bot through Lemonade in a browser, which needs a Model provider key; speech and transcription through the browser's own `/api/voice/*` routes passed without one ([#64](https://github.com/kamui/rakazo-stack/issues/64)). With a real OpenAI or Gemini key: `models.connect` accepting it, and the Owner's new bot replying on the model with no step in Rakazo's UI. The call's route, body, detection query, new-key update and refusal of a bad key passed on v0.16.0, the floor and the latest release then, with placeholder keys; the saving runs had the providers made unreachable from `api` ([#67](https://github.com/kamui/rakazo-stack/issues/67)).
- **Memory models:** Hindsight 1.3.0 retaining and recalling with a real OpenAI key on `gpt-6-luna`, and with a real Gemini key on `gemini-3.5-flash-lite`. Both models' documented output limits allow what fact extraction asks for ([§10](#10-memory)); no call has confirmed it ([#44](https://github.com/kamui/rakazo-stack/issues/44), [#67](https://github.com/kamui/rakazo-stack/issues/67)).
- **Weekly canary:** a scheduled run of `main` with all four parts passing, a forced failure of each opening its issue, a repeat commenting on it, an `update` starting from an earlier run's record, and the `memory` part failing on a branch without `shm_size`. All four parts passed on a manual run of a branch, the `end-to-end` part with the real key ([#77](https://github.com/kamui/rakazo-stack/issues/77), [#109](https://github.com/kamui/rakazo-stack/issues/109)).
- **Composio:** whether it accepts a callback to `http://127.0.0.1:5173/integrations/callback` and to a `ts.net` Instance URL; connecting one Composio-managed app as a Rakazo user and having a bot call it; and the key check's accepting reply, which no real key has produced yet. Only its refusals were recorded, with placeholder keys ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#8](https://github.com/kamui/rakazo-stack/issues/8), [#69](https://github.com/kamui/rakazo-stack/issues/69)).
- **Tailscale through setup:** with a real auth key, the Instance answering at its `ts.net` address from another device with a valid certificate; password sign-in at both addresses; only the web app answering on the device; a device-name clash ending on the assigned name; the rejoin with no key after the container is recreated; switching off; and the checklist item with HTTPS certificates off. The container's own behaviour was tested by hand on Windows in [#24](https://github.com/kamui/rakazo-stack/issues/24) ([#68](https://github.com/kamui/rakazo-stack/issues/68)).
- **Phones:** Rakazo's phone app with `BETTER_AUTH_URL` on localhost ([#24](https://github.com/kamui/rakazo-stack/issues/24)). Android end to end is optional: v1 documents Android as untested, and the check runs only if an Android phone is at hand ([#28](https://github.com/kamui/rakazo-stack/issues/28), [#79](https://github.com/kamui/rakazo-stack/issues/79)).

## 20. Known limitations to document

Collected from the decisions above, for the README and docs:

- Windows and macOS: runs only while the Self-hoster is signed in, and only after Docker Desktop has started ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- Docker Desktop's licence terms ([#4](https://github.com/kamui/rakazo-stack/issues/4)).
- The `docker` group is root-equivalent, and it can read every secret ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#29](https://github.com/kamui/rakazo-stack/issues/29)).
- Windows: use `docker.exe compose …` ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- Desktop Chrome, Edge and Safari dictate through the browser maker's cloud ([#15](https://github.com/kamui/rakazo-stack/issues/15)).
- Bot computers can't reach the machine, the LAN or the tailnet; tunnel credentials bypass the sidecar; the cached-check and cloud-endpoint gaps ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
- With Tailscale on, passkeys work only at the `ts.net` address ([#24](https://github.com/kamui/rakazo-stack/issues/24)).
- iOS push is best-effort; Android is untested ([#19](https://github.com/kamui/rakazo-stack/issues/19), [#28](https://github.com/kamui/rakazo-stack/issues/28)).
- The three memory behaviours ([§10](#memory-behaviours-to-document)), and that with OpenAI or Gemini everyone's conversations are learned through the Owner's key ([#45](https://github.com/kamui/rakazo-stack/issues/45), [#50](https://github.com/kamui/rakazo-stack/issues/50)).
- `~z-ai/glm-flash-latest` can move to a different model between Updates ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- Updates are best effort: nobody tests each upstream release before the Self-hoster gets it, which is why Update takes a Backup and offers a rollback ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Composio's free-tier cap and routine watches ([#34](https://github.com/kamui/rakazo-stack/issues/34)).
- A lost Backup passphrase makes the Backup unusable ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- Removing an Instance is manual ([#17](https://github.com/kamui/rakazo-stack/issues/17)). The steps start with `docker compose run --rm computer-egress --remove` in the Instance folder, before `docker compose down`, and say to skip it when another Instance or a `restricted` Rakazo uses the same Docker engine ([§12](#the-computer-egress-sidecar)).

## 21. Left to the implementation

No ticket fixes these names. The implementation picks them, and the docs then state them:

- the rest of `settings.json`'s schema (its start is in [§6](#settingsjson)).
