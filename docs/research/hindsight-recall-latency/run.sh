#!/usr/bin/env bash
# Run a python script from this dir in a throwaway client container on hsr-net, pinned away from the CPUs under test.
set -euo pipefail
docker run --rm --name "hsr-client-$$" --network hsr-net --cpuset-cpus "${CLIENT_CPUSET:-22,23}" \
  -v "$(dirname "$(readlink -f "$0")")":/bench:ro --entrypoint python \
  ghcr.io/vectorize-io/hindsight-api:1.3.0 -I "/bench/$1" "${@:2}"
