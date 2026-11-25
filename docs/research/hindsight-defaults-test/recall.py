"""Replay Rakazo v0.16.0's automatic recall (budget mid, max_tokens 4096, observations only unless
'all') against one bank, for the questions asked after the conversation, and check expected facts.

usage: HS_KEY=... recall.py BASE_URL BANK [all]
"""
import json
import os
import sys
import time
import urllib.request

BASE, BANK = sys.argv[1], sys.argv[2]
TYPES = ["observation", "world", "experience"] if sys.argv[3:] == ["all"] else ["observation"]
CHECKS = [
    ("What's my dog's name, and when does she need her vaccinations?", ["Pepper", "May", "Graça"]),
    ("What should I keep in mind when cooking for Alex and me?", ["vegetarian", "coriander", "mushroom"]),
    ("Which days do I run these days?", ["Saturday"]),
    ("When is Alex's birthday, and what kind of gifts do I prefer to give?", ["March 14", "experience"]),
    ("Who is visiting me soon, and is there anything I need to know about her diet?", ["Inês", "nut"]),
    ("Where am I travelling in October, and with whom?", ["Japan", "Alex"]),
    ("What do I do for work, and who is Rosa?", ["graphic design", "Rosa"]),
]
for query, expect in CHECKS:
    body = {"query": query, "budget": "mid", "max_tokens": 4096, "types": TYPES}
    req = urllib.request.Request(f"{BASE}/v1/default/banks/{BANK}/memories/recall", method="POST",
                                 data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json",
                                          "Authorization": f"Bearer {os.environ['HS_KEY']}"})
    t = time.time()
    with urllib.request.urlopen(req, timeout=60) as r:
        results = json.loads(r.read())["results"]
    secs = time.time() - t
    texts = [x.get("text", "") for x in results]
    joined = " ".join(texts).lower()
    found = [e for e in expect if e.lower() in joined]
    first = next((i + 1 for i, x in enumerate(texts) if any(e.lower() in x.lower() for e in expect)), None)
    print(f"Q: {query}\n   {len(results)} results in {secs:.2f}s; expected facts found: {len(found)}/{len(expect)} "
          f"{found}; first relevant at rank {first}")
    for i, x in enumerate(texts[:5]):
        print(f"   {i + 1}. {x[:160]}")
