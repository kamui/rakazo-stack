#!/bin/sh
# Reproduce the kamui/rakazo-stack#12 checks. Usage: ./run.sh nomic|qwen [long-message-tokens] [max-input-tokens]
# Creates a throwaway compose project `rs-embed-test`; remove it with:
#   docker compose -p rs-embed-test down -v --remove-orphans
set -eu
cd "$(dirname "$0")"

case "${1:-nomic}" in
  nomic) model=nomic-embed-text-v1-GGUF; dims=768; batch=2048 ;;
  qwen) model=Qwen3-Embedding-0.6B-GGUF; dims=1024; batch=none ;;
  *) echo "usage: $0 nomic|qwen [long-message-tokens] [max-input-tokens]" >&2; exit 2 ;;
esac

env_file=$(mktemp)
trap 'rm -f "$env_file"' EXIT
key_file=.lemonade-test-key
[ -f "$key_file" ] || (umask 077 && openssl rand -hex 24 > "$key_file")
printf 'LEMONADE_API_KEY=%s\nEMBED_MODEL=%s\nEMBED_DIMS=%s\nEMBED_MAX_INPUT_TOKENS=%s\n' \
  "$(cat "$key_file")" "$model" "$dims" "${3:-8192}" > "$env_file"
dc() { docker compose -p rs-embed-test --env-file "$env_file" "$@"; }

dc up -d lemonade tiktoken-cache
sleep 5
dc run --rm -T tester /harness/probe_lemonade.py catalog
dc run --rm -T tester /harness/probe_lemonade.py pull "$model"
# nomic-embed-text-v1 is a non-causal (BERT-style) model: llama-server rejects any input longer
# than its physical batch (-ub, default 512) unless it is raised to the model's 2048-token context.
dc run --rm -T tester /harness/probe_lemonade.py set-batch "$model" "$batch"
dc run --rm -T tester /harness/probe_lemonade.py shape "$model"

# Fresh Honcho database per model: the vector column width is fixed by the one-time setup step.
dc rm -sfv honcho honcho-deriver honcho-setup honcho-db >/dev/null 2>&1 || true
docker volume rm rs-embed-test_honcho-pgdata >/dev/null 2>&1 || true
dc up -d --wait honcho honcho-deriver
dc logs honcho-setup | tail -n 15
dc run --rm -T tester /harness/honcho_e2e.py "${2:-3000}"
