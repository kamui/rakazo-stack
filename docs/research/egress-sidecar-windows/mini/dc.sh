#!/usr/bin/env bash
SANDBOX_SUPERVISOR_TOKEN="$(cat "${RS27_TOKEN_FILE:-/tmp/rs27/token}")" \
  exec docker compose -p rs27-mini -f "$(dirname "$0")/compose.yaml" "$@"
