"""Give Hindsight some work without an LLM: seed N memories in chunks mode, then run recalls the
way Rakazo v0.16.0 calls them (budget mid, max_tokens 4096). Needs HS_KEY (the tenant key).

usage: HS_KEY=... use.py BASE_URL BANK N RECALLS
"""
import json
import os
import random
import sys
import time
import urllib.request

BASE, BANK, N, RECALLS = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
HEADERS = {"Content-Type": "application/json", "Authorization": f"Bearer {os.environ['HS_KEY']}"}
PEOPLE = ["Alice", "Bob", "Carmen", "Dmitri", "Elena", "Farah", "Goran", "Hana", "Ivan", "Jun"]
THINGS = ["the dentist appointment", "a flight to Tokyo", "the vegetable garden", "a sourdough starter",
          "the rent increase", "bouldering grades", "the book club pick", "a knee injury"]


def call(method, path, body=None):
    req = urllib.request.Request(BASE + path, method=method, headers=HEADERS,
                                 data=None if body is None else json.dumps(body).encode())
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.loads(r.read() or b"{}")


call("PATCH", f"/v1/default/banks/{BANK}/config", {"updates": {"retain_extraction_mode": "chunks"}})
rng = random.Random(46)
t0 = time.time()
for start in range(0, N, 100):
    items = [{"content": f"{rng.choice(PEOPLE)} mentioned {rng.choice(THINGS)}. Note {i}.",
              "document_id": f"seed-{i}"} for i in range(start, min(N, start + 100))]
    call("POST", f"/v1/default/banks/{BANK}/memories", {"items": items, "async": False})
print(f"seeded {N} in {time.time() - t0:.0f}s", flush=True)
times = []
for i in range(RECALLS):
    t = time.time()
    call("POST", f"/v1/default/banks/{BANK}/memories/recall",
         {"query": f"What did {rng.choice(PEOPLE)} say about {rng.choice(THINGS)}?", "budget": "mid",
          "max_tokens": 4096})
    times.append(time.time() - t)
times.sort()
print(f"{RECALLS} recalls: median {times[len(times) // 2]:.2f}s max {times[-1]:.2f}s", flush=True)
