#!/usr/bin/env bash
# One line: host load and the summed CPU% of containers that are not part of this bench.
echo "LOAD $(cut -d' ' -f1-3 /proc/loadavg) other_containers_cpu=$(docker stats --no-stream --format '{{.Name}} {{.CPUPerc}}' | awk '$1 !~ /^hsr-/ {gsub("%","",$2); s+=$2} END{print s"%"}')"
