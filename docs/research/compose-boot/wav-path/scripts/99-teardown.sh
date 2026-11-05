#!/usr/bin/env bash
# Removes only what this test created; pulled upstream images (lemonade-server, oven/bun) stay.
set -uo pipefail
docker rm -f -v rkz-wavtest-lemonade
for v in models llama data config; do docker volume rm rkz-wavtest-$v; done
docker network rm rkz-wavtest-net
docker rmi rkz-wavtest-ffmpeg:local
docker rmi f20e2babe163 2>/dev/null || true  # first, superseded -q build of the same Dockerfile
echo "--- leftovers named rkz-wavtest:"
docker ps -a --format '{{.Names}}' | grep rkz-wavtest
docker volume ls -q | grep rkz-wavtest
docker network ls --format '{{.Name}}' | grep rkz-wavtest
docker images --format '{{.Repository}}:{{.Tag}}' | grep rkz-wavtest
echo "--- (none above = clean)"
