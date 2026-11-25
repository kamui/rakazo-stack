#!/usr/bin/env bash
# Print size and peak used KiB of /dev/shm seen by the watcher in each container.
for c in hsr-db hsr-api; do
  docker exec "$c" sh -c 'awk "{if(\$3>m)m=\$3; s=\$2} END{print \"'"$c"' size_kib=\" s \" peak_used_kib=\" m \" samples=\" NR}" /tmp/shm.log' 2>/dev/null || echo "$c: no log"
done
