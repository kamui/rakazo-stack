# Install the latest stable upstream releases, not a tested pinned set

rakazo-stack is a light installer: the first `setup` installs the newest stable release of Rakazo, Lemonade, Hindsight and Tailscale and records their versions and image digests, and only `rakazo-stack update` moves them ([#32](https://github.com/kamui/rakazo-stack/issues/32)). We chose this over shipping a pinned, tested set of versions in each program release, because Rakazo, Lemonade and Hindsight each release every week or so ([#31](https://github.com/kamui/rakazo-stack/issues/31), [#44](https://github.com/kamui/rakazo-stack/issues/44)), and keeping a pinned set current would need someone testing every combination. The cost is that nobody tests a new upstream release with rakazo-stack before a Self-hoster gets it.

## Consequences

- Every Update takes a Backup first when anything it holds changes, and offers to roll back from it on failure. Rollback is always a restore, because Rakazo's and Hindsight's migrations only go forward.
- Holds (`update --hold`) are the workaround when an upstream release breaks the stack, and each program release carries a floor for each Component instead of a pin.
- The weekly CI canary (a fresh install plus an Update from the last passing versions) is how the project finds upstream breakage.
- Rakazo's compose file and egress script are fetched from the resolved tag at run time, not bundled. A Rakazo release without published images is skipped rather than built locally ([#45](https://github.com/kamui/rakazo-stack/issues/45)).
- Images run by recorded digest, never by a moving tag.
