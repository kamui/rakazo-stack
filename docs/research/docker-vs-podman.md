# Docker or Podman for Self-hosters?

Research for [#4](https://github.com/kamui/rakazo-stack/issues/4) on the
[v1 spec map](https://github.com/kamui/rakazo-stack/issues/1). Checked 2026-11-02 against
Rakazo v0.4.0 (commit `b9c652f8898eb970b5c6d2d235d3f78b66502068`), Podman v6.1.4 (latest
stable), podman-compose v1.6.0, and current docs.docker.com, docs.podman.io and podman-desktop.io.

## Answer

- **Use Docker.** That means Docker Engine with the Compose plugin on Linux, and Docker Desktop
  on Windows (WSL2) and Apple-silicon macOS. Podman fails the first half of the decision rule:
  Rakazo's sandbox computers do not start on it, rootful or rootless, on any OS. So the
  ease-of-install comparison never decides the outcome.
- **Why computers fail on Podman:** The published Compose file keeps bot homes in the `appdata`
  named volume. Before it creates a computer, Rakazo's supervisor asks the engine for its API
  version and refuses anything below 1.45 (Docker Engine 26+), which it needs for volume
  subpaths. Podman's Docker-compatible API reports `1.44` in v5.8.6+, v6.1.4 and v6.2.0-rc1, and
  `1.41` in v5.7.0 and older. Ubuntu 26.04 ships 5.7.0 and Debian 13 ships 5.4.2. Every computer
  start therefore fails with "Docker Engine 26+ (API 1.45+) is required for bot home volume
  subpaths". Upstream also documents Docker Engine 26+ as a requirement and does not claim
  Podman support.
- **The hypothesis does not hold.** Rootless Podman still needs admin on Linux: you install it
  with `sudo apt/dnf install podman`, and root writes the subuid/subgid entries. Running a
  long-lived rootless stack also needs extra setup:
  - a user socket for the supervisor to call
  - `loginctl enable-linger` so containers keep running after logout
  - a `podman-restart` unit so they come back after a reboot
  - on some systems, a root-written systemd drop-in so the supervisor's CPU limits work
  - a Compose provider installed separately

  Docker on Linux also needs sudo, once: one installer command. On Windows, both engines need
  admin once to enable WSL2, and both offer per-user installers that need no admin after that.
  On macOS, Docker Desktop 4.89+ installs interactively without a password.
- **Compose:** Docker Compose supports every compose-spec feature this stack uses. `podman
  compose` is only a wrapper that runs `docker-compose` when installed, falling back to
  `podman-compose`. podman-compose v1.6.0 handles `dockerfile_inline`, `!override`, `depends_on`
  conditions, external networks and `pids_limit`. It has no code for top-level `configs`
  (Honcho's upstream compose uses `configs.content` for its init SQL). It also treats
  `service_completed_successfully` as merely "stopped".
- **Rootless and privileged ports** don't matter here: every port the stack publishes is above
  1024 and bound to loopback.
- **Licensing:** Docker Engine is Apache-2.0. Docker Desktop is free for personal use, education,
  non-commercial open source and small businesses (fewer than 250 employees AND under $10M
  revenue). Larger companies need a paid subscription for commercial use. The spec should say
  this.
- **Mac GPU doesn't change the answer.** No engine gives containers Metal. Podman with a libkrun
  machine exposes a Vulkan-compute-only virtual GPU (Venus → MoltenVK), and only to images built
  with a patched Mesa. Docker Desktop exposes no GPU to containers on macOS. Metal-accelerated
  Lemonade on macOS will have to run natively on the host whichever engine is chosen.

## Details

### How Rakazo starts its sandbox computers

- The `supervisor` service runs from the app image with `user: root` and mounts
  `/var/run/docker.sock:/var/run/docker.sock` and `appdata:/data`, with
  `DOCKER_SOCKET=/var/run/docker.sock`
  ([docker-compose.images.yml L309–L347](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L309-L347)).
- It talks to the engine through `dockerode` 5.0.1 over that Unix socket
  (`infra/sandboxes/supervisor/package.json`;
  [index.ts L111–L112, L188–L199](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L188-L199)).
  `DOCKER_SOCKET` (or `DOCKER_HOST`) can point it at another socket, for example rootless
  Docker's
  ([self-host-docker-computers.md L32–L33](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)).
  So a Podman socket can be wired in. The problem is what the API returns.
- When the supervisor's `/data` is a **named volume** (as in the published Compose file), each
  bot's home is mounted into its computer as that volume with `VolumeOptions.Subpath`
  ([computer-spec.ts `computerHomeStorage` L497, `containerCreateOptions` L570](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L497-L624)).
  Before creating a computer it calls
  `assertVolumeSubpathSupport((await docker.version()).ApiVersion)`
  ([index.ts L250–L253](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L250-L253)).
  That check throws unless the API is ≥ 1.45
  ([computer-spec.ts L519–L524](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L519-L524)).
- Upstream's documented requirement is "Docker Engine 26+ (API 1.45+ for bot home volume
  subpaths), the Compose plugin"
  ([docs/self-host.md L9, L18](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host.md)).
  The desktop app "does not detect Podman"
  ([README.md L228](https://github.com/elie222/rakazo/blob/v0.4.0/README.md)).
  Searching upstream issues and PRs for "podman" finds nothing about engine support.
- These files are byte-identical in the v0.4.0 reference install's image and at the `v0.4.0`
  tag. The reference install (Docker Desktop on WSL2) reports Engine 29.9.2, API 1.56,
  Compose 5.6.1, and its computers run.

### What Podman's Docker-compatible API does with those calls

- **Version gate (blocking).** `/version` returns
  `ApiVersion = "<major>.<minor>"` of the compat tree's `CurrentAPI`
  ([pkg/api/handlers/compat/version.go L63–L81 @ v6.1.4](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/version.go#L63-L81)).
  That value is `1.44.0` in v6.1.4, v6.2.0-rc1 and v5.8.6+
  ([version/version.go L42–L43 @ v6.1.4](https://github.com/containers/podman/blob/v6.1.4/version/version.go#L42-L43)).
  It is `1.41.0` in v5.7.0 and v5.4.2
  ([v5.7.0](https://github.com/containers/podman/blob/v5.7.0/version/version.go)). The 6.0.0
  release notes say "The supported Docker Compatible API version has been bumped to v1.44"
  ([RELEASE_NOTES.md](https://github.com/containers/podman/blob/v6.1.4/RELEASE_NOTES.md)). So
  `assertVolumeSubpathSupport` throws on every Podman release, rootful or rootless. The value is
  server-reported, so a client-side `DOCKER_API_VERSION` cannot change it.
- Distro packages are older still: Ubuntu 26.04 "resolute" ships 5.7.0, 25.10 ships 5.4.2
  ([Launchpad](https://launchpad.net/ubuntu/+source/podman)), and Debian 13 "trixie" ships 5.4.2
  ([sources.debian.org](https://sources.debian.org/src/podman/)).
- Subpath itself: compat create maps `VolumeOptions.Subpath` to `--mount …,subpath=`
  ([containers_create.go L260–L262 @ v6.1.4](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/containers_create.go#L260-L262)).
  But 6.0.0 fixed "the `subpath` option for volumes when creating containers was ignored", so
  even Podman 5.x silently ignored it (release notes above).
- **Other gaps, after the gate (lower impact):**
  - Compat create has no mapping for `HostConfig.ReadonlyPaths`, so Rakazo's read-only
    `/usr/share/novnc` hardening would be silently dropped (no match for
    `ReadonlyPaths` in `containers_create.go` @ v6.1.4). **Unverified** at runtime.
  - Network create accepts `com.docker.network.bridge.name` but stores it as the interface name,
    and network inspect returns the stored `Options` without it
    ([networks.go L143–L160, L237–L239 @ v6.1.4](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/networks.go#L237-L239)).
    Rakazo's egress probe then reports "probe network lacks the named bridge"
    ([egress-probe.ts L108](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/egress-probe.ts#L108)),
    so `auto` egress falls back to open. That is harmless: restricted egress needs Linux Docker
    Engine with iptables `DOCKER-USER` anyway, and is already unsupported on Docker Desktop
    ([self-host-docker-computers.md, Restricted computer egress](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)).
  - The supervisor finds the web container by the `com.docker.compose.project`/`service` labels
    ([index.ts L1695](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1695)).
    Docker Compose sets these, and so does podman-compose
    ([podman_compose.py L2728–L2735, L2788](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py#L2728-L2735)).
  - Rootless only: the supervisor's default per-computer `NanoCpus` limit fails where the user
    has no `cpu` cgroup delegation ("the requested cgroup controller `cpu` is not available").
    The fix is a root-written `/etc/systemd/system/user@.service.d/delegate.conf`
    ([troubleshooting.md §26](https://github.com/containers/podman/blob/v6.1.4/troubleshooting.md)).
- **Workaround, not recommended:** a Compose override that replaces `appdata` with a host bind
  mount for `supervisor`, `api` and `worker` would skip the subpath path (bind homes use
  `Binds`). That changes upstream's data layout, is not what upstream tests, and would make
  backups differ by engine. **Unverified**, and outside "wrap upstream, never patch" in spirit.

### Install and admin prompts

| | Docker | Podman |
| --- | --- | --- |
| **Linux** | Docker Engine + Compose plugin from Docker's repo or convenience script, with sudo ([install](https://docs.docker.com/engine/install/)). Engine is Apache-2.0. Optional `docker` group (sudo), which is root-equivalent ([post-install](https://docs.docker.com/engine/install/linux-postinstall/)). | `sudo apt-get/dnf install podman` ([installation](https://podman.io/docs/installation)). Rootless also needs root for `/etc/subuid`/`/etc/subgid` entries ([rootless tutorial](https://github.com/containers/podman/blob/v6.1.4/docs/tutorials/rootless_tutorial.md)). For a long-lived stack it also needs: the user socket (`systemctl --user enable --now podman.socket`); `loginctl enable-linger`, or containers stop at logout ([troubleshooting §21](https://github.com/containers/podman/blob/v6.1.4/troubleshooting.md)); `podman-restart.service` so `restart:` policies survive reboot, because there is no daemon ([podman-run --restart](https://docs.podman.io/en/latest/markdown/podman-run.1.html)); cgroup `cpu` delegation on some systems (§26); and a separate Compose provider. |
| **Windows (WSL2)** | Per-user install is the default and "does not require administrator privileges"; "Enabling WSL 2 for the first time also requires administrator privileges". Needs WSL ≥ 2.1.5 and Windows 10 22H2+ or 11 23H2+ ([install](https://docs.docker.com/desktop/setup/install/windows-install/)). | Per-user MSI needs no admin, and WSL must already be installed (`wsl --install` from an admin shell). Requires Windows 11 ([Podman for Windows](https://github.com/containers/podman/blob/main/docs/tutorials/podman-for-windows.md)); Podman 6.0 removed Windows 10 support (release notes). The machine is rootless by default. Podman Desktop also offers a current-user install without admin ([install](https://podman-desktop.io/docs/installation/windows-install)). |
| **macOS (Apple silicon)** | Drag-and-drop install. From 4.89.0 "you aren't asked for a password"; the socket is `~/.docker/run/docker.sock`, and the `/var/run/docker.sock` symlink is optional ([mac install](https://docs.docker.com/desktop/setup/install/mac-install/), [permissions](https://docs.docker.com/desktop/setup/install/mac-permission-requirements/)). | pkg installer from GitHub releases (Homebrew discouraged), then `podman machine init` and `podman machine start` ([installation](https://podman.io/docs/installation)). Whether the pkg prompts for an admin password is **unverified**. |

The stack's Compose bind-mounts `/var/run/docker.sock` into the supervisor. On Docker Desktop
that path resolves inside Docker's VM, as the WSL2 reference install shows. On macOS without
the optional host symlink this is expected to behave the same, but that is **unverified**.

### Compose support for the features this stack uses

Features seen in the reference install's Compose files and upstream's:

- `build.dockerfile_inline` (Lemonade + ffmpeg)
- external networks (`rakazo_app`)
- `extra_hosts: host.docker.internal:host-gateway`
- healthchecks
- `depends_on` with `service_healthy` / `service_completed_successfully`
- named volumes with fixed `name:`
- `volumes: !override`
- top-level `configs` with inline `content` (Honcho)
- `env_file` with `required: false`
- `x-` anchors with `<<:` merge
- `init`, `pids_limit`, `mem_limit`, `security_opt`, `cap_drop`, `network_mode: none`

Coverage by tool:

- **Docker Compose** implements the compose spec, including `!override`/`!reset`
  ([merge reference](https://docs.docker.com/reference/compose-file/merge/)). The reference
  install runs all of the above on Compose 5.6.1.
- **`podman compose`** is "a thin wrapper around an external compose provider such as
  docker-compose or podman-compose", and docker-compose takes precedence when installed
  ([podman-compose(1)](https://docs.podman.io/en/latest/markdown/podman-compose.1.html)).
  Podman Desktop offers to install the Compose binary
  ([setting up Compose](https://podman-desktop.io/docs/compose/setting-up-compose)). Docker
  Compose's builds against Podman's API (`dockerfile_inline`) are **unverified**.
- **podman-compose v1.6.0** has code for `dockerfile_inline`
  ([L3277](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py#L3277)),
  `!override`/`!reset`
  ([L1688](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py#L1688);
  [1.4.0 changelog](https://github.com/containers/podman-compose/blob/v1.6.0/docs/Changelog-1.4.0.md)),
  `depends_on` conditions (1.3.0/1.4.0 changelogs) and `pids_limit` (1.4.0). Gaps:
  - Top-level `configs` exists only as a comment
    ([L2737](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py#L2737)).
  - `service_completed_successfully` maps to "stopped", not exit 0
    ([L1576](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py#L1576)).
  - It runs services in a pod by default (`in_pod`,
    [L2420](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py#L2420)).
- **Engine side on Podman:** `--add-host` accepts `host-gateway` since 4.7.0 (release notes), and
  Podman adds `host.containers.internal` and `host.docker.internal` to `/etc/hosts` by itself
  ([podman-run](https://docs.podman.io/en/latest/markdown/podman-run.1.html)).

### Rootless operation and ports

- Rootless Podman uses pasta networking by default. The compat API forces `bridge` for unset
  network modes "for better docker compat"
  ([containers_create.go L302–L313](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/containers_create.go#L302-L313)).
- Unprivileged users cannot bind ports below 1024 without changing
  `net.ipv4.ip_unprivileged_port_start` as root
  ([troubleshooting §48](https://github.com/containers/podman/blob/v6.1.4/troubleshooting.md)).
  This stack publishes only loopback ports above 1024 (5173, 3100, 13305, 13306, Honcho 8000), so
  this does not matter. Tailscale access is configured separately.
- Rootless Docker exists too: it needs the `uidmap` package and subuid entries, and
  `sudo loginctl enable-linger` to start on boot
  ([rootless mode](https://docs.docker.com/engine/security/rootless/)). Rakazo supports it via
  `DOCKER_SOCKET`, but not restricted egress. It is optional and not the default path.

### Licensing

- Docker Engine: Apache-2.0 ([install](https://docs.docker.com/engine/install/)).
- Docker Desktop: free for personal use, education, non-commercial open source and small
  businesses (< 250 employees AND < $10M revenue). Larger enterprises and government need a paid
  subscription for commercial use
  ([Windows](https://docs.docker.com/desktop/setup/install/windows-install/),
  [Mac](https://docs.docker.com/desktop/setup/install/mac-install/)).
- Podman and Podman Desktop: Apache-2.0
  ([podman LICENSE](https://github.com/containers/podman/blob/v6.1.4/LICENSE)).

### GPU from containers on Apple-silicon macOS

- **Podman Desktop:** a `libkrun` Podman machine exposes a virtualized GPU that translates Vulkan
  via MoltenVK to Metal. It "only supports vulkan compute shaders, not rendering / draw", and
  needs images built with a patched Mesa (`slp/mesa-libkrun-vulkan`) and `--device /dev/dri`
  ([GPU container access](https://podman-desktop.io/docs/podman/gpu)).
- **Docker Desktop:** `--gpus` is "only available on Windows with the WSL 2 backend" (NVIDIA
  GPU-PV). Nothing is offered for macOS containers
  ([GPU support](https://docs.docker.com/desktop/features/gpu/)).
- **For the map:** the settled goal is Lemonade on Metal on macOS, and neither engine provides
  Metal to containers. That points to running Lemonade natively on the Mac host and reaching it
  through `host.docker.internal`, whichever engine is chosen. Whether Lemonade's Kokoro and
  Whisper backends could use Vulkan inside a libkrun container instead is **unverified**.

## Open questions / follow-ups

- **(research)** On macOS, does Lemonade run natively with Metal for both Kokoro TTS and Whisper
  STT, and how does a container reach it (`host.docker.internal` on Docker Desktop)? This
  decides whether the Mac Instance has one native service outside Compose.
- **(grilling)** Docker Desktop's licence covers personal use, but a Self-hoster at a company
  with ≥250 employees needs a subscription. Is that a documented caveat, or should macOS/Windows
  also document an alternative engine (OrbStack, Colima) that meets API ≥ 1.45?
- **(task)** The setup command should preflight `docker version` (Server API ≥ 1.45, Engine 26+)
  and `docker compose version`, and fail with a clear message. This also catches someone
  pointing `DOCKER_HOST` at Podman.
- **(research)** Linux on WSL2: should rakazo-stack require Docker Desktop's WSL integration, or
  also allow Docker Engine installed inside the WSL distro? Docker's Engine install docs don't
  mention WSL.
- **(prototype, optional)** Revisit Podman only if upstream Rakazo lowers the API 1.45 gate, or
  Podman's compat API reaches 1.45 and is shipped by distros.

## Sources

- Rakazo v0.4.0 (`b9c652f`):
  [docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml),
  [supervisor index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts),
  [computer-spec.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts),
  [egress-probe.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/egress-probe.ts),
  [docs/self-host.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host.md),
  [docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md),
  [README.md](https://github.com/elie222/rakazo/blob/v0.4.0/README.md). The same files were read
  inside the v0.4.0 image (`/app/infra/sandboxes/supervisor/src/`).
- Podman v6.1.4:
  [version/version.go](https://github.com/containers/podman/blob/v6.1.4/version/version.go),
  [compat/version.go](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/version.go),
  [compat/containers_create.go](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/containers_create.go),
  [compat/networks.go](https://github.com/containers/podman/blob/v6.1.4/pkg/api/handlers/compat/networks.go),
  [RELEASE_NOTES.md](https://github.com/containers/podman/blob/v6.1.4/RELEASE_NOTES.md),
  [troubleshooting.md](https://github.com/containers/podman/blob/v6.1.4/troubleshooting.md),
  [rootless tutorial](https://github.com/containers/podman/blob/v6.1.4/docs/tutorials/rootless_tutorial.md);
  also version.go at [v5.7.0](https://github.com/containers/podman/blob/v5.7.0/version/version.go),
  [v5.8.6](https://github.com/containers/podman/blob/v5.8.6/version/version.go),
  [v6.2.0-rc1](https://github.com/containers/podman/blob/v6.2.0-rc1/version/version.go).
- podman-compose v1.6.0:
  [podman_compose.py](https://github.com/containers/podman-compose/blob/v1.6.0/podman_compose.py),
  [docs/Changelog-*.md](https://github.com/containers/podman-compose/tree/v1.6.0/docs).
- docs.podman.io: [podman-compose(1)](https://docs.podman.io/en/latest/markdown/podman-compose.1.html),
  [podman-run(1)](https://docs.podman.io/en/latest/markdown/podman-run.1.html),
  [podman-machine-init(1)](https://docs.podman.io/en/latest/markdown/podman-machine-init.1.html);
  [podman.io installation](https://podman.io/docs/installation);
  [Podman for Windows](https://github.com/containers/podman/blob/main/docs/tutorials/podman-for-windows.md).
- podman-desktop.io: [Windows install](https://podman-desktop.io/docs/installation/windows-install),
  [Compose setup](https://podman-desktop.io/docs/compose/setting-up-compose),
  [GPU](https://podman-desktop.io/docs/podman/gpu).
- docs.docker.com: [Engine install](https://docs.docker.com/engine/install/),
  [Linux post-install](https://docs.docker.com/engine/install/linux-postinstall/),
  [rootless](https://docs.docker.com/engine/security/rootless/),
  [Desktop on Windows](https://docs.docker.com/desktop/setup/install/windows-install/),
  [Desktop on Mac](https://docs.docker.com/desktop/setup/install/mac-install/),
  [Mac permissions](https://docs.docker.com/desktop/setup/install/mac-permission-requirements/),
  [GPU](https://docs.docker.com/desktop/features/gpu/),
  [Compose merge](https://docs.docker.com/reference/compose-file/merge/).
- Distro packages: [Launchpad podman](https://launchpad.net/ubuntu/+source/podman),
  [Debian sources podman](https://sources.debian.org/src/podman/).
