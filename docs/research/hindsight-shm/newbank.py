"""Create a bank the way a new Rakazo bot or Space does (first write to an unknown bank) and report the outcome.

usage: newbank.py BASE_URL BANK
"""
import json
import sys
import time
import urllib.error
import urllib.request

BASE, BANK = sys.argv[1], sys.argv[2]


def call(method: str, path: str, body: dict) -> None:
    req = urllib.request.Request(BASE + path, method=method, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    t = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            print(method, path, "OK", r.status, round(time.perf_counter() - t, 2), r.read()[:200].decode())
    except urllib.error.HTTPError as e:
        print(method, path, "HTTP", e.code, round(time.perf_counter() - t, 2), e.read()[:600].decode())


call("PATCH", f"/v1/default/banks/{BANK}/config", {"updates": {"retain_extraction_mode": "chunks"}})
call("POST", f"/v1/default/banks/{BANK}/memories",
     {"items": [{"content": f"First memory for {BANK}: likes green tea.", "document_id": f"{BANK}-1"}],
      "async": False})
