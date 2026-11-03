#!/usr/bin/env bash
exec docker compose -p rs27l -f "$(dirname "$0")/mini/compose.yaml" "$@"
