#!/usr/bin/env bash
# Runs Rakazo's restrict-computer-egress.sh, unmodified, against whichever iptables backend
# holds Docker's DOCKER-USER chain (spec §12). With no arguments it applies the rules every
# 60 seconds, because a firewall reload or a Docker restart can drop them. With arguments
# (`--print`, `--remove`) it runs the script once with them.
set -uo pipefail

script=/usr/local/lib/rakazo/restrict-computer-egress.sh

choose_backend() {
  if iptables-legacy -L DOCKER-USER -n >/dev/null 2>&1 && ! iptables -L DOCKER-USER -n >/dev/null 2>&1; then
    export RAKAZO_IPTABLES=iptables-legacy RAKAZO_IP6TABLES=ip6tables-legacy
  else
    unset RAKAZO_IPTABLES RAKAZO_IP6TABLES
  fi
}

if (($# > 0)); then
  choose_backend
  exec bash "$script" "$@"
fi

trap 'exit 0' TERM INT
applied=false
while true; do
  choose_backend
  if bash "$script" --apply; then
    if ! $applied; then
      echo "computer egress rules applied with ${RAKAZO_IPTABLES:-iptables}"
      applied=true
    fi
  else
    echo "computer egress rules could not be applied; trying again in 60 s" >&2
    applied=false
  fi
  sleep 60 &
  wait $!
done
