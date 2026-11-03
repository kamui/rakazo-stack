#!/usr/bin/env bash
# Starts or stops a stand-in HTTP listener on Windows' own 127.0.0.1 (not the WSL distro's),
# using Windows' python.exe through WSL interop. The PID is kept in $RS27_LISTENER_PID_FILE.
# Usage: win-listener.sh <start|stop> [port]
set -euo pipefail
PID_FILE=${RS27_LISTENER_PID_FILE:-$HOME/rs27work/win-listener.pid}
PORT=${2:-18927}
cd /mnt/c
case $1 in
  start)
    powershell.exe -NoProfile -Command "
      \$d = Join-Path \$env:TEMP 'rs27www'; New-Item -ItemType Directory -Force \$d | Out-Null
      Set-Content (Join-Path \$d 'index.html') 'ok'
      (Start-Process python.exe -ArgumentList '-m','http.server','--bind','127.0.0.1','$PORT','--directory',\$d -WindowStyle Hidden -PassThru).Id" |
      tr -d '\r' >"$PID_FILE"
    echo "listener on Windows 127.0.0.1:$PORT"
    ;;
  stop)
    powershell.exe -NoProfile -Command "
      Stop-Process -Id $(cat "$PID_FILE") -ErrorAction SilentlyContinue
      Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path \$env:TEMP 'rs27www')"
    rm -f "$PID_FILE"
    ;;
esac
