#!/usr/bin/env bash
# Reproduce the /dev/shm failure in a separate pgvector Postgres for Hindsight 1.3.0, then show
# that shm_size on the database container fixes it. No model provider key; nothing is spent.
# About 15 minutes: most of it is seeding 15,000 memories so memory_units passes the 8 MB at
# which Postgres starts building indexes in parallel. Uses containers, a volume and a network
# named hsr48-*, and removes them at the end.
set -euo pipefail
cd "$(dirname "$0")"
HERE=$(pwd)
IMG=ghcr.io/vectorize-io/hindsight-api:1.3.0
PG=pgvector/pgvector:pg18
NET=hsr48-net
trap 'docker rm -f hsr48-api hsr48-db >/dev/null 2>&1; docker volume rm hsr48-pgdata >/dev/null 2>&1; docker network rm $NET >/dev/null 2>&1' EXIT

client() { docker run --rm --network $NET -v "$HERE":/r:ro --entrypoint python "$IMG" -I "/r/$1" "${@:2}"; }

up() { # $1 = db shm size, empty for Docker's default
  docker network inspect $NET >/dev/null 2>&1 || docker network create --internal $NET >/dev/null
  docker rm -f hsr48-db hsr48-api >/dev/null 2>&1 || true
  docker run -d --name hsr48-db --network $NET ${1:+--shm-size "$1"} -e POSTGRES_USER=hindsight \
    -e POSTGRES_PASSWORD=repropw -e POSTGRES_DB=hindsight -v hsr48-pgdata:/var/lib/postgresql $PG >/dev/null
  until docker exec hsr48-db pg_isready -U hindsight -q; do sleep 1; done
  docker run -d --name hsr48-api --network $NET \
    -e HINDSIGHT_API_DATABASE_URL=postgresql://hindsight:repropw@hsr48-db:5432/hindsight \
    -e HINDSIGHT_API_LLM_PROVIDER=openai -e HINDSIGHT_API_LLM_API_KEY=dummy-not-a-key \
    -e HINDSIGHT_API_SKIP_LLM_VERIFICATION=true -e HINDSIGHT_API_ENABLE_AUTO_CONSOLIDATION=false "$IMG" >/dev/null
  until docker exec hsr48-api python -c "import urllib.request;urllib.request.urlopen('http://localhost:8888/health',timeout=2)" >/dev/null 2>&1; do sleep 2; done
  echo "db /dev/shm: $(docker exec hsr48-db df -h /dev/shm | tail -1)"
}

trials() { # $1 = tag
  echo "-- one new bank"; client newbank.py http://hsr48-api:8888 "$1-one" | cut -c1-150
  echo "-- two new banks at once"
  client newbank.py http://hsr48-api:8888 "$1-a" > "/tmp/hsr48-$1-a.txt" 2>&1 &
  client newbank.py http://hsr48-api:8888 "$1-b" | cut -c1-150; wait; cut -c1-150 "/tmp/hsr48-$1-a.txt"; rm -f "/tmp/hsr48-$1-a.txt"
  echo "-- the same index build by hand"
  docker exec hsr48-db psql -U hindsight -c "SET client_min_messages=debug1; CREATE INDEX hsr48_t ON memory_units USING hnsw (embedding vector_cosine_ops) WHERE fact_type='world' AND bank_id='none';" 2>&1 | grep -E 'parallel|serially|ERROR' || true
  docker exec hsr48-db psql -U hindsight -qc "DROP INDEX IF EXISTS hsr48_t"
  echo "-- db /dev/shm now: $(docker exec hsr48-db df -k /dev/shm | tail -1)"
}

echo "== Docker default /dev/shm (64 MB)"
up ""
docker exec hsr48-db psql -U hindsight -tAc "select 'postgres ' || version()" | cut -c1-40
docker exec hsr48-db psql -U hindsight -tAc "select 'pgvector ' || extversion from pg_extension where extname='vector'"
client seed.py http://hsr48-api:8888 bulk 15000 250 | tail -1
docker exec hsr48-db psql -U hindsight -tAc "select 'memory_units: ' || count(*) || ' rows, ' || pg_size_pretty(pg_relation_size('memory_units')) from memory_units"
trials shm64m
echo "== shm_size 1g on the database"
up 1g
trials shm1g
