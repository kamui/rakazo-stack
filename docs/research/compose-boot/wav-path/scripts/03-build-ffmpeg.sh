#!/usr/bin/env bash
# Throwaway image matching the planned relay: oven/bun alpine + Alpine's ffmpeg package.
set -euo pipefail
docker build -q -t rkz-wavtest-ffmpeg:local - <<'DOCKERFILE'
FROM oven/bun:1.4.4-alpine
RUN apk add --no-cache ffmpeg
DOCKERFILE
docker run --rm rkz-wavtest-ffmpeg:local sh -c 'cat /etc/alpine-release; bun --version; apk list --installed ffmpeg; ffmpeg -hide_banner -version | head -1'
