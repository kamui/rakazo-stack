# Does the computer-egress sidecar work on Docker Desktop for Windows? (#27, Windows half)

Research for [Does the computer-egress sidecar work on Docker Desktop for Windows and on Linux Docker Engine?](https://github.com/kamui/rakazo-stack/issues/27). It extends the macOS result in [Can bot computers on Docker Desktop for Mac be kept off Lemonade's model servers?](https://github.com/kamui/rakazo-stack/issues/25) ([write-up](https://github.com/kamui/rakazo-stack/blob/research/mac-lemonade-isolation/docs/research/mac-lemonade-isolation.md)). The Linux Docker Engine half is not covered here.

Pins: Rakazo `v0.4.0` (commit `b9c652f`); its `restrict-computer-egress.sh` was used unmodified (sha256 `c2802eef397db19603494401c771ed49d57ad371674c28d3a389a1c492ce3dc3`).

Hands-on runs:
- **Machine:** Windows 11 (build 26300), Docker Desktop 4.97.0 (244584) with the WSL 2 backend, Engine 29.9.2, containerd v2.4.2, runc 1.5.2, WSL 2.6.2.0 (kernel 6.6.87.2).
- **Networking settings:** WSL networking mode `nat`. Docker Desktop `NetworkType: gvisor`, `IPv6Only: false`, host networking off, proxy mode `system` with no system proxy set, Resource Saver on.
- **The machine's own Instance kept running beside the tests.** It is Rakazo, Lemonade and Honcho, with Rakazo's `SANDBOX_COMPUTER_EGRESS=open`, so its computers use `br-*` bridges that the rules never match.
- **Test setup:** a throwaway Compose project `rs27-mini` and a stand-in listener on Windows' own `127.0.0.1`. The ordinary probes needed no admin rights.

Scripts: [`egress-sidecar-windows/`](egress-sidecar-windows/). Transcripts: [`egress-sidecar-windows/transcripts/`](egress-sidecar-windows/transcripts/).

## Answer

- **Yes, the sidecar works on Docker Desktop for Windows, but only in a form that doesn't depend on the WSL distro.** The rules land in the right place, close every host path #24 found, leave the relay alone, and come back after Docker Desktop restarts, `wsl --shutdown` and a reboot. That holds once two conditions are met:
  1. the sidecar's image has the script built in, rather than bind-mounted from a WSL path;
  2. the supervisor's `docker.sock` doesn't come through WSL integration, which means running `docker compose` from Windows, not from inside a WSL distro.

  Using upstream's doc recipe as written (bind-mounted script, Compose run from the Ubuntu distro), the sidecar and the supervisor failed to start after the second `wsl --shutdown` and after the reboot. Docker never retried them.
- **Where the rules land: pass.**
  - A `network_mode: host` container shares the network namespace of Docker Desktop's engine. That namespace holds `docker0`, every `br-*` and `rakazo-c*` bridge, `eth0` (`192.168.65.3`) and `services1`. It is not the Ubuntu distro's namespace (which has `eth0`, `lo` and `tailscale0` only).
  - iptables there is the nf_tables backend (Alpine `iptables v1.8.13 (nf_tables)`). No legacy tables are in use, and `RAKAZO_IPTABLES=iptables-legacy` is not needed.
  - `DOCKER-USER` exists for IPv4 and IPv6 and already holds Docker Desktop's own rules. The script inserts its rules above them.
  - The engine's namespace has global IPv6, so `ip6tables` is required. The Alpine sidecar has it.
- **Isolation with a real v0.4.0 supervisor and computer (`restricted`): pass.**
  - The supervisor logged `restricted computer egress verified`, and the computer's bridge was `rakazo-c6128c2c`.
  - From inside the computer, all of these timed out:
    - a listener on **Windows'** `127.0.0.1`, via `host.docker.internal` and by IP `192.168.65.254`;
    - the Instance's published `web`/`api` ports (`5173`/`3100`), the same three ways: `host.docker.internal`, `192.168.65.254`, and the `api`/`web` container IPs;
    - the Windows LAN IP (`:445`, `:5173`);
    - a listener in the WSL distro, by the distro's IP and by its tailnet IP;
    - `169.254.169.254`;
    - Docker Desktop's endpoints: HTTP proxy `192.168.65.1:3128`, DNS `192.168.65.7:53`, and `gateway`/`http`/`vm.docker.internal`;
    - the bridge gateway.
  - Internet (`1.1.1.1:443`), DNS (embedded resolver) and SSH out (`github.com:22` banner) worked.
- **The relay path: pass.** A container on an ordinary Compose network (a `br-*` bridge) still reached Windows' `127.0.0.1`, other containers and the internet. Every managed rule is keyed on `-i rakazo-c+`.
- **Screens: pass at the network level only.**
  - `GET /computers/:id/screen` joined the supervisor and the project's `web` service to the computer's `rakazo-c*` bridge and returned 302. Both peers then fetched the noVNC page across the bridge.
  - `web` was an alpine stand-in, so the real web app's proxy and a browser were not tested.
- **Fail closed: pass, with one gap.**
  - With the rules missing, a freshly started supervisor logs `restricted computer egress unavailable`. `POST /computers` then returns 500 naming the script, even for resuming an existing computer.
  - Once the sidecar re-applies the rules, the next create succeeds, with no supervisor restart.
  - **Gap (not seen on the Mac):** `restricted` checks the rules once per supervisor process and caches the pass. When the rules were removed while the supervisor kept running, a new computer started and **reached Windows' `127.0.0.1` and the published web port**. The sidecar's 60 s loop bounds that window. The rules then cover the computer again as soon as they return, because they match the bridge name.
- **Survival.**

  | Event | Upstream recipe (script bind-mounted, Compose from WSL) | Script built into the image, Compose from Windows |
  |---|---|---|
  | Docker Desktop restart | pass: rules back within 1 s of the engine, supervisor verified, computer resumed blocked | not run (same engine path; nothing depends on WSL) |
  | Restart with the sidecar stopped | rules gone (0), `restricted` returns 500 until the sidecar starts | not run |
  | `wsl --shutdown` #1 | pass, but only because Ubuntu was back 9 s later, before the engine started containers | not run |
  | `wsl --shutdown` #2 | **fail**: the sidecar and supervisor exit 127 (`error mounting …/docker-desktop-bind-mounts/Ubuntu/…`), never retried, 0 rules | not run |
  | Windows reboot | **fail** for the WSL-mounted control container and for the Instance's own supervisor (exit 127) | **pass**: sidecar and supervisor up at boot, 14 + 7 rules, verified, computer resumed blocked |
- **Costs for a Self-hoster.**
  - Bot computers lose:
    - Windows itself: loopback and every published port;
    - the LAN and the tailnet (`100.64.0.0/10`), including a Tailscale node running in WSL;
    - the WSL distros;
    - other containers by IP;
    - Docker Desktop's HTTP proxy (`192.168.65.1:3128`). If the Self-hoster's Windows uses a system proxy, computers may lose the internet as well (not tested).
  - Once a screen is open, a computer can reach the supervisor's port `7091` on its own bridge. This follows from upstream's same-bridge exemption, and the API needs the supervisor's bearer token.
  - It adds one privileged sidecar, inside Docker Desktop's VM only. That is no more than the supervisor already has through the Docker socket.
- **Fragility.** This relies on Docker Desktop internals that Docker doesn't document:
  - the `DOCKER-USER` chain in the engine's namespace, the nf_tables backend, and Docker Desktop's own rules in that chain;
  - container traffic to `host.docker.internal` passing through `FORWARD`;
  - for the upstream recipe only, how WSL integration serves bind mounts.

  If the chain disappears, the script fails loudly and `restricted` refuses computers.

## Details

### 1. Where the rules land ([transcript 01](egress-sidecar-windows/transcripts/01-rules-test.log))

- **Namespace.**
  - The Ubuntu distro shows `lo`, `eth0` and `tailscale0` (netns `4026531840`).
  - A `--network host` container shows `services1`, `eth0`, `docker0`, every `br-*` and the containers' `veth*` interfaces, and its hostname is `docker-desktop`. So `network_mode: host` puts the sidecar where the bridges are filtered.
  - All WSL 2 distros share one utility VM, but Docker Desktop's engine runs in its own namespace inside it, not in the user's distro.
- **Backend.**
  - Alpine's `iptables` is `v1.8.13 (nf_tables)`.
  - The `iptables-legacy` package finds 0 legacy rules, and `nft list tables` shows only `ip`/`ip6` `filter`, `nat` and `raw`.
- **Docker Desktop's own `DOCKER-USER` rules (IPv4 and IPv6)**, which sit below the script's rules after `--apply`:
  - `-i eth0 -j ACCEPT`
  - `-i eth1 -j ACCEPT`
  - accepts to `services1` on TCP 3128, 5555 and 53
  - then a TCP reject to `services1`
- **Scoped test** ([`rules-test.sh`](egress-sidecar-windows/rules-test.sh)).
  - The script's `BRIDGE_PREFIX` was changed to `rs27t-c` in a scratch copy, as on the Mac. Before the rules, both the matching bridge and an ordinary network reached everything in the Answer.
  - After `--apply`, the matching bridge timed out on every private or host target, while `1.1.1.1:443`, `example.com:443`, `github.com:22` and an embedded-DNS lookup still worked. The ordinary network was unchanged.
  - The bridge gateway went from refused to timeout, which is what Rakazo's probe reads as "enforced".
  - Cleanup: `--remove` plus removing the networks left 0 `rs27t` rules in any table.

### 2. Real supervisor and computer ([transcript 02](egress-sidecar-windows/transcripts/02-rakazo-supervisor.log))

- **Setup.** [`mini/compose.yaml`](egress-sidecar-windows/mini/compose.yaml) has the same shape as the Mac's `mini/compose.yaml`, with these changes:
  - `SANDBOX_COMPUTER_EGRESS` defaults to `restricted`;
  - the health file is touched only after `--apply` succeeds;
  - an alpine `web` stand-in is added for screens.

  Computers were created and probed through the supervisor's own API with compose-boot's [`computer-probe.sh`](https://github.com/kamui/rakazo-stack/blob/research/compose-boot/docs/research/compose-boot/computer-probe.sh), using [`probe-all.sh`](egress-sidecar-windows/probe-all.sh). Machine-specific addresses are replaced with placeholders in the transcripts.
- **Windows' loopback.** The listener was Windows' `python.exe -m http.server --bind 127.0.0.1` ([`win-listener.sh`](egress-sidecar-windows/win-listener.sh)).
  - In WSL `nat` mode the distro can't reach it: `curl 127.0.0.1:18927` from Ubuntu fails.
  - Containers reach it through `host.docker.internal` (`192.168.65.254`, `fdc4:f303:9324::254`) unless the rules apply. This is the path #24 saw.
- **SSH out.**
  - Early `github.com:22` probes were flaky from the restricted computer, but equally from the unrestricted relay stand-in, from a default-bridge container and from the WSL distro itself (0/20 for all three at the same moment). That points to GitHub throttling repeated probes.
  - `ssh.github.com:443` banners were 10/10 from the computer. After the reboot, `github.com:22` returned an `SSH-2.0` banner from the restricted computer.
- **Screens** ([`screen-check.sh`](egress-sidecar-windows/screen-check.sh)).
  - The supervisor's screen route waits until the computer's screen port answers across the bridge ([`publishedScreenUrl`, index.ts L1611-L1644](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1611-L1644)). It joins the supervisor and every container of the Compose service `web` in its project to the bridge ([`composeScreenPeerIds`, L1688-L1701](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1688-L1701)).
  - Same-bridge traffic passes the script's `-i rakazo-c+ -o rakazo-c+ -j RETURN` ([docs L76-L79](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L76-L79)).
  - The route returned 302, and both peers fetched the noVNC HTML.
- **Cached verdict.**
  - In `restricted` mode, `ensureRestrictedEgressEnforced` stores a passing `verifyRestrictedEgress` in `restrictedEgressVerified` and never re-runs it ([index.ts L1224-L1253](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1224-L1253)). The docs say the check runs "before the first restricted computer starts" ([L60-L63](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L60-L63)).
  - Test: stop the sidecar, `--remove`, then create `rs27bot2` with the supervisor still running. The result was 200, and `host.docker.internal:18927` and `:5173` were open.
  - After a supervisor restart, the startup check ([L1174-L1179](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1174-L1179)) failed and creates returned 500.
  - On Docker Desktop, every way the rules disappear that this test saw (an engine restart, `wsl --shutdown`, a reboot) also restarts the supervisor, so the cache is cleared. The window is limited to the rules vanishing while the engine keeps running.

### 3. Survival ([03](egress-sidecar-windows/transcripts/03-docker-desktop-restart.log), [04](egress-sidecar-windows/transcripts/04-wsl-shutdown.log), [05](egress-sidecar-windows/transcripts/05-windows-reboot.log))

- **Docker Desktop restart.**
  - Use Windows' `docker.exe desktop restart`. Inside WSL the `docker` CLI is Docker Desktop for Linux's plugin: it stopped nothing and tried to start `/opt/docker-desktop/bin/com.docker.backend`.
  - All `unless-stopped` containers were back about 10 s later, and the rules were back before the supervisor's check. Computers (`RestartPolicy=no`) stay stopped until the next provision, which resumes them on the same `rakazo-c*` bridge.
  - Two more engine restarts happened during the session that the test didn't start. The sidecar recovered from both.
- **Why `wsl --shutdown` and reboots break the upstream recipe.**
  - Docker Desktop serves a bind mount from a WSL path through `/run/desktop/mnt/host/wsl/docker-desktop-bind-mounts/<distro>/…`. When the `docker` CLI runs inside a distro, `/var/run/docker.sock` is that distro's socket, served the same way.
  - Started from Windows' `docker.exe`, the same `-v /var/run/docker.sock:/var/run/docker.sock` mounts `/host-services/docker.proxy.sock`, Docker Desktop's own proxy (`/proc/<pid>/mountinfo` root for each).
  - When the engine starts containers before the distro is attached, those mounts fail at create time with `not a directory` (exit 127). Docker records `restarts=0` and doesn't try again. `unless-stopped` only restarts containers that ran and exited.
  - After shutdown #1 Ubuntu came back first, probably because the T3 Code desktop app reconnected to it, so everything recovered. After shutdown #2 and after the reboot it did not.
- **What survived everything.**
  - The sidecar built from [`mini/egress.Dockerfile`](egress-sidecar-windows/mini/egress.Dockerfile): `alpine:3` with `bash iptables ip6tables` and the script copied in, no mounts. It also needs no network at start.
  - The project brought up with Windows' `docker.exe compose -f compose.yaml -f compose.baked.yaml` ([override](egress-sidecar-windows/mini/compose.baked.yaml)).
  - On the reboot, a WSL-mounted control container (`rs27-canary`) failed while this variant came up, re-applied the rules and verified egress.
- **The Instance itself is exposed to the same race.** Its supervisor binds `/var/run/docker.sock`, and its Compose project was run from the Ubuntu distro. It failed with exit 127 after shutdown #2 and after the reboot, and had to be started by hand. This affects any rakazo-stack layout on Windows that runs Compose inside WSL, with or without the sidecar.

### 4. Costs and fragility

- **What the rules block,** from `rakazo-c*` bridges only: every non-public IPv4 and IPv6 destination plus `INPUT` to the engine's namespace ([script L56-L109](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L56-L109)). On Windows that covers:
  - Windows' loopback and LAN addresses;
  - WSL distros (`172.16.0.0/12` in `nat` mode);
  - tailnet addresses;
  - Docker Desktop's `192.168.65.0/24` services;
  - other containers.

  DNS keeps working because Docker 28+ forwards lookups from the engine's namespace ([docs L85-L87](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L85-L87)).
- **Undocumented internals relied on:**
  - the `DOCKER-USER` chain in the engine namespace, and Docker Desktop's own rules there;
  - the nf_tables backend at 4.97.0;
  - the interface names `eth0`/`services1`;
  - `host.docker.internal` traffic going through `FORWARD`.

  Upstream says the script "Requires Linux Docker Engine … Docker Desktop … not supported" ([L27-L29](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L27-L29)), and its VM recipe says "Docker Desktop has not been tested this way" ([docs L111-L142](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L111-L142)).
- **Failure modes:**
  - If a Docker Desktop update moved the engine to `firewall-backend: nftables`, the script would exit after 30 s ([L111-L120](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L111-L120)), and `restricted` would refuse computers after the next supervisor start.
  - The cached-verdict gap is the one silent failure seen.

## Not tested

- The real Rakazo `web` screen proxy and a browser (only an alpine stand-in on the network).
- The real `lemonade-relay` and Lemonade container under the rules (a stand-in container was used). On Windows they share no bridge with computers ([#24](https://github.com/kamui/rakazo-stack/issues/24)).
- WSL `mirrored` networking, a configured Windows system proxy, `IPv6Only`, Docker Desktop's host networking, the Hyper-V backend, Enhanced Container Isolation, and other Docker Desktop versions.
- A Docker Desktop restart and `wsl --shutdown` with the built-in-image, Windows-Compose variant (only the reboot was run with it).
- Bind mounts of Windows paths (`C:\…`) across a reboot, and whether `restart: always` retries a failed mount.
- The Windows-side watcher in Boot mode did not fire within two minutes of logon, so the reboot state was read with `docker inspect` afterwards.
- Linux Docker Engine (the other half of #27).

## Open questions / follow-ups

- **Compose layout on Windows.**
  - Ship the sidecar as a built image with the script pinned by tag and checked by digest.
  - Never bind-mount it.
  - Use `restricted`.
- **Setup on Windows should run `docker compose` from Windows (PowerShell), not from a WSL distro.** Otherwise the supervisor's `docker.sock` depends on the distro, and the Instance doesn't recover from `wsl --shutdown` or a reboot. This is a new constraint on the setup tool.
- **The cached verdict in `restricted`.**
  - An optional upstream ask is to re-check egress on each provision, or periodically.
  - Locally, setup or a health check could restart the supervisor whenever the sidecar re-applies rules that were missing.
- **Docs.** Upstream's VM recipe, which Docker Desktop for Windows can now be added to, should say not to bind-mount the script on WSL.

## How to reproduce

1. Docker Desktop on WSL 2. Download [`restrict-computer-egress.sh` at v0.4.0](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh).
2. **Scoped test:** from the WSL distro, `HOST_LAN_IP=<host IP> rules-test.sh <path-to-script>`. It starts and stops its own Windows and WSL listeners and removes its rules and networks.
3. **Supervisor test:**
   - Put the script beside [`mini/compose.yaml`](egress-sidecar-windows/mini/compose.yaml). Write a random token to `~/rs27work/token` and run `win-listener.sh start`.
   - Run `mini/dc.sh up -d --wait`. Then use `computer-probe.sh create rs27bot rs27space`, `probe-all.sh` and `screen-check.sh`.
4. **Restarts:** run `docker.exe desktop restart` from Windows, then `after-restart.sh rs27bot`. For `wsl --shutdown`, use [`windows-watch.ps1`](egress-sidecar-windows/windows-watch.ps1) `-Mode WslShutdown` from `%TEMP%`.
5. **Variant that survives:**
   - `docker build -t rs27-egress:local -f mini/egress.Dockerfile mini/`.
   - Copy `compose.yaml`, `compose.baked.yaml` and a `.env` with `SANDBOX_SUPERVISOR_TOKEN` to a Windows folder.
   - Run `docker.exe compose -p rs27-mini -f compose.yaml -f compose.baked.yaml up -d --force-recreate --wait` there.
6. **Clean up:**
   - delete the computers;
   - `dc.sh down -v`, then `vm-rules.sh remove`;
   - remove the `rakazo-computer-rs27*` networks and the `rs27-egress:local` image;
   - `win-listener.sh stop`;
   - check that `iptables-save | grep rakazo-c` is empty.

## Sources

- Rakazo v0.4.0 (commit `b9c652f`):
  - [infra/compose/restrict-computer-egress.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh)
  - [docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)
  - [infra/sandboxes/supervisor/src/index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts)
  - [infra/sandboxes/supervisor/src/computer-spec.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts)
  - [infra/sandboxes/supervisor/src/egress-probe.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/egress-probe.ts)
- Earlier rakazo-stack research:
  - [mac-lemonade-isolation (#25)](https://github.com/kamui/rakazo-stack/blob/research/mac-lemonade-isolation/docs/research/mac-lemonade-isolation.md)
  - [compose-boot (#24)](https://github.com/kamui/rakazo-stack/blob/research/compose-boot/docs/research/compose-boot.md) and its [`computer-probe.sh`](https://github.com/kamui/rakazo-stack/blob/research/compose-boot/docs/research/compose-boot/computer-probe.sh)
- Hands-on observations on this machine: the transcripts above. Docker Desktop's internals (namespaces, `DOCKER-USER` contents, bind-mount sources) were read from the running system, not from Docker documentation.
