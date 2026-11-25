"""Time Hindsight recall the way Rakazo v0.16.0 calls it (max_tokens 4096; budget as given).

usage: bench.py BASE_URL BANK BUDGET RUNS LABEL [CONCURRENCY] [TYPES]
Prints one 'RUN {json}' line per recall and a 'SUM {json}' summary.
"""
import json
import statistics
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

BASE, BANK, BUDGET, RUNS, LABEL = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5]
CONC = int(sys.argv[6]) if len(sys.argv) > 6 else 1
TYPES = sys.argv[7].split(",") if len(sys.argv) > 7 else ["observation", "world", "experience"]

QUERIES = [
    "What is Alice worried about?", "When is the flight to Tokyo?", "Did anyone recommend a ramen place?",
    "What did Bob decide about the billing migration?", "Who offered to help with the heat pump service?",
    "What allergies should I remember?", "What is the budget for the new couch?",
    "Anything about the Postgres upgrade deadline?", "What does Priya think of the book club pick?",
    "Remind me what happened at the climbing gym", "What did Keiko say about sleep tracking?",
    "Is there a dentist appointment coming up?", "What is the plan for the vegetable garden?",
    "Who is learning the cello?", "What should I know before the design review?",
    "What happened with the customer escalation in March?", "Any travel plans for Seoul?",
    "What does Luis prefer to do in the mornings?", "What's the status of the passport renewal?",
    "Which recipes has Maya mentioned?", "What did Tariq postpone?", "What is going on with the rent increase?",
    "Tell me about the knee injury", "What is the top priority for October?",
    "Who complained about the internet contract?", "What did Sven book?", "What should not be discussed in group chats?",
    "Has anyone mentioned film photography?", "What is happening with the hiring loop?",
    "What is the cold brew ratio?",
]


def recall(q: str) -> dict:
    body = {"query": q, "budget": BUDGET, "max_tokens": 4096, "types": TYPES}
    req = urllib.request.Request(f"{BASE}/v1/default/banks/{BANK}/memories/recall", method="POST",
                                 data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    t = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            data = json.loads(r.read())
        err = None
    except Exception as e:  # noqa: BLE001
        data, err = {}, repr(e)[:200]
    dt = time.perf_counter() - t
    results = data.get("results") or []
    return {"q": q, "s": round(dt, 3), "n": len(results), "ids": [m.get("id") for m in results[:10]], "err": err}


def pct(xs: list[float], p: float) -> float:
    xs = sorted(xs)
    k = (len(xs) - 1) * p
    lo, hi = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


qs = [QUERIES[i % len(QUERIES)] for i in range(RUNS)]
t0 = time.perf_counter()
if CONC == 1:
    out = [recall(q) for q in qs]
else:
    with ThreadPoolExecutor(CONC) as ex:
        out = list(ex.map(recall, qs))
wall = time.perf_counter() - t0
for o in out:
    print("RUN " + json.dumps({"label": LABEL, "budget": BUDGET, **o}), flush=True)
lat = [o["s"] for o in out if not o["err"]]
summary = {"label": LABEL, "budget": BUDGET, "conc": CONC, "runs": RUNS, "errors": sum(1 for o in out if o["err"]),
           "median": round(statistics.median(lat), 3) if lat else None,
           "p95": round(pct(lat, 0.95), 3) if lat else None,
           "min": round(min(lat), 3) if lat else None, "max": round(max(lat), 3) if lat else None,
           "mean_results": round(statistics.mean(o["n"] for o in out), 1), "wall": round(wall, 2)}
print("SUM " + json.dumps(summary), flush=True)
