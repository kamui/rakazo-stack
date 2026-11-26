# AGENTS.md

rakazo-stack installs a self-hosted Rakazo Instance. The repo holds a Rust program (`rakazo-stack`), a compose layer file, the Lemonade relay and the computer-egress sidecar. The v1 build is tracked in [Build: rakazo-stack v1](https://github.com/kamui/rakazo-stack/issues/57).

## The spec decides

- [`docs/spec.md`](docs/spec.md) is the source of truth for behaviour. Each ticket names the spec sections it implements: read them before writing code.
- A PR that changes behaviour, proves a "not tested" line or picks a §21 name updates the spec in the same PR.
- Use the terms in [`GLOSSARY.md`](GLOSSARY.md) exactly, in code, output and docs. Its "Avoid" lists are the words to replace.
- [`docs/adr/`](docs/adr/) records why v1 uses Hindsight, latest-stable versions and Rust.

## Upstream runs as released

Rakazo, Hindsight, Lemonade and Tailscale run unmodified: no forks, no patches, no locally built upstream images. Rakazo's compose file is written to the Instance folder byte for byte. Everything the stack changes goes through `.env` or `compose.stack.yaml`.

## This repo is public

Code, tests, fixtures, logs, commit messages, issues and PRs hold only generic values: placeholder keys, `example` hostnames, `~`-relative paths. Keep real secrets, keys, personal paths, hostnames, usernames, tailnet names and anything from a real Instance out of all of them. A pasted key is never printed or logged, not even partly; record replies for fixtures with the key already replaced.

## Docker on the dev machine

The dev machine runs other people's containers, so an agent works only on what it created:

- Give every test Instance its own Compose project name and its own Instance folder, and tear it down when the test ends, volumes and networks included.
- Scope every Docker command to that project (`-p <name>`, or a `com.docker.compose.project=<name>` filter). Unscoped `prune`, `down`, `rm` and `stop` belong to the owner.
- Restarting Docker Desktop, `wsl --shutdown` and reboots are the owner's to run.
- In WSL, call `docker.exe`, as the program does. Docker installed inside WSL is unsupported.

## Rust

- The toolchain is pinned in `rust-toolchain.toml`.
- Before opening a PR: `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test` all pass.
- Pure logic (parsing, version resolution, decisions, file generation) gets unit tests that need no Docker. Tests that start containers are integration tests and run on Linux CI.

## The relay

The Lemonade relay is TypeScript on Bun, in its own build folder, with its own `bun test`. It ships as a container image, separate from the Rust program.

## Where each OS is tested

| Path | Where |
|---|---|
| Windows (WSL 2 + Docker Desktop, through `docker.exe`) | The dev machine, from the first slice |
| Linux (Docker Engine) | GitHub's `ubuntu-latest` runners; later a Linux NAS |
| macOS (Apple silicon) | By hand on a Mac. CI only builds it and runs the `codesign` and `--version` gate |

A ticket's "Test environment" says which of these it needs.
