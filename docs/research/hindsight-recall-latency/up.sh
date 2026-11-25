#!/usr/bin/env bash
# (Re)create hsr-db and hsr-api on an internal network. Data survives in volume hsr-pgdata.
# Env: SHM_DB SHM_API (e.g. 1g; empty = Docker default), CPUS (e.g. 4), CPUSET (e.g. 16-19),
#      API_ENV (extra docker-run flags for the api), KEEP_DB=1 to leave the db container alone.
set -euo pipefail
NET=hsr-net
docker network inspect $NET >/dev/null 2>&1 || docker network create --internal $NET >/dev/null
limits=()
[ -n "${CPUS:-}" ] && limits+=(--cpus "$CPUS")
[ -n "${CPUSET:-}" ] && limits+=(--cpuset-cpus "$CPUSET")
if [ -z "${KEEP_DB:-}" ]; then
  docker rm -f hsr-db >/dev/null 2>&1 || true
  docker run -d --name hsr-db --network $NET ${SHM_DB:+--shm-size "$SHM_DB"} "${limits[@]}" \
    -e POSTGRES_USER=hindsight -e POSTGRES_PASSWORD=benchpw -e POSTGRES_DB=hindsight \
    -v hsr-pgdata:/var/lib/postgresql pgvector/pgvector:pg18 >/dev/null
  until docker exec hsr-db pg_isready -U hindsight -q; do sleep 1; done
fi
docker rm -f hsr-api >/dev/null 2>&1 || true
start=$(date +%s)
# shellcheck disable=SC2086
docker run -d --name hsr-api --network $NET ${SHM_API:+--shm-size "$SHM_API"} "${limits[@]}" \
  -e HINDSIGHT_API_DATABASE_URL=postgresql://hindsight:benchpw@hsr-db:5432/hindsight \
  -e HINDSIGHT_API_LLM_PROVIDER=openai -e HINDSIGHT_API_LLM_API_KEY=dummy-not-a-key \
  -e HINDSIGHT_API_SKIP_LLM_VERIFICATION=true -e HINDSIGHT_API_WORKER_ID=hindsight \
  ${API_ENV:-} ghcr.io/vectorize-io/hindsight-api:1.3.0 >/dev/null
until docker exec hsr-api python -c "import urllib.request;urllib.request.urlopen('http://localhost:8888/health',timeout=2)" >/dev/null 2>&1; do
  sleep 2
  if [ "$(docker inspect -f '{{.State.Running}}' hsr-api)" != true ]; then docker logs --tail 40 hsr-api; exit 1; fi
done
echo "api ready after $(( $(date +%s) - start ))s"
for c in hsr-db hsr-api; do
  docker exec -d "$c" sh -c 'rm -f /tmp/shm.log; while :; do df -k /dev/shm | tail -1 >> /tmp/shm.log; sleep 0.2; done'
done
