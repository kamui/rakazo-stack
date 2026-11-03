# Which Docker installs does v1 support on each OS?

Research for [#18](https://github.com/kamui/rakazo-stack/issues/18) on the
[v1 spec map](https://github.com/kamui/rakazo-stack/issues/1). Checked 2026-11-02 against Rakazo
v0.4.0, docker/docs at commit `2eaa01ac`, docker/docker-install at `ada7a567`, MicrosoftDocs/WSL at
`9508d265`, Colima v0.10.3, Lima v2.2.1, OrbStack 2.2.5 and Docker Desktop 4.97.0 (current Homebrew
casks), plus Docker's legal and pricing pages as of that date.

Decision rule: among installs that fully work for Rakazo, pick the easiest onboarding for a
Self-hoster. "Fully work" means all of the following:

- engine API 1.45 or later
- Docker Compose v2 with the features the stack uses
- a Docker API socket that the supervisor can mount for sandbox computers
- `host.docker.internal` reaching host services where the layout needs it (only native Lemonade
  on macOS)

## Answer

- **Linux (x86-64): Docker Engine with the Compose plugin, from Docker's own apt/dnf repository.**
  `get.docker.com` is a one-command shortcut that sets up the same repository.
  - It is free under Apache-2.0 for everyone.
  - It starts at boot through systemd with nobody logged in.
  - It currently ships Engine 29.x (API 1.5x).
  - It is the only engine where Rakazo's restricted computer egress works.
  - `host.docker.internal` needs `extra_hosts: host-gateway` and points at the bridge gateway, not
    host loopback. The Linux layout doesn't need it, because Lemonade runs in Compose.
  - Distro packages pass on Ubuntu 22.04, 24.04 and 26.04 (`docker.io` 29.1.3,
    `docker-compose-v2` 2.40.3) and on Debian 13 (26.1.5, exactly API 1.45). They fail on
    Debian 12 (Engine 20.10, Compose v1).
  - So v1 should check versions in a preflight step rather than mandate a package source. The
    docs should point to Docker's repository.
- **Docker Desktop for Linux is not supported in v1.** It needs KVM and a GUI desktop session. It
  runs a VM with its own context and a per-user socket, and only while the user is signed in. It
  carries Desktop's licence terms and can't do restricted egress, and it gives this stack nothing
  that Engine lacks.
- **Windows: Docker Desktop (WSL 2 backend, per-user install), with WSL integration turned on for
  the distro that runs rakazo-stack.** This is the setup the v0.4.0 reference install already uses
  (Engine 29.9.2, API 1.56, Compose 5.6.1).
  - The per-user installer needs no admin rights. Turning on WSL 2 the first time does.
  - Compose is bundled.
  - "Start Docker Desktop when you sign in" gives autostart.
- **Docker Engine installed inside the WSL2 distro works technically but is not the v1 default:**
  - systemd services do not keep a WSL instance alive. The distro stops about 15 s after its last
    shell closes (`instanceIdleTimeout`) unless the Self-hoster edits `.wslconfig`.
  - Nothing starts it at Windows sign-in.
  - `get.docker.com` stops for 20 s with "WSL DETECTED: We recommend using Docker Desktop".
  - Docker says to uninstall it before installing Desktop.

  Its upsides are that it is free for any company and has no host-loopback forwarding (see
  isolation below).
- **macOS (Apple silicon): Docker Desktop.**
  - Drag-and-drop install, with no password from 4.89.
  - Compose is bundled.
  - The VM defaults to 50% of host RAM.
  - `host.docker.internal` resolves in every container, and Desktop opens those connections from
  the host. That is what lets containers reach the native Lemonade.
  - The supervisor's `/var/run/docker.sock` bind mount is a documented Desktop pattern.
- **The macOS alternatives meet the technical bar on paper, but neither is easier:**
  - **OrbStack** (Docker 29.8.2, Compose 5.5.0, `host.docker.internal` documented) is free only
    for personal, non-commercial use. Any business use costs $8/user/month, which is stricter than
    Docker Desktop's small-business exemption.
  - **Colima** (MIT, Docker 29.x) needs Homebrew, the `docker` and `docker-compose` formulas, a
    `~/.docker/config.json` edit so the Compose plugin is found, and VM sizing (the default 2 GiB
    is too small for this stack). That is the most onboarding work of the three.

  Neither has been tested on Mac hardware for this stack.
- **Licence:** Docker Desktop is governed by the Docker Subscription Service Agreement (last
  updated 2026-08-26).
  - Use without a paid subscription is limited to non-commercial open source, or to "a commercial
    undertaking with fewer than 250 employees and less than US $10,000,000 … in annual revenue"
    (§3.3).
  - Government entities always need a paid subscription (§3.3).
  - Docker's own summary also lists personal use and education as free.
  - Paid seats that include Desktop: Pro $9, Team $15, Business $24 per user/month (billed
    annually).
  - Docker Engine stays Apache-2.0. A Self-hoster using rakazo-stack at work for a larger company
    needs a Desktop seat on Windows and macOS. Linux needs nothing.
- **Bot-computer isolation (affects the settled rule that computers must not reach Lemonade, the
  relay, Honcho or any database):**
  - **Docker Desktop, any OS:** every container, including Rakazo's per-bot computer networks,
    resolves `host.docker.internal` without `extra_hosts`. Desktop's backend then opens the
    connection on the host, so the container reaches anything listening on host loopback,
    including ports that other containers publish to `127.0.0.1`. Checked on the reference
    install: a container on an unrelated Compose network connected to the loopback-published
    Lemonade (13305) and relay (13306) ports. Upstream Rakazo warns about this, and restricted
    egress is unsupported on Desktop.
  - **Windows:** v1 must therefore not publish Lemonade, relay, Honcho or Postgres ports to the
    host. Services reach them over Compose networks only.
  - **macOS:** a native Lemonade on loopback is reachable from computers on every engine (Desktop
    by design; Colima because Lima maps its host IP to host loopback; OrbStack **unverified**). It
    needs its own guard.
  - **Linux Engine:** computers cannot reach loopback-published ports, and restricted egress drops
    traffic to the host and private ranges.
- **Preflight for every OS:** `docker version` server API ≥ 1.45 (Engine 26+), and
  `docker compose version` ≥ 2.24.4, the oldest Compose that covers every feature the reference
  install's Compose files use (`!override` is the newest).

## Details

### What Rakazo needs from the engine

- Upstream requires "Docker Engine 26+ (API 1.45+ for bot home volume subpaths), the Compose
  plugin" ([docs/self-host.md L9, L18 @ v0.4.0](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host.md#L9-L18)).
  The supervisor refuses to create computers below API 1.45 (see the
  [docker-vs-podman findings](https://github.com/kamui/rakazo-stack/blob/research/docker-vs-podman/docs/research/docker-vs-podman.md)).
  Engine 26.0 and 26.1 are API 1.45, 27.0 is 1.46 and 28.0 is 1.48
  ([Engine API version table](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/reference/api/engine/_index.md)).
- The published Compose file runs the supervisor with `DOCKER_SOCKET: /var/run/docker.sock` and
  bind-mounts `/var/run/docker.sock:/var/run/docker.sock`
  ([docker-compose.images.yml L325, L346 @ v0.4.0](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L325-L346)).
  On VM-based engines (Desktop, OrbStack, Colima) the bind source is resolved inside the VM,
  where the daemon's socket lives. Docker documents this mount on Desktop in its ECI example
  `docker run -v /var/run/docker.sock:/var/run/docker.sock docker:cli`
  ([ECI config L122](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/config.md)).
  It runs on the Windows reference install. On Mac hardware it is **unverified** for all three
  engines.
- A host-run supervisor finds Desktop's Mac socket at `~/.docker/run/docker.sock`; otherwise
  `DOCKER_SOCKET` or `DOCKER_HOST` override it
  ([index.ts L188–L199](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L188-L199);
  [self-host-docker-computers.md L32–L33](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L32-L33),
  "Set `DOCKER_SOCKET` for rootless Docker, Colima or another path"). That only matters outside
  Compose.
- Rakazo's desktop app tells users "Docker Desktop, OrbStack, or Docker Engine must be installed"
  ([README.md L206–L209](https://github.com/elie222/rakazo/blob/v0.4.0/README.md#L206-L209)).
- `api` and `worker` already carry `extra_hosts: host.docker.internal:host-gateway` upstream
  ([docker-compose.images.yml L498–L499](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L498-L499)).
  Computers get no `ExtraHosts`. Each runs on its own bridge network
  ([computer-spec.ts L441–L455, L623](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L441-L455)).
- **Compose features.** The reference install's files use the following, each with its minimum
  Compose version:
  - `!override`: 2.24.4 ([merge](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/reference/compose-file/merge.md),
    [summary.yaml](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/data/summary.yaml)
    "Compose replace file")
  - `env_file … required`: 2.24.0 ("Compose required")
  - `dockerfile_inline`: 2.17.0 ("Build dockerfile inline")

  2.24.4 is therefore the floor. Docker's channels currently ship Compose 5.x.

### Linux

- **Docker Engine from Docker's repository.**
  - The Ubuntu instructions install `docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin`
    ([ubuntu.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/install/ubuntu.md)).
    Supported x86-64 platforms are CentOS, Debian, Fedora, RHEL and Ubuntu
    ([engine/install/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/install/_index.md)).
  - Engine is "Apache License, Version 2.0" (same file, L127).
  - "On Debian and Ubuntu, the Docker service starts on boot by default"
    ([linux-postinstall.md L233–L238](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/install/linux-postinstall.md#L233-L238)).
  - The optional `docker` group "grants root-level privileges to the user" (L37).
- **Convenience script.** `get.docker.com` needs root, "is not recommended for production
  environments", installs the latest stable Engine, Buildx and Compose, and "may result in
  unexpected major version upgrades"
  ([install.sh L5–L27 @ ada7a567](https://github.com/docker/docker-install/blob/ada7a567e59b3935bd601bb21e422da50e80e376/install.sh#L5-L27)).
  It configures Docker's package repository, adding `docker-compose-plugin` to the install
  (L654, L772). That repository is where later `apt`/`dnf` upgrades come from.
- **Distro packages:**

  | Distro | Engine | Compose | Passes? |
  | --- | --- | --- | --- |
  | Ubuntu 22.04, 24.04, 26.04 | `docker.io` 29.1.3 (source `docker.io-app`) | `docker-compose-v2` 2.40.3 | Yes |
  | Debian 13 | `docker.io` 26.1.5 | `docker-compose` 2.26.1 | Yes, at exactly API 1.45 |
  | Debian 12 | `docker.io` 20.10.24 | `docker-compose` 1.29.2 | No |

  Sources: [Launchpad](https://launchpad.net/ubuntu/+source/docker.io-app),
  [sources.debian.org docker.io](https://sources.debian.org/src/docker.io/),
  [docker-compose](https://sources.debian.org/src/docker-compose/).

  Engine 26/27 also predates the Docker 28 DNS change: with restricted egress and a private host
  resolver, computers then need `SANDBOX_COMPUTER_DNS` set to public resolvers
  ([self-host-docker-computers.md, DNS paragraph](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)).
  That favours Docker's repository.
- **`host.docker.internal` on Engine.** "On Linux, `host-gateway` resolves to the host's IP on the
  default bridge network"
  ([compose networking L276–L286](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/compose/how-tos/networking.md#L276-L286)).
  Docker's own example serves on `0.0.0.0`
  ([docker/cli container_run.md](https://github.com/docker/cli/blob/master/docs/reference/commandline/container_run.md#add-host)).
  A service bound only to host `127.0.0.1` is not reached this way. Upstream says "Linux does not
  get `host.docker.internal` the same way by default"
  ([self-host-models.md L83–L84](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md#L79-L84)).
- **Restricted computer egress** "requires Linux Docker Engine with the iptables backend; Docker
  Desktop, rootless Docker, and `firewall-backend: nftables` are unsupported"
  ([self-host-docker-computers.md L106–L108](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L106-L108)).
  The ruleset drops computer traffic to 127/8, 10/8, 172.16/12, 192.168/16, 0/8 and so on, and
  adds an `INPUT` drop so computers can't open connections to the host
  ([restrict-computer-egress.sh L58–L76, L93–L104](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L58-L104)).
- **Docker Desktop for Linux:**
  - It "runs a Virtual Machine (VM) which creates and uses a custom docker context,
    `desktop-linux`".
  - It needs "KVM virtualization support", "systemd init system" and a GNOME, KDE or MATE desktop,
    and is packaged for x86-64 Ubuntu, Debian, RHEL and Fedora.
  - "For commercial use of Docker Engine obtained via Docker Desktop within larger enterprises …
    a paid subscription is required."
    ([linux/_index.md L29, L37, L120–L143](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/linux/_index.md)).
  - Its socket is per-user, `~/.docker/desktop/docker.sock`
    ([faqs/general.md L22](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/troubleshoot-and-support/faqs/general.md#L22)).
  - Ubuntu needs 26.04 or 24.04 plus GNOME extensions
    ([linux/ubuntu.md L31–L36](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/linux/ubuntu.md#L31-L36)).
  - Docker recommends stopping Engine while Desktop runs to avoid port conflicts (L50–L71).
  - Its VM networking goes through `qemu`
    ([networking/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/_index.md)),
    so the host-loopback reachability below is expected to apply here too (**unverified** on
    Linux).

### Windows (WSL2)

- **Docker Desktop requirements:**
  - WSL 2.1.5 or later.
  - Windows 10 22H2 (build 19045) or Windows 11 23H2 (build 22631) or later. The listed editions
    are "Enterprise, Pro, or Education".
  - The LanmanServer service on Automatic.
  - SLAT and 8 GB RAM.
  - Per-user install "does not require administrator privileges to install or update", and WSL 2
    is the default backend.

  ([windows-install.md L35–L72](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/windows-install.md#L35-L72)).
  Windows Home is not in the edition list. Whether Desktop supports it is **unverified**.
- **WSL integration:**
  - "Before installing Docker Desktop, uninstall any version of Docker Engine or the Docker CLI
    that was installed directly inside a WSL Linux distribution. Running both can cause
    conflicts."
  - Integration is on for the default distro by default.
  - Desktop runs in its own `docker-desktop` distro.

  ([wsl/_index.md L35–L36, L79–L93](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/wsl/_index.md#L35-L93)).
  "Start Docker Desktop when you sign in" is off by default
  ([settings.md L35](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/settings-and-maintenance/settings.md#L35)).
- **Docker Engine inside the WSL distro:**
  - **systemd.** It is the default for Ubuntu installed by `wsl --install`; other distros set
    `[boot] systemd=true`. But "systemd services will NOT keep your WSL instance alive"
    ([systemd.md L12, L92 @ 9508d265](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/systemd.md)).
  - **Idle shutdown.** `instanceIdleTimeout` defaults to 15000 ms ("Set to -1 to disable auto
    shutdown"), and `vmIdleTimeout` to 60000 ms
    ([wsl-config.md L248, L283](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/wsl-config.md)).
  - **Port publishing to Windows.** `localhostForwarding` (default `true`) makes "ports bound to
    wildcard or localhost in the WSL 2 VM … connectable from the host via `localhost:port`"
    (L239). In mirrored mode Windows and WSL share `127.0.0.1`
    ([networking.md L123–L130](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/networking.md)).
    So a loopback-published Rakazo web port reaches a Windows browser.
  - **Docker's position.** Docker's install script detects WSL and prints "WSL DETECTED: We
    recommend using Docker Desktop for Windows", then sleeps 20 s
    ([install.sh L282–L288, L495–L505](https://github.com/docker/docker-install/blob/ada7a567e59b3935bd601bb21e422da50e80e376/install.sh#L495-L505)).
    Docker's Engine install pages don't mention WSL.
  - **`host.docker.internal`.** Not needed on Windows, because Lemonade runs in Compose. On
    Engine it would behave as on Linux.
  - **Restricted egress** inside WSL's kernel and iptables is **unverified**.
- **WSL's own container CLI.** WSL 2.9.3+ ships `wslc.exe`. Its tutorial documents only its own
  CLI, with no Docker API socket or Compose
  ([tutorials/wsl-containers.md](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/tutorials/wsl-containers.md)),
  so it can't host the supervisor. Out of scope.

### macOS (Apple silicon)

| | Docker Desktop 4.97 | OrbStack 2.2.5 | Colima 0.10.3 |
| --- | --- | --- | --- |
| Engine / API | Same release train as the reference install (29.x, API 1.5x) | Docker 29.8.2, Compose 5.5.0 (v2.2.4 notes) | Docker 29.5.2 in colima-core v0.10.4; `colima update` moves it |
| Compose | Bundled | Bundled | Separate `docker-compose` formula, plus the `cliPluginsExtraDirs` edit |
| Socket | `~/.docker/run/docker.sock`; optional `/var/run/docker.sock` symlink | `~/.orbstack/run/docker.sock`; symlink if admin is granted | `~/.colima/default/docker.sock`; sets the docker context |
| `host.docker.internal` | Automatic in every container; reaches host loopback | Documented "to connect to a server running on Mac"; loopback **unverified** | Mapped by VM DNS to Lima's `192.168.5.2`, the host's loopback |
| Install | `.dmg`, no password (4.89+), or Homebrew cask | `.dmg` or cask, admin optional; macOS 14+ | Homebrew (or MacPorts/Nix) plus `colima start` with sizing; macOS 13+ |
| Autostart | Setting (off by default) | **unverified** | `brew services start colima` |
| Default VM memory | 50% of host RAM | **unverified** | 2 GiB |
| Licence | Free under the SSA thresholds | Free only for personal, non-commercial use; $8/user/month for business | MIT |
| Restricted egress | Unsupported; VM method "not tested" | Upstream documents applying the ruleset in the VM | **unverified** |

- **Docker Desktop:**
  - Install: "Docker Desktop starts. No further configuration is needed and you aren't asked for a
    password"
    ([mac-install.md L100](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/mac-install.md#L100)).
    Supported on the current and two previous major macOS releases; at least 4 GB RAM (L36–L42).
  - Socket and symlink
    ([mac-permission-requirements.md L42–L44](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/mac-permission-requirements.md#L42-L44)).
  - Memory "Defaults to 50% of your host's memory"
    ([settings.md L66](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/settings-and-maintenance/settings.md#L66)).
  - `host.docker.internal` "Resolves to the internal IP address of your host"
    ([networking-how-tos.md L16–L24](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/networking-how-tos.md#L16-L24)).
    Outbound container traffic is re-created on the host: "Docker Desktop's backend process
    receives the traffic and creates standard TCP/IP connections"
    ([networking/_index.md L42–L49](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/_index.md#L42-L49)).
  - Current cask version 4.97.0, requires macOS ≥ 14
    ([Homebrew cask API](https://formulae.brew.sh/api/cask/docker-desktop.json)).
- **OrbStack:**
  - Network behaviour ([container networking](https://docs.orbstack.dev/docker/network)).
  - Admin is optional; it is used for the CLI tools and the `/var/run/docker.sock` symlink.
    Socket `~/.orbstack/run/docker.sock`. It provides "an unmodified Docker engine". macOS 14+
    ([FAQ](https://docs.orbstack.dev/faq)).
  - Docker 29.8.2 / Compose 5.5.0 in v2.2.4, 2026-10-12
    ([release notes](https://docs.orbstack.dev/release-notes)).
  - Free tier "Personal, non-commercial use"; Pro "$8 per user per month" for "Business use" and
    "Commercial use" ([pricing](https://orbstack.dev/pricing)).
  - Rakazo documents applying its egress ruleset inside OrbStack's VM from a `network_mode: host`
    container
    ([self-host-docker-computers.md L111–L142](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L111-L142)).
- **Colima:**
  - MIT licence ([LICENSE @ v0.10.3](https://github.com/abiosoft/colima/blob/v0.10.3/LICENSE)).
  - Needs macOS 13+; autostart via `brew services`
    ([FAQ L99–L113](https://github.com/abiosoft/colima/blob/v0.10.3/docs/FAQ.md#L99-L113)).
  - Socket path, plus the note that apps unaware of contexts need `DOCKER_HOST` or a `sudo ln`
    (L189–L236).
  - "Docker client is required … `brew install docker`"
    ([README L81](https://github.com/abiosoft/colima/blob/v0.10.3/README.md#L81)).
  - The `docker-compose` formula's caveat requires adding `cliPluginsExtraDirs` to
    `~/.docker/config.json`
    ([formula API](https://formulae.brew.sh/api/formula/docker-compose.json)).
  - Defaults: 2 CPUs, 2 GiB, and `dnsHosts: host.docker.internal: host.lima.internal`
    ([colima.yaml L1–L13, L106–L108 @ v0.10.3](https://github.com/abiosoft/colima/blob/v0.10.3/embedded/defaults/colima.yaml)).
    dockerd starts with `--host-gateway-ip` set to Lima's host address
    ([daemon.go L105–L136](https://github.com/abiosoft/colima/blob/v0.10.3/environment/container/docker/daemon.go#L105-L136);
    [dns.go L35–L38](https://github.com/abiosoft/colima/blob/v0.10.3/environment/vm/lima/dns.go#L35-L38)).
  - Lima: "The loopback addresses of the host is `192.168.5.2` and is accessible from the guest
    as `host.lima.internal`"
    ([user.md L20–L22 @ v2.2.1](https://github.com/lima-vm/lima/blob/v2.2.1/website/content/en/docs/config/network/user.md#L20-L22)).
  - Docker bumped to 29.5.2 in [colima-core#18](https://github.com/abiosoft/colima-core/pull/18)
    (release v0.10.4).

### Docker Desktop licence

- Docker's docs summarise the agreement:
  - Desktop is free for "Small businesses (fewer than 250 employees AND less than $10 million in
    annual revenue)", "Personal use", "Education" and "Non-commercial open source projects".
  - It needs a paid subscription for "Professional use in larger organizations" and "Government
    entities".
  - "Docker Pro, Team, and Business subscriptions" include it.
  - "The licensing and distribution terms … such as Docker Engine, aren't changing."

  ([desktop-license.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/subscription-billing/desktop-license.md)).
- The [Docker Subscription Service Agreement](https://www.docker.com/legal/docker-subscription-service-agreement/)
  (last updated 2026-08-26):
  - §3.3: use without a paid subscription is "restricted (i) to use for a non-commercial open
    source project and/or (ii) use in a commercial undertaking with fewer than 250 employees and
    less than US $10,000,000 (or equivalent local currency) in annual revenue", and "Government
    Entities shall not use Docker Desktop … without a paid subscription".
  - §3.1(b)(i): personal "Developer Use" excludes "use in connection with an individual's
    employment or engagement by a company".
  - Whether a home Instance run by an employee of a large company counts as free personal use is
    not stated in so many words (**unverified**). Docker's summary says "Personal use" is free.
- Prices ([docker.com/pricing](https://www.docker.com/pricing/), checked 2026-11-02), per user
  per month: Pro $9 annual / $11 monthly, Team $15 / $16, Business $24. The
  [pricing FAQ](https://www.docker.com/pricing/faq/) repeats the 250-employee / $10M rule and says
  "Docker's open-source software such as the Docker Engine is accessible for all".
- Docker Business adds Enhanced Container Isolation. By default it "blocks bind mounting the
  Docker Engine socket", so an organisation with ECI enforced would need an exception for the
  Rakazo app image
  ([ECI _index.md L154](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/_index.md)).
  Air-gapped container proxy rules, which also need Business, apply to all containers, not just
  computers
  ([air-gapped-containers.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md)).

### Bot-computer isolation by install

- **Upstream's own warning:** "On Docker Desktop, a bot computer shell can often reach services
  bound to host `127.0.0.1` through that same hostname. Do not run sensitive unauthenticated
  services on loopback while bots run, or firewall / block that path."
  ([self-host-models.md L79–L84](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md#L79-L84)).
- **Observed on the v0.4.0 reference install (Docker Desktop, WSL2):**
  - Containers with no `extra_hosts` (the web and supervisor services) resolve
    `host.docker.internal` through Desktop's DNS. Their `/etc/hosts` has no entry.
  - A container attached only to an unrelated Compose network opened TCP connections to
    `host.docker.internal:13305` (Lemonade) and `:13306` (relay). Both ports are published to
    `127.0.0.1` only.
  - Computers are ordinary bridge containers, so they get the same path. Rakazo's egress default
    on Desktop is open ([self-host-docker-computers.md L72–L74](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L72-L74)).
- **Consequences for v1:**
  - **Windows (Desktop):** publish only what the Self-hoster's browser needs, which is the Rakazo
    web port. Keep Lemonade, the relay, Honcho and the databases unpublished and reachable only
    over Compose networks. Any other unauthenticated service the Self-hoster runs on Windows
    loopback is also exposed to computers.
  - **macOS:** the settled layout puts Lemonade on host loopback and has containers reach it via
    `host.docker.internal`. Docker Desktop and Colima forward that address to host loopback;
    OrbStack is **unverified**. Computers can therefore reach Lemonade directly, which breaks the
    settled rule unless something else guards it.
    - OrbStack's documented VM ruleset drops 0/8 and 192.168/16, which likely covers its host
      address (**unverified**).
    - Colima's host address `192.168.5.2` falls in 192.168/16, but applying the ruleset in Colima's
      VM is **unverified**.
  - **Linux Engine:** computers sit on their own bridges, Compose services aren't on them, and
    loopback-published ports aren't reachable through the gateway. Restricted egress additionally
    blocks host and private ranges.

## Open questions / follow-ups

- **(grilling)** Licence escape hatch. Should v1 document a free path for Self-hosters at larger
  companies (Docker Engine inside WSL2 on Windows; Colima on macOS), or only state that they need
  a Desktop seat?
- **(research)** How does a native macOS Lemonade on loopback stay out of reach of bot computers?
  Options: Lemonade's `LEMONADE_API_KEY` with the relay injecting it, the in-VM egress ruleset on
  OrbStack or Colima, or something else. Also check OrbStack's loopback behaviour.
- **(task)** Windows/Desktop Compose layout: publish only the Rakazo web port to `127.0.0.1`.
  Check whether the API port upstream publishes is reachable from computers, and whether that
  matters.
- **(task)** Setup preflight on every OS:
  - Server API ≥ 1.45, `docker compose version` ≥ 2.24.4, and context/socket detection.
  - Fail on Debian 12's `docker.io` and on Docker Desktop for Linux.
  - On Docker Desktop, warn when "Start when you sign in" is off.
- **(prototype)** On Apple-silicon hardware with Docker Desktop:
  - The supervisor's `/var/run/docker.sock` bind mount starts a computer.
  - `host.docker.internal:13305` reaches a loopback-bound `lemond`.
  - Repeat on OrbStack and Colima if they stay candidates.
- **(grilling)** Always-on. Docker Desktop and WSL need a signed-in user, so the Instance stops at
  sign-out on Windows and macOS. Linux Engine does not. Does v1 accept and document that?
- **(grilling)** Linux Docker access for the setup command: add the Self-hoster to the `docker`
  group (root-equivalent), or run Docker commands through `sudo`?
- **(research, small)** Does Docker Desktop support Windows Home today? The current requirements
  list only Enterprise, Pro and Education.

## Sources

- Rakazo v0.4.0:
  - [docs/self-host.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host.md)
  - [docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)
  - [docs/self-host-models.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-models.md)
  - [README.md](https://github.com/elie222/rakazo/blob/v0.4.0/README.md)
  - [infra/compose/docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml)
  - [infra/compose/restrict-computer-egress.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh)
  - [supervisor index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts)
  - [computer-spec.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts)
- docker/docs @ `2eaa01ac4fbe81c8cb8705724a05e19474dd681a`, under `content/manuals/`:
  - [engine/install/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/install/_index.md)
  - [engine/install/ubuntu.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/install/ubuntu.md)
  - [engine/install/linux-postinstall.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/install/linux-postinstall.md)
  - [desktop/setup/install/linux/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/linux/_index.md)
  - [desktop/setup/install/linux/ubuntu.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/linux/ubuntu.md)
  - [desktop/setup/install/windows-install.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/windows-install.md)
  - [desktop/setup/install/mac-install.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/mac-install.md)
  - [desktop/setup/install/mac-permission-requirements.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/setup/install/mac-permission-requirements.md)
  - [desktop/features/wsl/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/wsl/_index.md)
  - [desktop/features/networking/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/_index.md)
  - [desktop/features/networking/networking-how-tos.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/networking-how-tos.md)
  - [desktop/settings-and-maintenance/settings.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/settings-and-maintenance/settings.md)
  - [desktop/troubleshoot-and-support/faqs/general.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/troubleshoot-and-support/faqs/general.md)
  - [ECI _index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/_index.md)
  - [ECI config.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/config.md)
  - [air-gapped-containers.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md)
  - [subscription-billing/desktop-license.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/subscription-billing/desktop-license.md)
  - [compose/how-tos/networking.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/compose/how-tos/networking.md)
- docker/docs @ `2eaa01ac4fbe81c8cb8705724a05e19474dd681a`, elsewhere:
  - [reference/compose-file/merge.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/reference/compose-file/merge.md)
  - [data/summary.yaml](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/data/summary.yaml)
  - [reference/api/engine/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/reference/api/engine/_index.md)
- [docker/docker-install install.sh @ ada7a567](https://github.com/docker/docker-install/blob/ada7a567e59b3935bd601bb21e422da50e80e376/install.sh)
  and [docker/cli container_run.md](https://github.com/docker/cli/blob/master/docs/reference/commandline/container_run.md).
- Docker legal and pricing:
  [Subscription Service Agreement](https://www.docker.com/legal/docker-subscription-service-agreement/),
  [pricing](https://www.docker.com/pricing/), [pricing FAQ](https://www.docker.com/pricing/faq/).
- MicrosoftDocs/WSL @ `9508d265738ebb07b7c5eb4efb1b841aef61d44d`:
  - [WSL/systemd.md](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/systemd.md)
  - [WSL/wsl-config.md](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/wsl-config.md)
  - [WSL/networking.md](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/networking.md)
  - [WSL/tutorials/wsl-containers.md](https://github.com/MicrosoftDocs/WSL/blob/9508d265738ebb07b7c5eb4efb1b841aef61d44d/WSL/tutorials/wsl-containers.md)
- OrbStack: [FAQ](https://docs.orbstack.dev/faq),
  [container networking](https://docs.orbstack.dev/docker/network),
  [release notes](https://docs.orbstack.dev/release-notes), [pricing](https://orbstack.dev/pricing).
- Colima v0.10.3:
  - [README.md](https://github.com/abiosoft/colima/blob/v0.10.3/README.md)
  - [docs/FAQ.md](https://github.com/abiosoft/colima/blob/v0.10.3/docs/FAQ.md)
  - [embedded/defaults/colima.yaml](https://github.com/abiosoft/colima/blob/v0.10.3/embedded/defaults/colima.yaml)
  - [environment/container/docker/daemon.go](https://github.com/abiosoft/colima/blob/v0.10.3/environment/container/docker/daemon.go)
  - [environment/vm/lima/dns.go](https://github.com/abiosoft/colima/blob/v0.10.3/environment/vm/lima/dns.go)
  - [colima-core#18](https://github.com/abiosoft/colima-core/pull/18)
- Lima v2.2.1: [docs/config/network/user.md](https://github.com/lima-vm/lima/blob/v2.2.1/website/content/en/docs/config/network/user.md).
- Homebrew API: [docker-desktop](https://formulae.brew.sh/api/cask/docker-desktop.json),
  [orbstack](https://formulae.brew.sh/api/cask/orbstack.json),
  [colima](https://formulae.brew.sh/api/formula/colima.json),
  [docker-compose](https://formulae.brew.sh/api/formula/docker-compose.json).
- Distro packages: [Launchpad docker.io-app](https://launchpad.net/ubuntu/+source/docker.io-app),
  [Launchpad docker-compose-v2](https://launchpad.net/ubuntu/+source/docker-compose-v2),
  [Debian docker.io](https://sources.debian.org/src/docker.io/),
  [Debian docker-compose](https://sources.debian.org/src/docker-compose/).
