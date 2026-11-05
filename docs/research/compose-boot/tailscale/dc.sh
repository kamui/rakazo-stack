#!/usr/bin/env bash
# docker compose for the throwaway stack; -p is explicit because upstream's file sets `name: rakazo`.
set -euo pipefail
cd "$(dirname "$0")/stack"
exec docker compose -p rkz-tstest "$@"
