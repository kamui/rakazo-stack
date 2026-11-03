"""End-to-end check: Honcho v3.5.0 stores Lemonade embeddings and semantic search uses them.

Runs inside the tester container. Creates a fresh workspace, posts messages (one batch of
short ones, then a batch with one long message), waits for the embeddings, runs message
searches and conclusion queries whose words do not overlap the stored text, and prints
the request shapes the embed-spy proxy saw.
"""

import json
import os
import sys
import time

import httpx
import psycopg
import tiktoken

HONCHO = os.environ["HONCHO_URL"]
DB_URL = os.environ["DB_URL"]
LONG_TOKENS = int(sys.argv[1]) if len(sys.argv) > 1 else 3000
ENC = tiktoken.get_encoding("cl100k_base")

MESSAGES = [
    ("alice", "My golden retriever chewed through the garden hose again this morning."),
    ("assistant", "Oh no! Maybe try a bitter apple spray on the hose."),
    ("alice", "I'm flying to Lisbon on March 14th for my sister's wedding."),
    ("alice", "Work has been brutal; the quarterly audit deadline moved up by a week."),
    ("alice", "I've started learning to play the cello, lessons are on Thursdays."),
    ("alice", "My doctor says my cholesterol is high so I'm cutting back on red meat."),
    ("assistant", "Noted. I can suggest some lean recipes."),
    ("alice", "We are repainting the nursery a pale green before the baby arrives in June."),
    ("alice", "The car's check-engine light came on again on the motorway."),
    ("alice", "I prefer tea over coffee, especially Earl Grey in the afternoon."),
    ("alice", "My landlord finally agreed to fix the broken radiator."),
    ("alice", "I ran my first half marathon last Sunday in two hours ten minutes."),
]
# Queries share no words (or stems) with their target, so only the vector half of Honcho's
# hybrid search can find them.
QUERIES = [
    ("puppy destroying stuff outdoors", 0),
    ("travel abroad for a family celebration", 2),
    ("musical instrument classes", 4),
    ("heart health and diet", 5),
    ("expecting a child soon", 7),
    ("vehicle trouble", 8),
]
CONCLUSIONS = [
    "Alice owns a golden retriever.",
    "Alice is allergic to penicillin.",
    "Alice works as an accountant.",
    "Alice's sister lives in Portugal.",
    "Alice eats no meat on weekdays.",
]
CONCLUSION_QUERIES = [("What pet does she have?", 0), ("bad reactions to medicine", 1), ("her job", 2)]


def post(client: httpx.Client, path: str, body: dict) -> httpx.Response:
    r = client.post(f"{HONCHO}{path}", json=body)
    if r.status_code >= 400:
        print("HTTP", r.status_code, path, r.text[:400])
    return r


def embedding_rows(ws: str):
    with psycopg.connect(DB_URL) as conn:
        return conn.execute(
            """SELECT m.content IS NOT NULL, length(e.content), e.embedding IS NOT NULL,
                      vector_dims(e.embedding), e.sync_state, e.sync_attempts
               FROM message_embeddings e JOIN messages m ON m.public_id = e.message_id
               WHERE e.workspace_name = %s ORDER BY e.id""",
            (ws,),
        ).fetchall()


