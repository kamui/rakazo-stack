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

**Update**:
Moving an Instance to the newest versions of rakazo-stack and its components.
_Avoid_: Upgrade

**Hold**:
Keeping a component of an Instance at a version the Self-hoster chose, instead of the newest one.
_Avoid_: Pin

**Model provider**:
The outside AI service whose key runs an Instance's models, for chat and, when it can, for memory. The Self-hoster brings their own.
_Avoid_: AI provider, LLM provider
