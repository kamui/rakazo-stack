# rakazo-stack

Run [Rakazo](https://github.com/elie222/rakazo) on your own computer, with voice and memory included, set up by one command.

> **Not ready yet.** We're still deciding what the first version (v1) will do, so there's nothing to install. You can follow along in [Map: rakazo-stack v1 spec](https://github.com/kamui/rakazo-stack/issues/1). This page describes the plan. Anything marked _not decided yet_ is still open.

Words like **Self-hoster** (you, the person running it) and **Instance** (your copy of it) are explained in [GLOSSARY.md](GLOSSARY.md).

## What it is

- **One setup command.** You run `rakazo-stack setup` and give it your keys. It creates all the passwords it needs, writes the settings, and starts everything.
- **Official releases, not a fork.** It installs the newest stable releases of Rakazo, Lemonade and Hindsight exactly as their makers publish them, and never changes their code.
- **Updates when you say so.** `rakazo-stack update` moves everything to the newest releases. Nothing updates by itself.
- **For one person on one computer.** It isn't a hosted service and isn't built for sharing between groups of people.
- **Private unless you open it up.** By default only your own computer can reach it. If you want to use it from your phone, setup can add Tailscale.

## What's included

| Part | What it does | When it runs |
|---|---|---|
| [Rakazo](https://github.com/elie222/rakazo) | The app itself, including the separate "bot computers" its AI agents work in | Always |
| [Lemonade](https://github.com/lemonade-sdk/lemonade) | Speech and transcription on your own computer, so your voice doesn't go to an outside service | Always. On a Mac it runs outside Docker, so transcription can use the Mac's graphics chip |
| Lemonade relay | Converts recordings from your browser or phone into a format Lemonade can transcribe, and fixes a small mismatch between Rakazo and Lemonade's speech | Always |
| [Hindsight](https://github.com/vectorize-io/hindsight) | Long-term memory | Only if you give setup a model provider key |
| [Tailscale](https://tailscale.com) | Lets your phone and other devices reach it securely, at an address like `https://rakazo.<your-tailnet>.ts.net` | Only if you choose it |
| Bot-computer firewall | Stops bot computers from reaching your computer, your home network and your Tailscale devices | Always |

Setup also creates your admin account in Rakazo and connects your voice to Lemonade. With an OpenAI or Gemini key it connects chat for that account, and if you turned memory on, it connects that too. If any of these steps fails, it tells you how to finish it in Rakazo's settings.

Integrations through [Composio](https://composio.dev) are optional. Setup asks for a Composio key, and you can leave it blank.

## What you need

- **Docker.** Setup checks for it but won't install it for you.
  - **Linux:** Docker Engine and its Compose plugin, installed from Docker's own website, not Docker Desktop. Your user needs to run `docker` without `sudo`. Setup warns you that this gives your user full control of the computer.
  - **Windows and Mac:** Docker Desktop. It's free for personal use and for companies with fewer than 250 employees and under $10M in revenue. Bigger companies need a paid plan.
  - Minimum versions: Docker Engine API 1.45 and Docker Compose 2.24.4.
- **An account with a model provider.** Bring your own: setup asks which one and checks your key.
  - **OpenRouter** (recommended): one key runs chat and memory for everyone on your Instance. Setup picks GLM Flash. In our test it cost about $4–5 a month for one active user sending 30 messages a day, chat and memory together. Turn on **ZDR only** in your OpenRouter privacy settings so providers don't keep your messages.
  - **OpenAI** or **Gemini**: one key runs chat and memory, but only your admin account gets chat. Everyone else connects their own provider in Rakazo's settings. Gemini needs a paid key.
  - **Anthropic**: like OpenRouter, one key runs chat and memory for everyone on your Instance.
  - Leave the key blank and nothing is set up: memory stays off, and each person connects a provider in Rakazo's settings.
- **Optional:** a Composio key, and a Tailscale account with an auth key that isn't ephemeral.
- **Hardware:** at least 4 processor threads, 16 GB of RAM and 35 GB of free disk space. 8 threads and 50 GB of free disk space are recommended. If your computer has less, setup warns you and carries on.
  - A fresh install uses about 16 GB of disk. An update briefly needs more, because it keeps the old version until the new one works.
  - Rakazo's downloads are about 9 GB, the voice models about 2 GB, and memory about 2 GB.

## Where it runs

| System | Supported? | Notes |
|---|---|---|
| Linux on a standard 64-bit PC (x86-64) | Yes | Voice runs on the processor. |
| Windows 11 (23H2 or later) or Windows 10 (22H2) | Yes | Runs through WSL 2 (version 2.1.5 or later) with Docker Desktop. Turning on WSL 2 needs admin rights once. Voice runs on the processor. **Windows Home** works, but Docker no longer lists it as supported, so setup warns you and carries on. |
| Mac with Apple silicon (M1 or later) | Yes | Transcription uses the Mac's graphics chip; speech runs on the processor. |
| Mac with an Intel chip | No | |
| Linux on ARM | No | Lemonade only ships for standard 64-bit PCs. |
| Podman, OrbStack, Colima, or Docker installed directly inside WSL 2 | No | v1 supports only the Docker setups above. Podman also can't run Rakazo's bot computers: they need a newer Docker API version than it offers. |

On Windows and Mac, it only runs while you're logged in to your computer.

## Commands

None of these exist yet. This is the plan.

| Command | What it will do |
|---|---|
| `curl -fsSL https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh \| sh` | Downloads the `rakazo-stack` program for Linux (including WSL 2) or an Apple-silicon Mac, checks the download hasn't been tampered with, and starts `rakazo-stack setup`. To read the script before you run it, download it with `curl -fsSLO` and the same address, read `install.sh`, then run `sh install.sh`. |
| `rakazo-stack setup` | Asks you questions, then sets everything up and starts it. You can run it again any time: it remembers your answers, never shows your saved keys, never changes the passwords it created, and never changes versions. |
| `rakazo-stack setup --no-questions` | Does the same without asking, for scripts. It reads each answer from an environment variable, such as `RAKAZO_STACK_OWNER_EMAIL` and `RAKAZO_STACK_OWNER_PASSWORD`, or else from your saved answers. |
| `docker compose up -d` | Starts it. Run it from `~/rakazo-stack`. |
| `docker compose down` | Stops it. Your data is kept. |
| `docker compose ps` and `docker compose logs` | Shows what's running, and the logs. |
| `rakazo-stack backup` | Saves your data, settings and keys into one file locked with a passphrase you choose, in `~/rakazo-stack-backups`, or the folder you give with `--output`. Rakazo pauses while it runs. For cron or scripts, put the passphrase in `RAKAZO_STACK_BACKUP_PASSPHRASE`. If you lose the passphrase, nobody can open the backup. |
| `rakazo-stack restore <file>` | Brings an Instance back from a backup file, on the same computer or a new one, on any supported system. |
| `rakazo-stack update` | Updates the `rakazo-stack` program, then Rakazo, Lemonade and Hindsight, to their newest releases. It shows what will change and asks first, takes a backup, and offers to undo the update if it fails. `setup` and `backup` remind you, at most once a day, when there's something new. |
| `rakazo-stack update --hold rakazo=v0.5.1` | Keeps one part at a version you choose, for when a new release causes trouble. `--unhold rakazo` lets it update again. |

On Windows, type `docker.exe` instead of `docker` in these commands, for example `docker.exe compose up -d`, still run from `~/rakazo-stack` in WSL. Plain `docker` in WSL can leave parts of it unable to start after you restart your computer. If that happens, the next `rakazo-stack setup` or `rakazo-stack update` fixes it.

Everything setup writes goes in one folder, `~/rakazo-stack`, on every system. On Windows that's inside WSL, not on your C: drive. Your data itself lives in Docker's storage.

## FAQ

**Can I use it now?**
No. We're still planning v1. Follow [Map: rakazo-stack v1 spec](https://github.com/kamui/rakazo-stack/issues/1) to see progress.

**Do I have to pay for anything?**
Only your model provider, plus Docker Desktop if you're at a bigger company. Voice runs on your own computer. Composio's free plan is probably enough for integrations, and integrations are optional.

**Does it change Rakazo?**
No. It runs Rakazo's own published releases, and adds its extra pieces alongside them.

**How do I get new versions?**
Run `rakazo-stack update`. It installs the newest releases on a best-effort basis: nobody tests each new Rakazo, Lemonade or Hindsight release with rakazo-stack before you get it. That's why it takes a backup first and offers to undo a failed update. If a new release breaks something, `--hold` keeps that part where it was.

**Do I need memory?**
No. Leave the model provider key blank in setup and memory stays off. If you turn it off later, your saved memories are kept.

**Does memory send my conversations anywhere else?**
Only to your model provider, the same one chat uses. After each reply, Rakazo sends that turn to Hindsight, and Hindsight asks your model provider to pick out facts worth remembering. With OpenAI or Gemini, memory runs on the admin's key for everyone, so other people's conversations are learned through the admin's account. To keep only facts that bots save on purpose, turn off **Learn from conversations** under **Settings → Memory → Edit**, in each Space.

**How do I turn on memory in another Space?**
Setup turns memory on only in your first Space. Memory is set per Space, and only a Space's owner can connect it, so people you invite turn it on in their own Spaces. In that Space, open **Settings → Memory**, choose **Connect memory provider**, then **Hindsight**, open **Advanced** and choose **Self-hosted**. Enter `http://hindsight:8888`, and paste the key from the `HINDSIGHT_API_TENANT_API_KEY` line in `~/rakazo-stack/secrets.env`. Hindsight can only be reached from inside rakazo-stack, so the key works only by typing it into Rakazo, and Rakazo keeps each Space's memory separate.

**Can memory get things wrong?**
Yes. Three things to know:
- Memory learns from what bots say as well as from what you say, so a bot's guess can be saved as a fact about you.
- A new memory can take up to about 2 minutes before bots can recall it.
- Clearing a conversation also deletes everything memory learned from it. Facts a bot saved on purpose, for example after you said "remember that…", are kept.

Bots can't delete a single memory from Hindsight. To remove a wrong one learned from a conversation, clear that conversation. A bot's own notes are separate: click the bot's name at the top of the chat and open **Memory** to edit or delete them.

**Can I use Honcho for memory instead?**
Not in v1, which uses Hindsight. Honcho is planned as an option for a later version.

**Can I use it on my phone?**
Yes, with Rakazo's phone app through Tailscale. We tested this on an iPhone, and notifications worked too, though a future change to Rakazo's notification service could stop that. Android hasn't been tested yet. Use the `https://….ts.net` address: the iPhone app won't connect to a plain `http://` Tailscale address. The phone app updates itself, so if it asks you to update your server, run `rakazo-stack update`.

**Why does dictation in Chrome, Edge or Safari on my computer go to the internet?**
Those browsers always send your speech to their maker's servers, and Rakazo can't change that. To keep dictation on your own computer, use Firefox, a phone browser, or Rakazo's desktop app.

**Does voice use my graphics card?**
Only transcription, and only on Apple-silicon Macs. Everything else runs on the processor, and graphics-card support on Linux and Windows isn't planned for v1.

**Why can't bot computers reach my computer or my home network?**
Bot computers run code that AI agents write, so on every system v1 keeps them away from your own computer, your home network and your Tailscale devices. On a Mac this also protects the voice engines, which run outside Docker and can't be password-protected. You can't turn this off. They can still reach the internet.

**Does it keep running when I log out?**
Not on Windows or Mac, because Docker Desktop stops when you log out. A version that keeps running is not planned for v1. After you restart your computer, the Instance starts again only once Docker Desktop is running, so turn on **Start Docker Desktop when you sign in to your computer** in Docker Desktop's settings.

**Can I put it on my own domain name?**
Setup won't do that for you. There's an unsupported option to use your own address if you set up the domain and security certificates yourself. The web server that forwards your domain to Rakazo has to run on the same computer.

**Why does setup ask for my Rakazo password?**
Only when something still needs setting up, such as your admin account, voice or memory. It never saves the password.

**Can I move my existing Rakazo install into this?**
Not in v1.
