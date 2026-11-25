#!/usr/bin/env bash
# Start N CPU-hog containers (4 busy loops each) on $CPUSET, so the bench containers get only a fair share.
N=$1; for i in $(seq 1 "$N"); do
  docker run -d --name "hsr-hog$i" --cpuset-cpus "$CPUSET" --entrypoint sh pgvector/pgvector:pg18 \
    -c 'for i in 1 2 3 4; do (while :; do :; done) & done; wait' >/dev/null
done
