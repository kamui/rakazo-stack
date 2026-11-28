# rakazo-stack guide

How to install, run, back up, move, update and remove an Instance. The [README](../README.md) is the short version. Terms such as Self-hoster, Instance, Component, Backup, Hold and Checklist are defined in [GLOSSARY.md](../GLOSSARY.md).

On Windows, run every command here in your WSL terminal, and type `docker.exe` wherever this guide types `docker`.

- [Install](#install)
- [Setup](#setup)
- [Access: this computer, Tailscale, or your own Instance URL](#access-this-computer-tailscale-or-your-own-instance-url)
- [Running it day to day](#running-it-day-to-day)
- [Backups](#backups)
- [Restore](#restore)
- [Updates](#updates)
- [Removing an Instance](#removing-an-instance)

## Install

```sh
curl -fsSL https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh | sh
```

`install.sh`:

- works on x86-64 Linux with glibc (WSL 2 included) and Apple-silicon Macs, and stops on anything else, including Alpine and other musl Linux;
- downloads the program for your system and checks it against the SHA-256 digests written into that release's `install.sh`;
- if the GitHub CLI `gh` is installed and signed in, checks where the program was built with `gh attestation verify`, and installs nothing if that fails. Without `gh`, it prints the command to check it yourself:

  ```sh
  gh attestation verify ~/.local/bin/rakazo-stack --repo kamui/rakazo-stack
  ```

- puts the program at `~/.local/bin/rakazo-stack`, without `sudo`. If `~/.local/bin` isn't on your `PATH`, it prints the line to add to your shell profile; it never edits the profile itself;
- starts `rakazo-stack setup` when you run it from a terminal.

To read the script before you run it, download it, read it, then run it:

```sh
curl -fsSLO https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh
less install.sh
sh install.sh
```

To install without starting setup:

```sh
curl -fsSL https://github.com/kamui/rakazo-stack/releases/latest/download/install.sh | sh -s -- --no-setup
```

To install an exact version, which is also how to go back to an older program, put the version in the address:

```sh
curl -fsSL https://github.com/kamui/rakazo-stack/releases/download/v0.1.0/install.sh | sh
```

Installing a newer program changes nothing in a running Instance until you run `setup` or `update`.

## Setup

```sh
rakazo-stack setup
```

Setup checks your computer first. A problem it can't work around stops it before anything changes, with what to do; a smaller one prints a warning and carries on. Then it asks:

| Question | Notes |
|---|---|
| Owner email and password | Your Owner account in Rakazo. The password is 16–128 characters, typed twice; if the two don't match, setup asks again, up to three times. Setup never saves it. |
| Model provider | `openrouter` (the default), `openai`, `gemini`, `anthropic` or `none`. |
| The Model provider's API key | Blank is allowed. Setup checks the key with the provider before saving it, and never saves a key the provider rejects. |
| Composio project API key | Blank is allowed, and leaves integrations off. Checked with Composio the same way. |
| Access | `localhost` (the default), `tailscale`, or `bring-your-own` (advanced, unsupported). See [Access](#access-this-computer-tailscale-or-your-own-instance-url). |
| Memory | Asked only when you give a key. On by default; `off` stops Hindsight and keeps its data. |

Keys and passwords are typed without being shown. Setup then starts everything, sets up your Owner account, voice and memory in Rakazo, and ends with a **Checklist**: anything left for you to do, such as a setting to turn on at your Model provider. The Checklist is printed again at the end of every run.

### Running setup again

Run `rakazo-stack setup` again whenever you want to change an answer. It shows your earlier answers as defaults, and a saved key as "set, press Enter to keep, or paste a new one". It never prints a saved key, never changes the passwords it created, and never changes Component versions: only `rakazo-stack update` does that. When something in Rakazo still has to be set up, such as memory you turned on later, it asks once for the Owner password you already sign in with. If you mistype it, setup says so and asks again, up to three times. A Component you switch on later, such as memory or Tailscale, starts at its newest release.

If you removed something in Rakazo that setup had set up, such as your voice settings, setup asks whether to add it back.

To turn integrations off, delete the `COMPOSIO_API_KEY` line from `~/rakazo-stack/secrets.env` and run setup again. A blank answer keeps a saved key.

### Setup without questions

For scripts, `--no-questions` takes each answer from an environment variable, then from your saved answers, then from the default:

```sh
RAKAZO_STACK_OWNER_EMAIL=owner@example.com \
RAKAZO_STACK_OWNER_PASSWORD='a-long-owner-password' \
RAKAZO_STACK_MODEL_PROVIDER=none \
rakazo-stack setup --no-questions
```

Each key comes from `RAKAZO_STACK_` followed by its name in `secrets.env`, for example `RAKAZO_STACK_OPENROUTER_API_KEY`. A first run needs at least the Owner email and password. A later run needs the password only when something in Rakazo still has to be set up; without it, the Checklist says to run again with it set. [docs/files.md](files.md#environment-variables) lists every variable.

### Choosing other models

Setup picks a model for chat and one for memory. To choose your own, add `rakazo_model` or `memory_model` to `~/rakazo-stack/settings.json` and run setup again; [docs/files.md](files.md#settingsjson) has the details. Choosing another Model provider removes both, because they name models at the old one.

OpenRouter's default, `~z-ai/glm-flash-latest`, is an alias that OpenRouter moves when Z.ai releases a new Flash model, so your model can change between Updates. Set `rakazo_model` and `memory_model` to a fixed model, such as `z-ai/glm-5.3-flash`, to stop that.

### Memory in other Spaces

Setup connects memory only in your first Space. To connect another Space, its owner opens **Settings → Memory → Connect memory provider → Hindsight → Advanced → Self-hosted** in that Space, enters `http://hindsight:8888`, and pastes the key from the `HINDSIGHT_API_TENANT_API_KEY` line in `~/rakazo-stack/secrets.env`.

If you turn memory off, setup stops Hindsight and keeps its data. It doesn't disconnect memory in Rakazo: its Checklist names each Space still connected, because until its owner disconnects it under **Settings → Memory**, each reply first tries a recall that fails.

### Update reminders and GitHub's limit

At the end of a run, `setup` and `backup` check GitHub at most once a day, and print "Updates available: run `rakazo-stack update`" when there's something new. Set `RAKAZO_STACK_NO_UPDATE_CHECK=1` to turn that off.

GitHub allows 60 requests an hour from one address without a token. If you hit that limit, set `GITHUB_TOKEN` or `GH_TOKEN` to a GitHub token; the program sends it only to `api.github.com` and never prints it.

## Access: this computer, Tailscale, or your own Instance URL

The Instance URL is the address people sign in at. Setup offers three:

| Access | Instance URL | Who can reach it |
|---|---|---|
| `localhost` (the default) | `http://127.0.0.1:5173` | Only this computer. The microphone works, because browsers trust localhost. |
| `tailscale` | `https://<device>.<your-tailnet>.ts.net` | Your devices on your tailnet, including phones. |
| `bring-your-own` | the `https://` address you give | Whoever your own proxy lets in. Advanced and unsupported. |

Rakazo's own sign-in decides who can use it: sign-ups are off, so only accounts you create can sign in.

Changing the Instance URL later signs everyone out, breaks passkeys made on the old address, makes phones sign in again and leaves links in old emails pointing at the old address. Setup warns you and asks first; no data is lost.

### Tailscale

Setup runs its own Tailscale, which joins your tailnet as a device of its own. It doesn't use a Tailscale already installed on your computer.

1. In the Tailscale admin console, create an auth key that is **not ephemeral**: Tailscale deletes ephemeral devices after they go offline. Setup refuses OAuth client secrets and keys it can tell are ephemeral, but it can't tell from every ephemeral key, so check this yourself. Don't add tags.
2. Run `rakazo-stack setup`, choose `tailscale`, paste the key and pick a device name (the default is `rakazo`).
3. Do what the Checklist says, once: in the admin console, enable MagicDNS and HTTPS certificates, and turn off key expiry for the device: on the device's page (the Checklist links it), click **Machine settings** and choose **Disable key expiry**. That's the device's own key, not the auth key from step 1, whose expiration only limits how long it can add devices. Device names then appear in public Certificate Transparency logs. If you enable them after setup, run `docker compose restart tailscale` from `~/rakazo-stack`. Each later run lists only the steps still left, and drops the item when they're all done.

Once the device has joined, setup deletes the auth key: the device keeps its identity, so later runs need no key. If the name is taken, Tailscale picks another, such as `rakazo-1`, and setup uses it.

Anyone on your tailnet can open the Instance URL and see Rakazo's sign-in page. To narrow that down, you can add [Tailscale ACLs](https://tailscale.com/kb/1018/acls) that let only your own devices or users reach the device. That's optional hardening; setup doesn't do it.

With Tailscale on, password sign-in works at both `http://127.0.0.1:5173` and the `ts.net` address, but passkeys work only at the `ts.net` address.

### Phone apps

Rakazo's phone apps can use your Instance through Tailscale:

1. Install Tailscale on the phone and sign in to the same tailnet.
2. In Rakazo's app, choose **Use a custom server** when you sign in, and enter the `https://….ts.net` address. The iPhone app won't connect to a plain `http://` Tailscale address.
3. Sign in with your password. The app hides passkeys for self-hosted Rakazo.

Notifications work on an iPhone today, on a best-effort basis: they depend on how Rakazo sends notifications, which could change in a way you can't fix. Android hasn't been tested. The apps update themselves; if one asks for a newer Rakazo, run `rakazo-stack update`.

### Your own Instance URL (unsupported)

`bring-your-own` is for people who run their own proxy and certificates. You give an `https://` address, with an optional port and no path. Rakazo stays on `127.0.0.1:5173`, so your proxy has to run on the same computer and forward to it; setup won't open Rakazo to the network. The proxy, certificates and firewall are yours to set up. It can't be combined with Tailscale.

## Running it day to day

From `~/rakazo-stack`:

| Task | Linux and Mac | Windows (in WSL) |
|---|---|---|
| Start | `docker compose up -d` | `docker.exe compose up -d` |
| Stop (your data is kept) | `docker compose down` | `docker.exe compose down` |
| What's running | `docker compose ps` | `docker.exe compose ps` |
| Logs | `docker compose logs` | `docker.exe compose logs` |

These work because setup writes the Compose settings to `~/rakazo-stack/.env`. Compose takes a variable from your shell over the same name in `.env`, so if your shell exports one of its names, such as `COMPOSE_FILE` or `COMPOSE_PROJECT_NAME`, your commands get the shell's value; setup's own commands never do.

- **Windows:** always type `docker.exe`. Plain `docker` in WSL makes containers that can fail to start after a restart of your computer; the next `rakazo-stack setup` or `rakazo-stack update` fixes them.
- **Mac:** voice runs outside Docker, as a background item of your account, so `docker compose down` doesn't stop it. The first time setup installs it, macOS 13 and later shows a "Background Items Added" notice. It's listed as `lemond` in **System Settings → General → Login Items & Extensions**, under **Background App Activity** (**Allow in the Background** on older macOS), not under Open at Login. If you turn it off there, voice stops working, and setup's Checklist says so.
- **Windows and Mac:** the Instance runs only while you're signed in and Docker Desktop is running. Turn on **Start Docker Desktop when you sign in to your computer** in Docker Desktop's settings, or the Instance stays down after a restart until you open Docker Desktop.

The bot-computer firewall can show the rules it keeps:

```sh
docker compose run --rm computer-egress --print
```

## Backups

```sh
rakazo-stack backup
```

A Backup is one encrypted file holding everything needed to bring the Instance back: your settings and keys, Rakazo's database, memory's database, bot computer homes, files people uploaded, and Tailscale's device identity. It leaves out what setup downloads again, such as Docker images and voice models.

- It asks for a passphrase, twice. For scripts, put it in `RAKAZO_STACK_BACKUP_PASSPHRASE` instead. **If you lose the passphrase, nobody can open the Backup.** There's no unencrypted option, because bot computer homes hold signed-in browser profiles and tokens.
- Rakazo stops while the Backup is taken, for about half a minute, and starts again on its own, even if the Backup fails or you press Ctrl-C.
- The file is `~/rakazo-stack-backups/rakazo-stack-backup-<UTC time>.tar.age`, for example `rakazo-stack-backup-20261127T120101Z.tar.age`. To write it to another folder, such as one on a backup drive, add `--output` and the folder:

  ```sh
  rakazo-stack backup --output ~/Backups/rakazo
  ```

- Before it finishes, the Backup reads the file back and checks every part. A Backup that fails the check is deleted.
- The program never deletes old Backups. Delete the ones you don't need yourself.

### Backups on a schedule

rakazo-stack takes a Backup only when you run it. On Linux, cron can run it for you. Put the passphrase in a file only you can read:

```sh
install -m 600 /dev/null ~/.rakazo-stack-backup-passphrase
```

Open `~/.rakazo-stack-backup-passphrase` in a text editor and type the passphrase on its one line. Then run `crontab -e` and add a line like this one, which takes a Backup every night at 3:00:

```
0 3 * * * RAKAZO_STACK_BACKUP_PASSPHRASE="$(cat "$HOME/.rakazo-stack-backup-passphrase")" "$HOME/.local/bin/rakazo-stack" backup >>"$HOME/rakazo-stack-backup.log" 2>&1
```

Each run adds its output to `~/rakazo-stack-backup.log`. Anyone who can read the passphrase file can open your Backups, so keep it at mode 600.

### Opening a Backup by hand

You don't need rakazo-stack to read a Backup. With the [`age`](https://github.com/FiloSottile/age) tool, which asks for the passphrase:

```sh
age --decrypt -o backup.tar rakazo-stack-backup-20261127T120101Z.tar.age
tar -xf backup.tar
```

Inside are `manifest.json`, `settings.json`, `secrets.env`, `rakazo.dump` (a `pg_dump -Fc` of Rakazo's database), `appdata.tar`, and, when you have them, `hindsight.dump` and `tailscale-state.tar`. `secrets.env` holds every password and key in plain text, so delete the unpacked files when you're done.

## Restore

```sh
rakazo-stack restore ~/rakazo-stack-backups/rakazo-stack-backup-20261127T120101Z.tar.age
```

Restore brings an Instance back from a Backup on a computer with no Instance yet: the same one, or a new one, on any supported system. To move to a new computer, install rakazo-stack there, copy the Backup file over and run `restore`.

- It asks for the passphrase, or reads `RAKAZO_STACK_BACKUP_PASSPHRASE`.
- It brings back the exact versions the Backup recorded, so a Backup taken before an Update is a real way back. Only if one of them is too old for this rakazo-stack does it move that Component to its newest release, and it says so. A Backup made by a newer rakazo-stack is refused: update the program first.
- Your passwords, keys and Instance URL come back as they were, so your Composio connections keep working. Moving between Linux, Windows and Mac sets voice up afresh for the new system.
- It then runs setup without questions and ends with the usual Checklist. It never needs your Owner password.

**Tailscale.** If the Backup holds a Tailscale device, two copies of it can't be online at once. Restore asks "Is the machine this Backup came from switched off for good? [y/N]", or reads `RAKAZO_STACK_OLD_MACHINE_OFF` (`yes` or `no`). Answer yes and the Instance comes back as the same device, at the same address. Answer no and it joins as a new device, with a new auth key, so it gets a new name such as `rakazo-1`, and with it a new Instance URL.

**Replacing an Instance.** On a computer that already has an Instance, restore refuses to run. `--replace` deletes the current Instance's data first, once you type its name, `rakazo-stack`:

```sh
rakazo-stack restore --replace ~/rakazo-stack-backups/rakazo-stack-backup-20261127T120101Z.tar.age
```

Take a Backup of the current Instance first if you might want it back. The Backup file can't be inside `~/rakazo-stack`, which the replace deletes.

If a restore fails part way, it says at which step and leaves what it did for you to look at. Run `restore --replace` with the same file to try again; the Backup file itself is never changed.

## Updates

```sh
rakazo-stack update
```

An Update moves the program and every Component to its newest stable release. In order, it:

1. offers a newer `rakazo-stack` program, if there is one, installs it with the same checks as `install.sh`, and carries on in the new program;
2. checks your computer, and that there's room for the new images;
3. shows each Component's current and new version, any Holds, and links to the release notes, and asks "Update? [Y/n]". Read Rakazo's "Self-host upgrade notes" in its release notes: a release can need a step by hand;
4. downloads everything while the Instance keeps running;
5. takes a Backup, asking for the passphrase unless `RAKAZO_STACK_BACKUP_PASSPHRASE` is set, and keeps Rakazo stopped until the new versions start;
6. starts the new versions, then removes the images of the old ones.

If everything is already up to date, it says "Everything is up to date." and changes nothing.

Updates are best effort: nobody tests each upstream release with rakazo-stack before you get it. That's why the Backup comes first.

**If an Update fails.** When Rakazo, Hindsight or a database fails to start on the new versions, `update` asks whether to roll back from the Backup it just took. Yes restores that Backup, losing only what happened since the new versions started; the next `update` shows that the last one failed. No leaves the new versions in place, and prints the `restore --replace` command to roll back later. Problems with voice, Tailscale or the bot-computer firewall don't roll back: they go on the Checklist, and running `rakazo-stack setup` again retries them.

**Database moves.** When a new release moves to a new major version of Postgres, `update` says so in step 3, and the Backup is required: `update` moves the data across from it.

**Options:**

- `--yes` asks nothing, for scripts. It needs `RAKAZO_STACK_BACKUP_PASSPHRASE`, and if an Update fails it doesn't roll back: it prints the `restore --replace` command instead.
- `--no-backup` skips the Backup, so no rollback is possible. It's refused when Postgres moves.

### Holds

A Hold keeps one Component at a version you choose, for example when its newest release causes trouble:

```sh
rakazo-stack update --hold rakazo=v0.16.0
```

The Components are `rakazo`, `hindsight`, `lemonade` and `tailscale`. A Hold can keep a Component where it is, or move it to a newer release and keep it there. It can't move one back, because its data has already moved forward with it: restoring an older Backup is the way back. A Hold below the oldest version this rakazo-stack works with is refused.

To let it update again:

```sh
rakazo-stack update --unhold rakazo
```

If a newer rakazo-stack can't work with a version you held, `update` says so, moves that Component to its newest release and removes the Hold.

## Removing an Instance

```sh
rakazo-stack uninstall
```

`uninstall` removes the Instance and all its data, and nothing that isn't the Instance's:

1. It offers to take a Backup first, asks whether to delete the images the Instance downloaded that no other container uses, with their total size (Enter keeps them), and asks you to type the Instance's name, `rakazo-stack`. Only then does it take the Backup, so a wrong name changes nothing.
2. It removes the bot-computer firewall's rules, unless another Instance, or a Rakazo you set up yourself with restricted computer egress, runs on the same Docker engine: then the rules stay, because its bot computers rely on them, and `uninstall` says so.
3. It removes the bot computers, the containers, the data, the images setup built, and the bot computers' networks.
4. On a Mac, it removes voice's background item.
5. It deletes `~/rakazo-stack` and the program.

If it stops part way, run it again: it finishes the job and says what was already gone. Your Backups stay, and the last lines say where they are. If you saved one inside `~/rakazo-stack` with `--output`, `uninstall` stops before it changes anything and names it, so you can move it out. If the Instance joined your tailnet, its device stays there too, and the last line links its page in the Tailscale admin console, where **Machine settings** → **Remove** removes it.

In a script, which has no terminal to type into, pass the name instead. It then asks nothing, takes no Backup, and keeps the images unless you add `--delete-images`:

```sh
rakazo-stack uninstall --confirm rakazo-stack
```

### Removing an Instance by hand

If the program is gone, take a Backup first if you might want the Instance back, then run these from `~/rakazo-stack`.

1. Remove the bot-computer firewall's rules. Skip this if another Instance, or a Rakazo you set up yourself with restricted computer egress, uses the same Docker engine: its bot computers rely on the same rules.

   ```sh
   docker compose stop computer-egress
   docker compose run --rm computer-egress --remove
   ```

2. Remove the bot computers, which Rakazo starts outside Compose. The first command lists the Instance's computers; the second removes them:

   ```sh
   docker ps -a --filter label=rakazo.managed=true --filter volume=rakazo-stack_appdata
   docker ps -aq --filter label=rakazo.managed=true --filter volume=rakazo-stack_appdata | xargs -r docker rm -f
   ```

3. Remove the containers, the data and the images setup built:

   ```sh
   docker compose --profile '*' down --volumes --rmi local
   ```

4. Remove the bot computers' networks. This lists them:

   ```sh
   docker network ls --filter label=rakazo.botId
   ```

   Remove each one with `docker network rm` and its name, for example `docker network rm rakazo-computer-<bot>-<id>`.

5. On a Mac, remove voice's background item:

   ```sh
   launchctl bootout gui/$(id -u)/com.github.kamui.rakazo-stack.lemond
   rm ~/Library/LaunchAgents/com.github.kamui.rakazo-stack.lemond.plist
   ```

6. Delete the Instance folder and the program:

   ```sh
   cd ~
   rm -rf ~/rakazo-stack ~/.local/bin/rakazo-stack
   ```

Your Backups stay in `~/rakazo-stack-backups` until you delete them. The images Docker downloaded stay too; `docker image ls` lists them, and `docker image rm` removes one.
