# Known limitations

What v1 doesn't do, and the trade-offs it makes on purpose. Terms are defined in [GLOSSARY.md](../GLOSSARY.md).

## Running

- **Windows and Mac: only while you're signed in.** Docker Desktop stops when you sign out, so the Instance does too, and after a restart it starts only once Docker Desktop is running. Turn on **Start Docker Desktop when you sign in to your computer** in Docker Desktop's settings; setup doesn't check it. On Linux, Docker Engine starts at boot, so the Instance runs without anyone signed in.
- **Windows: type `docker.exe`.** Run `docker.exe compose …` instead of `docker compose …`, from `~/rakazo-stack` in WSL. Plain `docker` in WSL makes containers that can fail to start after a restart; the next `rakazo-stack setup` or `rakazo-stack update` repairs them.
- **Docker Desktop's licence.** It's free for personal use, education, non-commercial open source, and businesses with fewer than 250 employees and under $10M in revenue. Other businesses, and every government body, need a paid subscription. Docker Engine on Linux is free for everyone.
- **One Instance per computer**, for one Self-hoster. No hosted or multi-group mode.
- **`rakazo-stack uninstall` leaves the Tailscale device on your tailnet.** Remove it in the Tailscale admin console; the command's last line links its page. See [the guide](guide.md#removing-an-instance).
- **No start, stop, status or logs commands of its own:** use `docker compose`, as in [the guide](guide.md#running-it-day-to-day).
- **Backups only when you run them.** There's no built-in schedule; [the guide](guide.md#backups-on-a-schedule) shows a cron line for Linux.
- **A lost Backup passphrase makes the Backup unusable.** Nobody can recover it.
- **No moving an existing Rakazo in.** rakazo-stack starts a new Rakazo of its own.

## Security

- **The `docker` group is as powerful as `root`.** On Linux, setup needs your user to run `docker` without `sudo`, through the `docker` group, which lets it control the whole computer. Anyone in that group, and your own account, can read every secret in `~/rakazo-stack/secrets.env` and `.env`: they're protected only by file permissions, with no keychain.
- **Anyone on your tailnet can reach the sign-in page** when Tailscale is on. Rakazo's sign-in decides who gets in, and sign-ups are off. Tailscale ACLs that let only your own devices reach the device are optional hardening; see [the guide](guide.md#tailscale).
- **With Tailscale on, passkeys work only at the `ts.net` address.** Password sign-in works at both addresses.

## Bot computers

Bot computers run code that AI agents write. On every system, a firewall keeps them off this computer, your home network and your tailnet, and away from voice, memory and the databases. They can still reach the internet. You can't turn the firewall off. Its gaps:

- **They can't reach anything on this computer, your home network or your tailnet**, even when you'd want them to. On Windows they also lose Docker Desktop's web proxy, which may cut them off the internet on a PC that uses a proxy (untested).
- **Only private addresses are blocked.** An agent given Tailscale, VPN or tunnel credentials can connect through them and get past the firewall, so never give an agent those unless that's what you want.
- **One unsupported way to let an agent reach one device at home or on your tailnet on purpose**, untested: create a Tailscale auth key with a tag of its own, write Tailscale ACLs that let that tag reach only that one device and port, and give the agent that key, not your own. Everything else on your tailnet then stays out of its reach, as far as your ACLs are right. rakazo-stack doesn't help with any of this.
- **The firewall check is cached.** Rakazo checks the rules once each time its supervisor starts. If something clears the computer's firewall without restarting Docker, new bot computers can get out until the firewall puts its rules back, within 60 seconds.
- **Cloud addresses in public ranges stay reachable**, such as Azure's `168.63.129.16`. This matters only on a cloud virtual machine.
- **On Docker Desktop it relies on Docker's internals.** If a Docker Desktop update removes what the rules attach to, bot computers are refused rather than started without the firewall.
- **Docker's `nftables` firewall backend isn't supported.** With it, bot computers can't start; setup warns you and says how to switch back.
- **Optional requests to the upstream projects** are drafted on [#40](https://github.com/kamui/rakazo-stack/issues/40) (Rakazo checking the firewall before each bot computer starts) and [#26](https://github.com/kamui/rakazo-stack/issues/26) (Lemonade protecting its voice engines with its key), for you to file if you want them.

## Voice

- **Desktop Chrome, Edge and Safari dictate through their maker's cloud.** They always send dictated speech to the browser maker, and Rakazo has no setting to change that. Firefox, phone browsers and Rakazo's desktop app send your voice to the Instance, where Lemonade transcribes it. A request to Rakazo for a "Prefer server transcription" setting is drafted on [#15](https://github.com/kamui/rakazo-stack/issues/15), for you to file if you want it.
- **Rakazo's phone app uses the phone's own speech recognition during calls.** Replies use the Instance's voice unless you pick the phone's.
- **Graphics cards:** only transcription on Apple-silicon Macs uses one. Everything else runs on the processor, and graphics-card support on Linux and Windows isn't planned for v1.
- **After an Update, voice comes back last**, because Lemonade downloads its engines again.

## Phones

- **iPhone notifications are best effort.** They work today without any setup, because of how Rakazo sends notifications. Rakazo could change that at any time, and nobody running their own Instance could fix it.
- **Android is untested**: signing in at a custom address, plain `http://` addresses, notifications and live status.
- **Use the `https://….ts.net` address.** The iPhone app won't connect to a plain `http://` Tailscale address.
- **Passkeys are hidden** in the phone apps for self-hosted Rakazo; sign in with your password.
- **The apps update themselves** and lose features one by one against an older Rakazo, without warning. When the app asks, run `rakazo-stack update`.

## Memory

- **Memory learns from what bots say** as well as from what people say, so a bot's guess can be saved as a fact about you.
- **A new memory can take up to about 2 minutes** before bots can recall it.
- **Clearing a conversation deletes everything memory learned from it.** Facts a bot saved on purpose are kept.
- **Each conversation goes to your Model provider a second time**, for memory to pick out facts.
- **With OpenAI or Gemini, everyone's conversations are learned through the Owner's key**, because memory runs on that one key for the whole Instance.
- **Setup connects memory only in your first Space.** Other Spaces are connected by their owners; [the README](../README.md#faq) says how.
- **Honcho isn't an option in v1.**

## Models and Updates

- **`~z-ai/glm-flash-latest` can change under you.** It's an OpenRouter alias that moves when Z.ai releases a new Flash model, not when you update. Set a fixed model to stop that; see [the guide](guide.md#choosing-other-models).
- **Updates are best effort.** rakazo-stack installs each Component's newest release, and nobody tests each upstream release with it before you get it. That's why `update` takes a Backup first and offers to roll back a failed Update, and why Holds exist.
- **A Component can't be moved back to an older version** by `update`: its data has already moved forward. Restoring an older Backup is the way back.

## Integrations

- **Composio's free Hobby plan caps tool calls.** It needs no card and pauses rather than bills, and gives 20,000 tool calls to Composio-managed apps a month. A household's chat uses about 500–3,000 a month, but a routine that watches something checks it every 5 minutes or more, and two watches every 5 minutes use nearly the whole cap on their own.
- **Only apps Composio signs you in to work.** Spotify, X (Twitter), Shopify and other apps that need your own sign-in setup can't be connected through Composio.
- **Restoring a Backup keeps its Composio key.** Using a different key afterwards loses everyone's existing app connections.

## Access

- **No plain home-network mode.** Browsers block the microphone on plain `http://` addresses other than localhost. Use Tailscale to reach the Instance from other devices.
- **Your own Instance URL is unsupported.** Setup won't set up an address, certificates or Tailscale Funnel for you. With `bring-your-own`, your proxy has to run on the same computer, because Rakazo stays on `127.0.0.1:5173`.
- **Changing the Instance URL signs everyone out**, breaks passkeys made on the old address, makes phones sign in again and leaves links in old emails pointing at the old address. MCP servers connected with OAuth may need adding again (unverified).
