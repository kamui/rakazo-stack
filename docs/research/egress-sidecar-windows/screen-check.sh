#!/usr/bin/env bash
# Opens a computer's screen through the test supervisor (GET /computers/:id/screen), which joins the
# supervisor and the project's "web" service to the computer's rakazo-c* bridge and waits for the
# screen port. Then fetches the screen URL from the supervisor and from the web stand-in.
# The token-bearing URL is never printed; only its host:port shape is.
# Usage: DC=dc.sh screen-check.sh <botId> <spaceId>
set -euo pipefail
D=${DC:?path to dc.sh}
bot=$1 space=$2

url=$("$D" exec -T supervisor node -e '
  const [bot, space] = process.argv.slice(1);
  fetch(`http://127.0.0.1:7091/computers/rakazo-bot-${bot}/screen`, {
    redirect: "manual",
    headers: {
      authorization: `Bearer ${process.env.SANDBOX_SUPERVISOR_TOKEN}`,
      "x-rakazo-bot-id": bot,
      "x-rakazo-space-id": space,
    },
    signal: AbortSignal.timeout(120_000),
  }).then(async (r) => {
    console.error(`GET /computers/rakazo-bot-${bot}/screen -> ${r.status}`);
    if (r.status >= 300 && r.status < 400) console.log(r.headers.get("location"));
    else console.error(await r.text());
  });
' "$bot" "$space")
[[ -n $url ]] || exit 1
echo "screen URL shape: $(sed -E 's#^([a-z]+)://[^:/]+(:[0-9]+)?.*#\1://<computer IP>\2/…#' <<<"$url")"

net=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "rakazo-bot-$bot")
echo "containers on $net: $(docker network inspect -f '{{range .Containers}}{{.Name}} {{end}}' "$net")"
echo "supervisor fetch: $("$D" exec -T supervisor node -e '
  fetch(process.argv[1], { signal: AbortSignal.timeout(10_000) })
    .then(async (r) => { const t = await r.text(); console.log(r.status, t.length + " bytes", /noVNC|<html/i.test(t) ? "html" : "non-html"); })
    .catch((e) => console.log("failed", e.message));
' "$url")"
echo "web stand-in fetch: $("$D" exec -T web sh -c 'wget -q -T 10 -O- "$1" | wc -c | sed "s/$/ bytes/"' sh "$url" 2>&1)"
