# rakazo-stack

Run [Rakazo](https://github.com/elie222/rakazo) on your own computer, with voice and memory included, set up by one command.

Words like **Self-hoster** (you, the person running it) and **Instance** (your copy of it) are explained in [GLOSSARY.md](GLOSSARY.md).

## What it is

- **One setup command.** You run `rakazo-stack setup` and give it your keys. It creates all the passwords it needs, writes the settings, and starts everything.
- **Official releases, not a fork.** It installs the newest stable releases of Rakazo, Lemonade, Hindsight and Tailscale exactly as their makers publish them, and never changes their code.
- **Updates when you say so.** `rakazo-stack update` moves everything to the newest releases, behind a Backup. Nothing updates by itself.
- **For one person on one computer.** It isn't a hosted product and isn't built for sharing between groups of people.
- **Private unless you open it up.** By default only your own computer can reach it. If you want to use it from your phone, setup can add Tailscale.

## What's included

| Part | What it does | When it runs |
|---|---|---|
| [Rakazo](https://github.com/elie222/rakazo) | The app itself, including the separate "bot computers" its AI agents work in | Always |
| [Lemonade](https://github.com/lemonade-sdk/lemonade) | Speech and transcription on your own computer, so your voice doesn't go to an outside company | Always. On a Mac it runs outside Docker, so transcription can use the Mac's graphics chip |
| Lemonade relay | Converts recordings from your browser or phone into a format Lemonade can transcribe, and fixes a small mismatch between Rakazo and Lemonade's speech | Always |
| [Hindsight](https://github.com/vectorize-io/hindsight) | Long-term memory | Only if you give setup a Model provider key |
| [Tailscale](https://tailscale.com) | Lets your phone and other devices reach it securely, at an address like `https://rakazo.<your-tailnet>.ts.net` | Only if you choose it |
| Bot-computer firewall | Stops bot computers from reaching your computer, your home network and your Tailscale devices | Always |

Setup also creates your Owner account in Rakazo and connects your voice to Lemonade. With an OpenAI or Gemini key it connects chat for that account, and if memory is on, it connects memory in your first Space. If any of these steps fails, the Checklist at the end of setup tells you how to finish it in Rakazo's settings.

Integrations through [Composio](https://composio.dev) are optional. Setup asks for a Composio project API key, and you can leave it blank.

## What you need

- **Docker.** Setup checks for it but won't install it for you.
  - **Linux:** Docker Engine and its Compose plugin, from Docker's own repository (`get.docker.com` is a shortcut to it), not Docker Desktop. Your user needs to run `docker` without `sudo`, through the `docker` group. Setup warns you that this gives your user full control of the computer.
  - **Windows and Mac:** Docker Desktop.
  - Minimum versions: Docker Engine API 1.45 (Engine 26 or later) and Docker Compose 2.24.4.
- **Docker Desktop's licence.** Docker Desktop is free for personal use, education, non-commercial open source, and businesses with fewer than 250 employees and under $10M in revenue. Other businesses, and every government body, need a paid subscription. Docker Engine on Linux is free for everyone.
- **A Model provider account.** Bring your own: setup asks which one and checks your key.
  - **OpenRouter** (recommended): one key runs chat and memory for everyone on your Instance. Setup picks GLM Flash (`~z-ai/glm-flash-latest`). In our test it cost about $3.70–5 a month for one active user sending 30 messages a day, chat and memory together. Turn on **ZDR only** in your OpenRouter privacy settings so providers don't keep your messages; setup checks it for you.
  - **OpenAI** or **Gemini**: one key runs chat and memory, but only your Owner account gets chat. Everyone else connects their own provider in Rakazo's settings. Gemini needs a key with billing turned on.
  - **Anthropic**: like OpenRouter, one key runs chat and memory for everyone on your Instance.
  - Leave the key blank and nothing is set up: memory stays off, and each person connects a provider in Rakazo's settings.
- **Optional:** a Composio project API key, and a Tailscale account with an auth key that isn't ephemeral.
- **Hardware:** at least 4 processor threads, 16 GB of RAM and 35 GB of free disk space where Docker keeps its data. 8 threads and 50 GB of free disk space are recommended. If your computer has less, setup warns you and carries on.
  - A new Instance with memory on uses about 16 GB of disk, 6.4 GB of it for memory. An Update briefly needs more, up to about 27.5 GB, because it keeps the old versions until the new ones work.
  - The whole Instance uses about 5–6 GB of RAM, and 7–8 GB while a bot computer is busy.

## Where it runs

| System | Supported? | Notes |
|---|---|---|
| Linux on a standard 64-bit PC (x86-64), with glibc | Yes | Voice runs on the processor. |
| Windows 11 (23H2 or later) or Windows 10 (22H2) | Yes | Runs inside WSL 2 (version 2.1.5 or later) with Docker Desktop. Turning on WSL 2 needs Windows administrator rights once. Voice runs on the processor. **Windows Home** works, but Docker no longer lists it as supported, so setup warns you and carries on. |
| Mac with Apple silicon (M1 or later), macOS 13 or later | Yes | Transcription uses the Mac's graphics chip; speech runs on the processor. |
| Mac with an Intel chip | No | |
| Linux on ARM, or Alpine and other musl Linux | No | Lemonade only ships for standard 64-bit PCs, and the program is built for glibc. |
| Docker Desktop for Linux | No | Use Docker Engine on Linux. |
| Podman, OrbStack, Colima, or Docker installed directly inside WSL 2 | No | v1 supports only the Docker setups above. Podman also can't run Rakazo's bot computers: they need a newer Docker API version than it offers. |

On Windows and Mac, it only runs while you're signed in to your computer.

## Install

```sh
curl -fsSL https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh | sh
```

This downloads the `rakazo-stack` program for Linux (including WSL 2) or an Apple-silicon Mac to `~/.local/bin/rakazo-stack`, checks the download hasn't been tampered with, and starts `rakazo-stack setup`. On Windows, run it in your WSL terminal.

To read the script before you run it:

```sh
curl -fsSLO https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh
less install.sh
sh install.sh
```

[The guide](docs/guide.md#install) covers installing an exact version and what the script checks.

## Commands

| Command | What it does |
|---|---|
| `rakazo-stack setup` | Asks you questions, then sets everything up and starts it. Run it again any time to change your answers: it remembers them, never shows your saved keys, never changes the passwords it created, and never changes versions. It ends with a Checklist of anything left for you to do in Rakazo or elsewhere. |
| `rakazo-stack setup --no-questions` | The same without asking, for scripts. Each answer comes from an environment variable, such as `RAKAZO_STACK_OWNER_EMAIL`, or else from your saved answers. |
| `rakazo-stack backup` | Saves your data, settings and keys into one file locked with a passphrase you choose, in `~/rakazo-stack-backups`, or the folder you give with `--output`. Rakazo pauses for about half a minute while it runs. If you lose the passphrase, nobody can open the Backup. |
| `rakazo-stack restore <file>` | Brings an Instance back from a Backup file, on the same computer or a new one, on any supported system. |
| `rakazo-stack update` | Updates the `rakazo-stack` program, then Rakazo, Lemonade, Hindsight and Tailscale, to their newest releases. It shows what will change and asks first, takes a Backup, and offers to roll back if the Update fails. `setup` and `backup` remind you, at most once a day, when there's something new. |
| `rakazo-stack update --hold rakazo=v0.16.0` | Keeps a Component at a version you choose, for when a new release causes trouble. `--unhold rakazo` lets it update again. |

[The guide](docs/guide.md) explains each command in full. Each command also has `--help`.

## Day to day

There are no start or stop commands of its own. Use Docker Compose from `~/rakazo-stack`:

| Task | Linux and Mac | Windows (in WSL) |
|---|---|---|
| Start | `docker compose up -d` | `docker.exe compose up -d` |
| Stop (your data is kept) | `docker compose down` | `docker.exe compose down` |
| What's running | `docker compose ps` | `docker.exe compose ps` |
| Logs | `docker compose logs` | `docker.exe compose logs` |

On Windows, always type `docker.exe` instead of `docker`. Plain `docker` in WSL can leave parts of the Instance unable to start after you restart your computer. If that happens, the next `rakazo-stack setup` or `rakazo-stack update` fixes it.

On a Mac, voice runs outside Docker, so `docker compose down` doesn't stop it.

Everything setup writes goes in one folder, `~/rakazo-stack`, on every system. On Windows that's inside WSL, not on your C: drive. Your data itself lives in Docker's storage. Setup rewrites most of the files in the folder on every run, so don't edit them: [docs/files.md](docs/files.md) says which ones you may change.

## More

- [docs/guide.md](docs/guide.md): each command in detail, Tailscale, cron Backups, moving to a new computer, and removing an Instance.
- [docs/limitations.md](docs/limitations.md): what v1 can't do, and the security trade-offs.
- [docs/files.md](docs/files.md): the files in `~/rakazo-stack`, every field of `settings.json`, and every environment variable.
- [docs/spec.md](docs/spec.md): the full build spec.

## FAQ

**Do I have to pay for anything?**
Only your Model provider, plus Docker Desktop if you're at a bigger company. Voice runs on your own computer. Composio's free plan is probably enough for integrations, and integrations are optional; [docs/limitations.md](docs/limitations.md#integrations) says where its limit bites.

**Does it change Rakazo?**
No. It runs Rakazo's own published releases, and adds its extra pieces alongside them.

**How do I get new versions?**
Run `rakazo-stack update`. It installs the newest releases on a best-effort basis: nobody tests each new Rakazo, Lemonade, Hindsight or Tailscale release with rakazo-stack before you get it. That's why it takes a Backup first and offers to roll back a failed Update. If a new release breaks something, `--hold` keeps that Component where it was.

**Do I need memory?**
No. Leave the Model provider key blank in setup, or answer `off` to the memory question, and memory stays off. If you turn it off later, your saved memories are kept, and setup's Checklist names any Space still connected to memory, for its owner to disconnect.

**Does memory send my conversations anywhere else?**
Only to your Model provider, the same one chat uses, so each conversation goes to it a second time. After each reply, Rakazo sends that turn to Hindsight, and Hindsight asks your Model provider to pick out facts worth remembering. With OpenAI or Gemini, memory runs on the Owner's key for everyone, so other people's conversations are learned through the Owner's account. To keep only facts that bots save on purpose, turn off **Learn from conversations** under **Settings → Memory → Edit**, in each Space.

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
Yes, with Rakazo's phone app through Tailscale. In the app, choose **Use a custom server** when you sign in, and use the `https://….ts.net` address: the iPhone app won't connect to a plain `http://` Tailscale address. Sign in with your password; the app hides passkeys for self-hosted Rakazo. We tested this on an iPhone, and notifications worked too, but that's best effort: the way Rakazo sends notifications could change in a way nobody running their own Instance can fix. Android hasn't been tested. The phone app updates itself, so if it asks for a newer version of Rakazo, run `rakazo-stack update`.

**Why does dictation in Chrome, Edge or Safari on my computer go to the internet?**
Those browsers always send your speech to their maker's cloud, and Rakazo can't change that. To keep dictation on your own computer, use Firefox, a phone browser, or Rakazo's desktop app. During calls, Rakazo's phone app listens with the phone's own speech recognition.

**Does voice use my graphics card?**
Only transcription, and only on Apple-silicon Macs. Everything else runs on the processor, and graphics-card support on Linux and Windows isn't planned for v1.

**Why can't bot computers reach my computer or my home network?**
Bot computers run code that AI agents write, so on every system v1 keeps them away from your own computer, your home network and your Tailscale devices. On a Mac this also protects the voice engines, which run outside Docker and can't be password-protected. You can't turn this off. They can still reach the internet. [docs/limitations.md](docs/limitations.md#bot-computers) lists the gaps.

**Does it keep running when I sign out?**
Not on Windows or Mac, because Docker Desktop stops when you sign out. A version that keeps running is not planned for v1. After you restart your computer, the Instance starts again only once Docker Desktop is running, so turn on **Start Docker Desktop when you sign in to your computer** in Docker Desktop's settings. If it's off, the Instance stays down until you open Docker Desktop.

**Can I give it my own web address?**
Setup won't set one up for you. There's an unsupported option to use your own Instance URL if you set up the address, the security certificate and the proxy that forwards it to Rakazo yourself. That proxy has to run on the same computer. [The guide](docs/guide.md#your-own-instance-url-unsupported) has the details.

**Why does setup ask for my Rakazo password?**
Only when something still needs setting up in Rakazo, such as your Owner account, voice or memory. It never saves the password.

**Can I move my existing Rakazo into this?**
Not in v1. rakazo-stack starts a new Rakazo of its own.

**How do I remove it?**
There's no uninstall command. [The guide](docs/guide.md#removing-an-instance) lists the steps.