def main() -> None:
    started = time.time()
    ws = f"e2e-{int(started)}"
    client = httpx.Client(timeout=120)
    post(client, "/workspaces", {"id": ws})
    for peer in ("alice", "assistant"):
        post(client, f"/workspaces/{ws}/peers", {"id": peer})
    post(client, f"/workspaces/{ws}/sessions", {"id": "s1", "peers": {"alice": {}, "assistant": {}}})

    msgs = f"/workspaces/{ws}/sessions/s1/messages"
    r = post(client, msgs, {"messages": [{"peer_id": p, "content": c} for p, c in MESSAGES]})
    print("batch 1 (12 short messages):", r.status_code)
    long_text = ENC.decode(ENC.encode(" ".join(c for _, c in MESSAGES) * 400)[:LONG_TOKENS])
    r = post(client, msgs, {"messages": [
        {"peer_id": "alice", "content": "Short note one before the long one."},
        {"peer_id": "alice", "content": long_text},
    ]})
    print(f"batch 2 (1 short + 1 long message, {len(long_text)} chars, {LONG_TOKENS} cl100k tokens):", r.status_code)

    deadline = time.time() + 90
    while time.time() < deadline:
        rows = embedding_rows(ws)
        if rows and all(row[2] or row[5] >= 1 for row in rows):
            break
        time.sleep(3)
    rows = embedding_rows(ws)
    print(f"message_embeddings rows: {len(rows)}; with vector: {sum(r[2] for r in rows)}; "
          f"dims: {sorted({r[3] for r in rows if r[3]})}; states: {sorted({(r[4], r[5]) for r in rows})}")
    print("rows without vector (chunk chars, state, attempts):", [(r[1], r[4], r[5]) for r in rows if not r[2]])

    hits = 0
    for query, want in QUERIES:
        r = post(client, f"/workspaces/{ws}/search", {"query": query, "limit": 5})
        got = [m["content"] for m in r.json()] if r.status_code == 200 else []
        rank = got.index(MESSAGES[want][1]) + 1 if MESSAGES[want][1] in got else None
        hits += rank == 1
        print(f"search {query!r}: expected rank {rank}; top: {got[0][:60] if got else None!r}")
    print(f"message search top-1 hits: {hits}/{len(QUERIES)}")

    r = post(client, f"/workspaces/{ws}/conclusions", {"conclusions": [
        {"content": c, "observer_id": "assistant", "observed_id": "alice"} for c in CONCLUSIONS]})
    print("create conclusions:", r.status_code)
    hits = 0
    for query, want in CONCLUSION_QUERIES:
        r = post(client, f"/workspaces/{ws}/conclusions/query",
                 {"query": query, "top_k": 3, "filters": {"observer": "assistant", "observed": "alice"}})
        got = [c["content"] for c in r.json()] if r.status_code == 200 else []
        hits += bool(got) and got[0] == CONCLUSIONS[want]
        print(f"conclusions {query!r}: top: {got[0] if got else None!r}")
    print(f"conclusion query top-1 hits: {hits}/{len(CONCLUSION_QUERIES)}")

    with psycopg.connect(DB_URL) as conn:
        print("column types:", conn.execute(
            """SELECT c.relname, format_type(a.atttypid, a.atttypmod) FROM pg_attribute a
               JOIN pg_class c ON c.oid = a.attrelid
               WHERE a.attname = 'embedding' AND c.relname IN ('documents', 'message_embeddings')""").fetchall())
        print("documents with vector:", conn.execute(
            "SELECT count(*), min(vector_dims(embedding)) FROM documents WHERE workspace_name = %s AND embedding IS NOT NULL",
            (ws,)).fetchone())
        print("hnsw indexes:", [r[0] for r in conn.execute(
            "SELECT indexname FROM pg_indexes WHERE indexdef ILIKE '%%USING hnsw%%'").fetchall()])

    print("embed-spy log (requests Honcho sent during this run):")
    with open("/spylog/requests.jsonl") as f:
        for line in f:
            e = json.loads(line)
            if e["t"] < started:
                continue
            req, resp = e["request"], e["response"]
            chars = req["input_chars"]
            print(json.dumps({"status": e["status"], "s": e["seconds"], "auth": e["auth"], "ua": e["user_agent"],
                              "keys": req["keys"], "encoding_format": req["encoding_format"],
                              "dimensions": req["dimensions"], "n_inputs": req["n_inputs"],
                              "max_input_chars": max(chars) if chars else None,
                              "n_vectors": resp.get("n_vectors"), "vector_lengths": resp.get("vector_lengths"),
                              "element_types": resp.get("element_types"), "error": resp.get("error", "")[:160]}))


if __name__ == "__main__":
    main()
