# How the pinned upstreams ship upgrades

Research for [How do the pinned upstreams ship upgrades?](https://github.com/kamui/rakazo-stack/issues/31). Checked 2026-11-03 against Rakazo v0.4.0, Honcho v3.5.0, Lemonade v2026.44.1, `pgvector/pgvector:pg18`, `oven/bun:1.4.4-alpine`, Alpine 3.24's ffmpeg and `tailscale/tailscale`. Sources are source code at the pinned tags, release notes, registry APIs and official docs. Anything not confirmed is marked **unverified**.

## Summary

| Upstream | Cadence | Tags that move | Migrations | Downgrade | Breaking changes announced in |
|---|---|---|---|---|---|
| Rakazo | every few days to weeks | `vX.Y`, `latest`, `edge` (compose default) | one-off `migrate` container on every start | partial: forward-only, but drops are deferred one release | `CHANGELOG.md` "Self-host upgrade notes", release notes |
| Honcho | weekly to fortnightly | `latest` (follows `main`, not releases) | only via `docker/entrypoint.sh` or `python -m src.migrate`; not the image default | image swap down to `compat_floor` (3.2.2 for v3.5.0) | `CHANGELOG.md`, release body |
| Lemonade | weekly (calendar versions `YYYY.WW.N`) | `latest`, `edge`, `candidate` | JSON config migrated forward on start; backends re-downloaded per version | undocumented | release body "Breaking Changes" (no changelog file) |
| pgvector Postgres | pgvector irregular; Postgres minors quarterly, majors yearly | `pg18`, and even `0.8.7-pg18` is rebuilt | `ALTER EXTENSION vector UPDATE` per database | minors: binary swap; majors: no | `CHANGELOG.md`; postgresql.org |
| oven/bun | patches every few days to weeks | `1.4-alpine`, `1-alpine`, `alpine` | n/a | n/a | blog post per release; minors break things |
| Alpine ffmpeg | version bumps inside a stable branch | old `.apk`s are deleted | n/a | n/a | aports history |
| Tailscale | stable about every 4 weeks | `stable`, `latest`, `vX.Y` | state dir carries over (**unverified** as a guarantee) | via `tailscale update --version` | tailscale.com/changelog |

Every one of these tags is re-pushed or floats except a digest. Pin by digest.

## Rakazo (`elie222/rakazo`, v0.4.0 = `b9c652f`)

- **Releases:** v0.1.0 2026-09-03, v0.2.0 10-10, v0.3.x 10-19 to 10-28, v0.4.0 11-01. Android builds get GitHub releases under the `android` tag.
- **Images:** `ghcr.io/elie222/rakazo/{app,computer,updater}`, amd64 + arm64. `vX.Y.Z` is fixed; `vX.Y` moves on patches; `latest` moves to each stable release; `edge` follows every push to main; `sha-<commit>` is fixed. `docker-compose.images.yml` defaults `RAKAZO_IMAGE_TAG` and `RAKAZO_COMPUTER_IMAGE_TAG` to `edge`, so the stack must set both (`docs/self-host-upgrading.md`).
- **Upgrade:** change the tag, `pull`, `up -d --wait`. Upstream's optional `updater` sidecar (opt-in profile, holds the Docker socket) moves an install to the newest stable `sha-<commit>`; the stack should not enable it.
- **Breaking changes:** `CHANGELOG.md` (Keep a Changelog) under per-release "Self-host upgrade notes", and the GitHub release notes. v0.4.0's notes: rerun `harden-host.sh`, set `RAKAZO_ADMIN_EMAILS` for stdio MCP servers, reconnect older Pipedream connections, update the computer image together with the API.
- **Migrations run on start:** a one-off `migrate` container runs `pnpm db:deploy` (Prisma 7.10.1 `migrate deploy`, about 195 migrations), also migrates the job-queue schema and creates the `rakazo_app` login. `api` starts only after it succeeds, so a failed migration keeps the new API down (`apps/api/src/cli/migrate-database.ts`).
- **Downgrades are partial:** "Database migrations are not reversed." Migrations that drop things are marked `-- deferred` and applied on the next migrate run, so rolling back one release stays safe (`packages/db/src/deferred-migrations.ts`). Rolling back past the content-encryption release first needs `content-encryption decrypt`.
- **Postgres:** the published-images compose uses `postgres:16@sha256:e17e8606…` with `pgdata` at `/var/lib/postgresql/data`, still true on `main`. The production and source-checkout compose files moved to `postgres:18`, mounted at `/var/lib/postgresql`, with an entrypoint that refuses non-18 images.
- **Major upgrade:** `infra/compose/upgrade-postgres.sh` does `pg_dumpall` from 16 into a new 18 cluster, checks roles, grants, RLS policies and row counts, keeps the old volume read-only for rollback, needs one extra copy of disk plus 1 GiB, and is idempotent. It is written for the production layout (expects `/var/lib/postgresql/16/docker`), so it doesn't fit the images-compose layout directly. Whether upstream ships a 16→18 path for `docker-compose.images.yml` is **unverified**.
- **Store apps against an older server:** no minimum-server-version check. The app sends `x-rakazo-client-build: mobile/<build time>`; a server can refuse old clients with `RAKAZO_MIN_CLIENT_BUILD` (HTTP 426), but nothing refuses a newer app. Shared types mark fields "absent from older servers" and the app degrades per feature (for example a 404 on `aiConsent/status` shows "Update your server to use AI data sharing in this mobile version."). Mobile code updates over the air on launch and on foreground. Upstream promises only that "Product contracts stay compatible across cloud and self-hosted"; how long a v0.4.0 server keeps working with newer apps is **unverified**. The stack should never set `RAKAZO_MIN_CLIENT_BUILD`.

Sources (under `https://github.com/elie222/rakazo/blob/v0.4.0/`): `docs/self-host-upgrading.md`, `docs/self-host-clients.md`, `docs/mobile-release.md`, `CHANGELOG.md`, `infra/compose/docker-compose.images.yml`, `infra/compose/docker-compose.prod.yml`, `infra/compose/upgrade-postgres.sh`, `apps/api/src/cli/migrate-database.ts`, `packages/db/src/deferred-migrations.ts`, `apps/mobile/lib/client-update.ts`, `apps/mobile/lib/api.ts`, `packages/contracts/src/client-build.ts`, `packages/contracts/src/domain.ts`; registry `https://ghcr.io/v2/elie222/rakazo/app/manifests/<tag>`.

## Honcho (`plastic-labs/honcho`, v3.5.0)

- **Releases:** about weekly to fortnightly since August 2026 (3.5.0 2026-10-29, 3.4.1 10-23, 3.4.0 10-20, 3.3.0 10-09, 3.2.2 09-29, …). Semver is claimed, but minors change behaviour (3.5.0 makes workspace `configuration` updates replace instead of merge); read every "Changed" section.
- **Image tags:** exact `vX.Y.Z` plus `latest`, and **`latest` is pushed from `main`, not from releases** (`.github/workflows/docker-build.yml`). `v3.5.0` = `sha256:23ff96bf62ab2ca14928e6766db1a30a7729f5b7c9e2285af0cd4a179c0d393d`; `latest` differs (main is 12 commits ahead). No floating `v3`/`v3.5`. amd64 + arm64. Not on Docker Hub.
- **Breaking changes:** `CHANGELOG.md`, mirrored in the release body and `docs/changelog/introduction.mdx`, which say explicitly when "This release includes a migration". No v2→v3 upgrade guide.
- **Migrations do not run by default.** The image `CMD` is `fastapi run …` with no entrypoint. Migrations run only from `docker/entrypoint.sh` (`scripts/provision_db.py`, then the server), which upstream compose sets on the api service only. Canonical command: `python -m src.migrate [revision]` (Alembic, 31 revisions, head `c4d8e2f6a1b3`), with a Postgres advisory lock (a second migrator waits up to 300 s) and a 1 s `lock_timeout` per statement. The docs' `uv run alembic upgrade head` doesn't work in the runtime image (no `uv`). The deriver never migrates; api and deriver only validate the embedding columns at startup, and `/health` doesn't check the database.
- **Extensions:** `vector` is required (`CREATE EXTENSION IF NOT EXISTS vector` in `migrations/env.py`); `pg_trgm` and `btree_gin` are optional. A non-superuser role needs a superuser to pre-create them; upstream mounts `database/init.sql` for that.
- **Downgrades:** "Rolling Honcho back is an image swap. It never runs a downgrade." Policy is expand-then-contract and "the previous release has to run on the new schema", enforced in CI. `migrations/compat.toml` at v3.5.0 sets `compat_floor = "3.2.2"`: images back to 3.2.2 run on the v3.5.0 schema. The floor is only in that file, not the release notes.
- **Typical upgrades:** 3.5.0 added a table and nullable tombstone columns; 3.3.0 added a trigram GIN index about 1.5× the size of `messages`, built concurrently; 3.2.2 built an index without `CONCURRENTLY`.
- **Postgres:** no supported version is documented; upstream uses `pgvector/pgvector:pg15` everywhere, so **pg18 is untested upstream** (the boot test in [Boot the drafted compose layout end to end](https://github.com/kamui/rakazo-stack/issues/24) ran it fine).
- **Since v3.5.0:** no release yet; `main` has no new migrations.

Sources (under `https://github.com/plastic-labs/honcho/blob/v3.5.0/`): `CHANGELOG.md`, `.github/workflows/docker-build.yml`, `Dockerfile`, `docker/entrypoint.sh`, `docker-compose.yml.example`, `src/migrate.py`, `scripts/provision_db.py`, `migrations/env.py`, `migrations/README.md`, `migrations/compat.toml`, `tests/compat_matrix/README.md`, `database/init.sql`, `src/startup/embedding_validator.py`, `docs/v3/contributing/self-hosting.mdx`, `.github/workflows/unittest.yml`; registry `https://ghcr.io/v2/plastic-labs/honcho/tags/list`.

## Lemonade (`lemonade-sdk/lemonade`, v2026.44.1)

- **Versions:** calendar `YYYY.WW.N` since v2026.39.1 (before that semver up to v11.9.0; the switch was flagged as breaking for anything comparing versions). `N` counts commits on the release branch, so numbers skip. Candidates are tagged `candidate-v<ver>` (prereleases).
- **Cadence:** weekly. A release branch is cut Wednesdays 16:00 UTC; a human promotes a candidate to stable; the week is skipped if blockers remain Friday. Recent stables: 39.1, 40.0, 41.1, 42.1, 43.3, 44.1 (2026-10-28).
- **Container image:** `ghcr.io/lemonade-sdk/lemonade-server`. Tag pushes publish `:vX` and move `:latest`; release branches publish `:candidate`; `main` publishes `:edge`. `:v2026.44.1` = `sha256:4602f637446980aeb25c3b6350d4fe414770f10f8372b459d7ba32b577347185`. **amd64 only.** One image for every backend. Runs as UID 10001 (old root-owned volumes need a `chown`). Volumes: models `/opt/lemonade/.cache/huggingface`, backends `/opt/lemonade/.cache/lemonade`, config `/opt/lemonade/.config/lemonade`.
- **macOS embeddable:** a plain tarball (`lemond`, `lemonade`, `resources/*.json`); upgrade means replacing files. `lemond` is **ad-hoc signed only** (`spctl` rejects it; fine for `curl` downloads, see [Will a Bun-compiled setup program run on every supported OS?](https://github.com/kamui/rakazo-stack/issues/22)). The `.pkg`'s `lemond` is Developer-ID signed and notarized. SHA-256 matches GitHub's published digest. No self-update and no check for new Lemonade versions.
- **Outbound calls by default:** a Hugging Face model-update check at startup (`auto_check_model_updates: false` turns it off), the UDP beacon (`--no-broadcast`), backend downloads from GitHub releases (`no_fetch_executables: true` blocks them once installed), `offline: true` skips model downloads. Telemetry is off by default.
- **State migrates forward on start:** `config.json` stores overrides with `config_version` (now 2) and is rewritten by forward-only migrations; other JSON files move or get re-keyed once. There is no database.
- **Backends are pinned per Lemonade version** in `resources/backend_versions.json` (whisper.cpp v1.8.4 at this tag) and **re-downloaded on any upgrade or downgrade** whose pins changed. Breaking runtime moves have cost multi-GB re-downloads (ROCm 10 in 41.1).
- **Pitfall:** a relative `models_dir` resolves against the executable's folder; keep models, config and cache outside a folder replaced on upgrade (the Instance layout in [How does setup install Lemonade and ffmpeg on macOS?](https://github.com/kamui/rakazo-stack/issues/17) already does).
- **Downgrades:** undocumented; the MSI blocks them. Inferred from code: an older `lemond` leaves a newer config alone, but one-way re-keys may not be understood. Snapshot the config folder before upgrading.
- **Breaking changes:** only in each release body's "Breaking Changes" section (AI-drafted, human-edited) and Discord; no changelog file. v2026.44.1's section is empty. Candidate 45.3 lists API status-code tweaks (405 vs 404, 400 vs 500) and no changes to the Dockerfile, CLI flags, config version or macOS paths.

Sources (under `https://github.com/lemonade-sdk/lemonade/blob/v2026.44.1/`): `docs/dev/release.md`, `tools/version.py`, `.github/workflows/build-and-push-container.yml`, `.github/workflows/cpp_server_build_test_release.yml`, `Dockerfile`, `docs/guide/install/docker.md`, `docs/embeddable/*.md`, `docs/guide/configuration/README.md`, `src/cpp/resources/defaults.json`, `src/cpp/resources/backend_versions.json`, `src/cpp/server/config_file.cpp`, `src/cpp/server/backend_manager.cpp`; releases `v2026.44.1`, `candidate-v2026.45.3`, `v2026.39.1`; registry `https://ghcr.io/v2/lemonade-sdk/lemonade-server/tags/list`; `codesign`/`spctl`/`pkgutil` on the release artifacts.

## pgvector Postgres

- **No CI for images:** the maintainer pushes from `Makefile` targets. `pgN`, `pgN-bookworm`, `X.Y.Z-pgN` are bookworm; `pgN-trixie` and `X.Y.Z-pgN-trixie` are trixie (the README now recommends trixie; plain `postgres:18` is trixie). amd64 + arm64. Built `FROM postgres:$PG_MAJOR-$DEBIAN_CODENAME`.
- **Everything floats:** `pg18` moves with each pgvector release, Postgres minor and Debian base. Even `0.8.7-pg18` was rebuilt on 2026-10-31 (a month after 0.8.7) to pick up Postgres 18.6; old version tags stop being rebuilt. Rebuild timing is manual and **unverified**. Only a digest is immutable.
- **Extension upgrade:** a new image swaps the `.so` immediately, but the catalog version only changes after `ALTER EXTENSION vector UPDATE;` in each database (idempotent, cheap).
- **Cadence:** pgvector 0.8.1–0.8.7 between 2025-09 and 2026-10, mostly fixes, some for index corruption and buffer overflows. 0.9.0 unreleased.
- **Postgres policy:** one major a year, each supported 5 years (PG18 until 2030-11-14). Minors at least quarterly (next 2026-11-12), binary swap and restart. PG19 is at RC1 (2026-10-15); no `pgvector:pg19` yet.

## Postgres major upgrades with the official image

- **No automatic `pg_upgrade`** (docker-library/postgres#37, open since 2014).
- **PG18 layout:** `PGDATA=/var/lib/postgresql/18/docker`, `VOLUME /var/lib/postgresql`, so a future `pg_upgrade --link` works within one volume (docker-library/postgres#1259).
- **Bumping only the tag fails loudly:** the 18+ entrypoint finds an older cluster (or a mount at `/var/lib/postgresql/data`) and exits 1; it never silently initialises an empty cluster. A compose file that keeps the ≤17 mount at `/var/lib/postgresql/data` is rejected by 18+.
- **Options:** dump and restore (endorsed by upstream and docker-library; the dump carries `CREATE EXTENSION vector`); `tianon/postgres-upgrade` (a "PoC", up to `17-to-18`, no pgvector, so it would need a custom image); logical replication (overkill here).
- **Recommendation for small single-user databases:** `pg_dumpall` or `pg_dump -Fc`, restore into a fresh container on a new volume, then `ALTER EXTENSION vector UPDATE`; keep the old volume for rollback. This is what Rakazo's own `upgrade-postgres.sh` does.

## oven/bun

- On release, `1.4.4-alpine`, `1.4-alpine`, `1-alpine` and `alpine` share one digest; `latest` and unsuffixed tags are Debian; canaries are daily. amd64 + arm64.
- `1.4.4-alpine` is `FROM alpine:3.24` (3.24.3) and **never rebuilt**, so OS fixes arrive only with a new Bun release.
- Patches every few days to weeks (1.4.0 2026-08-20 … 1.4.4 2026-10-22). **Minors break things** (the 1.4 post lists Node ABI, linker, `.env`, YAML 1.2, strict TOML changes); no formal semver policy found. Pin `1.4.4-alpine` by digest.

## Alpine ffmpeg

- v3.24 (the Bun image's base) ships **8.1.3-r0** (2026-10-24); v3.23 8.0.1, edge 9.0.2.
- Upstream version bumps land inside a stable branch (8.1.1 → 8.1.2 security → 8.1.3 in 3.24), and **old `.apk`s are removed from the mirror**, so an exact `ffmpeg=8.1.2-r0` pin already fails. Use `ffmpeg~=8.1` or no pin, and let the Bun image fix the Alpine branch (exact `~=` semantics untested).
- Alpine branches each May and November; **community (where ffmpeg lives) is supported only until the next stable release**. v3.24 ffmpeg stops getting regular fixes once 3.25 ships (about Nov/Dec 2026, **unverified**), so the relay's base must follow Bun releases onto newer Alpine.

## Tailscale

- `tailscale/tailscale` (Docker Hub) and `ghcr.io/tailscale/tailscale`. `latest` = `stable` = `v1.104` = `v1.104.4` today; unstable is `unstable-vX.Y.Z`. Images lag GitHub releases (v1.104.5 published 2026-11-03, no image yet).
- Even minors stable, odd unstable; stable about every four weeks.
- `TS_STATE_DIR` must persist, or the container joins as a new node each restart (and TS-2025-008 tells container users under Tailnet Lock to set it). State carrying across versions works in practice but isn't a documented guarantee (**unverified**).
- Downgrades via `tailscale update --version`; state compatibility on downgrade **unverified**. No published minimum client version for the hosted control plane (security bulletins name minimum fixed versions; the Kubernetes operator supports proxies up to 4 minors older).

Sources: `https://hub.docker.com/v2/repositories/pgvector/pgvector/tags`, `https://github.com/pgvector/pgvector` (Makefile, Dockerfile, README, CHANGELOG), `https://github.com/docker-library/postgres/pull/1259`, `https://github.com/docker-library/postgres/issues/37`, `https://github.com/docker-library/postgres/blob/master/18/trixie/docker-entrypoint.sh`, `https://github.com/tianon/docker-postgres-upgrade`, `https://www.postgresql.org/support/versioning/`, `https://www.postgresql.org/developer/roadmap/`, `https://www.postgresql.org/docs/18/pgupgrade.html`, `https://github.com/oven-sh/bun/blob/bun-v1.4.4/dockerhub/alpine/Dockerfile`, `https://github.com/oven-sh/bun/blob/bun-v1.4.4/.github/workflows/release.yml`, `https://bun.com/1.4`, `https://pkgs.alpinelinux.org/packages?name=ffmpeg`, `https://gitlab.alpinelinux.org/alpine/aports/-/commits/3.24-stable/community/ffmpeg`, `https://alpinelinux.org/releases/`, `https://wiki.alpinelinux.org/wiki/Alpine_Package_Keeper`, `https://hub.docker.com/v2/repositories/tailscale/tailscale/tags`, `https://tailscale.com/kb/1168/versions`, `https://tailscale.com/docs/features/containers/docker/docker-params`, `https://tailscale.com/security-bulletins`, `https://tailscale.com/docs/reference/tailscale-cli`.

## What this means for the stack

- **Pin every image by digest**, with the tag kept for readability. No upstream tag used here is immutable.
- **Rakazo's Postgres is 16** in the published-images compose; the stack inherits it unmodified, and the move to 18 depends on upstream shipping a path for that compose file.
- **Honcho needs an explicit migration step**; the compose-layout prototype already sets `entrypoint: ["sh", "docker/entrypoint.sh"]` on the api, and mounts its pg18 volume at `/var/lib/postgresql` as required. Still missing: `ALTER EXTENSION vector UPDATE` after each pgvector image bump.
- **Lemonade upgrades re-download backends**; on macOS keep models, config and cache outside the versioned binary folder, and consider `auto_check_model_updates: false`.
- **The relay's ffmpeg follows the Bun image's Alpine branch**; pin Bun by digest and ffmpeg loosely.
- **Newer phone apps against a pinned server degrade per feature** rather than refusing; there is no stated compatibility window.
