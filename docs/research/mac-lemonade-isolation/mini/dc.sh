#!/usr/bin/env bash
SANDBOX_SUPERVISOR_TOKEN="$(cat "${RS25_TOKEN_FILE:-/tmp/rs25exp/token}")" \
  exec docker compose -p rs25-mini -f "$(dirname "$0")/compose.yaml" "$@"
