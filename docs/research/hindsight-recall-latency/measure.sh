#!/usr/bin/env bash
# Measure Hindsight 1.3.0 recall latency on this machine. Written for an Apple-silicon Mac with
# Docker Desktop; also runs on Linux and WSL 2. Needs docker, about 8 GB of free disk, and internet
# once (image pulls and the 33 MB FlashRank model). No model provider key; nothing is spent.
#
#   ./measure.sh            # full run, about 45-60 minutes
#   KEEP=1 ./measure.sh     # keep the containers' data volumes for a rerun
#   RUNS=10 ./measure.sh    # fewer recalls per measurement (default 60)
#
# Prints machine facts and one SUM line per measurement; paste the whole output into the ticket.
set -euo pipefail
cd "$(dirname "$0")"
HERE=$(pwd)
IMG=ghcr.io/vectorize-io/hindsight-api:1.3.0
PG=pgvector/pgvector:pg18
NET=hsr-net
RUNS=${RUNS:-60}

echo "== machine"
uname -sm
if sysctl -n machdep.cpu.brand_string >/dev/null 2>&1; then
  sysctl -n machdep.cpu.brand_string hw.perflevel0.physicalcpu hw.perflevel1.physicalcpu hw.memsize 2>/dev/null || true
fi
docker version --format 'docker server {{.Server.Version}} {{.Server.Os}}/{{.Server.Arch}}'
docker info --format 'docker VM: {{.NCPU}} CPUs, {{.MemTotal}} bytes, {{.OperatingSystem}}'
docker pull -q "$IMG" >/dev/null
docker pull -q "$PG" >/dev/null
docker image inspect "$IMG" --format 'image {{.Architecture}} {{index .RepoDigests 0}}'

cleanup() {
  docker rm -f hsr-api hsr-db >/dev/null 2>&1 || true
  if [ -z "${KEEP:-}" ]; then
    docker volume rm hsr-pgdata hsr-flashrank >/dev/null 2>&1 || true
    docker network rm $NET >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

client() {
  docker run --rm --network $NET -v "$HERE":/bench:ro --entrypoint python "$IMG" -I "/bench/$1" "${@:2}"
}

up() { # extra docker-run flags for the api go in $API_FLAGS
  docker network inspect $NET >/dev/null 2>&1 || docker network create --internal $NET >/dev/null
  if ! docker inspect hsr-db >/dev/null 2>&1; then
    docker run -d --name hsr-db --network $NET --shm-size 1g -e POSTGRES_USER=hindsight \
      -e POSTGRES_PASSWORD=benchpw -e POSTGRES_DB=hindsight -v hsr-pgdata:/var/lib/postgresql $PG >/dev/null
    until docker exec hsr-db pg_isready -U hindsight -q; do sleep 1; done
  fi
  docker rm -f hsr-api >/dev/null 2>&1 || true
  local start; start=$(date +%s)
  # shellcheck disable=SC2086
  docker run -d --name hsr-api --network $NET ${CPUS:+--cpus "$CPUS"} \
    -e HINDSIGHT_API_DATABASE_URL=postgresql://hindsight:benchpw@hsr-db:5432/hindsight \
    -e HINDSIGHT_API_LLM_PROVIDER=openai -e HINDSIGHT_API_LLM_API_KEY=dummy-not-a-key \
    -e HINDSIGHT_API_SKIP_LLM_VERIFICATION=true -e HINDSIGHT_API_WORKER_ID=hindsight \
    -e HINDSIGHT_API_ENABLE_AUTO_CONSOLIDATION=false ${API_FLAGS:-} "$IMG" >/dev/null
  until docker exec hsr-api python -c "import urllib.request;urllib.request.urlopen('http://localhost:8888/health',timeout=2)" >/dev/null 2>&1; do
    sleep 2
    [ "$(docker inspect -f '{{.State.Running}}' hsr-api)" = true ] || { docker logs --tail 40 hsr-api; exit 1; }
  done
  echo "api ready after $(( $(date +%s) - start ))s"
}

bench() { client bench.py http://hsr-api:8888 bench "$@" | grep '^SUM'; }

echo "== seed 5000 memories (chunks mode, no LLM)"
up
if [ "$(docker exec hsr-db psql -U hindsight -tAc "select count(*) from memory_units where bank_id='bench'" 2>/dev/null || echo 0)" -lt 5000 ]; then
  client seed.py http://hsr-api:8888 bench 5000 200 | tail -1
fi

for setting in default cand100 flashrank; do
  echo "== reranker: $setting"
  case $setting in
    default) API_FLAGS="" ;;
    cand100) API_FLAGS="-e HINDSIGHT_API_RERANKER_MAX_CANDIDATES=100" ;;
    flashrank)
      docker run --rm -u 0 -v hsr-flashrank:/fr --entrypoint sh "$IMG" -c 'chown 1000:1000 /fr'
      docker run --rm -v hsr-flashrank:/fr --entrypoint python "$IMG" -c \
        "from flashrank import Ranker; Ranker(model_name='ms-marco-MiniLM-L-12-v2', cache_dir='/fr')" >/dev/null 2>&1
      API_FLAGS="-e HINDSIGHT_API_RERANKER_PROVIDER=flashrank -e HINDSIGHT_API_RERANKER_FLASHRANK_CACHE_DIR=/fr -v hsr-flashrank:/fr:ro" ;;
  esac
  API_FLAGS=$API_FLAGS up
  bench mid 1 "$setting-cold"
  bench mid 3 "$setting-warmup" >/dev/null
  for b in low mid high; do bench "$b" "$RUNS" "$setting"; done
  bench mid "$RUNS" "$setting-conc2" 2
  bench mid "$RUNS" "$setting-conc4" 4
  docker logs hsr-api 2>&1 | grep 'phases\]' | tail -2 | cut -c1-120 || true
done
