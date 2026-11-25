# Measure an Instance with Hindsight on an Apple-silicon Mac (#46)

The Windows run measured RAM and disk on Docker Desktop for Windows ([`../hindsight-hardware.md`](../hindsight-hardware.md)). This page is a step list to fill in the Mac rows. It works on its own: no API keys, no accounts, and nothing is spent.

**What runs:**
- Lemonade natively (`lemond`), as #42 decided for macOS, with Whisper-Large-v3-Turbo on Metal and kokoro-v1.
- Docker Desktop running Rakazo, `lemonade-relay`, Hindsight 1.3.0 and `hindsight-db`, Tailscale (not logged in), and one bot computer.

[`stack/measure-mac.sh`](stack/measure-mac.sh) repeats the Windows stages: memory off → + Hindsight (boot peak, idle, after seeding 500 memories and 20 recalls) → + Tailscale → + one bot computer. Hindsight gets a dummy model key and no internet. It is measured idle and under recall, not while extracting facts. Everything is a throwaway Compose project named `rs46`.

## What you need

- An Apple-silicon Mac with 16 GB or more. Write down the model, RAM and macOS version; the script prints them too.
- Docker Desktop running, with default settings. Write down its **Resources → Memory limit**.
- About 20 GB free disk: ~13 GB of images and ~2 GB of voice models.
- `git`, `curl` and `openssl`, all present on a stock Mac.
- Nothing listening on ports 13955–13957. The Instance binds them on `127.0.0.1` only.
- No other Compose project named `rs46` and no container named `rs46-computer-probe`.
- Close what you can, as #42's Mac run was under heavy background load. Note what stays open.

## Steps

1. **Get the scripts.**

   ```sh
   git clone https://github.com/kamui/rakazo-stack.git
   cd rakazo-stack
   git switch research/hindsight-hardware
   cd docs/research/hindsight-hardware/stack
   ```

2. **Choose the Rakazo image tag.** Rakazo v0.16.0 was released without images on GHCR. On 2026-11-25, `app:v0.16.0` and `computer:v0.16.0` returned 404. Check again:

   ```sh
   docker manifest inspect ghcr.io/elie222/rakazo/app:v0.16.0 >/dev/null 2>&1 && echo published || echo missing
   ```

   - If it's published, use `TAG=v0.16.0`.
   - If it's missing, use the newest published tag (`TAG=v0.15.0`, arm64 images exist). Say so in the results. The Windows run used a local build of v0.16.0, and RAM shouldn't differ much between adjacent releases.
   - To build v0.16.0 instead (takes a while):

     ```sh
     git clone --depth 1 -b v0.16.0 https://github.com/elie222/rakazo.git /tmp/rakazo-v0.16.0
     docker build -f /tmp/rakazo-v0.16.0/infra/compose/Dockerfile -t rs46-local/rakazo-app:v0.16.0 /tmp/rakazo-v0.16.0
     ```

     Then add `RAKAZO_IMAGE=rs46-local/rakazo-app` in step 3, and use `computer:v0.15.0` for the bot computer.

3. **Write `.env`.** It holds random secrets only and never prints them.

   ```sh
   TAG=v0.15.0   # or v0.16.0, from step 2
   RAKAZO_IMAGE_TAG=$TAG RAKAZO_COMPUTER_IMAGE_TAG=$TAG \
     HW_LEMONADE_UPSTREAM=http://host.docker.internal:13957 ./make-env.sh .
   ```

4. **Pull images first**, so download time doesn't land inside the measurements.

   ```sh
   docker compose -p rs46 --env-file .env -f rakazo.v0.16.0.images.yml -f compose.hw.yaml \
     --profile hindsight --profile tailscale pull
   docker pull ghcr.io/elie222/rakazo/computer:$TAG
   docker pull busybox:1
   ```

