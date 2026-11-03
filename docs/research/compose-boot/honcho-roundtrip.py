# Honcho v3.5.0 memory round trip, run inside the honcho container:
#   dc.sh exec -T honcho /app/.venv/bin/python - < honcho-roundtrip.py
# Mints an admin JWT with Honcho's own script and never prints it.
import json
import subprocess
import urllib.error
import urllib.request

BASE = "http://localhost:8000/v3"
jwt = subprocess.run(
    ["/app/.venv/bin/python", "scripts/generate_jwt.py", "--admin", "--print-only"],
    capture_output=True, text=True, check=True,
).stdout.strip()


def call(method, path, body=None, auth=True):
    headers = {"Content-Type": "application/json"}
    if auth:
        headers["Authorization"] = f"Bearer {jwt}"
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=60) as res:
            return res.status, json.loads(res.read() or b"null")
    except urllib.error.HTTPError as err:
        return err.code, err.read().decode()[:300]


def show(label, result, keep=lambda b: b):
    status, body = result
    print(f"{label}: {status} {json.dumps(keep(body)) if status < 300 else body}")
    return body


ws, sess = "rs-compose-boot", "s1"
show("no token, POST /workspaces", call("POST", "/workspaces", {"id": ws}, auth=False))
show("POST /workspaces", call("POST", "/workspaces", {"id": ws}), lambda b: b["id"])
for peer in ("owner", "assistant"):
    show(f"POST /peers {peer}", call("POST", f"/workspaces/{ws}/peers", {"id": peer}), lambda b: b["id"])
show("POST /sessions", call("POST", f"/workspaces/{ws}/sessions",
     {"id": sess, "peers": {"owner": {}, "assistant": {}}}), lambda b: b["id"])
show("POST /messages", call("POST", f"/workspaces/{ws}/sessions/{sess}/messages", {"messages": [
    {"peer_id": "owner", "content": "My favourite tea is genmaicha."},
    {"peer_id": "assistant", "content": "Noted: genmaicha."},
]}), lambda b: [m["id"] for m in b])
show("POST /messages/list", call("POST", f"/workspaces/{ws}/sessions/{sess}/messages/list", {}),
     lambda b: [(m["peer_id"], m["content"]) for m in b["items"]])
show("GET /sessions/{id}/context", call("GET", f"/workspaces/{ws}/sessions/{sess}/context"),
     lambda b: [(m["peer_id"], m["content"]) for m in b.get("messages", [])])
show("POST /sessions/{id}/search", call("POST", f"/workspaces/{ws}/sessions/{sess}/search",
     {"query": "genmaicha"}), lambda b: [m["content"] for m in b])
show("POST /peers/owner/chat", call("POST", f"/workspaces/{ws}/peers/owner/chat",
     {"query": "What tea does the owner like?"}))
