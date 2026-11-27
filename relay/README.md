# Lemonade relay

The relay between Rakazo and Lemonade ([spec §9](../docs/spec.md#the-lemonade-relay)). It is TypeScript on Bun and ships as its own container image. It listens on `13306` and takes `UPSTREAM` and `LEMONADE_API_KEY` from its environment.

## Tests

Both need Bun and ffmpeg. From this folder:

- `bun test` runs the relay against a stub upstream.
- `test/image.sh` builds the image and runs the same tests against its container. In WSL: `DOCKER=docker.exe test/image.sh`.
