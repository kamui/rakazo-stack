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

Every `setup`, `update` and `restore` run starts with preflight. A **stop** ends the run before anything on the machine changes. A **warn** prints a warning and carries on. Nothing in preflight is fixed automatically.

| Check | Result | Ticket |
|---|---|---|
| Another `rakazo-stack` command holds the lock in the Instance folder | stop: "another rakazo-stack command is running" | [#32](https://github.com/kamui/rakazo-stack/issues/32) |
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
| macOS: lemond's stack port is already taken | stop, naming what uses it | [#17](https://github.com/kamui/rakazo-stack/issues/17) |
| Recorded Component versions are below the program's floor | stop: "run `rakazo-stack update`" (`setup` only) | [#32](https://github.com/kamui/rakazo-stack/issues/32) |
| GitHub unreachable | stop on a first `setup`, on `update`; re-runs of `setup` don't need GitHub | [#32](https://github.com/kamui/rakazo-stack/issues/32) |

`update` adds a free-disk check sized to the images it will pull ([#32](https://github.com/kamui/rakazo-stack/issues/32)). Setup checks free disk again before it pulls voice models ([#17](https://github.com/kamui/rakazo-stack/issues/17)).

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
- **Hindsight:** GitHub's latest release of `vectorize-io/hindsight`. Release `vX.Y.Z` maps to image tag `X.Y.Z` (Hindsight's tags have no `v`) ([#44](https://github.com/kamui/rakazo-stack/issues/44)). Tested on 1.3.0.
- **`hindsight-db`:** `pgvector/pgvector:pg18`. It floats within PostgreSQL 18 (minors and security fixes), never to a new major ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Lemonade:** GitHub's latest release of `lemonade-sdk/lemonade`. Linux/Windows use the image of that tag; macOS downloads the embeddable tarball and checks it against GitHub's published SHA-256 for that asset ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Tailscale:** the version behind the `stable` tag in the image registry, not GitHub releases, because images lag releases ([#24](https://github.com/kamui/rakazo-stack/issues/24)).
- **Relay and sidecar bases:** fixed in each program release (for example `oven/bun:1.4.4-alpine`). The relay's ffmpeg comes from Alpine and is pinned loosely (`~=8.1`), because Alpine deletes old package versions ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Recording.** `settings.json` records each Component's running version and image digest ([#32](https://github.com/kamui/rakazo-stack/issues/32)). Because no upstream tag stays put, the stack runs images by recorded digest (for example `RAKAZO_IMAGE_TAG=v0.15.0@sha256:…`), never by a moving tag ([#31](https://github.com/kamui/rakazo-stack/issues/31)).
- **Lemonade models** change only with Lemonade's version: Lemonade runs with `auto_check_model_updates: false` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Never enabled:** Rakazo's `updater` sidecar. Nothing updates on its own ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Who moves versions

- The first `setup` resolves and records the latest versions ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Re-runs of `setup` keep the recorded versions. The one exception: a Component switched on later (memory, Tailscale) starts at its latest version ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Only `rakazo-stack update` moves recorded versions ([§14](#14-rakazo-stack-update)).
- `restore` brings back the versions recorded in the Backup ([§16](#16-rakazo-stack-restore)).

### Floors and Holds

- Each program release knows the lowest version of each Component it can drive. "Latest" has no ceiling ([#32](https://github.com/kamui/rakazo-stack/issues/32)). v1's floors are **Rakazo v0.16.0**, where setup's memory connect call was proven ([#45](https://github.com/kamui/rakazo-stack/issues/45)), and **Hindsight 1.3.0**, the version tested ([#1](https://github.com/kamui/rakazo-stack/issues/1)). If no release at or above the floor has published images, setup stops and says so.
- `rakazo-stack update --hold <component>=<version>` keeps a Component at that version; `--unhold <component>` releases it. Holds live in `settings.json` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- A Hold below the floor is refused. If a newer program raises the floor above an existing Hold, `update` moves that Component to latest and says so in its confirmation; with `--yes` it stops instead, so a Hold is never dropped silently ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

## 5. Compose layout

### Shape

- **One Compose project** named `rakazo-stack` (`COMPOSE_PROJECT_NAME=rakazo-stack` in `.env`; only tests change it, [§19](#test-instances)). Without it, upstream's `name: rakazo` would let a hand-built Rakazo on the same machine share the Instance's volumes ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- **Two compose files**, both in the Instance folder ([#10](https://github.com/kamui/rakazo-stack/issues/10), [#32](https://github.com/kamui/rakazo-stack/issues/32)):
  - `rakazo.<tag>.images.yml`: Rakazo's `infra/compose/docker-compose.images.yml` from the recorded tag, byte for byte.
  - `compose.stack.yaml`: rakazo-stack's layer, bundled in the program.
- Setup writes `COMPOSE_FILE=rakazo.<tag>.images.yml:compose.stack.yaml`, `COMPOSE_PATH_SEPARATOR=:` and `COMPOSE_PROFILES` to `.env`, so plain `docker compose up -d` works from the Instance folder ([#10](https://github.com/kamui/rakazo-stack/issues/10)). The separator is spelled out because Windows' `docker.exe` otherwise splits `COMPOSE_FILE` on `;`.
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
| `lemonade` | `ghcr.io/lemonade-sdk/lemonade-server:<tag>@<digest>`, unmodified | `stack`, `egress` | `LEMONADE_API_KEY`; volumes `lemonade-models:/opt/lemonade/.cache/huggingface`, `lemonade-llama:/opt/lemonade/llama`, `lemonade-data:/opt/lemonade/.cache/lemonade`, `lemonade-config:/opt/lemonade/.config/lemonade`; not published | [#10, #17] |
| `lemonade-relay` | built locally ([§9](#the-lemonade-relay)) | `stack`, `egress` | listens on `13306`; `UPSTREAM=${LEMONADE_UPSTREAM:-http://lemonade:13305}`; `LEMONADE_API_KEY`; not published | [#10, #17] |
| `hindsight` | `ghcr.io/vectorize-io/hindsight-api:<X.Y.Z>@<digest>` (the full image, not `-slim`) | `stack`, `hindsight-data`, `egress` | listens on `8888`; `stop_grace_period: 75s`; healthcheck on `/health`; env in [§10](#hindsight-settings); not published | [#44, #45, #47] |
| `hindsight-db` | `pgvector/pgvector:pg18@<digest>` | `hindsight-data` | `shm_size: 1g`; `POSTGRES_USER=hindsight`, `POSTGRES_DB=hindsight`, generated password; volume `hindsight-pgdata:/var/lib/postgresql`; healthcheck `pg_isready` | [#44, #48] |
| `tailscale` | `tailscale/tailscale:<version>@<digest>` | `app` | `TS_USERSPACE=true`, `TS_AUTH_ONCE=true`, `TS_STATE_DIR=/var/lib/tailscale`, `TS_HOSTNAME`, `TS_AUTHKEY`, `TS_SERVE_CONFIG`; volume `tailscale-state` | [#8, #10, #24] |
| `computer-egress` | built locally ([§12](#the-computer-egress-sidecar)) | `network_mode: host` | `cap_add: [NET_ADMIN]`, `restart: unless-stopped` | [#40](https://github.com/kamui/rakazo-stack/issues/40) |

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
| `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS` | `lemonade-relay` | [#10](https://github.com/kamui/rakazo-stack/issues/10) |
| `SANDBOX_COMPUTER_EGRESS` | `restricted` (never `auto`, never `open`) | [#25, #40] |
| `BETTER_AUTH_URL`, `WEB_ORIGIN`, `API_URL`, `RAKAZO_HOST` | from the access mode ([§11](#11-access-localhost-tailscale-and-phones)) | [#8, #24] |
| `OPENROUTER_API_KEY` or `ANTHROPIC_API_KEY`, `PI_DEFAULT_PROVIDER`, `PI_DEFAULT_MODEL` | OpenRouter or Anthropic only ([§8](#8-model-provider)) | [#36](https://github.com/kamui/rakazo-stack/issues/36) |
| `COMPOSIO_API_KEY` | the pasted key, or blank | [#3](https://github.com/kamui/rakazo-stack/issues/3) |
| `LEMONADE_UPSTREAM` | macOS only: `http://host.docker.internal:<lemond port>` | [#17](https://github.com/kamui/rakazo-stack/issues/17) |

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
  computer-egress/                 build context for the sidecar, with Rakazo's restrict-computer-egress.sh
  lemonade/                        macOS only: versions/<v>/, config, models (HF_HOME)
  logs/                            setup run logs (no secrets); lemond logs on macOS
  .lock                            the lock setup, update, backup and restore take (an OS file lock, released if the program dies)
```

- Setup re-applies the modes on every run ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- Setup rewrites `.env`, both compose files and the build folders on every run. `.env` starts with a "generated, edits are overwritten" line. The docs say not to edit these files ([#29](https://github.com/kamui/rakazo-stack/issues/29), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Compose files sit beside `.env` because Compose reads `.env` and resolves relative paths from the project folder ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- The only data inside the folder is native lemond's files on macOS (about 2 GB of models) ([#29](https://github.com/kamui/rakazo-stack/issues/29)).
- Outside the folder: the macOS LaunchAgent plist in `~/Library/LaunchAgents` (mode 600) ([#17](https://github.com/kamui/rakazo-stack/issues/17)), Backups in `~/rakazo-stack-backups/` ([#30](https://github.com/kamui/rakazo-stack/issues/30)), and the program in `~/.local/bin/rakazo-stack` ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- The lock that `setup`, `update`, `backup` and `restore` take, and the timestamp of the last update check, also live in the Instance folder ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### `settings.json`

JSON, rewritten by setup on every run ([#29](https://github.com/kamui/rakazo-stack/issues/29)). It holds:

- The Self-hoster's choices: Model provider (no key), access mode, Tailscale device name, bring-your-own Instance URL, the Owner's email, whether memory is on ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Optional overrides: one Rakazo model and one memory model. Only overrides are stored; defaults belong to the program version ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#51](https://github.com/kamui/rakazo-stack/issues/51)).
- Recorded versions and image digests for each Component, and the program version that wrote it ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Holds ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Which database-only settings setup filled in (the Owner's voice, the Owner's model connection, the default Space's memory), so a re-run can tell "removed in Rakazo" from "never set" ([#23](https://github.com/kamui/rakazo-stack/issues/23), [#36](https://github.com/kamui/rakazo-stack/issues/36)).
- A failed Update, if the last one was rolled back ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

The schema so far; later slices add the fields above as they land:

```json
{
  "schema": 1,
  "program_version": "0.1.0",
  "components": {
    "rakazo": {
      "version": "v0.16.0",
      "images": { "app": "sha256:…", "computer": "sha256:…" }
    }
  }
}
```

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
| Hindsight's database password | `hindsight-db` and `HINDSIGHT_API_DATABASE_URL` | [#44](https://github.com/kamui/rakazo-stack/issues/44) |
| `HINDSIGHT_API_TENANT_API_KEY` | Hindsight's API key, 8–4096 characters; also stored by Rakazo for each connected Space | [#44, #49] |

#### Pasted keys

The Model provider key, `COMPOSIO_API_KEY` and the Tailscale auth key ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#8](https://github.com/kamui/rakazo-stack/issues/8), [#36](https://github.com/kamui/rakazo-stack/issues/36)). A re-run shows a pasted key as "set, press Enter to keep, or paste a new one" and never prints it ([#9](https://github.com/kamui/rakazo-stack/issues/9)).

## 7. `rakazo-stack setup`

One re-runnable command. It is an interactive wizard on a first run and safe to re-run at any time to change things. A no-questions mode reads `settings.json`, with keys and the Owner password from environment variables, for scripted runs and for `restore`'s hand-over ([#9](https://github.com/kamui/rakazo-stack/issues/9)).

### Questions

| Question | First run | Re-run | Ticket |
|---|---|---|---|
| Owner email and password | Asked. Password at least 16 characters | Password asked only when a database-only setting will be filled in; never saved | [#9, #19, #21] |
| Model provider | OpenRouter (default), OpenAI, Gemini, Anthropic, or none | Pre-filled | [#36](https://github.com/kamui/rakazo-stack/issues/36) |
| Model provider key | Blank allowed. Checked with the provider before saving; a rejected key is never saved | "set, press Enter to keep" | [#9, #36] |
| Composio project API key | Blank allowed. Checked with Composio before saving; a rejected key is never saved | "set, press Enter to keep" | [#3, #9] |
| Access | localhost only (default), or Tailscale | Pre-filled; switching warns first ([§11](#switching-the-instance-url)) | [#8](https://github.com/kamui/rakazo-stack/issues/8) |
| Tailscale auth key and device name | A key that isn't ephemeral; device name default `rakazo` | Device name pre-filled with the name Tailscale actually assigned | [#8, #24] |
| Bring-your-own Instance URL | Advanced, unsupported; mutually exclusive with Tailscale | Pre-filled | [#8](https://github.com/kamui/rakazo-stack/issues/8) |
| Memory | On when a key is given; Enter keeps it on, `off` stops Hindsight but keeps its data | Pre-filled | [#9](https://github.com/kamui/rakazo-stack/issues/9) |

The wizard asks no model questions. The model overrides are edited in `settings.json` ([#36](https://github.com/kamui/rakazo-stack/issues/36)).

Setup **stops before changing anything** on a failed preflight or invalid input: a short Owner password, no Tailscale auth key on a first join, a malformed bring-your-own URL, or a rejected key ([#9](https://github.com/kamui/rakazo-stack/issues/9)).

### First run, step by step

1. **Lock and preflight** ([§3](#preflight)).
2. **Questions** (wizard) or `settings.json` plus environment variables (no-questions mode).
3. **Check keys.** Model provider key, then on OpenRouter the ZDR only probe ([§8](#zdr-only-probe)), then the Composio key ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#36](https://github.com/kamui/rakazo-stack/issues/36), [#39](https://github.com/kamui/rakazo-stack/issues/39)).
4. **Resolve versions** for every Component that will run, skipping releases without published images ([§4](#how-versions-are-resolved)).
5. **Write the Instance folder.** Generate missing secrets; write `secrets.env`, `settings.json`, the fetched Rakazo compose file and egress script, the bundled layer file and build folders, and `.env`; apply modes ([#29](https://github.com/kamui/rakazo-stack/issues/29), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
6. **macOS only: native Lemonade** ([§9](#macos-native-lemond)): download and verify `lemond`, write the plist, load the LaunchAgent, set `whispercpp.backend=metal` ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
7. **Build and pull.** Build `lemonade-relay` and `computer-egress`; pull the other images by digest.
8. **Start.** `docker compose up -d --wait` ([#32](https://github.com/kamui/rakazo-stack/issues/32)). On Windows, first force-recreate any of the Instance's containers that WSL's `docker` made ([below](#windows-docker-through-dockerexe)) ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
9. **Voice models.** Linux and Windows: set `whispercpp.backend=cpu` against the running Lemonade, never `auto`, which picked Vulkan on a software renderer in [#24](https://github.com/kamui/rakazo-stack/issues/24) ([#1](https://github.com/kamui/rakazo-stack/issues/1)). After a free-disk check, pull and load `kokoro-v1` and that OS's Whisper model through the relay, then run one speech and one transcription so the first-load cost (17 s for Whisper on Metal) happens during setup ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#24](https://github.com/kamui/rakazo-stack/issues/24), [#42](https://github.com/kamui/rakazo-stack/issues/42)).
10. **Tailscale** (if on): read the device name Tailscale assigned (for example `rakazo-1` on a clash), derive the Instance URL, and if it differs from what `.env` holds, rewrite `.env` and recreate Rakazo's services ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
11. **Egress check.** Look for `restricted computer egress verified` in the supervisor log. If it's missing, add a checklist item that bot computers are off until it's fixed, with the cause. Never fall back to open egress ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
12. **Database-only settings** ([below](#database-only-settings-in-rakazo)).
13. **Checklist and update nudge** ([below](#the-checklist)).

### Re-runs

- Pre-fill every answer from `settings.json` and the running Instance ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Keep recorded versions; only a Component switched on later starts at its latest version ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Never regenerate a secret; never print a pasted key ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Rewrite the stack files and `.env`, re-apply file modes, and re-run steps 6–13 idempotently.
- Turning memory **on** later: generate Hindsight's secrets, resolve and start Hindsight, connect the default Space ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Turning memory **off** (`off`, or a blank key): drop the `hindsight` profile so `hindsight` and `hindsight-db` stop; keep `hindsight-pgdata`. Nothing is deleted unless separately asked ([#9](https://github.com/kamui/rakazo-stack/issues/9)). Setup doesn't disconnect memory in Rakazo; it finds the Spaces still connected (`SELECT "spaceId" FROM space_memory_configs WHERE provider = 'hindsight'`, read-only) and adds a checklist item for each ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- A re-run started by the install one-liner with a newer program behaves the same: it keeps recorded versions and changes nothing a newer program needs except stack files. If recorded versions are below the new floor, it stops with "run `rakazo-stack update`" ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#35](https://github.com/kamui/rakazo-stack/issues/35)).

### Database-only settings in Rakazo

Rakazo keeps the Owner, each user's voice credential, each user's model connections and each Space's memory connection in its database, encrypted. Setup fills them only through Rakazo's own code paths, never raw SQL writes ([#11](https://github.com/kamui/rakazo-stack/issues/11)). The calls are Rakazo's internal, undocumented `/rpc`. They were proven on v0.4.0 (Owner, voice, memory with Honcho) and v0.16.0 (memory with Hindsight); on other versions they are best effort, and any failure becomes a checklist item with the UI steps ([#21](https://github.com/kamui/rakazo-stack/issues/21), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#45](https://github.com/kamui/rakazo-stack/issues/45)).

Sequence ([#21](https://github.com/kamui/rakazo-stack/issues/21), [#23](https://github.com/kamui/rakazo-stack/issues/23), [#36](https://github.com/kamui/rakazo-stack/issues/36), [#44](https://github.com/kamui/rakazo-stack/issues/44)):

1. Wait for health checks (Rakazo, the relay, Lemonade, and Hindsight when memory is on; Hindsight takes about 60–70 s).
2. **Detect** what exists with read-only SQL as the database owner role in Rakazo's Postgres (no password needed):
   - Owner: a verified `user` with a `credential` account.
   - Voice: any `user_voice_credentials` row for the Owner.
   - Memory: `SELECT 1 FROM space_memory_configs WHERE "spaceId" = $1 AND provider = 'hindsight'` for the Owner's default Space.
   - Model connection (OpenAI/Gemini only): an existing connection for the Owner.
3. **Decide** what to fill:
   - Missing and never filled by setup: fill it.
   - Missing but `settings.json` says setup filled it before: treat it as removed in Rakazo's UI. The wizard asks "Your voice settings were removed in Rakazo. Add them back? [y/N]" (same for memory and the model connection); no-questions mode leaves it removed ([#23](https://github.com/kamui/rakazo-stack/issues/23)).
   - Present: leave it alone, even if it differs from what setup would write ([#21](https://github.com/kamui/rakazo-stack/issues/21)).
4. Only if something will be filled: ask for the Owner password.
5. Owner missing: `docker compose exec -T -e OWNER_EMAIL -e OWNER_PASSWORD api node dist/cli/bootstrap-owner.js`. Never run `bootstrap-owner` as a check: it ignores arguments (including `--help`) and rewrites `deployment_settings` on every run ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#21](https://github.com/kamui/rakazo-stack/issues/21)).
6. `POST /api/auth/sign-in/email` with `{email, password}`; read `.token`; send it as `Authorization: Bearer …`. Send no `Origin`, cookie or `Sec-Fetch-*` headers. Sign-in is rate-limited to 10 tries per 15 minutes ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#21](https://github.com/kamui/rakazo-stack/issues/21)).
7. Voice missing: `POST /rpc/voice/connect` with `{"json":{"provider":"openai-compatible","baseUrl":"http://lemonade-relay:13306/api/v1","ttsModel":"kokoro-v1","sttModel":"<Whisper model>"}}`. The server lists models and voices before saving and picks the voice (`af_heart`) ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#21](https://github.com/kamui/rakazo-stack/issues/21), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
8. OpenAI or Gemini and no Owner connection: the `models.connect` RPC with the key and `makeDefault`. A new key on a re-run updates this connection ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
9. Memory on and missing: `POST /rpc/memory/connectProvider` with `{"json":{"provider":"hindsight","settings":{"mode":"local","baseUrl":"http://hindsight:8888"},"credentials":{"apiKey":"<HINDSIGHT_API_TENANT_API_KEY>"}}}`. Pass nothing for `retainConversations` or other memory toggles: Rakazo's defaults stay ([#44](https://github.com/kamui/rakazo-stack/issues/44), [#45](https://github.com/kamui/rakazo-stack/issues/45), [#50](https://github.com/kamui/rakazo-stack/issues/50)).
10. `POST /api/auth/sign-out`; discard the token ([#21](https://github.com/kamui/rakazo-stack/issues/21)).
11. Record in `settings.json` what was filled.
12. A non-2xx reply (`{"json":{"code","status","message"}}`) becomes a checklist item with its message and the UI steps; setup carries on ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#21](https://github.com/kamui/rakazo-stack/issues/21)).

Setup never touches other users' settings or other Spaces ([#11](https://github.com/kamui/rakazo-stack/issues/11), [#49](https://github.com/kamui/rakazo-stack/issues/49)).

### The checklist

Every run ends with a checklist of what is left to do by hand ([#9](https://github.com/kamui/rakazo-stack/issues/9)). Items, each only when it applies:

- Any failed database-only fill-in, with the UI steps ([#9](https://github.com/kamui/rakazo-stack/issues/9)).
- Bot computers are off because restricted egress isn't verified, with the cause ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- Docker's firewall backend is `nftables` ([§3](#preflight)) ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
- macOS: the lemond LaunchAgent is off in Login Items or keeps failing, with the fix or the last log lines ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
- Turn on **ZDR only** in OpenRouter (only when the probe found it off or couldn't check) ([#39](https://github.com/kamui/rakazo-stack/issues/39)).
- No Model provider key: each person connects a provider in **Settings → Models**; the Owner connects one before creating a bot, because Rakazo's onboarding stops at "Connect a model" ([#24](https://github.com/kamui/rakazo-stack/issues/24), [#36](https://github.com/kamui/rakazo-stack/issues/36)).
- OpenAI or Gemini: other people connect their own provider in Rakazo ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- Switched away from OpenAI or Gemini: remove the old connection in **Settings → Models**, because until then it stays the Owner's default ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- Memory off but a Space still connected to Hindsight: that Space's owner disconnects it under **Settings → Memory**, because until then each reply first tries a recall that fails ([#1](https://github.com/kamui/rakazo-stack/issues/1)).
- Memory on: how to turn memory on in another Space: **Settings → Memory → Connect memory provider → Hindsight → Advanced → Self-hosted**, `http://hindsight:8888`, and the key from the `HINDSIGHT_API_TENANT_API_KEY` line in `~/rakazo-stack/secrets.env` ([#49](https://github.com/kamui/rakazo-stack/issues/49)).
- Tailscale on: enable MagicDNS and HTTPS certificates in the Tailscale admin console, and turn off key expiry for the device ([#8](https://github.com/kamui/rakazo-stack/issues/8), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- "For private dictation, use Firefox or the Rakazo desktop app" ([#15](https://github.com/kamui/rakazo-stack/issues/15)).
- After an Update: any Component that became a checklist item ([§14](#failures)) ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### The update nudge

`setup` and `backup` check GitHub at most once a day. When the program or any un-held Component has a newer version, they print one line: "Updates available: run `rakazo-stack update`". A failed check is silent. `RAKAZO_STACK_NO_UPDATE_CHECK=1` turns it off. `restore` doesn't check ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Per-OS differences

| | Linux | Windows (WSL 2) | macOS |
|---|---|---|---|
| Docker CLI the program uses | `docker` | `docker.exe` via WSL interop, `--context desktop-linux` | `docker` |
| Lemonade | `lemonade` container | `lemonade` container | native `lemond` LaunchAgent |
| Whisper model | `Whisper-Small` | `Whisper-Small` | `Whisper-Large-v3-Turbo` on Metal |
| `LEMONADE_UPSTREAM` | default (`http://lemonade:13305`) | default | `http://host.docker.internal:<lemond port>` |
| Computer-egress sidecar | host namespace; auto-detects legacy iptables | Docker Desktop's engine namespace | Docker Desktop's VM |
| Extra preflight | Docker Desktop for Linux refused; `docker` without sudo | `/mnt/` home refused; `docker.exe`, interop, Linux containers mode, Windows Home warning, VM memory | lemond port free |
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

- The key goes into `HINDSIGHT_API_LLM_API_KEY` for every provider, including OpenAI and Gemini, which Rakazo itself connects per user ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- With OpenAI or Gemini, memory runs on the Owner's key for everyone, so other people's conversations are learned through the Owner's account. The README FAQ says so ([#50](https://github.com/kamui/rakazo-stack/issues/50)).
- `OPENAI_API_KEY` and `GEMINI_API_KEY` are not written to Rakazo's env: Rakazo doesn't use them for runs ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Overrides.** `settings.json` may hold one Rakazo model and one memory model (`HINDSIGHT_API_LLM_MODEL`). No per-operation Hindsight models (`HINDSIGHT_API_RETAIN_LLM_*`, `…_CONSOLIDATION_LLM_*`, `…_REFLECT_LLM_*`) ([#36](https://github.com/kamui/rakazo-stack/issues/36), [#51](https://github.com/kamui/rakazo-stack/issues/51)).
- **Defaults belong to the program version.** `rakazo-stack update` moves to a newer program's defaults; overrides always win ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Switching provider** never touches stored memories: Hindsight's embeddings run locally and don't depend on the provider ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **`~z-ai/glm-flash-latest` is an OpenRouter alias.** It moves when Z.ai ships a new Flash model, not on an Update. A Self-hoster who wants a fixed model sets the override (for example `z-ai/glm-5.3-flash`) ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Cost.** Measured on OpenRouter defaults with Rakazo v0.16.0 and Hindsight 1.3.0: chat about $1.3 a month and memory about $2.4–3.7 a month for one user at 30 turns a day, so about $3.7–5 for the Instance. A Space on the Shared memory scope roughly doubles the memory part ([#45](https://github.com/kamui/rakazo-stack/issues/45)).

### ZDR only probe

On OpenRouter, after every successful key check, including no-questions runs ([#39](https://github.com/kamui/rakazo-stack/issues/39)):

- Request: one embeddings call for `openai/text-embedding-3-small` with `provider: {only: ["openai"], allow_fallbacks: false}` and the input `zdr check`. OpenAI's endpoint isn't ZDR.
- `404` with `zdr-violation-by-guardrail`: ZDR only is on. No checklist item.
- `200`: it's off (the call costs about $0.00000002). The wizard explains that Rakazo's chat can then go to providers that keep data, links `openrouter.ai/workspaces/default/guardrails`, and offers **Check again** (repeats the probe) or **Continue without it** (default; adds the checklist item). No-questions mode warns and adds the item.
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
- Lemonade runs with `auto_check_model_updates: false` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- Lemonade runs without `--convert` and without ffmpeg on every OS: the relay converts audio ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
- Lemonade re-downloads its backends after every version change, so voice comes back last after an Update ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).

### The Lemonade relay

`lemonade-relay` sits between Rakazo and Lemonade on every OS ([#17](https://github.com/kamui/rakazo-stack/issues/17)). Rakazo's voice Server URL is `http://lemonade-relay:13306/api/v1`.

- **Image:** built locally from a Bun Alpine base fixed in the program (`oven/bun:1.4.4-alpine` today), plus Alpine's ffmpeg (`~=8.1`), with the relay's code copied in. It runs as user `bun` ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#31](https://github.com/kamui/rakazo-stack/issues/31), [#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **Converts every transcription upload** to 16 kHz mono WAV (`ffmpeg -i <in> -ar 16000 -ac 1 -c:a pcm_s16le <out>.wav`) before forwarding it. Browsers upload WebM/Opus (and probably MP4/AAC on Safari), which whisper-server can't decode without ffmpeg ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- **Rewrites `lang_code`** on `/audio/speech`: Rakazo sends Kokoro's one-letter codes (`a`, `b`, `e`, `f`, `h`, `i`, `j`, `p`, `z`), which Kokoros passes to espeak as-is. The relay maps them to `en-us`, `en-gb-x-rp`, `es`, `fr-fr`, `hi`, `it`, `ja`, `pt-br`, `cmn`. This part goes away once [lucasjinreal/Kokoros#144](https://github.com/lucasjinreal/Kokoros/issues/144) ships in Lemonade; the relay stays for the conversion ([#1](https://github.com/kamui/rakazo-stack/issues/1), [#17](https://github.com/kamui/rakazo-stack/issues/17)).
- **Joins Whisper's line breaks** in transcription text: newlines become single spaces, because Whisper breaks lines at its segment boundaries, even mid-sentence. `srt` and `vtt` replies are left as they are, because their line breaks are part of the format ([#24](https://github.com/kamui/rakazo-stack/issues/24), [#1](https://github.com/kamui/rakazo-stack/issues/1)).
- **Adds `Authorization: Bearer $LEMONADE_API_KEY`** to every request. Only the relay holds the key ([#10](https://github.com/kamui/rakazo-stack/issues/10)).
- Forwards to `UPSTREAM`: `http://lemonade:13305` on Linux/Windows, `http://host.docker.internal:<lemond port>` on macOS. Idle timeout 255 s, because transcription can take tens of seconds before the first byte.
- Rakazo accepts the relay as a private endpoint because of `MCP_ALLOW_PRIVATE_ENDPOINT=true` and `RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS=lemonade-relay`. A bare service name is refused without the second setting ([#2](https://github.com/kamui/rakazo-stack/issues/2), [#10](https://github.com/kamui/rakazo-stack/issues/10)).
- The relay must follow Bun onto newer Alpine releases as Alpine's community packages lose support ([#31](https://github.com/kamui/rakazo-stack/issues/31)).

### macOS: native `lemond`

Containerised Lemonade is ruled out on the Mac: the image is amd64 only and Docker Desktop gives containers no GPU ([#5](https://github.com/kamui/rakazo-stack/issues/5)). Setup runs its own `lemond` ([#17](https://github.com/kamui/rakazo-stack/issues/17)):

- **Never reuses** a Lemonade from the upstream `.pkg` or the Homebrew cask `lemonade-server`; never touches one.
- **Install:** download `lemonade-embeddable-<ver>-macos-arm64.tar.gz` with the program's HTTP client (so no quarantine flag), check it against GitHub's published SHA-256, unpack into `~/rakazo-stack/lemonade/versions/<ver>/`. An Update unpacks the new version beside it and swaps ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#17](https://github.com/kamui/rakazo-stack/issues/17), [#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Port:** a fixed loopback port of the stack's own, not 13305, so it can't collide with an upstream Lemonade. lemond exits rather than pick another port, so preflight stops if the port is taken.
- **LaunchAgent** with one fixed label, `RunAtLoad` and `KeepAlive`:
  - Command: `lemond --no-broadcast --port <port> <dir>`. `--no-broadcast` stops the UDP discovery beacon on the LAN ([#16](https://github.com/kamui/rakazo-stack/issues/16)).
  - Environment: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `HF_HOME` inside `~/rakazo-stack/lemonade/`, `LEMONADE_API_KEY` (lemond reads the key from its environment only).
  - Plist in `~/Library/LaunchAgents`, mode 600, rewritten from `secrets.env` on every run.
  - Stdout and stderr go to `~/rakazo-stack/logs/`.
  - Load with `launchctl bootstrap gui/$UID <plist>`; reload (`bootout` then `bootstrap`) only when the plist changed. Never `launchctl unload -w`.
- **Config:** `lemonade config set whispercpp.backend=metal` against the running lemond, not by writing `config.json`. No `-t`, no `metal_args` (gone in Lemonade v2026.45.3) ([#17](https://github.com/kamui/rakazo-stack/issues/17), [#42](https://github.com/kamui/rakazo-stack/issues/42)).
- **Health on every run:** `launchctl print gui/$UID/<label>` shows it loaded, and `/api/v1/health` answers with the key. Otherwise a checklist item with the fix (for example re-enabling it in Login Items) or the last log lines. Setup never falls back to another voice setup.
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

- **On** only when the Self-hoster gives a Model provider key, any of the four. Hindsight refuses to start without an LLM key, so a blank key means memory is off ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **Setup connects memory only in the default Space** ([§7](#database-only-settings-in-rakazo)). Rakazo lets only a Space's owner connect memory, so other Spaces, including other people's, are connected by hand from the checklist and README instructions ([#49](https://github.com/kamui/rakazo-stack/issues/49)).
- **Learn from conversations stays on** (Rakazo's default). Setup passes nothing for `retainConversations` and doesn't ask. It can be turned off per Space under **Settings → Memory → Edit** ([#50](https://github.com/kamui/rakazo-stack/issues/50)).
- **One memory model** for all of Hindsight's work ([#51](https://github.com/kamui/rakazo-stack/issues/51)).
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

### Tailscale

- A stack-managed `tailscale` container joins the Self-hoster's tailnet as its own device, in userspace mode. The host's own Tailscale, if any, isn't used ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Setup asks for a plain auth key that isn't ephemeral (Tailscale removes ephemeral machines after they go offline) and a device name, default `rakazo`. No tags, OAuth or ephemeral nodes. `TS_AUTH_ONCE` plus the `tailscale-state` volume keep the identity, so later restarts need no key ([#8](https://github.com/kamui/rakazo-stack/issues/8), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- Serve gets a Let's Encrypt certificate and proxies HTTPS 443 to `http://web:5173`, keeping the `ts.net` Host header that Rakazo's `RAKAZO_HOST` check needs. Only Rakazo's web app is served; nothing else answers on the device ([#24](https://github.com/kamui/rakazo-stack/issues/24)).
- One-time manual steps (checklist): enable MagicDNS and HTTPS certificates in the admin console (device names then appear in public Certificate Transparency logs), and turn off key expiry for the device ([#8](https://github.com/kamui/rakazo-stack/issues/8), [#24](https://github.com/kamui/rakazo-stack/issues/24)).
- Anyone on the tailnet can open the Instance URL; Rakazo sign-in (sign-ups off) decides who uses it. Tailscale ACLs are documented as optional hardening ([#8](https://github.com/kamui/rakazo-stack/issues/8)).
- Tested on Docker Desktop for Windows with `tailscale/tailscale:v1.104.4`; untested on a Mac ([#24](https://github.com/kamui/rakazo-stack/issues/24)).

### Switching the Instance URL

A re-run can turn Tailscale on or off, or change the bring-your-own URL. Before it does, the wizard warns that this signs everyone out, breaks passkeys made on the old address, makes phones sign in again and leaves old email links stale, and asks to confirm. No data is lost ([#8](https://github.com/kamui/rakazo-stack/issues/8)). MCP servers connected with OAuth may need re-adding (unverified) ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

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

- **Image:** built locally from an Alpine base fixed in the program, with `iptables`, `ip6tables` and Alpine's `iptables-legacy` baked in, and Rakazo's `infra/compose/restrict-computer-egress.sh` from the recorded Rakazo tag built into the image, unmodified. Nothing is downloaded at boot, so computers aren't refused when the internet is down ([#26](https://github.com/kamui/rakazo-stack/issues/26), [#27](https://github.com/kamui/rakazo-stack/issues/27), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **Runs** with `network_mode: host`, `cap_add: [NET_ADMIN]` and `restart: unless-stopped`, re-applying the rules every 60 s. Its rules match only `rakazo-c+` bridges, so the relay and screens are unaffected ([#25](https://github.com/kamui/rakazo-stack/issues/25), [#27](https://github.com/kamui/rakazo-stack/issues/27)).
- **Legacy iptables (Linux):** on every pass, a wrapper checks which backend holds `DOCKER-USER`. If `iptables-legacy -L DOCKER-USER` succeeds and `iptables -L DOCKER-USER` doesn't, it sets `RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy` before running the script ([#40](https://github.com/kamui/rakazo-stack/issues/40)).
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
- No per-app auth configs, public URL or webhook. Each Rakazo user connects their own apps in Rakazo; the browser returns to `<WEB_ORIGIN>/integrations/callback` ([#3](https://github.com/kamui/rakazo-stack/issues/3)).
- Only Composio-managed sign-in works: Spotify, X/Twitter, Shopify and other bring-your-own-OAuth apps can't be connected through Composio ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#34](https://github.com/kamui/rakazo-stack/issues/34)).
- The free Hobby tier (no card, hard-capped, pauses rather than bills) gives 20,000 managed-app tool calls a month. A household's chat use is about 0.5–3K, but a routine watch polls every 5+ minutes and two 5-minute watches nearly use the cap alone. The docs say so ([#34](https://github.com/kamui/rakazo-stack/issues/34)).
- Restoring with a different Composio key orphans existing connections; restore keeps the backed-up key ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

## 14. `rakazo-stack update`

The only command that moves recorded versions ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Steps

1. **Lock.**
2. **Program first.** If a newer rakazo-stack exists, ask "rakazo-stack vX is available (you have vY). Install it? [Y/n]", run the latest `install.sh --no-setup` (same SHA-256 and attestation checks as a fresh install), and hand over to the new program, which keeps the lock. Newer Components may need a newer layer file or floor.
3. **Preflight**, plus a free-disk check sized to the images to pull.
4. **Resolve** the latest versions (skipping Rakazo releases without images) and fetch the new Rakazo tag's compose file and egress script. Detect any Postgres major move.
5. **Confirm.** List each Component's old → new version, held Components ("held at v0.5.1, latest is v0.5.3"), skipped releases, any Postgres move ("Postgres 16 → 18: a Backup is required"), any failed earlier Update, and links to each release's notes (Rakazo's "Self-host upgrade notes" can need manual steps). Ask "Update? [Y/n]". With nothing newer: "Everything is up to date".
6. **Fetch while running:** pull every image, build the relay and sidecar, and on macOS download and verify `lemond`.
7. **Backup**, if anything it holds changes (Rakazo, Hindsight, either Postgres image, Tailscale). A Lemonade-only Update skips it. Rakazo's `api`, `worker`, `supervisor` and bot computers stay stopped afterwards, so the Backup holds every write.
8. **Postgres major move**, if needed (below).
9. **Switch:** rewrite the stack files and `.env`, swap `lemond` on macOS, `docker compose up -d --wait`. Rakazo's `migrate` and Hindsight's startup migrations run. As soon as the new containers start, record the new versions in `settings.json`: the data now belongs to them.
10. `ALTER EXTENSION vector UPDATE` in Hindsight's database (every Update; cheap and safe to repeat) ([#31](https://github.com/kamui/rakazo-stack/issues/31)).
11. **Health checks**, then the database-only fill-ins as in setup. Voice comes back last, because Lemonade re-downloads its backends.
12. **Clean up:** remove the images of replaced versions. A rollback or an older restore pulls them again by digest ([#42](https://github.com/kamui/rakazo-stack/issues/42)).

Sources: ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#42](https://github.com/kamui/rakazo-stack/issues/42), [#44](https://github.com/kamui/rakazo-stack/issues/44), [#45](https://github.com/kamui/rakazo-stack/issues/45)).

### Options

- `--yes` skips both questions. With `--yes` and no Backup passphrase in the environment, `update` stops before changing anything.
- `--no-backup` skips the Backup; refused when Postgres moves.
- `--hold <component>=<version>`, `--unhold <component>` ([§4](#floors-and-holds)).
- If GitHub can't be reached, `update` stops with nothing changed.

Sources: ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Postgres major moves

`update` compares each database volume's major version (`PG_VERSION`) with its image's. On a mismatch the Backup is mandatory: `update` removes the old volume and restores that Backup's dump into a fresh one under the new image, reusing restore's database step. This covers Rakazo's compose file moving from `postgres:16` to `18`, and `hindsight-db` if the stack ever leaves `pg18`. Postgres 18+ refuses to start on an older cluster, so a missed move fails loudly rather than starting empty ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Failures

- **Steps 1–7** (up to and including the Backup): the Instance stays as it was. Anything stopped restarts on the old versions, nothing is recorded, and `update` exits with the error.
- **A failed Postgres move, or Rakazo, Hindsight or either Postgres failing to start or pass health checks**, is a failed Update. `update` asks "Update failed at <step>. Roll back to the previous versions from the Backup taken at <time>? [Y/n]". Yes runs `restore --replace` on that Backup inside update's lock, without restore's type-the-name confirmation, losing only what the new versions did since they started. With `--yes`, it exits non-zero and prints the exact `restore --replace <file>` command. No leaves the new versions in place; re-running `setup` retries starting them.
- **Lemonade (including backend downloads), Tailscale, the relay, the sidecar and the fill-ins** become checklist items. The Instance stays on the new versions, and re-running `setup` retries them.
- After a rollback, no Hold is added. `settings.json` records the failed Update, and the next `update` shows "The last update to Rakazo v0.5.2 failed on <date> and was rolled back".
- With no pre-update Backup (`--no-backup`), no rollback is offered: `update` explains the data may already belong to the new versions, and the options are fixing forward or restoring an older Backup. It never swaps images back.

Sources: ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

## 15. `rakazo-stack backup`

Asks nothing, so `update` and cron can run it. Manual only: the program installs no scheduler; the docs show a cron example for Linux ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

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

### How it runs

1. Lock.
2. Stop Rakazo's `api`, `worker` and `supervisor`, and the bot computers (not Compose services; found by the `rakazo.managed=true` label). Postgres keeps running.
3. With memory off but `hindsight-pgdata` kept, start `hindsight-db` alone for its dump and stop it afterwards ([#1](https://github.com/kamui/rakazo-stack/issues/1)). Dump each database inside its container: `docker compose exec -T <service> pg_dump -Fc --no-owner --no-privileges …` (without `-T`, terminal allocation corrupts the dump). Postgres volumes are never copied, so dumps survive Postgres major moves and moves between OSes.
4. Archive `appdata` (and `tailscale-state`) in a throwaway Linux container with GNU tar. Dumping first means every row has its file.
5. Start everything again (unless `update` asked to keep Rakazo stopped).
6. **Self-check:** decrypt the file, run `pg_restore --list` on each dump, list the archives and check every pipe step's exit code. On any failure, delete the file and exit with an error.

Sources: ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#32](https://github.com/kamui/rakazo-stack/issues/32)).

### File and encryption

- One file per Backup, named `rakazo-stack-backup-<timestamp>`, in `~/rakazo-stack-backups/` (mode 700, outside the Instance folder), or `--output <dir>` ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- Always encrypted with a passphrase the Self-hoster picks, in the `age` format, implemented inside the program. The `age` CLI can still decrypt it by hand. No unencrypted option: bot computer homes hold logged-in browser profiles and CLI tokens. In no-questions use the passphrase comes from an environment variable. A lost passphrase makes the Backup unusable, and the docs say so ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- The program never deletes or rotates Backups ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

## 16. `rakazo-stack restore`

Brings an Instance back from a Backup, on the same machine or a new one, on any supported OS ([#30](https://github.com/kamui/rakazo-stack/issues/30)).

### Rules

- **Existing Instance:** refuses if `~/rakazo-stack` or any `rakazo-stack_*` volume exists. `--replace` wipes the current Instance's data after the Self-hoster types its name to confirm; it suggests a Backup first but doesn't take one ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- **Versions:** restore brings back exactly the Component versions and digests recorded in the Backup, so a pre-update Backup is a real way back. If they're below the program's floor, it says so and restores onto the latest versions instead, migrating forward. A Backup made by a newer rakazo-stack is refused, pointing to `rakazo-stack update` ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Other OS:** restore keeps the Self-hoster's choices and re-derives the OS-specific parts (native Lemonade on macOS, the `lemonade` container elsewhere). Archives unpack inside a Linux container, so a case-insensitive Mac disk doesn't matter, and Rakazo's `data-init` resets ownership on every start ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- **Secrets:** `secrets.env` comes back as-is. `ENCRYPTION_KEY`, `BETTER_AUTH_SECRET` and `HINDSIGHT_API_TENANT_API_KEY` match the data, and pasted keys come back, so Composio connections keep working ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **Instance URL:** kept, apart from the Tailscale case below. Changing it afterwards is a normal setup re-run ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- **Tailscale:** with `tailscale-state` restored, the Instance comes back as the same device and Instance URL. A running old copy would cause a "Duplicate node key", so restore asks whether the old machine is switched off for good. If not, it drops the state and rejoins with a new auth key, which gives a new device name (for example `rakazo-1`), so the Instance URL changes and the sign-out warning applies. `--replace` on the same machine skips the question ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- `restore` doesn't run the update check ([#32](https://github.com/kamui/rakazo-stack/issues/32)).

### Steps

1. Lock and setup's preflight (Docker versions, ports, the WSL home check).
2. Decrypt and verify the Backup (as backup's self-check) and apply the version rule. The machine is untouched until this passes.
3. Check for an existing Instance; ask the Tailscale question.
4. Write `settings.json` and `secrets.env` into a new Instance folder with the right modes.
5. Start only `postgres` and `hindsight-db` on fresh volumes, create the `rakazo_app` role Rakazo's dump expects, and run `pg_restore --single-transaction` for each. This runs before Rakazo's `migrate` and Hindsight's API, which both migrate on start.
6. Unpack `appdata`, and `tailscale-state` if kept, in a throwaway Linux container.
7. Hand over to setup's no-questions run. It rewrites the stack files and `.env`, adds the OS-specific parts, pulls models and starts everything, finds the Owner, voice and memory settings already in the database (so it never needs the Owner password), and ends with the usual checklist.

**Failure:** restore doesn't resume. If it fails after step 4, it names the failed step and leaves the half-restored Instance for inspection. The fix is `restore --replace` with the same file; the Backup file is never modified.

Sources: ([#30](https://github.com/kamui/rakazo-stack/issues/30), [#32](https://github.com/kamui/rakazo-stack/issues/32), [#44](https://github.com/kamui/rakazo-stack/issues/44)).

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

### `install.sh` and the one-liner

v1 installs only through a `curl … | sh` one-liner that runs `install.sh` from the GitHub release ([#35](https://github.com/kamui/rakazo-stack/issues/35)):

- `https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh` installs the newest program; `https://github.com/kamui/rakazo-stack/releases/download/vX.Y.Z/install.sh` installs an exact version, which is also how to go back a version ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Detects with `uname -s` / `uname -m`. Supports exactly `Linux x86_64` and `Darwin arm64`, handles Rosetta, and refuses everything else. Detects musl/Alpine and stops, because v1 ships only the glibc executable: "Alpine/musl Linux isn't supported; use a glibc distro such as Ubuntu or Debian" ([#22](https://github.com/kamui/rakazo-stack/issues/22), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Downloads the matching program to a temp file, checks it against SHA-256 digests baked into `install.sh`, then renames it into place at `~/.local/bin/rakazo-stack`. No `sudo` ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- If `gh` is installed and signed in, runs `gh attestation verify`; a failure stops the install and installs nothing. Without `gh`, prints one line ("build origin not checked; to check it yourself: …") and continues ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- If `~/.local/bin` isn't on `PATH`, prints the exact line to add. Never edits shell profiles ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- When a terminal is available, ends by running `~/.local/bin/rakazo-stack setup` with input from `/dev/tty`. With no terminal, or `sh -s -- --no-setup`, it only installs and prints the next command ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Installing a newer program changes nothing in the running Instance until setup or update runs ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- The docs also show how to download `install.sh`, read it, then run it ([#35](https://github.com/kamui/rakazo-stack/issues/35)).

### Releases

- Immutable GitHub releases holding the two binaries and `install.sh` (with their digests baked in), plus build attestations ([#22](https://github.com/kamui/rakazo-stack/issues/22), [#35](https://github.com/kamui/rakazo-stack/issues/35)).
- Each release is gated on a `macos-latest` (arm64) smoke test: `codesign --verify --strict`, then `rakazo-stack --version`. Apple silicon kills an executable whose signature is invalid, so a broken build must not reach a release ([#22](https://github.com/kamui/rakazo-stack/issues/22)).
- No Homebrew, `.pkg`, `.exe`, zip or `git clone` path in v1 ([#35](https://github.com/kamui/rakazo-stack/issues/35)).
- No self-update command beyond `rakazo-stack update`, which updates the program first ([§14](#14-rakazo-stack-update)). A periodic self-offer like `omz update` is for after v1 ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#35](https://github.com/kamui/rakazo-stack/issues/35)).

## 19. Testing and CI

### Weekly canary

Nobody tracks upstream releases by hand. A weekly GitHub Actions job on Linux runs ([#32](https://github.com/kamui/rakazo-stack/issues/32), [#48](https://github.com/kamui/rakazo-stack/issues/48)):

- a fresh install against the latest versions;
- an `update` from the versions of its last passing run;
- a memory check that creates a **second** Hindsight bank on a populated database, so a missing `shm_size` shows up. It needs no Model provider key: it seeds memories without the LLM and skips Hindsight's LLM check, as [#48](https://github.com/kamui/rakazo-stack/issues/48)'s `repro.sh` did;
- when the repo secret `OPENROUTER_API_KEY` is set, a short end-to-end memory check through Rakazo on the default model (`~z-ai/glm-flash-latest`): a few turns are learned, then recalled. The key is a separate OpenRouter key with a spending limit of about $1 a month, never printed in the log. Without the secret, this part is skipped and the run says so ([#77](https://github.com/kamui/rakazo-stack/issues/77)).

It opens an issue when any part fails. macOS isn't covered (GitHub's Mac runners can't run Docker). There is no canary for the ZDR only probe, because its result depends on the privacy settings of the account that owns the key ([#39](https://github.com/kamui/rakazo-stack/issues/39)).

### Test Instances

Tests on a machine that also runs a real Instance need their own. The Instance folder already follows `$HOME`, and two environment variables, for tests only and not documented for Self-hosters, change what the spec otherwise fixes: `RAKAZO_STACK_TEST_PROJECT` (the Compose project name) and `RAKAZO_STACK_TEST_WEB_PORT` (the web port). The integration tests run `setup` with a temporary `$HOME`, their own project and a free port, then remove the project with its volumes and networks. CI runs them on `ubuntu-latest`; the dev machine runs them on WSL.

### Release gate

The `macos-latest` smoke test in [§18](#releases) ([#22](https://github.com/kamui/rakazo-stack/issues/22)).

### Acceptance checks not yet run

Each of these was decided from source, docs or a partial run and must pass during implementation:

- **Windows:** a reboot and `wsl --shutdown` with the `docker.exe` layout: the sidecar's rules come back, the supervisor verifies, and the Instance runs without a manual start. Also a Docker Desktop restart ([#41](https://github.com/kamui/rakazo-stack/issues/41)).
- **macOS:** a real LaunchAgent (mode-600 plist, `KeepAlive`, start at login, the name shown in Login Items for an ad-hoc-signed binary, logging) ([#16](https://github.com/kamui/rakazo-stack/issues/16), [#17](https://github.com/kamui/rakazo-stack/issues/17)).
- **macOS:** the Tailscale container ([#24](https://github.com/kamui/rakazo-stack/issues/24)); real lemond children behind the sidecar's rules ([#25](https://github.com/kamui/rakazo-stack/issues/25)); whether Docker Desktop for Mac's default `/dev/shm` is also 64 MB ([#48](https://github.com/kamui/rakazo-stack/issues/48)).
- **Linux:** a real reboot with the sidecar; a home LAN, tailnet and global IPv6; the legacy-iptables wrapper (the variables were tested, the wrapper wasn't) ([#27](https://github.com/kamui/rakazo-stack/issues/27), [#40](https://github.com/kamui/rakazo-stack/issues/40)).
- **Updates:** an Update across Rakazo releases with the layer file; a Postgres 16 → 18 move on Rakazo's images compose layout; the rollback path; `install.sh --no-setup` driven from the running program ([#32](https://github.com/kamui/rakazo-stack/issues/32)).
- **Backup and restore:** Backup size and downtime; a Linux-to-macOS round trip; the `rakazo_app` role step against the images compose file ([#30](https://github.com/kamui/rakazo-stack/issues/30)).
- **Voice:** the relay's conversion latency ([#17](https://github.com/kamui/rakazo-stack/issues/17)).
- **Hardware:** a real slow 4-thread x86-64 CPU; Linux on bare metal (Linux figures are derived from Docker Desktop's VM) ([#42](https://github.com/kamui/rakazo-stack/issues/42), [#46](https://github.com/kamui/rakazo-stack/issues/46), [#47](https://github.com/kamui/rakazo-stack/issues/47)).
- **Rakazo calls:** whether voice calls through Lemonade need the Owner's AI-recipient consent (`aiConsent`), which v0.16.0 requires for calls but not for chat or memory; if so, it becomes a checklist item ([#45](https://github.com/kamui/rakazo-stack/issues/45)). The exact path, body and detection query of the OpenAI/Gemini `models.connect` call, never run ([#36](https://github.com/kamui/rakazo-stack/issues/36)).
- **Memory models:** whether OpenAI's `gpt-6-luna` and Gemini's `gemini-3.5-flash-lite` allow the 64,000 output tokens Hindsight's fact extraction asks for; if not, setup lowers `HINDSIGHT_API_RETAIN_MAX_COMPLETION_TOKENS` for that provider ([#44](https://github.com/kamui/rakazo-stack/issues/44)).
- **Composio:** whether it accepts a callback to `http://127.0.0.1:5173` or a `ts.net` Instance URL ([#3](https://github.com/kamui/rakazo-stack/issues/3), [#8](https://github.com/kamui/rakazo-stack/issues/8)).
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
- Removing an Instance is manual ([#17](https://github.com/kamui/rakazo-stack/issues/17)).

## 21. Left to the implementation

No ticket fixes these names. The implementation picks them, and the docs then state them:

- the no-questions option (the README says "not decided yet");
- the environment variables for keys, the Owner password and the Backup passphrase in no-questions use;
- a shorter address for the one-liner, if any (the README says "not decided yet"; the GitHub release URLs above work);
- macOS lemond's port and the LaunchAgent label;
- the variable name for Hindsight's database password in `secrets.env`;
- the names of the locally built relay and sidecar images;
- the update-check timestamp file and the Backup file's extension;
- the rest of `settings.json`'s schema (its start is in [§6](#settingsjson));
- whether the Tailscale auth key is kept in `secrets.env` after the first join.
