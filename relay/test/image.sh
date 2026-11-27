#!/usr/bin/env bash
# Builds the relay image and runs the test suite against it, with the stub upstream in a second
# container. Needs bun and ffmpeg. In WSL, run it with DOCKER=docker.exe.
set -euo pipefail
cd "$(dirname "$0")/.."

docker=${DOCKER:-docker}
name=rkst-relay-test-$$
key=placeholder-lemonade-key

cleanup() {
  $docker rm -f -v "$name-relay" "$name-stub" >/dev/null 2>&1 || true
  $docker network rm "$name" >/dev/null 2>&1 || true
  $docker image rm "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
  echo "FAIL: $1" >&2
  $docker logs "$name-relay" >&2 || true
  exit 1
}

url() {
  local address
  address=$($docker port "$name-$1" "$2" | tr -d '\r')
  echo "http://$address"
}

wait_for() {
  curl --silent --fail --output /dev/null --retry 30 --retry-delay 1 --retry-all-errors "$1"
}

$docker build --tag "$name" .
$docker network create "$name" >/dev/null
$docker create --name "$name-stub" --network "$name" --network-alias upstream \
  --publish 127.0.0.1::13305 "$name" bun /tmp/stub.ts >/dev/null
$docker cp test/stub.ts "$name-stub:/tmp/stub.ts"
$docker start "$name-stub" >/dev/null
$docker run --detach --name "$name-relay" --network "$name" --publish 127.0.0.1::13306 \
  --env UPSTREAM=http://upstream:13305 --env "LEMONADE_API_KEY=$key" "$name" >/dev/null

relay_url=$(url relay 13306)
stub_url=$(url stub 13305)
wait_for "$stub_url/__stub/requests"
wait_for "$relay_url/api/v1/models"

user=$($docker exec "$name-relay" id -un | tr -d '\r')
[ "$user" = bun ] || fail "the relay runs as $user, not bun"
mounts=$($docker inspect --format '{{len .Mounts}}' "$name-relay" | tr -d '\r')
[ "$mounts" = 0 ] || fail "the relay has $mounts mounts"
$docker exec "$name-relay" ffmpeg -version | sed -n 1p

RELAY_URL=$relay_url STUB_URL=$stub_url bun test

$docker stop "$name-stub" >/dev/null
status=$(curl --silent --output /dev/null --write-out '%{http_code}' "$relay_url/api/v1/models")
[ "$status" = 502 ] || fail "a stopped upstream gave $status, not 502"

logs=$($docker logs "$name-relay" 2>&1)
[[ $logs == *"upstream failed"* && $logs == *"ffmpeg exited"* ]] || fail "the error paths weren't logged"
[[ $logs != *"$key"* ]] || fail "the key is in the relay's logs"

$docker stop "$name-relay" >/dev/null
exit_code=$($docker inspect --format '{{.State.ExitCode}}' "$name-relay" | tr -d '\r')
[ "$exit_code" = 0 ] || fail "docker stop ended the relay with $exit_code"

echo "PASS: the image passed"
