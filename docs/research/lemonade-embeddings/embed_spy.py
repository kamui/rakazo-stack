"""Reverse proxy between Honcho and Lemonade that logs the shape of each embeddings call.

Logs one JSON line per request to /spylog/requests.jsonl: input count and sizes,
`encoding_format`, `dimensions`, and the response's vector count, length and element type.
Never logs the Authorization header value or the text.
"""

import json
import os
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM = os.environ["UPSTREAM"]
LOG_PATH = "/spylog/requests.jsonl"


def summarize_request(body: bytes) -> dict:
    try:
        req = json.loads(body)
    except ValueError:
        return {"unparsed_bytes": len(body)}
    inputs = req.get("input")
    inputs = inputs if isinstance(inputs, list) else [inputs]
    return {
        "keys": sorted(req.keys()),
        "model": req.get("model"),
        "encoding_format": req.get("encoding_format", "<absent>"),
        "dimensions": req.get("dimensions", "<absent>"),
        "n_inputs": len(inputs),
        "input_chars": [len(i) if isinstance(i, str) else type(i).__name__ for i in inputs],
    }


def summarize_response(body: bytes) -> dict:
    try:
        resp = json.loads(body)
    except ValueError:
        return {"unparsed_bytes": len(body)}
    data = resp.get("data")
    if not isinstance(data, list):
        return {"error": str(resp)[:300]}
    embs = [d.get("embedding") for d in data]
    return {
        "n_vectors": len(embs),
        "vector_lengths": sorted({len(e) for e in embs if isinstance(e, list)}),
        "element_types": sorted({type(e[0]).__name__ if isinstance(e, list) and e else type(e).__name__ for e in embs}),
        "indexes_in_order": [d.get("index") for d in data] == list(range(len(data))),
        "usage": resp.get("usage"),
    }


class Handler(BaseHTTPRequestHandler):
    def _forward(self, method: str) -> None:
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else None
        headers = {k: v for k, v in self.headers.items() if k.lower() not in {"host", "content-length", "accept-encoding"}}
        req = urllib.request.Request(UPSTREAM + self.path, data=body, method=method, headers=headers)
        started = time.perf_counter()
        try:
            with urllib.request.urlopen(req, timeout=600) as resp:
                status, resp_body, resp_headers = resp.status, resp.read(), resp.headers
        except urllib.error.HTTPError as e:
            status, resp_body, resp_headers = e.code, e.read(), e.headers
        elapsed = time.perf_counter() - started
        if self.path.endswith("/embeddings") and body is not None:
            auth = self.headers.get("Authorization", "")
            entry = {
                "t": time.time(),
                "path": self.path,
                "auth": "bearer" if auth.startswith("Bearer ") else ("other" if auth else "none"),
                "user_agent": self.headers.get("User-Agent"),
                "request": summarize_request(body),
                "status": status,
                "seconds": round(elapsed, 3),
                "response": summarize_response(resp_body),
            }
            with open(LOG_PATH, "a") as f:
                f.write(json.dumps(entry) + "\n")
        self.send_response(status)
        for k, v in resp_headers.items():
            if k.lower() not in {"transfer-encoding", "connection", "content-length"}:
                self.send_header(k, v)
        self.send_header("Content-Length", str(len(resp_body)))
        self.end_headers()
        self.wfile.write(resp_body)

    def do_POST(self) -> None:
        self._forward("POST")

    def do_GET(self) -> None:
        self._forward("GET")

    def log_message(self, format: str, *args: object) -> None:
        pass


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
