"""Rough relevance proxy: share of the top 5 recalled memories that contain every key phrase of the query.

usage: quality.py BASE_URL BANK LABEL BUDGET...
"""
import json
import sys
import urllib.request

BASE, BANK, LABEL, BUDGETS = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
CASES = [
    ("What is Alice worried about?", ["alice", "worried"]),
    ("When is the flight to Tokyo?", ["flight to tokyo"]),
    ("Did anyone recommend a ramen place?", ["recommended", "ramen"]),
    ("What did Bob decide about the billing migration?", ["bob", "decided", "billing migration"]),
    ("Who offered to help with the heat pump service?", ["heat pump", "offered to help"]),
    ("What allergies should I remember?", ["allergy"]),
    ("What is the budget for the new couch?", ["new couch", "budget"]),
    ("Who is learning the cello?", ["cello"]),
    ("What did Tariq postpone?", ["tariq", "postponed"]),
    ("Who complained about the internet contract?", ["complained about", "internet contract"]),
    ("What did Sven book?", ["sven", "booked"]),
    ("Tell me about the knee injury", ["knee injury"]),
    ("What did Keiko say about sleep tracking?", ["keiko", "sleep tracking"]),
    ("Any travel plans for Seoul?", ["seoul"]),
    ("What is happening with the hiring loop?", ["hiring loop"]),
    ("Has anyone mentioned film photography?", ["film photography"]),
]
for budget in BUDGETS:
    hits = top1 = 0
    for q, keys in CASES:
        body = {"query": q, "budget": budget, "max_tokens": 4096, "types": ["observation", "world", "experience"]}
        req = urllib.request.Request(f"{BASE}/v1/default/banks/{BANK}/memories/recall", method="POST",
                                     data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=120) as r:
            texts = [m["text"].lower() for m in json.loads(r.read())["results"][:5]]
        ok = [all(k in t for k in keys) for t in texts]
        hits += sum(ok)
        top1 += bool(ok and ok[0])
    print(f"QUALITY {LABEL} {budget}: precision@5={hits / (5 * len(CASES)):.0%} top1-relevant={top1}/{len(CASES)}",
          flush=True)
