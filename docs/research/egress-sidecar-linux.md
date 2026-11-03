# Does the computer-egress sidecar work on Linux Docker Engine? (#27, Linux half)

Research for [Does the computer-egress sidecar work on Docker Desktop for Windows and on Linux Docker Engine?](https://github.com/kamui/rakazo-stack/issues/27). This is the Linux half. The Windows half is [egress-sidecar-windows](https://github.com/kamui/rakazo-stack/blob/research/egress-sidecar-windows/docs/research/egress-sidecar-windows.md), and the macOS original is [mac-lemonade-isolation (#25)](https://github.com/kamui/rakazo-stack/blob/research/mac-lemonade-isolation/docs/research/mac-lemonade-isolation.md).

Pins: Rakazo `v0.4.0` (commit `b9c652f`). Its `restrict-computer-egress.sh` was used unmodified (sha256 `c2802eef397db19603494401c771ed49d57ad371674c28d3a389a1c492ce3dc3`, checked in the workflow).

Hands-on runs:
- **No Linux machine was available,** so everything ran on throwaway GitHub-hosted runners. The workflow is [`.github/workflows/egress-sidecar-linux.yml`](../../.github/workflows/egress-sidecar-linux.yml) on this branch.
- **Runner:** Ubuntu 24.04.5 with an Azure kernel, root access, no global IPv6.
- **Run 2 (primary):** [run 41614516954](https://github.com/kamui/rakazo-stack/actions/runs/41614516954), with Docker Engine 29.9.2 from `download.docker.com`. That is what a Self-hoster installing today gets, and the same Engine as the Windows test.
- **Run 1:** [run 41613297648](https://github.com/kamui/rakazo-stack/actions/runs/41613297648), on the runner's own Engine 28.0.4. A script bug lost most of run 1's probe output, but its rule, restart, legacy, ufw and firewalld results match run 2.
- **One job per scenario:** default nf_tables, legacy iptables, ufw, firewalld, and Docker's nftables firewall backend.

Script: [`linux-test.sh`](egress-sidecar-linux/linux-test.sh). Compose project: [`mini/`](egress-sidecar-linux/mini/). Transcripts: [`transcripts/`](egress-sidecar-linux/transcripts/), with run 1 in [`transcripts/run1-engine-28.0.4/`](egress-sidecar-linux/transcripts/run1-engine-28.0.4/).

## Answer

- **Yes. On Linux Docker Engine the sidecar can replace upstream's `sudo bash restrict-computer-egress.sh` step and its systemd unit,** with one extra setting on legacy-iptables hosts.
  - The sidecar runs the unmodified script from an image with it built in, using `network_mode: host`, `NET_ADMIN`, `restart: unless-stopped` and a 60 s re-apply loop.
  - The supervisor uses `SANDBOX_COMPUTER_EGRESS=restricted`.
  - It needs no sudo from setup: Docker access is already root-equivalent. Persistence comes from the restart policy.
- **Where the rules land: pass.** With `network_mode: host` the sidecar shares the host's network namespace (same `net:[…]` inode), which is where Docker's bridges and `DOCKER-USER` live.
- **iptables backend.**
  - **nf_tables (Ubuntu's default): pass as is.**
  - **Legacy: needs `RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy`.** Without them the sidecar logs `DOCKER-USER chain not present — is Docker running with the iptables backend?` and never becomes healthy, and `restricted` refuses computers with a 500. With them (and Alpine's `iptables-legacy` package), all rules land in the legacy tables, none in nf_tables, and the next create succeeds without a supervisor restart.
  - **Docker's own `firewall-backend: nftables` (Engine 29.9.2): not supported, but fails closed.** There is no `DOCKER-USER` chain, so the sidecar never becomes healthy and `restricted` refuses computers. Engine 28.0.4 rejects the setting outright.
- **Isolation with a real v0.4.0 supervisor and computer: pass.**
  - The supervisor logged `restricted computer egress verified`, and the computer's bridge was `rakazo-c6128c2c`.
  - From inside the computer, all of these timed out:
    - the host's services on all addresses (via the bridge gateway, `docker0` and the host IP);
    - ports published on `0.0.0.0`, via the host IP and the gateway;
    - ports published on `127.0.0.1`;
    - another container by IP;
    - the supervisor by IP;
    - cloud metadata `169.254.169.254`.
  - Internet, DNS (embedded resolver) and SSH out (`github.com:22`) worked.
  - For comparison, an unrestricted container on an ordinary bridge reached the host's services, the `0.0.0.0`-published ports and cloud metadata. It was refused on the `127.0.0.1`-published ports even without the rules: on Linux, unlike Docker Desktop, those were never reachable from containers.
- **The relay path: pass.** A container on an ordinary Compose network still reached the host, other containers and the internet.
- **Screens: pass at the network level** (alpine `web` stand-in, as on Windows). `GET /computers/:id/screen` returned 302, and the supervisor and `web` both fetched the noVNC page across the computer's bridge. With a screen open, the computer can reach the supervisor's `:7091` on its own bridge (upstream's same-bridge exemption; the API needs the bearer token).
- **Restarts: pass.**
  - After `systemctl restart docker`, the rules were back and the supervisor verified.
  - After a simulated boot (Docker stopped, the whole nft ruleset flushed, Docker started), the same.
  - Computers resumed still blocked, with no supervisor restart.
  - With the sidecar stopped, a simulated boot leaves 0 rules and `restricted` returns 500, even for a resume. Starting the sidecar fixes it without restarting the supervisor.
  - A real reboot was not possible on a runner.
- **The cached-verdict gap is the same as on Windows.** With the rules removed and the supervisor still running, a new computer started and reached the host, a `0.0.0.0`-published port and cloud metadata. A supervisor restart then fails closed. When the rules return, they cover that computer again, because they match its bridge name.
- **ufw: pass, no interference.**
  - The script's two `INPUT` rules stay above ufw's jumps after `ufw enable`, `ufw reload`, `ufw disable` + `enable`, and one sidecar loop. `DOCKER-USER` is untouched.
  - ufw's default `deny (incoming)` already blocks every container from the host's own services (the baseline timed out). It does not block ports Docker publishes, other containers or cloud metadata; the sidecar's rules do.
  - The ufw default also cut the relay stand-in off from a host service. That only matters if something the stack needs listens on the host. On Linux, Lemonade runs as a container.
- **firewalld: pass, no interference.**
  - Docker 29 reports `Firewall Backend: iptables+firewalld`, and firewalld 2.1.1 uses its own nftables tables.
  - After `firewall-cmd --reload` and after `systemctl restart firewalld`, all 14 + 7 rules were still there immediately, and the computer stayed blocked.
- **Not covered by the rules: cloud endpoints in public address ranges.** On these Azure-hosted runners, the restricted computer still reached Azure's WireServer at `168.63.129.16:80`. This matters only for Self-hosters on cloud VMs with such endpoints. The script blocks only non-public ranges.
- **Costs.** As on Linux restricted mode generally, computers lose:
  - the host and its published ports;
  - the LAN, the tailnet (`100.64.0.0/10`) and other containers;
  - cloud metadata.

  The sidecar is one more privileged container. That is no more than the supervisor already has through the Docker socket.

## Details

### Scenarios

| Scenario | Host firewall | Sidecar | Supervisor | Computer | After reload/restart |
|---|---|---|---|---|---|
| nft | iptables-nft, Docker `iptables` backend | healthy, 14 v4 + 7 v6 rules | verified | blocked; internet/DNS/SSH ok | `docker` restart and simulated boot: rules back, computer resumed blocked |
| legacy (default env) | `update-alternatives` to iptables-legacy | **not healthy**: `DOCKER-USER chain not present` | unavailable; create 500 | none | n/a |
| legacy + `RAKAZO_IPTABLES=iptables-legacy` | same | healthy, rules only in legacy tables | create 200, no restart | blocked | n/a |
| ufw | ufw active, default deny in/routed | healthy, rules above ufw's jumps | verified | blocked | `ufw reload`, disable/enable: order kept, still blocked |
| firewalld | firewalld 2.1.1 (nftables), Docker `iptables+firewalld` | healthy | verified | blocked | `--reload`, restart: rules kept, still blocked |
| docker-nftables | Docker `firewall-backend: nftables` (29.9.2) | **not healthy**: no `DOCKER-USER` | unavailable; create 500 | none | n/a |

### Probe targets

- The host ran two listeners: `127.0.0.1:18927` and `0.0.0.0:18928` (Python `http.server`).
- A `pubapp` stand-in published `127.0.0.1:3100` and `127.0.0.1:5173` the way upstream publishes Rakazo's API and web, plus `0.0.0.0:8080`.
- Probes are TCP connects from inside the container (bash `/dev/tcp`, 4 s): `open`, `timeout` (dropped) or `refused`.
- The "baseline" is the computer image on an ordinary bridge, where no rules apply.

### Choosing the backend without the Self-hoster

- Upstream's `sudo` path runs the host's own `iptables`, which follows the host's alternative, so it picks the right backend automatically.
- The sidecar ships its own iptables (Alpine's is nf_tables), so it must be told on legacy hosts.
- rakazo-stack can detect this from inside the sidecar image without patching upstream. If `iptables-legacy -L DOCKER-USER` succeeds and `iptables -L DOCKER-USER` does not, set `RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy` before calling the script. This wrapper was **not tested**; the env vars themselves were.

## Not tested

- A real reboot. Only `systemctl restart docker` and a simulated boot (Docker stopped, nft ruleset flushed, Docker started) were run.
- A home Linux machine: a real LAN, a tailnet, a host with global IPv6. The runners had no global IPv6, so the `ip6tables` rules were installed but not exercised.
- Other distributions (Debian, Fedora with firewalld as the default, Arch), rootless Docker, and Podman.
- The real Rakazo `web` screen proxy and a browser; the real `lemonade-relay` and Lemonade under the rules.
- Upstream's `sudo bash restrict-computer-egress.sh` plus systemd path, used as a comparison, and running it alongside the sidecar.
- The backend-detection wrapper.

## Open questions / follow-ups

- **Linux layout:** use the same sidecar as Windows and macOS, with the script built into the image and `restricted`, plus backend detection for legacy hosts. Drop the planned `sudo` step and systemd unit.
- **The cached verdict in `restricted`** is the same gap on every platform. See the Windows doc's follow-up: an upstream ask, or a supervisor restart when the sidecar finds rules missing.
- **Cloud VMs:** decide whether rakazo-stack documents, or blocks, provider endpoints in public ranges (Azure `168.63.129.16`).
- **`firewall-backend: nftables`:** if Docker makes it the default, restricted mode stops working everywhere until upstream supports it. It fails closed, so computers would refuse to start rather than run open.

## How to reproduce

1. Push to `research/egress-sidecar-linux`, or run the workflow by hand (`workflow_dispatch`).
2. Each matrix job:
   - fetches and checksums the script;
   - installs Docker Engine from `download.docker.com`;
   - pulls the images;
   - runs `sudo linux-test.sh <scenario>` with a random, masked supervisor token;
   - uploads `transcript.log`.
3. The runners are discarded afterwards, so no cleanup is needed.

## Sources

- Rakazo v0.4.0 (commit `b9c652f`):
  - [infra/compose/restrict-computer-egress.sh](https://github.com/elie222/rakazo/blob/v0.4.0/infra/compose/restrict-computer-egress.sh): backends and env vars at L27-L38, the `DOCKER-USER` wait at L111-L120.
  - [docs/self-host-docker-computers.md](https://github.com/elie222/rakazo/blob/v0.4.0/docs/self-host-docker-computers.md): the "Docker in a Linux VM" sidecar at L111-L142.
  - [infra/sandboxes/supervisor/src/index.ts](https://github.com/elie222/rakazo/blob/v0.4.0/infra/sandboxes/supervisor/src/index.ts): the cached `restrictedEgressVerified` at L1224-L1253.
- Earlier rakazo-stack research:
  - [egress-sidecar-windows (#27)](https://github.com/kamui/rakazo-stack/blob/research/egress-sidecar-windows/docs/research/egress-sidecar-windows.md)
  - [mac-lemonade-isolation (#25)](https://github.com/kamui/rakazo-stack/blob/research/mac-lemonade-isolation/docs/research/mac-lemonade-isolation.md)
  - [compose-boot (#24)](https://github.com/kamui/rakazo-stack/blob/research/compose-boot/docs/research/compose-boot.md), including `computer-probe.sh`.
- Hands-on: the workflow runs and transcripts above.
