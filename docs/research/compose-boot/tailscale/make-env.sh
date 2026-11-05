#!/usr/bin/env bash
# Throwaway .env for rkz-tstest. Usage: make-env.sh <env-file> <web-port> <instance-host>
# Secrets are random and never printed; the Tailscale auth key is passed at run time, never written here.
set -euo pipefail
env_file=$1 web_port=$2 host=$3
umask 077
hex() { openssl rand -hex "$1"; }
cat >"$env_file" <<EOT
COMPOSE_FILE=rakazo.v0.4.0.images.yml:compose.stack.yaml
COMPOSE_PROFILES=tailscale
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
RAKAZO_WEB_PORT=$web_port
RAKAZO_WEB_BIND=127.0.0.1
BETTER_AUTH_URL=https://$host
WEB_ORIGIN=https://$host
API_URL=https://$host
RAKAZO_HOST=$host
SIGNUPS_ENABLED=false
SANDBOX_PROVIDER=docker
MCP_ALLOW_PRIVATE_ENDPOINT=true
RAKAZO_OPENAI_COMPAT_PRIVATE_HOSTS=lemonade-relay
LOG_LEVEL=info
LEMONADE_API_KEY=$(hex 32)
HONCHO_DB_PASSWORD=$(hex 16)
HONCHO_AUTH_JWT_SECRET=$(hex 32)
MODEL_PROVIDER_API_KEY=sk-dummy-not-a-real-key
TS_HOSTNAME=rkz-test
OWNER_EMAIL=owner@example.com
OWNER_PASSWORD=$(hex 16)
EOT
