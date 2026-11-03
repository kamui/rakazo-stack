# Will a Bun-compiled setup program run on every supported OS?

Ticket: [#22](https://github.com/kamui/rakazo-stack/issues/22). Context: [#9](https://github.com/kamui/rakazo-stack/issues/9) chose "one TypeScript program compiled with Bun into a single executable per platform, fetched by an install script".

Pinned versions: **Bun v1.4.4** (released 2026-10-22, commit `38afee6c8`, the latest release when this was written; [tag](https://github.com/oven-sh/bun/tree/bun-v1.4.4)). Everything below was read at that tag unless it says otherwise.

## Answer

- **macOS arm64 works with no Apple Developer account on the `curl | sh` path.** `bun build --compile --target=bun-darwin-arm64` writes an ad-hoc, linker-signed code signature into the output itself, even when cross-compiling from Linux. No separate `codesign` step is needed. A file fetched by `curl` is **not** quarantined, so Gatekeeper never evaluates it.
- **Bun's signing has regressed twice, so CI must test on real hardware.** v1.3.12 produced truncated signatures (SIGKILL on launch) and v1.4.0 produced invalid ones (SIGKILL on macOS 27). Both are fixed in v1.4.4. Pin Bun ≥ 1.4.1 and add a macOS arm64 smoke test (`codesign --verify --strict`, then run `--version`) before publishing.
- **Developer ID signing and notarization are not needed for the `curl | sh` path, but they would be for a browser download.** A browser sets `com.apple.quarantine`, and Gatekeeper then blocks an unnotarized binary. The cost is the Apple Developer Program at 99 USD per year, plus a Developer ID certificate, hardened-runtime signing with the JIT entitlements, `notarytool` in CI, and secret handling. A bare binary can't be stapled. Recommendation: don't pay for it in v1, and don't offer a browser download of the raw binary. Revisit if a Homebrew tap or a `.pkg` is wanted.
- **macOS floor: 13.0.** Bun's own requirement; the measured `LC_BUILD_VERSION` of a compiled binary is also 13.0.
- **Linux x86-64 and WSL2: ship `bun-linux-x64` (glibc) only.** It needs glibc ≥ 2.17 (measured max symbol version: 2.17) and an SSE4.2 CPU. Since v1.4.x there is no separate baseline/modern build: the `-baseline` and `-modern` target suffixes are aliases and produce the same bytes (measured). The `-musl` build is only for Alpine-style distros; skip it in v1, or add it later and pick it by `/etc/alpine-release` as Bun's own installer does. Inside WSL2 this is an ordinary Linux ELF run, with nothing WSL-specific beyond "the distro must be glibc-based, and WSL 2, not WSL 1".
- **One `ubuntu-latest` job can build both assets** (Bun downloads the target runtime from the npm registry). Add a `macos-latest` (arm64, free for public repos) job to smoke-test. Sizes for a hello-world program with `--minify`: linux-x64 **109 MB** (52 MB gzipped), darwin-arm64 **89 MB** (41 MB gzipped), musl 103 MB (48 MB). The real app will add a little on top; the Bun runtime dominates.
- **Install script verification: bake SHA-256 digests into the script, publish releases as immutable, and verify attestations when `gh` happens to be installed.** A `SHA256SUMS` file next to the assets only protects against corruption, because it shares a trust root with the binary. Hard-requiring `gh`, `cosign` or `minisign` contradicts "Docker is the only prerequisite". Details and trade-offs below.
- **Asset choice: `uname -s`/`uname -m`.** Support exactly `Linux x86_64` and `Darwin arm64`. Refuse everything else with a clear message, with a Rosetta-aware message for an `x86_64` shell on an Apple-silicon Mac. Do not copy Bun's installer fallthrough, which silently defaults to `linux-x64`.
- **No blocker found**, so no alternative delivery is needed. A Homebrew tap is a fallback only if Gatekeeper prompting turns up on the `curl` path (not expected).

## Details

### 1. macOS arm64

**The compiled binary is already ad-hoc signed.** Bun's Mach-O writer builds the signature itself. In [`src/exe_format/macho.rs`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/src/exe_format/macho.rs) the `MachoSigner` writes a `CSMAGIC_EMBEDDED_SIGNATURE` super blob holding one SHA-256 `CodeDirectory` (page size 4096, `flags = 0x20002`, i.e. ad-hoc plus linker-signed). It runs for `CPU_TYPE_ARM64` outputs unless the `BUN_NO_CODESIGN_MACHO_BINARY` feature flag is set. Because the signer is part of Bun and not a call to `codesign`, it also works when cross-compiling from Linux. Bun's [test](https://github.com/oven-sh/bun/blob/bun-v1.4.4/test/bundler/compile-macho-codesign.test.ts) cross-compiles `--target=bun-darwin-arm64` and checks page hashes and that the signature ends at end-of-file, and runs a native `codesign --verify` on macOS arm64 only.

Independent check (on a Linux host, with Bun v1.4.4 verified against the release's `SHASUMS256.txt`): I cross-compiled a hello-world for `bun-darwin-arm64` and parsed the Mach-O with a small script. Result: `LC_CODE_SIGNATURE` present, `CodeDirectory` `flags=0x20002`, SHA-256, 21,599 page hashes all matching, signature ends at EOF, `LC_BUILD_VERSION` minos 13.0, identifier `a.out`. I could **not** run `codesign` or execute the binary on macOS (no Mac available), so "the kernel runs it" is **unverified** here; it rests on Bun's own CI and the fixed issues below.

**Why a signature matters:** an unsigned arm64 binary is killed at launch. Bun's own [#29120](https://github.com/oven-sh/bun/issues/29120) shows `codesign -dv` reporting "code object is not signed at all" and `./myapp` printing `Killed: 9`. Apple's claim that Apple silicon requires a valid signature (an ad-hoc one suffices) is in the Big Sur release notes; I could not retrieve that page, so the Apple-side wording is **unverified**. Apple's [Platform Security guide](https://support.apple.com/guide/security/app-code-signing-process-sec7c917bf14/web) says the iOS-family OSes require all executable code to be signed with an Apple-issued certificate, and says nothing about macOS ad-hoc signing.

**Known Bun signing regressions (all closed):**

| Issue | Bun | Symptom | Fix |
|---|---|---|---|
| [#29120](https://github.com/oven-sh/bun/issues/29120) | 1.3.12 | Truncated signature (`datasize` too small), SIGKILL; reproducible when cross-compiling from Linux | PR [#29122](https://github.com/oven-sh/bun/pull/29122); closed 2026-04-14 |
| [#32159](https://github.com/oven-sh/bun/issues/32159) | 1.3.13 | `codesign -v`: "invalid signature (code or signature have been modified)" (last partial page hashed padded) | closed 2026-08-22 |
| [#39758](https://github.com/oven-sh/bun/issues/39758) / [#39764](https://github.com/oven-sh/bun/issues/39764) | 1.4.0 | Compiled binaries SIGKILL'd on macOS 27; 1.3.14 output still ran | PR [#39837](https://github.com/oven-sh/bun/pull/39837) merged 2026-08-22; release 1.4.1 or later |

The regression test for #32159 is present at the v1.4.4 tag. The takeaway is process: pin the Bun version, and gate every release on a macOS arm64 runner that runs `codesign --verify --strict` and the binary. I did not need `codesign --force -s -` as a workaround at v1.4.4, since the signature parses correctly. If a future Bun regresses, re-signing on the macOS runner with `codesign --force -s -` is the standard escape hatch (Bun's docs show `codesign --force` with `--deep` for real identities: [executables docs](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/bundler/executables.mdx), section "Code signing on macOS").

**Quarantine and Gatekeeper on the `curl` path.** Apple Developer Technical Support, in [Resolving Trusted Execution Problems](https://developer.apple.com/forums/thread/706442), says: "User-level apps on macOS implement a quarantine system for new downloads. For example, if Safari downloads a zip archive, it quarantines that archive. This involves setting the `com.apple.quarantine` extended attribute", and "Unix-y networking tools, like `curl` and `scp`, don't quarantine the files they download." It also says "running a quarantined app always invokes Gatekeeper"; the post says Gatekeeper may run at other times too and that this is not documented. So a binary fetched by `curl` has no quarantine attribute and gets no first-launch Gatekeeper dialog. Gatekeeper's documented trigger is a user "downloads and opens" software ([Apple Platform Security: Gatekeeper](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web)). Empirically confirming "no prompt for a curl-fetched ad-hoc binary on current macOS" needs a Mac: **unverified**.

**Browser download is different.** If a Self-hoster downloads the binary in a browser, it is quarantined, and by Apple's description Gatekeeper then checks the signature and notarization ([Gatekeeper](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web)): an ad-hoc, unnotarized binary would be blocked or require an explicit override. This follows from the documents but was not tested (**unverified**). Mitigation: document only the `curl | sh` route, never link the raw macOS asset as the primary download. Also, `tar`/`unzip` don't propagate quarantine (same Apple thread), so shipping `.tar.gz` assets is fine.

**What Developer ID + notarization would involve** (not needed for v1):
- Apple Developer Program membership, 99 USD per membership year ([Apple](https://developer.apple.com/programs/whats-included/)).
- Sign with a "Developer ID Application" certificate (ad hoc and other certificate types aren't accepted), then submit to the notary service; notarization "typically takes less than an hour"; a ZIP, DMG or flat pkg can be submitted, but "you can't staple to" a ZIP ([Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow), [Notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)). A bare Mach-O therefore relies on Gatekeeper's online ticket lookup.
- Notarization requires the hardened runtime. A Bun-compiled binary needs the JIT entitlements (`com.apple.security.cs.allow-jit` and `allow-unsigned-executable-memory`) ([Bun codesign guide](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/guides/runtime/codesign-macos-executable.mdx), [executables docs](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/bundler/executables.mdx)); I did not test notarization with them.
- Apple's service can be driven without macOS (the docs list a direct REST interface), but `codesign` with a real identity needs a Mac, so the signing job would run on `macos-latest`, with the certificate and notary credentials held as repository secrets.

### 2. Linux x86-64 and WSL2

- **Targets** ([`executables.mdx`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/bundler/executables.mdx)): `bun-linux-x64` (glibc) and `bun-linux-x64-musl`. The docs note that on x64 "Bun ships a single binary that targets Nehalem (SSE4.2) and selects AVX2/AVX-512 code paths at runtime. The `-baseline` and `-modern` target suffixes are still accepted ... and resolve to the same binary." Measured: compiling with `bun-linux-x64-baseline` and `bun-linux-x64` gave outputs of identical size whose first 109 MB are byte-identical. (Older Bun versions split these; pin ≥ 1.4 to avoid the choice. A pre-Nehalem CPU is unsupported either way: [installation docs](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/installation.mdx), "CPU Requirements".)
- **glibc**: "Bun's glibc binaries require glibc 2.17 or newer" (installation docs). Measured with `objdump -T` on the compiled binary: highest versioned symbol `GLIBC_2.17`; dynamic dependencies are `libc.so.6`, `ld-linux-x86-64.so.2`, `libpthread.so.0`, `libdl.so.2`, `libm.so.6`; interpreter `/lib64/ld-linux-x86-64.so.2`, `for GNU/Linux 3.2.0`. Every distro Docker's Engine install targets (Debian, Ubuntu, Fedora, RHEL-family; see [Docker installs](https://github.com/kamui/rakazo-stack/blob/research/docker-installs/docs/research/docker-installs.md)) is on glibc 2.28 or newer, so glibc ≥ 2.17 is a non-issue. Alpine (musl) is the only mainstream case that needs the musl build.
- **Kernel**: Bun recommends Linux kernel 5.6+ and degrades gracefully down to 3.10 (installation docs). WSL2 ships a real Linux kernel; the reference host runs a 6.6-series WSL2 kernel.
- **Executed**: I ran a cross-compiled `bun-linux-x64` hello-world on an x86-64 WSL2 host. It started and spawned `docker` normally (output JSON with `platform: "linux"`, `arch: "x64"`).
- **WSL2-specific points**: none beyond (a) the distro must be glibc-based, (b) the binary lives on the distro's own ext4 filesystem (no Windows Mark-of-the-Web or SmartScreen involvement, since the Windows loader isn't used), and (c) Docker Desktop's WSL integration must be on, which the setup preflight already checks ([Docker installs](https://github.com/kamui/rakazo-stack/blob/research/docker-installs/docs/research/docker-installs.md)). WSL 1 can't run Docker Desktop anyway. Whether a compiled Bun binary runs under WSL 1 is **unverified** and irrelevant.

### 3. Building and publishing from GitHub Actions

- **Cross-compile**: `bun build --compile --minify --target=<t> src/cli.ts --outfile <name>`. Bun fetches the target runtime from the npm registry (`@oven/bun-<os>-<arch>[-musl]`, version = the running Bun's version), caching it in the install cache ([`compile_target.rs`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/src/options_types/compile_target.rs)). Verified: from a Linux x64 Bun 1.4.4, `--target=bun-linux-x64`, `bun-linux-x64-musl` and `bun-darwin-arm64` all built in about 1 s each (hello world). So a single `ubuntu-latest` runner can build both v1 assets. Windows-only flags (icon, metadata) can't be used when cross-compiling, which is irrelevant here.
- **Runners**: `ubuntu-latest` is Linux x64; `macos-latest`, `macos-14` and `macos-15` are macOS arm64 (3 CPU M1, 7 GB), and "use of the standard GitHub-hosted runners is free and unlimited on public repositories" ([GitHub runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)). Use `macos-latest` for the smoke test, and `ubuntu-latest` for a Linux smoke test (the Linux binary can also be tested there).
- **Sizes** (hello world that shells out to `docker`, `--minify`, Bun 1.4.4):

  | Target | Raw | gzip -9 |
  |---|---|---|
  | `bun-linux-x64` | 109,123,760 B (~109 MB) | 51.6 MB |
  | `bun-linux-x64-musl` | 102,712,744 B | 48.1 MB |
  | `bun-darwin-arm64` | 89,159,906 B (~89 MB) | 40.8 MB |

  Publish `.tar.gz` (or `.zip`) assets to cut the download by about half, with the app's own code adding a small amount.
- **Publish**: create the GitHub release as a draft, upload assets, then publish. This ordering is GitHub's recommendation for immutable releases ([docs](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)).

### 4. Install script: picking the asset and verifying it

**Asset detection.** Bun's own installer ([`install.sh`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/src/runtime/cli/install.sh)) switches on `uname -ms` (`Darwin arm64` → `darwin-aarch64`, `Linux x86_64` → `linux-x64`), uses `/etc/alpine-release` to pick musl, and checks `sysctl -n sysctl.proc_translated` to notice a shell running under Rosetta. Ours should:
1. `uname -s`: `Linux` or `Darwin`, else stop.
2. `uname -m`: Linux needs `x86_64` (stop on `aarch64`/others: Linux ARM is out of scope); Darwin needs `arm64`. If Darwin reports `x86_64`, check `sysctl -n sysctl.proc_translated`: `1` means an Apple-silicon Mac whose shell runs under Rosetta, so use the arm64 asset (or tell the user to open a native terminal); otherwise stop (Intel Macs are out of scope).
3. Linux with `/etc/alpine-release` → musl asset if one is published, else a clear message.
4. WSL needs no special case: `uname -s` reports `Linux`. A friendly warning if `uname -r` lacks `WSL2` and a Windows host is suspected is optional. I observed the `microsoft-standard-WSL2` kernel string on the reference host but did not find a primary spec for the format, so detection by that string is **unverified**.

**Verification options and trade-offs.**

| Option | What it protects against | Cost / catch |
|---|---|---|
| `SHA256SUMS` file from the same release, checked with `sha256sum` / `shasum -a 256` | Truncated or corrupted downloads | Same origin as the binary: no protection if the release is tampered with. Works on stock Linux and macOS (macOS has `shasum`, not `sha256sum`). |
| Digests baked into `install.sh`, which CI generates and uploads as a release asset (`…/releases/latest/download/install.sh`) | Swapped assets after publish, mirror or CDN tampering | The script and the digests share the release as the trust root, so the user's trust is "TLS to GitHub plus the repo's release process". That is the same trust a `curl \| sh` already extends. |
| GitHub immutable releases ([docs](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)) | Assets or tags altered after publication; gives a signed release attestation | Needs the repository or org setting turned on; the owner must publish drafts. Verification: `gh release verify` / `gh release verify-asset` ([manual](https://cli.github.com/manual/gh_release_verify-asset)), which needs `gh`. |
| Artifact attestations ([docs](https://docs.github.com/en/actions/concepts/security/artifact-attestations)): `actions/attest@v4` with `id-token: write`, `attestations: write`, `contents: read`; `subject-path` = binaries ([how to](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)); verify with `gh attestation verify <file> -R kamui/rakazo-stack` (add `--signer-workflow`) ([manual](https://cli.github.com/manual/gh_attestation_verify)) | The binary came from this repo's workflow (SLSA v1.0 Build Level 2, Sigstore public-good instance for public repos with a public transparency log) | Requires `gh` (and network access to GitHub's API unless a `--bundle` plus trusted root is supplied offline). `gh` is not guaranteed on the Self-hoster's machine. Not a guarantee the code is safe. Whether `gh attestation verify` needs `gh auth login` for a public repo: **unverified**. |
| `cosign` or `minisign` signature | A key- or identity-bound signature on the checksums | Neither is installed by default. A public key pinned in `install.sh` has the same origin as the script. Adds a key to manage (minisign) or another tool to fetch (cosign). Not worth it for v1. |
| GitHub's per-asset `digest` field (`sha256:…`) in the Releases API (verified present on Bun's own release; [REST docs](https://docs.github.com/en/rest/releases/assets) list the field) | Corruption | Same origin; no better than `SHA256SUMS`. |

Recommended shape (for a follow-up decision): `install.sh` (a release asset, version-stamped, containing the expected SHA-256 of each platform asset) downloads the asset for the detected platform, checks the digest and refuses to install on mismatch, and, when `gh` is present, additionally runs `gh attestation verify`. Enable immutable releases. Document the manual `gh attestation verify` command for the cautious. The first verification point is also where `curl | sh` is weakest (the script itself), which is why the script is pinned to a release rather than the default branch. Also have the install script create the target directory with the right permissions and install atomically (write to a temp file, then `mv`), which matters because overwriting a running signed Mach-O in place can crash it ([Apple: Updating Mac software](https://developer.apple.com/documentation/security/updating-mac-software)).

### 5. Alternatives (only if a blocker turns up)

No blocker found, so these are for the record only:
- **Homebrew tap on macOS.** Homebrew downloads with `curl` as well, so the quarantine behaviour matches; it would add an update path and let the tap run the same ad-hoc signed binary. The `bun-v1.4.4` docs don't discuss Homebrew and I did not research the tap mechanics (**unverified**). Consider only if Self-hosters want `brew upgrade`.
- **Developer ID + notarization** (section 1) if a browser download or a `.pkg`/`.dmg` is ever wanted.
- **Run the compiled binary inside a container** is not an option for the setup tool itself: it must run before and around Docker (preflight).

## Open questions / follow-ups

1. **(task)** Release pipeline spec: pin Bun ≥ 1.4.1 (v1.4.4 verified), build linux-x64 and darwin-arm64 on `ubuntu-latest`, smoke-test on `macos-latest` (`codesign --verify --strict`, run `--version`) and `ubuntu-latest`, publish as a draft then release, attest with `actions/attest`, enable immutable releases, generate `install.sh` with baked digests. How should the setup tool report its own version, and is a self-update command in scope?
2. **(prototype)** On a real Apple-silicon Mac (and ideally macOS 13 and the newest macOS), `curl` a CI-built binary and confirm: no quarantine attribute, no Gatekeeper dialog, `codesign -dv` shows `Signature=adhoc`, and the binary runs. Also confirm behaviour when the same file is downloaded by a browser. Everything about actual macOS behaviour here is documentation-based, not run.
3. **(grilling)** Do we require `gh` for verification (strong, but adds a prerequisite) or make it opportunistic (recommended above)? And do we offer musl/Alpine in v1?
4. **(research, low priority)** Whether `gh attestation verify` works unauthenticated for a public repo, and whether a `--bundle` plus bundled trusted root could let `install.sh` verify offline without `gh` (unlikely without `gh` itself).
5. **Fog:** future Bun regressions in Mach-O signing; a version-pinned canary build on macOS 27 before each Bun bump.

## Sources

Bun at tag `bun-v1.4.4`:
- [`docs/bundler/executables.mdx`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/bundler/executables.mdx) (targets, baseline/modern note, code signing)
- [`docs/guides/runtime/codesign-macos-executable.mdx`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/guides/runtime/codesign-macos-executable.mdx)
- [`docs/installation.mdx`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/docs/installation.mdx) (glibc 2.17, kernel, CPU, macOS 13)
- [`src/exe_format/macho.rs`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/src/exe_format/macho.rs), [`test/bundler/compile-macho-codesign.test.ts`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/test/bundler/compile-macho-codesign.test.ts)
- [`src/options_types/compile_target.rs`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/src/options_types/compile_target.rs)
- [`src/runtime/cli/install.sh`](https://github.com/oven-sh/bun/blob/bun-v1.4.4/src/runtime/cli/install.sh)
- Release [bun-v1.4.4](https://github.com/oven-sh/bun/releases/tag/bun-v1.4.4) and its `SHASUMS256.txt`
- Issues [#29120](https://github.com/oven-sh/bun/issues/29120), [#32159](https://github.com/oven-sh/bun/issues/32159), [#39758](https://github.com/oven-sh/bun/issues/39758), [#39764](https://github.com/oven-sh/bun/issues/39764); PRs [#29122](https://github.com/oven-sh/bun/pull/29122), [#39837](https://github.com/oven-sh/bun/pull/39837)

Apple:
- [Resolving Trusted Execution Problems (DTS)](https://developer.apple.com/forums/thread/706442) (quarantine, curl)
- [Gatekeeper and runtime protection](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web), [App code signing process](https://support.apple.com/guide/security/app-code-signing-process-sec7c917bf14/web)
- [Notarizing macOS software before distribution](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution), [Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow), [Updating Mac software](https://developer.apple.com/documentation/security/updating-mac-software)
- [Apple Developer Program](https://developer.apple.com/programs/whats-included/) (99 USD per year)

GitHub:
- [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
- [Artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations), [using them](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/use-artifact-attestations)
- [Immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases), [Release assets REST API](https://docs.github.com/en/rest/releases/assets)
- [`gh attestation verify`](https://cli.github.com/manual/gh_attestation_verify), [`gh release verify-asset`](https://cli.github.com/manual/gh_release_verify-asset)

Local measurements (this document, Bun v1.4.4 downloaded to a scratch directory and verified against `SHASUMS256.txt`): cross-compile sizes, `objdump -T` glibc symbols, Mach-O signature parse, baseline-vs-default byte comparison, run of the Linux binary on an x86-64 WSL2 host. Not run: any macOS execution, `codesign`, notarization.
