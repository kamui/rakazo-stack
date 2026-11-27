# Computer-egress sidecar

The sidecar that keeps bot computers off the host, the LAN and the tailnet ([spec §12](../docs/spec.md#the-computer-egress-sidecar)). Setup writes this folder's `Dockerfile` and `computer-egress.sh` into the Instance's `computer-egress/` build folder, next to Rakazo's `restrict-computer-egress.sh` from the recorded tag, which is built in unmodified.

`computer-egress.sh` picks the iptables backend that holds `DOCKER-USER` and applies the rules every 60 seconds. With arguments it runs the script once with them, for example `--print` or `--remove`.

## Tests

`test/image.sh` fetches the script, builds the image and tests it inside a throwaway Docker-in-Docker engine, on the nftables-backed and the legacy iptables, so its rules never reach the engine that runs the test. In WSL: `DOCKER=docker.exe test/image.sh`. `RAKAZO_TAG` picks the Rakazo tag (default `v0.16.0`).