5. **Run the measurement.** It takes about 15 minutes, plus about 2 GB of model downloads.

   ```sh
   COMPUTER_IMAGE=ghcr.io/elie222/rakazo/computer:$TAG \
     ./measure-mac.sh clips "$HOME/rs46-mac-work" 2>&1 | tee mac-ram.log
   ```

   The script does this:
   - Downloads Lemonade's `lemonade-embeddable-2026.47.1-macos-arm64.tar.gz` and checks its SHA-256 against the release digest (`b6efc95a…74df8`). It stops if the digest doesn't match.
   - Starts `lemond` on `127.0.0.1:13957` with its own `HF_HOME` under the work directory.
   - Sets `whispercpp.backend=metal`, pulls and loads both models, and makes one transcription and one reply.
   - Brings up the Docker stages.

   To measure a newer stable Lemonade, set `LEMONADE_VERSION` and `LEMONADE_SHA256` from the release page.

   If `lemond` doesn't come up:
   - Run `"$HOME/rs46-mac-work/lemonade/"*/lemond --help`, and look at `out/lemond.log`.
   - The positional argument is the directory for `config.json` and backends. The script passes the extracted folder.

6. **Fill in the Mac column** of the tables in [`../hindsight-hardware.md`](../hindsight-hardware.md), section 2 (see "What to read from the log" below).

7. **Tear down.** This removes only what the script created.

   ```sh
   kill "$(cat "$HOME/rs46-mac-work/lemond.pid")"
   docker rm -f rs46-computer-probe
   docker compose -p rs46 --env-file .env -f rakazo.v0.16.0.images.yml -f compose.hw.yaml \
     --profile hindsight --profile tailscale down -v
   rm -rf "$HOME/rs46-mac-work" out .env
   ```

   Remove any images you pulled only for this with `docker image rm`.

8. **Send back `mac-ram.log`.** It holds no secrets. Commit it as `transcripts/mac-ram.log` on this branch, and put the machine, the Docker Desktop memory limit and the image tag at the top.

## What to read from the log

All container figures are `docker stats` memory. For native processes, use **footprint** (`vmmap`). It includes Metal buffers and swapped or compressed pages, so it's the number to budget for. RSS drops whenever macOS swaps an idle model out (#42).

| Row | Where in the log | Mac |
|---|---|---|
| Rakazo: api, worker, web, supervisor, postgres | `--- idle, memory off`, sum of those five | |
| lemonade-relay | same snapshot | |
| Native voice: lemond + whisper-server + koko, footprint | `--- native voice, after one transcription and one reply` and the last snapshot | |
| Hindsight (API + worker), empty and idle | `--- idle, with Hindsight (empty)`, `rs46-hindsight-1` | |
| Hindsight boot peak and time to healthy | `hindsight peak from start…`, `hindsight healthy after…` | |
| Hindsight peak while seeding 500 memories and 20 recalls, and recall times | `hindsight peak while seeding…`, `20 recalls: median … max …` | |
| Hindsight 30 s and 150 s after use | the two `with Hindsight, … after …` snapshots | |
| hindsight-db | `rs46-hindsight-db-1` in the same snapshots | |
| Tailscale, not logged in | `--- idle, with Hindsight and Tailscale` | |
| One bot computer, idle and with a browser | the two stage 4 snapshots | |
| Docker Desktop VM: total, used, available | `vm:` line in the baseline and the last snapshot | |
| macOS Docker processes (VirtualMachine footprint, `com.docker.*`, Docker Desktop) | `mac …` lines in the same snapshots | |
| Swap in use and free-memory percentage | `mac swap:` and `System-wide memory free percentage` | |
| Disk: images, volumes, voice models | `=== disk` | |

**Then check the macOS minimum (16 GB):**
- Total = macOS's own use + native voice footprint + Docker Desktop's VM. The VM reserves up to its memory limit; its footprint shows how much it actually holds.
- Does the Instance with memory, Tailscale and one idle computer fit in 16 GB with room for the user's own apps? Swap growing during the run is the sign that it doesn't.
