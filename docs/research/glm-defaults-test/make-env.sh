#!/usr/bin/env bash
# Writes .env (Rakazo + Honcho secrets) and honcho.env (the nine GLM slots). Secrets are never printed.
# Usage: make-env.sh <dir> <openrouter-key-file>
set -euo pipefail
dir=$1 key_file=$2
umask 077
hex() { openssl rand -hex "$1"; }
key=$(cat "$key_file")
model='~z-ai/glm-flash-latest'
api_port=13900 web_port=13901
cat >"$dir/.env" <<EOF
POSTGRES_USER=rakazo
POSTGRES_DB=rakazo
POSTGRES_PASSWORD=$(hex 16)
POSTGRES_APP_PASSWORD=$(hex 16)
BETTER_AUTH_SECRET=$(hex 32)
ENCRYPTION_KEY=$(hex 32)
SCREEN_PROXY_SECRET=$(hex 32)
SANDBOX_SUPERVISOR_TOKEN=$(hex 32)
MASTER_KEY_PROVIDER=local
CONTENT_ENCRYPTION=on
SECRET_STORE=encrypted
RAKAZO_IMAGE=ghcr.io/elie222/rakazo/app
RAKAZO_IMAGE_TAG=v0.4.0
RAKAZO_COMPUTER_IMAGE=ghcr.io/elie222/rakazo/computer
RAKAZO_COMPUTER_IMAGE_TAG=v0.4.0
RAKAZO_API_PORT=$api_port
RAKAZO_WEB_PORT=$web_port
RAKAZO_WEB_BIND=127.0.0.1
BETTER_AUTH_URL=http://127.0.0.1:$web_port
WEB_ORIGIN=http://127.0.0.1:$web_port
API_URL=http://127.0.0.1:$web_port
RAKAZO_HOST=localhost
SIGNUPS_ENABLED=false
SANDBOX_PROVIDER=none
MCP_ALLOW_PRIVATE_ENDPOINT=true
LOG_LEVEL=info
PI_DEFAULT_PROVIDER=openrouter
PI_DEFAULT_MODEL=$model
OPENROUTER_API_KEY=$key
HONCHO_DB_PASSWORD=$(hex 16)
HONCHO_JWT_SECRET=$(hex 32)
OWNER_EMAIL=owner@example.com
OWNER_PASSWORD=$(hex 16)
EOF
extra='{"extra_body":{"provider":{"zdr":true,"data_collection":"deny","require_parameters":true}}}'
{
  echo "LLM_OPENAI_API_KEY=$key"
  echo "LLM_OPENAI_BASE_URL=http://or-proxy:8080/api/v1"
  echo "EMBEDDING_MODEL_CONFIG__TRANSPORT=openai"
  echo "EMBEDDING_MODEL_CONFIG__MODEL=openai/text-embedding-3-small"
  echo "EMBEDDING_MODEL_CONFIG__OVERRIDES__BASE_URL=http://or-proxy:8080/api/v1"
  for slot in DERIVER_MODEL_CONFIG SUMMARY_MODEL_CONFIG \
    DIALECTIC_LEVELS__minimal__MODEL_CONFIG DIALECTIC_LEVELS__low__MODEL_CONFIG \
    DIALECTIC_LEVELS__medium__MODEL_CONFIG DIALECTIC_LEVELS__high__MODEL_CONFIG \
    DIALECTIC_LEVELS__max__MODEL_CONFIG DREAM_DEDUCTION_MODEL_CONFIG DREAM_INDUCTION_MODEL_CONFIG; do
    echo "${slot}__TRANSPORT=openai"
    echo "${slot}__MODEL=$model"
    echo "${slot}__THINKING_EFFORT=low"
    echo "${slot}__OVERRIDES__PROVIDER_PARAMS=$extra"
  done
} >"$dir/honcho.env"
