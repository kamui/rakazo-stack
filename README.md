# rakazo-stack

Run [Rakazo](https://github.com/elie222/rakazo) on your own computer, with voice and memory included, set up by one command.

> **Not ready yet.** We're still deciding what the first version (v1) will do, so there's nothing to install. You can follow along in [Map: rakazo-stack v1 spec](https://github.com/kamui/rakazo-stack/issues/1). This page describes the plan. Anything marked _not decided yet_ is still open.

Words like **Self-hoster** (you, the person running it) and **Instance** (your copy of it) are explained in [GLOSSARY.md](GLOSSARY.md).

## What it is

- **One setup command.** You run `rakazo-stack setup` and give it your keys. It creates all the passwords it needs, writes the settings, and starts everything.
- **Official releases, not a fork.** It installs the newest stable releases of Rakazo, Lemonade and Honcho exactly as their makers publish them, and never changes their code.
- **Updates when you say so.** `rakazo-stack update` moves everything to the newest releases. Nothing updates by itself.
- **For one person on one computer.** It isn't a hosted service and isn't built for sharing between groups of people.
- **Private unless you open it up.** By default only your own computer can reach it. If you want to use it from your phone, setup can add Tailscale.

## What's included

| Part | What it does | When it runs |
|---|---|---|
| [Rakazo](https://github.com/elie222/rakazo) | The app itself, including the separate "bot computers" its AI agents work in | Always |
| [Lemonade](https://github.com/lemonade-sdk/lemonade) | Speech and transcription on your own computer, so your voice doesn't go to an outside service | Always. On a Mac it runs outside Docker, so transcription can use the Mac's graphics chip |
| Lemonade relay | Fixes a small mismatch between Rakazo and Lemonade. It goes away once [Kokoros#144](https://github.com/lucasjinreal/Kokoros/issues/144) is fixed in Lemonade | Always |
| [Honcho](https://github.com/plastic-labs/honcho) | Long-term memory | Only if your model provider can run it: OpenRouter, OpenAI or Gemini |
| [Tailscale](https://tailscale.com) | Lets your phone and other devices reach it securely, at an address like `https://rakazo.<your-tailnet>.ts.net` | Only if you choose it |
| Bot-computer firewall | Stops bot computers from reaching your computer, your home network and your Tailscale devices | Always |

Setup also creates your admin account in Rakazo and connects your voice to Lemonade. If you turned memory on, it connects that too. If any of these steps fails, it tells you how to finish it in Rakazo's settings.

Integrations through [Composio](https://composio.dev) are optional. Setup asks for a Composio key, and you can leave it blank.

## What you need

- **Docker.** Setup checks for it but won't install it for you.
  - **Linux:** Docker Engine and its Compose plugin, installed from Docker's own website, not Docker Desktop. Your user needs to run `docker` without `sudo`. Setup warns you that this gives your user full control of the computer.
  - **Windows and Mac:** Docker Desktop. It's free for personal use and for companies with fewer than 250 employees and under $10M in revenue. Bigger companies need a paid plan.
  - Minimum versions: Docker Engine API 1.45 and Docker Compose 2.24.4.
- **An account with a model provider.** Bring your own: setup asks which one and checks your key.
  - **OpenRouter** (recommended): one key runs chat and memory for everyone on your Instance. Setup picks GLM Flash, which costs about $6 a month for one active user. Turn on **ZDR only** in your OpenRouter privacy settings so providers don't keep your messages.
  - **OpenAI** or **Gemini**: one key runs chat and memory, but only your admin account gets chat. Everyone else connects their own provider in Rakazo's settings. Gemini needs a paid key.
  - **Anthropic**: runs chat for everyone, but memory stays off, because Anthropic has no model for searching memories. To use Claude models with memory, use them through OpenRouter.
  - Leave the key blank and nothing is set up: memory stays off, and each person connects a provider in Rakazo's settings.
- **Optional:** a Composio key, and a Tailscale account.
- **Hardware:** the minimum is _not decided yet_. As a guide:
  - Rakazo's downloads are about 9 GB.
  - The voice models are about 2 GB.
  - Docker Desktop on Windows needs at least 8 GB of RAM.

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
| An install script you run with `curl … \| sh` | Downloads the `rakazo-stack` program for Linux (including WSL 2) or an Apple-silicon Mac, and checks the download hasn't been tampered with. The address is _not decided yet_. |
| `rakazo-stack setup` | Asks you questions, then sets everything up and starts it. You can run it again any time: it remembers your answers, never shows your saved keys, never changes the passwords it created, and never changes versions. |
| `rakazo-stack setup`, with a no-questions option | Does the same without asking, for scripts. The option's name is _not decided yet_. |
| `docker compose up -d` | Starts it. Run it from `~/rakazo-stack`. |
| `docker compose down` | Stops it. Your data is kept. |
| `docker compose ps` and `docker compose logs` | Shows what's running, and the logs. |
| `rakazo-stack backup` | Saves your data, settings and keys into one file locked with a passphrase you choose. Rakazo pauses while it runs. |
| `rakazo-stack restore <file>` | Brings an Instance back from a backup file, on the same computer or a new one, on any supported system. |
| `rakazo-stack update` | Updates the `rakazo-stack` program, then Rakazo, Lemonade and Honcho, to their newest releases. It shows what will change and asks first, takes a backup, and offers to undo the update if it fails. `setup` and `backup` remind you, at most once a day, when there's something new. |
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
Run `rakazo-stack update`. It installs the newest releases on a best-effort basis: nobody tests each new Rakazo, Lemonade or Honcho release with rakazo-stack before you get it. That's why it takes a backup first and offers to undo a failed update. If a new release breaks something, `--hold` keeps that part where it was.

**Do I need memory?**
No. Leave the model provider key blank in setup, or use Anthropic, and memory stays off. If you turn it off later, your saved memories are kept.

**Can I use it on my phone?**
Yes, with Rakazo's phone app through Tailscale. We tested this on an iPhone, and notifications worked too, though a future change to Rakazo's notification service could stop that. Android hasn't been tested yet. Use the `https://….ts.net` address: the iPhone app won't connect to a plain `http://` Tailscale address. The phone app updates itself, so if it asks you to update your server, run `rakazo-stack update`.

**Why does dictation in Chrome, Edge or Safari on my computer go to the internet?**
Those browsers always send your speech to their maker's servers, and Rakazo can't change that. To keep dictation on your own computer, use Firefox, a phone browser, or Rakazo's desktop app.

**Does voice use my graphics card?**
Only transcription, and only on Apple-silicon Macs. Everything else runs on the processor, and graphics-card support on Linux and Windows isn't planned for v1.

**Why can't bot computers reach my computer or my home network?**
Bot computers run code that AI agents write, so on every system v1 keeps them away from your own computer, your home network and your Tailscale devices. On a Mac this also protects the voice engines, which run outside Docker and can't be password-protected. You can't turn this off. They can still reach the internet.

**Does it keep running when I log out?**
Not on Windows or Mac, because Docker Desktop stops when you log out. A version that keeps running is not planned for v1.

**Can I put it on my own domain name?**
Setup won't do that for you. There's an unsupported option to use your own address if you set up the domain and security certificates yourself.

**Why does setup ask for my Rakazo password?**
Only when something still needs setting up, such as your admin account, voice or memory. It never saves the password.

**Can I move my existing Rakazo install into this?**
Not in v1.
