#!/usr/bin/env bash
# Builds the computer-egress image and tests it inside a throwaway Docker-in-Docker engine,
# once on the nftables-backed iptables and once on legacy iptables. The rules land in that
# engine's network namespace, never in the namespace of the Docker that runs this script.
# In WSL, run it with DOCKER=docker.exe. RAKAZO_TAG picks the Rakazo tag whose script is
# built in (default v0.16.0).
set -euo pipefail
cd "$(dirname "$0")/.."

docker=${DOCKER:-docker}
tag=${RAKAZO_TAG:-v0.16.0}
dind_image=docker:29.9.2-dind
name=rkst-egress-test-$$
context=$(mktemp -d)
image=computer-egress

cleanup() {
  $docker rm -f -v "$name" >/dev/null 2>&1 || true
  rm -rf "$context"
}
trap cleanup EXIT

fail() {
  echo "FAIL ($mode): $1" >&2
  $docker exec "$name" docker logs sidecar >&2 2>&1 || true
  exit 1
}

# Runs a command in the inner engine's host namespace.
inner() {
  $docker exec "$name" "$@" | tr -d '\r'
}

# "open", "dropped" when the connection timed out, or "refused". Busybox's timeout exits 143.
connect() {
  local network=$1 address=$2 port=$3 code
  code=$(inner docker run --rm --network "$network" --entrypoint bash $image -c \
    "timeout 3 bash -c 'exec 3<>/dev/tcp/$address/$port'; echo \$?" 2>/dev/null)
  case $code in
    0) echo open ;;
    124 | 143) echo dropped ;;
    *) echo refused ;;
  esac
}

rules() {
  inner sh -c "$1-save | grep -v '^[#:]' | grep -v '^COMMIT' | sed 's/\[[0-9]*:[0-9]*\]//'"
}

curl --silent --show-error --fail --location --output "$context/restrict-computer-egress.sh" \
  "https://raw.githubusercontent.com/elie222/rakazo/$tag/infra/compose/restrict-computer-egress.sh"
cp Dockerfile computer-egress.sh "$context/"

for mode in nftables legacy; do
  legacy_env=()
  iptables=iptables
  if [ "$mode" = legacy ]; then
    legacy_env=(--env DOCKER_IPTABLES_LEGACY=1)
    iptables="iptables-legacy"
  fi
  $docker rm -f -v "$name" >/dev/null 2>&1 || true
  $docker run --detach --privileged --name "$name" "${legacy_env[@]}" "$dind_image" \
    --host=unix:///var/run/docker.sock >/dev/null
  for _ in $(seq 60); do
    if $docker exec "$name" docker info >/dev/null 2>&1; then break; fi
    sleep 1
  done
  inner "$iptables" -L DOCKER-USER -n >/dev/null || fail "the inner engine has no DOCKER-USER chain in $iptables"

  tar -C "$context" -c . | $docker exec -i "$name" docker build --quiet --tag $image - >/dev/null

  inner docker run --rm --entrypoint cat $image /usr/local/lib/rakazo/restrict-computer-egress.sh \
    >"$context/in-image.sh"
  cmp -s "$context/in-image.sh" "$context/restrict-computer-egress.sh" ||
    fail "the script in the image differs from Rakazo's at $tag"

  inner docker network create --opt com.docker.network.bridge.name=rakazo-ctest computer >/dev/null
  inner docker network create plain >/dev/null
  inner docker run --detach --name listener --network host --entrypoint sh $image -c 'while true; do nc -l -p 8080 </dev/null; done' >/dev/null
  gateway() {
    inner docker network inspect "$1" --format '{{(index .IPAM.Config 0).Gateway}}'
  }
  computer_gateway=$(gateway computer)
  plain_gateway=$(gateway plain)

  [ "$(connect computer "$computer_gateway" 8080)" = open ] || fail "the host isn't reachable before the rules"
  before=$(rules "$iptables")

  inner docker run --detach --name sidecar --network host --cap-add NET_ADMIN $image >/dev/null
  for _ in $(seq 30); do
    if inner docker logs sidecar 2>&1 | grep -q 'computer egress rules applied'; then break; fi
    sleep 1
  done
  logs=$(inner docker logs sidecar 2>&1)
  [[ $logs == *"computer egress rules applied with $iptables"* ]] || fail "the sidecar didn't apply its rules with $iptables"

  added=$(diff <(echo "$before") <(rules "$iptables") | grep '^>' || true)
  removed=$(diff <(echo "$before") <(rules "$iptables") | grep '^<' || true)
  [ -n "$added" ] || fail "no rules were added"
  [ -z "$removed" ] || fail "rules were removed: $removed"
  if echo "$added" | grep -v -- '-i rakazo-c+'; then
    fail "a rule doesn't match only rakazo-c+ bridges"
  fi

  [ "$(connect computer 1.1.1.1 443)" = open ] || fail "a computer bridge can't reach the internet"
  [ "$(connect computer "$computer_gateway" 8080)" = dropped ] || fail "a computer bridge reaches the host"
  [ "$(connect computer "$plain_gateway" 8080)" = dropped ] || fail "a computer bridge reaches the host's other address"
  [ "$(connect plain "$plain_gateway" 8080)" = open ] || fail "another bridge lost the host"
  [ "$(connect plain 1.1.1.1 443)" = open ] || fail "another bridge lost the internet"

  inner docker exec sidecar "$iptables" -I DOCKER-USER 1 -j RETURN
  inner docker restart sidecar >/dev/null
  for _ in $(seq 30); do
    if [ "$(inner docker exec sidecar "$iptables" -S DOCKER-USER | sed -n 2p)" = "-A DOCKER-USER -i rakazo-c+ -o rakazo-c+ -j RETURN" ]; then break; fi
    sleep 1
  done
  [ "$(inner docker exec sidecar "$iptables" -S DOCKER-USER | sed -n 2p)" = "-A DOCKER-USER -i rakazo-c+ -o rakazo-c+ -j RETURN" ] ||
    fail "a restart didn't put the rules back above a rule inserted at the head"
  inner docker exec sidecar "$iptables" -D DOCKER-USER -j RETURN

  inner docker stop sidecar >/dev/null
  exit_code=$(inner docker inspect --format '{{.State.ExitCode}}' sidecar)
  [ "$exit_code" = 0 ] || fail "docker stop ended the sidecar with $exit_code"
  [ "$(connect computer "$computer_gateway" 8080)" = dropped ] || fail "stopping the sidecar removed its rules"

  inner docker run --rm --network host --cap-add NET_ADMIN $image --remove >/dev/null
  [ "$(rules "$iptables")" = "$before" ] || fail "--remove left rules behind"
  [ "$(connect computer "$computer_gateway" 8080)" = open ] || fail "--remove didn't open the host again"
  echo "PASS ($mode)"
done

echo "PASS: the image passed"
