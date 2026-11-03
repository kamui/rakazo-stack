"""Probe Lemonade's /embeddings directly with the same OpenAI SDK Honcho uses.

Usage (inside the tester container):
    probe_lemonade.py catalog | pull MODEL | set-batch MODEL N|none | shape MODEL
"""

import json
import os
import sys
import time

import httpx
import tiktoken
from openai import OpenAI

BASE = os.environ["LEMONADE_URL"]
KEY = os.environ["LEMONADE_API_KEY"]
AUTH = {"Authorization": f"Bearer {KEY}"}
ENC = tiktoken.get_encoding("cl100k_base")

SAMPLE = (
    "Yesterday I finally fixed the leaking kitchen tap, then walked the dog along the canal "
    "before dinner. Tomorrow I want to call my sister about the trip to Lisbon in March. "
)


def text_of_tokens(n: int) -> str:
    """A text that is exactly n cl100k tokens long, the unit Honcho budgets in."""
    toks = ENC.encode(SAMPLE * (n // 20 + 2))[:n]
    return ENC.decode(toks)


def catalog() -> None:
    r = httpx.get(f"{BASE}/models", params={"show_all": "true"}, headers=AUTH, timeout=60)
    r.raise_for_status()
    rows = [m for m in r.json()["data"] if "embeddings" in (m.get("labels") or [])]
    for m in rows:
        print(json.dumps({k: m.get(k) for k in ("id", "checkpoint", "recipe", "size", "downloaded", "labels")}))
    print("unauthenticated status:", httpx.get(f"{BASE}/models", timeout=60).status_code)


def pull(model: str) -> None:
    t = time.perf_counter()
    r = httpx.post(f"{BASE}/pull", json={"model_name": model}, headers=AUTH, timeout=1800)
    print(model, r.status_code, r.text[:300], f"{time.perf_counter() - t:.1f}s")


def set_batch(model: str, n: str) -> None:
    """Persist llama-server's physical/logical batch size for one model (recipe_options.json)."""
    body = {"llamacpp_args": f"-ub {n} -b {n}"} if n != "none" else {"llamacpp_args": None}
    r = httpx.post(f"{BASE}/models/{model}/options", json=body, headers=AUTH, timeout=60)
    print("options:", r.status_code, json.dumps(r.json().get("effective", r.text))[:500])
    r = httpx.post(f"{BASE}/unload", json={"model_name": model}, headers=AUTH, timeout=60)
    print("unload:", r.status_code, r.text[:200])


def call(client: OpenAI, model: str, inputs, **kw):
    try:
        resp = client.embeddings.create(model=model, input=inputs, **kw)
        embs = [d.embedding for d in resp.data]
        types = {type(e).__name__ if not isinstance(e, list) else f"list[{type(e[0]).__name__}]" for e in embs}
        return {"ok": True, "n": len(embs), "lens": sorted({len(e) for e in embs}), "types": sorted(types),
                "order_ok": [d.index for d in resp.data] == list(range(len(resp.data))),
                "usage": resp.usage.model_dump() if resp.usage else None}
    except Exception as e:  # noqa: BLE001 - recording the failure is the point
        return {"ok": False, "error": f"{type(e).__name__}: {str(e)[:300]}"}


def shape(model: str) -> None:
    client = OpenAI(base_url=BASE, api_key=KEY, timeout=600)
    t = time.perf_counter()
    print("cold load + 1 input:", call(client, model, ["hello"], encoding_format="float"), f"{time.perf_counter() - t:.1f}s")
    native = int(os.environ["EMBED_DIMS"])
    cases = {
        "float, 1 input": (["hello world"], {"encoding_format": "float"}),
        "encoding_format omitted (SDK asks for base64)": (["hello world"], {}),
        f"dimensions={native} (native)": (["hello world"], {"encoding_format": "float", "dimensions": native}),
        "dimensions=256": (["hello world"], {"encoding_format": "float", "dimensions": 256}),
        "dimensions=1536": (["hello world"], {"encoding_format": "float", "dimensions": 1536}),
        "empty string": ([""], {"encoding_format": "float"}),
        "batch 64": ([f"{i} {SAMPLE}" for i in range(64)], {"encoding_format": "float"}),
        "batch 512": ([f"{i} {SAMPLE}" for i in range(512)], {"encoding_format": "float"}),
        "batch 2048 (Honcho default max)": ([f"{i} {SAMPLE}" for i in range(2048)], {"encoding_format": "float"}),
    }
    for n in (500, 511, 512, 1000, 2000, 2047, 4000, 6500, 8100, 8192):
        cases[f"one input of {n} cl100k tokens"] = ([text_of_tokens(n)], {"encoding_format": "float"})
    cases["4 inputs of 2000 tokens"] = ([text_of_tokens(2000)] * 4, {"encoding_format": "float"})
    cases["short + one 1000-token input"] = ([SAMPLE, text_of_tokens(1000)], {"encoding_format": "float"})
    for name, (inputs, kw) in cases.items():
        t = time.perf_counter()
        print(f"{name}: {json.dumps(call(client, model, inputs, **kw))} {time.perf_counter() - t:.2f}s")

    # Same text twice must give the same vector; related texts should be closer than unrelated ones.
    r = client.embeddings.create(model=model, input=[
        "I adopted a puppy last week.", "I adopted a puppy last week.",
        "We got a new dog recently.", "The quarterly tax filing deadline is in April."],
        encoding_format="float")
    v = [d.embedding for d in r.data]

    def cos(a, b):
        return sum(x * y for x, y in zip(a, b)) / (sum(x * x for x in a) ** 0.5 * sum(y * y for y in b) ** 0.5)

    print("deterministic:", round(cos(v[0], v[1]), 6), "related:", round(cos(v[0], v[2]), 4),
          "unrelated:", round(cos(v[0], v[3]), 4), "norm:", round(sum(x * x for x in v[0]) ** 0.5, 4))


if __name__ == "__main__":
    cmd, *args = sys.argv[1:]
    {"catalog": catalog, "pull": pull, "set-batch": set_batch, "shape": shape}[cmd](*args)
