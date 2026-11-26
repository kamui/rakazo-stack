# The program is written in Rust, not TypeScript on Bun

Planning chose one TypeScript program compiled with Bun into a single executable ([#9](https://github.com/kamui/rakazo-stack/issues/9), [#22](https://github.com/kamui/rakazo-stack/issues/22)). Before implementation started, v1 switched to Rust. The program is an installer that runs once on other people's machines, where nobody can debug it, and much of its code is error paths: half-finished runs, retries and rollback. Rust's compiler makes every error and every case explicit, and a native executable removes the risk that the language runtime itself misbehaves: Bun's executable builds broke macOS code signing twice (1.3.12, 1.4.0) ([#22](https://github.com/kamui/rakazo-stack/issues/22)). The executable is also much smaller than Bun's (89–109 MB for a hello world in [#22](https://github.com/kamui/rakazo-stack/issues/22)).

## Consequences

- Changing the program is slower than it would be in TypeScript. That matters because "latest stable, best effort" ([ADR 0002](0002-latest-stable-not-pinned.md)) means it will keep following changes in Rakazo, Hindsight, Lemonade and Docker.
- The macOS executable builds on a `macos-latest` runner instead of being cross-compiled on Linux. The release gate stays the same: `codesign --verify --strict`, then `rakazo-stack --version`.
- The Lemonade relay is separate code that runs in its own container, and it stays TypeScript on a Bun Alpine base ([#17](https://github.com/kamui/rakazo-stack/issues/17)). So the repo has two languages.
- v1 still ships only the glibc Linux executable and refuses Alpine/musl. A static musl build could lift that later.
- The Bun-specific findings in [#22](https://github.com/kamui/rakazo-stack/issues/22) (signing regressions, executable sizes, cross-compiling from one runner) no longer apply. Its delivery findings still apply: no Developer ID or notarization for `curl | sh`, the install script's checks, immutable releases.
