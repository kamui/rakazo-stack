"""Seed a Hindsight bank with N synthetic memories in chunks mode (no LLM).

usage: seed.py BASE_URL BANK N [BATCH] [START]
"""
import json
import random
import sys
import time
import urllib.request

BASE, BANK, N = sys.argv[1], sys.argv[2], int(sys.argv[3])
BATCH = int(sys.argv[4]) if len(sys.argv) > 4 else 100
START = int(sys.argv[5]) if len(sys.argv) > 5 else 0

PEOPLE = ["Alice", "Bob", "Carmen", "Dmitri", "Elena", "Farah", "Goran", "Hana", "Ivan", "Jun",
          "Keiko", "Luis", "Maya", "Nikhil", "Olga", "Priya", "Quentin", "Rosa", "Sven", "Tariq"]
PLACES = ["Lisbon", "Kyoto", "Denver", "the cabin", "the office", "Berlin", "Oaxaca", "the gym",
          "the farmers market", "Vancouver", "the climbing gym", "Seoul", "the library", "Nairobi"]
TOPICS = [
    ("work", ["the quarterly roadmap", "the billing migration", "the hiring loop", "the on-call rotation",
              "the design review", "the customer escalation", "the Postgres upgrade", "the launch checklist"]),
    ("health", ["a knee injury", "the new running plan", "a dentist appointment", "sleep tracking",
                "a vitamin D deficiency", "the physio exercises", "an allergy to shellfish"]),
    ("food", ["a sourdough starter", "the ramen place downtown", "a vegetarian lasagna recipe",
              "cold brew ratios", "a spicy kimchi stew", "the birthday cake order"]),
    ("travel", ["a flight to Tokyo", "the train pass", "a rental car booking", "a passport renewal",
                "the hotel near the station", "a hiking permit"]),
    ("hobby", ["a 1000-piece puzzle", "learning the cello", "a woodworking bench", "the book club pick",
               "bouldering grades", "a vegetable garden", "film photography"]),
    ("home", ["the leaking faucet", "the internet contract", "a new couch", "the heat pump service",
              "the rent increase", "the smart thermostat"]),
]
VERBS = ["mentioned", "decided", "complained about", "asked about", "is excited about", "finished",
         "postponed", "booked", "recommended", "is worried about", "changed their mind about"]
DETAILS = [
    "They want a reminder the week before.", "The budget is around {n} dollars.",
    "It should be done before {month}.", "{p2} offered to help with it.",
    "They prefer mornings for this.", "The last attempt did not go well, so they want a different approach.",
    "They said it is the top priority for {month}.", "There is a deadline on the {day}th.",
    "They would rather not discuss it in group chats.", "It reminded them of a trip to {place2}.",
]
MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August",
          "September", "October", "November", "December"]


def memory(rng: random.Random, i: int) -> dict:
    cat, subjects = rng.choice(TOPICS)
    p, p2 = rng.sample(PEOPLE, 2)
    text = (f"{p} {rng.choice(VERBS)} {rng.choice(subjects)} while at {rng.choice(PLACES)}. "
            + " ".join(rng.choice(DETAILS).format(n=rng.randint(20, 3000), month=rng.choice(MONTHS),
                                                  p2=p2, day=rng.randint(1, 28), place2=rng.choice(PLACES))
                       for _ in range(rng.randint(2, 4)))
            + f" (note {i}, {cat})")
    ts = f"2026-{rng.randint(1, 11):02d}-{rng.randint(1, 28):02d}T{rng.randint(7, 22):02d}:00:00Z"
    return {"content": text, "timestamp": ts, "context": cat, "document_id": f"seed-{i}"}


def call(method: str, path: str, body: dict | None = None, timeout: float = 600) -> dict:
    req = urllib.request.Request(BASE + path, method=method,
                                 data=None if body is None else json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read() or b"{}")


call("PATCH", f"/v1/default/banks/{BANK}/config", {"updates": {"retain_extraction_mode": "chunks"}})
rng = random.Random(42 + START)
t0 = time.time()
for start in range(START, START + N, BATCH):
    items = [memory(rng, i) for i in range(start, min(START + N, start + BATCH))]
    t = time.time()
    call("POST", f"/v1/default/banks/{BANK}/memories", {"items": items, "async": False})
    print(f"retained {start + len(items) - START}/{N} batch={time.time() - t:.1f}s", flush=True)
print(f"done in {time.time() - t0:.0f}s", flush=True)
