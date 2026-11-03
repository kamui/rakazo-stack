# Can bot computers on Docker Desktop for Mac be kept off Lemonade's model servers? (#25)

Research for [Can bot computers on Docker Desktop for Mac be kept off Lemonade's model servers?](https://github.com/kamui/rakazo-stack/issues/25), which follows the hands-on finding in [Test Lemonade natively on an Apple-silicon Mac](https://github.com/kamui/rakazo-stack/issues/16).

Pins: Rakazo `v0.4.0` (commit `b9c652f`), Lemonade `v2026.44.1` (commit `69948a7d`), whisper.cpp `v1.8.4` as built by `lemonade-sdk/whisper.cpp-rocm` (commit `d92d51e9`; `examples/server/server.cpp` is byte-identical to `ggml-org/whisper.cpp` `v1.8.4`), Kokoros `b21` (`lemonade-sdk/Kokoros`, commit `3696f82c`), Docker docs at commit `2eaa01ac`.

Hands-on runs: an Apple-silicon Mac, macOS 27.2, Docker Desktop 4.97.0 (Engine 29.9.2) with default settings. They used throwaway Compose projects, a stand-in listener on the Mac's `127.0.0.1`, and no admin rights. Everything was removed afterwards. Scripts: [`mac-lemonade-isolation/`](mac-lemonade-isolation/). Transcripts: [`mac-lemonade-isolation/transcripts/`](mac-lemonade-isolation/transcripts/).

## Answer

- **Yes, without admin rights, without a paid plan and without patching upstream.**
  - Run Rakazo's own `restrict-computer-egress.sh` (v0.4.0, unmodified) inside Docker Desktop's Linux VM. It runs from a small sidecar container with `network_mode: host` and `NET_ADMIN`.
  - Set `SANDBOX_COMPUTER_EGRESS=restricted` for the supervisor.
  - Rakazo's docs already describe this sidecar for "Docker in a Linux VM" (OrbStack), but say "Docker Desktop has not been tested this way". It was tested here and works.
- **What it blocks.** A real Rakazo v0.4.0 bot computer timed out on:
  - the Mac's `127.0.0.1` through `host.docker.internal`, and through `192.168.65.254` by IP;
  - the supervisor port on the host;
  - `169.254.169.254`;
  - Docker Desktop's internal endpoints, including its HTTP proxy on `192.168.65.1:3128`.

  The supervisor logged `restricted computer egress verified`. Internet, DNS and SSH out still worked. lemond's children bind `127.0.0.1` the same way as the stand-in listener, so `:8001`, `:8002`, the next ports and `:9000` are blocked too.
- **The relay still works.** The rules match only the supervisor's `rakazo-c*` bridges. A container on an ordinary Compose network (where `lemonade-relay` sits) still reached the Mac's `127.0.0.1` through `host.docker.internal`.
- **It needs a sidecar, because the rules don't survive a restart.** A Docker Desktop restart wipes them. With the sidecar stopped, 0 rules were left. With `restart: unless-stopped`, the sidecar put them back.
- **Use `restricted` on the Mac, not the default `auto`.**
  - After a restart, Docker starts containers from their restart policies and ignores Compose `depends_on`. `auto` probes once at supervisor start and keeps that answer. If the supervisor probes before the sidecar has re-applied the rules, it stays `open` until the next supervisor restart. In the test the sidecar won by 7 s, which was luck, not ordering.
  - `restricted` fails closed. Without the rules, creating a computer returns 500 naming the script. Once the sidecar is back, the next create succeeds with no supervisor restart.
- **Nothing on the Lemonade side can protect the children.**
  - lemond hard-codes each child's host as `127.0.0.1` and picks the first free port from 8001. It has no setting for child host, port, socket or key.
  - whisper-server `v1.8.4` and Kokoros `b21` have no API key or Unix socket option.
  - whisper-server's `--host` and `--request-path` aren't reserved by lemond, but lemond always calls `http://127.0.0.1:<port>/inference`. Changing either breaks transcription.
- **lemond's WebSocket port `:9000` is not a hole.** It enforces `LEMONADE_API_KEY` (or `LEMONADE_ADMIN_API_KEY`) on every upgrade. Unauthenticated `/spans/stream` connections are accepted but get no data.
- **Rakazo's other settings don't help.**
  - `SANDBOX_COMPUTER_DNS` only changes resolvers. The computer still connects to `192.168.65.254` by IP.
  - There is no extra-hosts, internal-network or per-computer network option.
  - `SANDBOX_SCREEN_NETWORK=internal` would put computers on the supervisor's own network.
- **The Docker Desktop features that could block host access need Docker Business and admin rights.**
  - Air-gapped containers (`containersProxy` with a PAC file that can reject `host.docker.internal`) and Enhanced Container Isolation both need Business.
  - Both are set through `admin-settings.json` in `/Library/Application Support/com.docker.docker/`, which needs admin.
  - ECI also blocks Docker socket mounts by default, which would break Rakazo's supervisor.
- **macOS `pf` and the Application Firewall need admin, and they can't tell the relay from a bot computer.** Docker Desktop's docs say all container traffic leaves through `com.docker.backend`, running as the same user as lemond.
- **What it costs.**
  - Bot computers also lose the Mac, the LAN, the tailnet (`100.64.0.0/10`) and other installs' published ports. This is the same as Linux restricted mode, and it also closes the host-loopback paths #24 found.
  - It relies on Docker Desktop internals that Docker doesn't document: an iptables `DOCKER-USER` chain in the VM (nf_tables backend at 4.97.0). If a Docker Desktop update removes it, `restricted` refuses to start computers rather than running them open.
- **How setup verifies it.** Read the supervisor log for `restricted computer egress verified`. Optionally probe from a real computer that `host.docker.internal:8001` times out, using compose-boot's `computer-probe.sh` pattern.

## Options compared

"Blocks children?" means a bot computer can no longer open `127.0.0.1:8001`/`:8002`/... on the Mac. "Relay works?" means `lemonade-relay` still reaches `host.docker.internal:13305`.

| Option | Blocks children? | Relay works? | Admin? | Paid? | Verifiable how |
|---|---|---|---|---|---|
| **A. VM firewall sidecar + `SANDBOX_COMPUTER_EGRESS=restricted`** | **Yes** (tested with a real computer) | **Yes** (tested) | No | No | Supervisor log `restricted computer egress verified`; computer probe times out; `restricted` refuses computers when the rules are missing |
| B. Air-gapped containers (`containersProxy` + PAC) | Probably: the PAC can reject by host/IP and port (not tested) | Yes, if the PAC allows `:13305` | Yes (`admin-settings.json` in `/Library`) | Docker Business + enforced sign-in | Probe from a container |
| C. Enhanced Container Isolation | No: nothing in it targets host access | n/a | Yes | Docker Business | n/a; also blocks the supervisor's Docker socket mount by default |
| D. `SANDBOX_COMPUTER_DNS` | No: connections by IP still work | Yes | No | No | Tested: `192.168.65.254:<port>` reachable without DNS |
| E. `SANDBOX_SCREEN_NETWORK=internal` on a Docker `internal` network | Likely, but computers lose the internet and share the supervisor's network | Only if the relay is off that network | No | No | Rejected from source; not run |
| F. lemond / whisper-server / Kokoros options (host, socket, key, ports) | No such option for the children | n/a | No | No | Rejected from source |
| G. macOS `pf` | Only if lemond runs as a separate macOS user and `pf` filters on `user` | Yes | Yes (`pfctl`, a new user, a boot-time anchor) | No | `pfctl -s rules`; probe from a container. Not tested |
| H. Application Firewall | No: it decides incoming connections per receiving app, and both sides connect to the same listener | n/a | Yes | No | n/a |
| I. Per-app outbound firewall (third-party) blocking `com.docker.backend` → `127.0.0.1:8001-8099` | Probably (not tested) | Yes | Yes (system extension approval) | Varies | Probe from a container |
| J. Run Lemonade in a container or VM instead | Yes | Yes | No | No | Loses Metal Whisper and Kokoro on arm64 ([lemonade-macos](https://github.com/kamui/rakazo-stack/blob/research/lemonade-macos/docs/research/lemonade-macos.md)) |
| K. Accept and document | No | Yes | No | No | n/a |

## Details

### A. VM firewall sidecar (tested)

- **Why it can work.** Docker Desktop runs the engine in a Linux VM. A container with `network_mode: host` shares the VM's network namespace, and `NET_ADMIN` lets it change the VM's iptables. Docker's own ECI page says this is possible without ECI: its example is privileged containers altering "Docker Desktop's network configuration", which ECI then prevents ([ECI `_index.md` L77-L79](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/_index.md#L77-L79)).
- **The VM's firewall at 4.97.0.**
  - iptables uses the nf_tables backend, and the `DOCKER-USER` chain exists for IPv4 and IPv6.
  - Docker Desktop puts its own rules there: `-i eth0 -j ACCEPT`, plus accepts to a `services1` interface on ports 3128, 5555 and 53 ([transcript 01](mac-lemonade-isolation/transcripts/01-vm-egress-test.log)).
  - The script inserts its rules at the head, above those.
- **Rules.** The unmodified [`restrict-computer-egress.sh --apply`](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L367-L369) inserts:
  - `INPUT` drops for `rakazo-c+`;
  - `DOCKER-USER` drops from `rakazo-c+` to every non-public range, including `192.168.0.0/16` (Docker Desktop's `192.168.65.0/24`) and `fc00::/7` (its `fdc4:…` IPv6) ([L56-L109](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L56-L109)).

  The supervisor names computer bridges `rakazo-c<hash>` only in restricted mode ([`computerNetworkCreateOptions`, computer-spec.ts L441-L455](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L441-L455)).
- **Test 1: scoped scratch run** ([`vm-egress-test.sh`](mac-lemonade-isolation/vm-egress-test.sh), [transcript 01](mac-lemonade-isolation/transcripts/01-vm-egress-test.log)).
  - The script's `BRIDGE_PREFIX` was changed to `rs25t-c` so the rules could match only the test bridge.
  - Before the rules, both networks reached the Mac's `127.0.0.1:18925` through `host.docker.internal` and `192.168.65.254`.
  - After the rules, the matching bridge timed out on both and still reached `1.1.1.1:443` and `example.com:443`. The ordinary network was unchanged.
  - The bridge gateway went from refused to timeout. That is exactly what Rakazo's probe reads as "enforced".
- **Test 2: the real supervisor** ([`mini/compose.yaml`](mac-lemonade-isolation/mini/compose.yaml), [transcript 02](mac-lemonade-isolation/transcripts/02-rakazo-supervisor.log)).
  - Setup: the supervisor from `ghcr.io/elie222/rakazo/app:v0.4.0`, configured as in upstream's compose file, plus the sidecar from [Rakazo's docs L111-L142](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L111-L142) and a relay stand-in.
  - With `auto`, the supervisor logged `restricted computer egress verified`. A computer created through its API got bridge `rakazo-c596a7b0`.
  - From inside the computer, these timed out: `host.docker.internal:18925`, `192.168.65.254:18925`, `host.docker.internal:7091`, `169.254.169.254:80`, `http.docker.internal:3128`, `gateway.docker.internal:53/80` and `vm.docker.internal:22`.
  - From inside the computer, these were open: `1.1.1.1:443`, `example.com:443` and `github.com:22`.
  - The relay stand-in still reached the Mac listener.
- **Restarts.**
  - `docker desktop restart` wipes the rules: 0 left with the sidecar stopped. With `restart: unless-stopped` the sidecar re-applies them (12 `DOCKER-USER` rules plus the `INPUT` pair).
  - Computers have no restart policy, so they stay stopped until the supervisor provisions them again.
  - The resumed computer was still blocked.
- **Why `restricted`, not `auto`.**
  - `auto` caches the first completed verdict until the supervisor restarts ([Rakazo docs L65-L77](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L65-L77)).
  - After a VM restart, the supervisor and the sidecar start together from their restart policies. The sidecar first runs `apk add`, which needs the network. The ordering is not guaranteed.
  - With `restricted` and no rules, `POST /computers` returned 500 `SANDBOX_COMPUTER_EGRESS=restricted is not enforced on this Docker host … (a computer bridge reached the host)`. After the sidecar came back, the next create returned 200 with no supervisor restart, as the docs say ([L59-L63](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L59-L63)).
  - Upstream's compose file takes `SANDBOX_COMPUTER_EGRESS` from `.env` ([docker-compose.images.yml L333](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L333)), so rakazo-stack sets it without patching.
- **Trust.**
  - The sidecar is root in the VM's network namespace. That's no more than the supervisor already has through the Docker socket ([Rakazo docs L8](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L8)).
  - Computers can't reach the sidecar, because it has no ports.
  - It needs no Mac admin rights and no Docker Desktop setting.
- **Fragility.**
  - The approach depends on Docker Desktop internals that Docker doesn't document: the VM's iptables backend, the `DOCKER-USER` chain, and container traffic to `host.docker.internal` passing through `FORWARD`.
  - If an update moves the engine to `firewall-backend: nftables`, the chain disappears. The script then exits after 30 s ("DOCKER-USER chain not present", [L111-L120](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh#L111-L120)), and `restricted` refuses computers. The failure is loud, not silent.
  - If Docker Desktop rewrote `DOCKER-USER` at runtime, the 60 s loop would put the rules back. Computers would be open for up to one loop interval.
- **Harness shortcuts not to copy into the real layout:**
  - The test sidecar runs `apk add` at every start. A real one should use an image with `bash iptables ip6tables` already installed, so boot needs no network.
  - Its loop `touch`es the health file even if `--apply` fails. Use `--apply && touch`.

### B, C. Docker Desktop's paid controls

- **Air-gapped containers.**
  - What it does: applies proxy rules or a PAC file to running containers' outbound TCP on `transparentPorts` ([air-gapped-containers.md L27-L30](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md#L27-L30)).
  - `PROXY reject.docker.internal:<port>` blocks, and the docs' own example blocks `host.docker.internal` ([L165](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md#L165), [L189-L192](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md#L189-L192)). Docker Desktop 4.29.0 added "connections to `host.docker.internal` to be blocked by a rule in a PAC file" ([release-notes.md L3474](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/release-notes.md#L3474)).
  - The PAC sees hostnames only on ports 80 and 443, and IP addresses elsewhere ([L39](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md#L39)). A rule for `:8001`+ would have to match `192.168.65.254` and the port.
  - It applies to every container, not just computers. That's fine here, because only lemond itself needs the children.
  - Requirements: Docker Business, enforced sign-in and Settings Management ([L41-L47](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md#L41-L47); [summary.yaml L16-L17](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/data/summary.yaml#L16-L17)). The settings file lives at `/Library/Application Support/com.docker.docker/admin-settings.json` and is ignored unless the user is signed in to a Business org ([configure-json-file.md L17-L27, L35-L37](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/settings-management/configure-json-file.md#L17-L37)). Writing under `/Library` needs admin.
- **Enhanced Container Isolation.**
  - It requires Docker Business ([summary.yaml L241-L242](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/data/summary.yaml#L241-L242)).
  - It hardens containers against the VM. It has no host-access rule.
  - It blocks Docker socket mounts by default ([config.md L18](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/config.md#L18)), which the supervisor needs. It also blocks `--network=host` ([`_index.md` L113-L117](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/_index.md#L113-L117)), so it would also stop option A.
- **Free settings don't help.**
  - The network settings (`defaultNetworkingMode`, `dnsInhibition`, `portBindingBehavior`, `vpnkitCIDR`) have nothing that disables `host.docker.internal` ([settings-reference.md L637-L676](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/settings-management/settings-reference.md#L637-L676)).
  - Bridge driver options such as `enable_ip_masquerade` and `gateway_mode_ipv4` act on a whole network ([bridge.md L121-L123](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/network/drivers/bridge.md#L121-L123)). Rakazo creates computer networks itself, so rakazo-stack can't pass them.

### D, E. Rakazo v0.4.0 supervisor options

- **What the supervisor sets on a computer.** Its create call sets `NetworkMode` and optional `Dns` only. It has no `ExtraHosts`, internal flag or other network knob ([`containerCreateOptions`, computer-spec.ts L570-L627](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L570-L627)).
- **Per-bot networks** are plain bridges with labels. Driver options are set only in restricted mode (the bridge name), and there is no `Internal` ([L441-L455](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L441-L455)).
- **Environment variables read by the supervisor** for computer networking:
  - `SANDBOX_COMPUTER_EGRESS` (`open|restricted|auto`, [L199-L222](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L199-L222));
  - `SANDBOX_COMPUTER_DNS` (resolver IPs, [L224-L242](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts#L224-L242));
  - `SANDBOX_SCREEN_NETWORK`;
  - `SANDBOX_CONTROL_VIA_LOOPBACK`.
- **`SANDBOX_COMPUTER_DNS`** doesn't stop connections by IP. Test 1 reached `192.168.65.254:<port>` directly.
- **`SANDBOX_SCREEN_NETWORK=internal`.**
  - It puts every computer on the supervisor's first network ([index.ts L1675-L1686](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts#L1675-L1686)) and turns off restricted mode ([docs L106-L109](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md#L106-L109)).
  - Making that network a Docker `internal` one would cut computers off from the internet. It would also put them beside the supervisor's API.
  - Upstream hard-codes `isolated` ([docker-compose.images.yml L332](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml#L332)). Rejected.

### F. Lemonade, whisper-server and Kokoros

- **lemond's child ports.** `WrappedServer::choose_port()` returns `find_free_port(8001)` ([wrapped_server.cpp L558-L569](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/wrapped_server.cpp#L558-L569)). The child host is fixed at `127.0.0.1` ([wrapped_server.h L635](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/wrapped_server.h#L635), [wrapped_server.cpp L220](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/wrapped_server.cpp#L220)).
- **lemond's config keys.** `defaults.json` has `host`, `port` and `websocket_port`, but nothing for child servers ([defaults.json](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/defaults.json)).
- **Kokoro.** lemond starts it with `openai --ip 127.0.0.1 --port <port>` and no user arguments ([kokoro_server.cpp L92-L120](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/kokoro/kokoro_server.cpp#L92-L120)).
  - Kokoros' server takes only `--ip` and `--port` ([koko/src/main.rs L75-L80](https://github.com/lemonade-sdk/Kokoros/blob/b21/koko/src/main.rs#L75-L80)).
  - It serves `/v1/audio/speech`, `/v1/audio/voices` and `/v1/models` with no authentication ([kokoros-openai/src/lib.rs L482-L487](https://github.com/lemonade-sdk/Kokoros/blob/b21/kokoros-openai/src/lib.rs#L482-L487)).
- **Whisper.** lemond passes `-m <model> --port <port>` and reserves only `-m`, `--model` and `--port`. `whispercpp.*_args` are appended ([whispercpp_server.cpp L258-L290](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp#L258-L290)). But lemond always posts to `http://127.0.0.1:<port>/inference` ([L490](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp#L490), [L549](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp#L549)).
  - whisper-server's options are `--host` (default `127.0.0.1`), `--port`, `--public`, `--request-path`, `--inference-path`, `--convert` and `--tmp-dir`. There is no key and no socket ([server.cpp L59-L62, L173-L179, L254-L258](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/v1.8.4/examples/server/server.cpp#L170-L180)).
  - `/load` is always registered ([L1127](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/v1.8.4/examples/server/server.cpp#L1127)), and the server binds a TCP host and port ([L1208](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/v1.8.4/examples/server/server.cpp#L1208)).
  - Moving `--host` off `127.0.0.1`, or adding a secret `--request-path`, breaks lemond's own calls.
  - Another loopback address (`127.0.0.2`) would need `ifconfig lo0 alias` (root) and would still break lemond's `127.0.0.1` calls.
- **WebSocket `:9000`.** It binds lemond's `host` ([websocket_server.cpp L215-L219](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/websocket_server.cpp#L215-L219)) and reads `LEMONADE_API_KEY` and `LEMONADE_ADMIN_API_KEY` ([L33-L47](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/websocket_server.cpp#L33-L47)).
  - It rejects upgrades with a missing or wrong key ([L394-L433](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/websocket_server.cpp#L394-L433)).
  - The one exception is `/spans/stream`, which is accepted unauthenticated but only fed to authenticated sessions ([L891-L896](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/websocket_server.cpp#L891-L896)).
  - The plain GET in #16's probe got no HTTP response, which fits.

### G-I. macOS host firewalls

- **All container traffic looks like one process.** Docker Desktop: "All outbound container network traffic originates from the `com.docker.backend` process", and "to restrict VM or container networking apply rules to … `com.docker.backend`" ([networking `_index.md` L49, L81](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/_index.md#L49)). A host firewall therefore sees the relay and a bot computer as the same process.
  - A host rule could still block every container from the child ports and leave `:13305` open, because only lemond itself uses the children.
- **`pf`.**
  - It needs root: `/dev/pf` is `root:wheel 0600`, and `/etc/pf.conf` is root-owned.
  - `com.docker.backend` and lemond connect from the same address (`127.0.0.1`) as the same user.
  - `pf.conf(5)`'s `user` keyword matches "the user that opened the connection" for outgoing packets. So a `pf` rule could tell the two apart only if lemond ran as a separate macOS user. That means creating a user (admin), loading an anchor at boot (a root LaunchDaemon), and running a per-user lemond as someone else.
  - Not tested.
- **Application Firewall.** `socketfilterfw(8)` decides whether an application may "receive incoming connections". The receiving app is the child server for both lemond and Docker Desktop, so it can't separate them. Changing it needs admin.
- **Third-party per-app outbound firewalls.** These could block `com.docker.backend` from `127.0.0.1:8001-8099` and `:9000`. They need a system extension approved by an admin. Not tested.

## Not tested

- **Real lemond children behind the rules.** A `127.0.0.1` stand-in was used. #16 showed the real children bind `127.0.0.1` and are reached the same way.
- **The full stack.** Only the supervisor ran, not the whole stack with the real `lemonade-relay` and `voice.connect`.
- **Screens.** The supervisor and `web` join a computer's bridge for screens, and same-bridge traffic should pass the `RETURN` rule. This was not exercised on Docker Desktop.
- **Other Docker Desktop configurations:** `ipv6only` networking mode, Docker VMM (beta), other VM settings, and other Docker Desktop versions. Only 4.97.0 with default settings was run.
- **Docker Desktop on Windows/WSL2** and **Linux Docker Engine** with the same sidecar.
- **Options B, G and I.** These need a Business plan or admin rights and were not tried.

## Open questions / follow-ups

- **Grilling input ([#26](https://github.com/kamui/rakazo-stack/issues/26)).** The rule "bot computers must not reach Lemonade" can hold on macOS in v1 without admin rights or a paid plan. The cost:
  - computers lose the Mac, the LAN and the tailnet;
  - one extra privileged-in-the-VM sidecar;
  - reliance on Docker Desktop's VM firewall internals.

  The alternative is accept-and-document.
- **New task: add the sidecar to the compose layout on macOS.**
  - A `computer-egress` service with a built image (`bash iptables ip6tables` preinstalled) and Rakazo's `restrict-computer-egress.sh` pinned to `v0.4.0` (fetched by tag and checked by digest).
  - `SANDBOX_COMPUTER_EGRESS=restricted` in `.env`.
  - A health check that fails when `--apply` fails.
  - Setup checks the supervisor log, and optionally runs a computer probe of `host.docker.internal:8001`.
- **New research: the same sidecar on Docker Desktop for Windows/WSL2 and on Linux Docker Engine.**
  - On Windows it isn't needed for Lemonade, which runs in a container there. It would close the host-loopback and published-port paths #24 found.
  - On Linux it could replace the `sudo bash restrict-computer-egress.sh` step that #10/#24 planned. It needs no sudo, because Docker access is already root-equivalent. Rule persistence would come from the restart policy, not systemd.
  - Watch the iptables backend: Alpine's `iptables` is the nf_tables variant. On a legacy-iptables host, `RAKAZO_IPTABLES=iptables-legacy` would be needed.
- **Upstream (optional, outside the map's control).** Ask Lemonade to authenticate or socket-bind its child servers. Then the macOS rule would not depend on Docker internals.

## How to reproduce

1. Start Docker Desktop. Run `python3 -m http.server --bind 127.0.0.1 18925` as a stand-in for lemond's children.
2. Scoped test: download [`restrict-computer-egress.sh` at v0.4.0](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh), then run `vm-egress-test.sh <path-to-it>`. It stops its own listener and removes its rules, networks and temp files.
3. Supervisor test: put the script beside [`mini/compose.yaml`](mac-lemonade-isolation/mini/compose.yaml). Write a random token to a file named by `RS25_TOKEN_FILE`, then run `mini/dc.sh up -d --wait`.
4. Probe with [compose-boot's `computer-probe.sh`](https://github.com/kamui/rakazo-stack/blob/research/compose-boot/docs/research/compose-boot/computer-probe.sh) (`DC=mini/dc.sh`): `create`, then `probe <bot> <space> host.docker.internal:18925 1.1.1.1:443`.
5. Clean up:
   - `delete` the computers;
   - stop the sidecar and run the script with `--remove` from a host-network `NET_ADMIN` container;
   - run `mini/dc.sh down -v` and remove the `rakazo-computer-*` networks.

## Sources

- Rakazo v0.4.0 (commit `b9c652f`):
  - [infra/compose/restrict-computer-egress.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh)
  - [infra/compose/docker-compose.images.yml](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/docker-compose.images.yml)
  - [infra/sandboxes/supervisor/src/computer-spec.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/computer-spec.ts)
  - [infra/sandboxes/supervisor/src/index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts)
  - [docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md)
- Lemonade v2026.44.1 (commit `69948a7d`):
  - [src/cpp/server/wrapped_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/wrapped_server.cpp)
  - [src/cpp/include/lemon/wrapped_server.h](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/include/lemon/wrapped_server.h)
  - [src/cpp/server/backends/kokoro/kokoro_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/kokoro/kokoro_server.cpp)
  - [src/cpp/server/backends/whispercpp/whispercpp_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/backends/whispercpp/whispercpp_server.cpp)
  - [src/cpp/server/websocket_server.cpp](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/server/websocket_server.cpp)
  - [src/cpp/resources/defaults.json](https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/src/cpp/resources/defaults.json)
- whisper.cpp v1.8.4, Lemonade's build (`lemonade-sdk/whisper.cpp-rocm`, commit `d92d51e9`): [examples/server/server.cpp](https://github.com/lemonade-sdk/whisper.cpp-rocm/blob/v1.8.4/examples/server/server.cpp), the same file as [ggml-org/whisper.cpp v1.8.4](https://github.com/ggml-org/whisper.cpp/blob/v1.8.4/examples/server/server.cpp).
- Kokoros b21 (`lemonade-sdk/Kokoros`, commit `3696f82c`): [koko/src/main.rs](https://github.com/lemonade-sdk/Kokoros/blob/b21/koko/src/main.rs), [kokoros-openai/src/lib.rs](https://github.com/lemonade-sdk/Kokoros/blob/b21/kokoros-openai/src/lib.rs).
- Docker docs (commit `2eaa01ac`):
  - [air-gapped-containers.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/air-gapped-containers.md)
  - [settings-management/configure-json-file.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/settings-management/configure-json-file.md)
  - [settings-reference.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/settings-management/settings-reference.md)
  - [enhanced-container-isolation/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/_index.md)
  - [enhanced-container-isolation/config.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/enterprise/hardened-desktop/enhanced-container-isolation/config.md)
  - [desktop/features/networking/_index.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/_index.md)
  - [networking-how-tos.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/features/networking/networking-how-tos.md)
  - [engine/network/drivers/bridge.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/engine/network/drivers/bridge.md)
  - [desktop/release-notes.md](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/content/manuals/desktop/release-notes.md)
  - [data/summary.yaml](https://github.com/docker/docs/blob/2eaa01ac4fbe81c8cb8705724a05e19474dd681a/data/summary.yaml)
- macOS man pages on macOS 27.2: `pf.conf(5)` (`user` keyword), `pfctl(8)`, `socketfilterfw(8)`.
- Earlier rakazo-stack research:
  - [lemonade-mac-test (#16)](https://github.com/kamui/rakazo-stack/blob/research/lemonade-mac-test/docs/research/lemonade-mac-test.md), [`06-probes.log`](https://github.com/kamui/rakazo-stack/blob/research/lemonade-mac-test/docs/research/lemonade-mac-test/transcripts/06-probes.log)
  - [compose-boot (#24)](https://github.com/kamui/rakazo-stack/blob/research/compose-boot/docs/research/compose-boot.md)
  - [lemonade-macos (#5)](https://github.com/kamui/rakazo-stack/blob/research/lemonade-macos/docs/research/lemonade-macos.md)
