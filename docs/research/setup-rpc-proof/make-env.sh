#!/usr/bin/env bash
# Writes a throwaway env file for the rs-rpc-proof stack. Secrets are random and never printed.
# Usage: make-env.sh <env-file> <api-port> <web-port>
set -euo pipefail
env_file=$1 api_port=$2 web_port=$3
umask 077
hex() { openssl rand -hex "$1"; }
cat >"$env_file" <<EOF
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
HONCHO_DB_PASSWORD=$(hex 16)
HONCHO_JWT_SECRET=$(hex 32)
OWNER_EMAIL=owner@example.com
OWNER_PASSWORD=$(hex 16)
EOF
