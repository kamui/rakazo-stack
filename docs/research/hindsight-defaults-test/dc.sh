#!/usr/bin/env bash
# dc.sh <compose args...>: docker compose for the rs45 project. The OpenRouter key is read from
# $OR_KEY_FILE into this process's environment only, and stripped from everything printed.
set -uo pipefail
cd "$(dirname "$0")"
: "${OR_KEY_FILE:?}"
OPENROUTER_API_KEY="$(cat "$OR_KEY_FILE")" docker compose -p rs45 --env-file .env \
  -f rakazo.v0.16.0.images.yml -f compose.override.yml "$@" 2>&1 | sed "s|$(cat "$OR_KEY_FILE")|[redacted]|g"
exit "${PIPESTATUS[0]}"
