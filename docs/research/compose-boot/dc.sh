#!/usr/bin/env bash
# docker compose for the throwaway stack. -p is explicit on purpose: upstream's file sets
# `name: rakazo`, which would otherwise address a real install on the same Docker host.
set -euo pipefail
: "${STACK_DIR:?directory holding the prototype files and .env}"
cd "$STACK_DIR"
exec docker compose -p rs-compose-boot "$@"
