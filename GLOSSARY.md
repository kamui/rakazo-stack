# rakazo-stack

An all-in-one, self-hosted setup that runs Rakazo together with the services it needs for memory and voice on a single machine, without paid third-party services beyond the model provider.

## Language

**Self-hoster**:
The person who installs and runs an Instance on their own machine. Distinct from Rakazo's Owner account, even when the same person holds both.
_Avoid_: Operator, owner, host, admin

**Instance**:
One installed copy of rakazo-stack on one machine, with its own data and configuration.
_Avoid_: Deployment, server, install

**Instance URL**:
The one address an Instance treats as its home: where people sign in, and the base for links and integration callbacks.
_Avoid_: Origin, domain, server URL

**Instance folder**:
The one folder on the machine that holds an Instance's settings, secrets and stack files. Its data lives in Docker volumes, not here.
_Avoid_: Install dir, data dir

**Backup**:
One encrypted file holding everything needed to bring an Instance back, on the same machine or another. A database dump is one part of a Backup, not a Backup itself.
_Avoid_: Snapshot, export, dump

**Component**:
One of the upstream projects an Instance runs, such as Rakazo, Lemonade, Hindsight or Tailscale. Each has its own version, which an Update moves and a Hold keeps.
_Avoid_: Service, dependency, package

**Run log**:
The file one run of setup, update, backup or restore writes in the Instance folder's `logs/`: what it printed and each command it ran, every line timed, with secrets and personal values replaced.
_Avoid_: Setup log, debug log, trace

**Checklist**:
The list of steps left for the Self-hoster to do by hand, printed at the end of every setup run.
_Avoid_: To-do list, manual steps

**Update**:
Moving an Instance to the newest versions of rakazo-stack and its components.
_Avoid_: Upgrade

**Hold**:
Keeping a component of an Instance at a version the Self-hoster chose, instead of the newest one.
_Avoid_: Pin

**Model provider**:
The outside AI service whose key runs an Instance's models, for chat and for memory. The Self-hoster brings their own.
_Avoid_: AI provider, LLM provider
