#!/usr/bin/env bash
# Starts the unmodified upstream Lemonade image as rkz-wavtest-lemonade on 127.0.0.1:23415.
# The API key is generated in memory and handed over by env-var name only (never on argv, never on disk).
set -euo pipefail
IMG=ghcr.io/lemonade-sdk/lemonade-server:v2026.45.3
docker network create rkz-wavtest-net >/dev/null
for v in models llama data config; do docker volume create rkz-wavtest-$v >/dev/null; done
LEMONADE_API_KEY=$(openssl rand -hex 32) docker run -d --name rkz-wavtest-lemonade \
  --network rkz-wavtest-net -p 127.0.0.1:23415:13305 -e LEMONADE_API_KEY \
  -v rkz-wavtest-models:/opt/lemonade/.cache/huggingface \
  -v rkz-wavtest-llama:/opt/lemonade/llama \
  -v rkz-wavtest-data:/opt/lemonade/.cache/lemonade \
  -v rkz-wavtest-config:/opt/lemonade/.config/lemonade \
  "$IMG"
