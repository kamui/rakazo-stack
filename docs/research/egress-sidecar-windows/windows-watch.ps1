# Windows-side watcher for the survival tests that also stop the WSL distro running the test session.
# -Mode WslShutdown: waits, runs `wsl --shutdown`, then records recovery.
# -Mode Boot: started at logon from the Startup folder after a reboot; records recovery.
# Logs to $env:TEMP\rs27-watch-<Mode>.log with UTC times. Uses Windows' own docker.exe, never the WSL distro.
param([ValidateSet('WslShutdown', 'Boot')][string]$Mode = 'WslShutdown', [int]$DelaySeconds = 60)

$log = Join-Path $env:TEMP "rs27-watch-$Mode.log"
$docker = 'C:\Program Files\Docker\Docker\resources\bin\docker.exe'
function Log($msg) { "{0:HH:mm:ss} {1}" -f (Get-Date).ToUniversalTime(), $msg | Add-Content -Path $log -Encoding utf8 }
function Distros { ((wsl.exe -l -v) -replace "`0", '' | Where-Object { $_ -match '\S' } | Select-Object -Skip 1 | ForEach-Object { ($_ -replace '\s+', ' ').Trim() }) -join '; ' }
function Q($argsList) { (& $docker @argsList 2>&1 | Out-String).Trim() }

Set-Content -Path $log -Value "# rs27 watcher, mode $Mode" -Encoding utf8
if ($Mode -eq 'WslShutdown') {
  Log "waiting $DelaySeconds s before wsl --shutdown"
  Start-Sleep -Seconds $DelaySeconds
  Log "distros before: $(Distros)"
  Log 'wsl --shutdown'
  wsl.exe --shutdown
  Log "wsl --shutdown returned; distros: $(Distros)"
} else {
  Log "logon; uptime since $((Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('HH:mm:ss')); distros: $(Distros)"
}

$deadline = (Get-Date).AddMinutes(15)
$up = $false
while ((Get-Date) -lt $deadline) {
  $v = Q @('version', '--format', '{{.Server.Version}}')
  if ($LASTEXITCODE -eq 0) { $up = $true; break }
  Start-Sleep -Seconds 3
}
if (-not $up) { Log "engine not answering after 15 min; last: $v"; Log "desktop status: $(Q @('desktop','status'))"; Log "distros: $(Distros)"; exit 1 }
Log "engine up: $v; distros: $(Distros)"

$names = 'rs27-canary', 'rs27-mini-computer-egress-1', 'rs27-mini-supervisor-1', 'rs27-mini-relay-standin-1', 'rs27-mini-web-1', 'rakazo-bot-rs27bot', 'rakazo-supervisor-1', 'rakazo-web-1', 'lemonade-server'
# No double quotes: Windows PowerShell 5.1 drops them from arguments passed to native programs.
$count = 'apk add -q --no-cache iptables ip6tables >/dev/null && printf ''v4 %s (INPUT %s), v6 %s\n'' $(iptables-save | grep -c rakazo-c+) $(iptables -S INPUT | grep -c rakazo-c+) $(ip6tables-save | grep -c rakazo-c+)'
$deadline = (Get-Date).AddMinutes(5)
$rules = ''
while ((Get-Date) -lt $deadline) {
  $rules = Q @('run', '--rm', '--network', 'host', '--cap-add', 'NET_ADMIN', 'alpine:3', 'sh', '-c', $count)
  Log "rules: $rules"
  if ($rules -match '^v4 14') { break }
  Start-Sleep -Seconds 5
}
foreach ($n in $names) { Log ("{0,-30} {1}" -f $n, (Q @('inspect', '-f', '{{.State.Status}} restarts={{.RestartCount}} started={{.State.StartedAt}} exit={{.State.ExitCode}} err={{.State.Error}}', $n))) }
Log "sidecar log tail:`n$(Q @('logs', '--tail', '5', 'rs27-mini-computer-egress-1'))"
Start-Sleep -Seconds 15
$sup = (& $docker logs rs27-mini-supervisor-1 2>&1 | Select-String -Pattern 'restricted computer egress|supervisor listening' | Select-Object -Last 4 | ForEach-Object { $_.Line.Substring(0, [Math]::Min(200, $_.Line.Length)) }) -join "`n"
Log "supervisor egress lines (last 4):`n$sup"
Log "distros at end: $(Distros)"
Log 'done'
