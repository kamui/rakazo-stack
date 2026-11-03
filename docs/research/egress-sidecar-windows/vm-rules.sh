#!/usr/bin/env bash
# Runs the unmodified restrict-computer-egress.sh, or a count of its rules, inside Docker Desktop's
# engine namespace from a one-off host-network NET_ADMIN container.
# Usage: vm-rules.sh <count|remove|apply|list> [path-to-restrict-computer-egress.sh]
set -euo pipefail
SCRIPT=${2:-$HOME/rs27work/mini/restrict-computer-egress.sh}
case $1 in
  count) cmd='echo "rakazo-c+ rules: v4 $(iptables-save | grep -c "rakazo-c+") (INPUT $(iptables -S INPUT | grep -c "rakazo-c+")), v6 $(ip6tables-save | grep -c "rakazo-c+")"' ;;
  list) cmd='iptables -S INPUT | grep rakazo-c; iptables -S DOCKER-USER; ip6tables -S DOCKER-USER | grep rakazo-c' ;;
  remove) cmd='bash /egress.sh --remove' ;;
  apply) cmd='bash /egress.sh --apply && echo applied' ;;
  *) echo "usage: $0 <count|remove|apply|list>" >&2; exit 2 ;;
esac
docker run --rm --network host --cap-add NET_ADMIN --cap-add NET_RAW \
  -v "$SCRIPT:/egress.sh:ro" alpine:3 sh -c "apk add -q --no-cache bash iptables ip6tables >/dev/null && $cmd"
