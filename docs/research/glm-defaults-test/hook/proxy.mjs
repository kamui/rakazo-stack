// Honcho's base URL points here; every request is forwarded unchanged to OpenRouter and logged.
import { record, summarizeRequest, summarizeResponse } from "./summarize.mjs";

const UPSTREAM = "https://openrouter.ai";

Bun.serve({
  port: 8080,
  idleTimeout: 255,
  async fetch(req) {
    const url = new URL(req.url);
    const text = await req.text();
    const started = Date.now();
    const entry = { t: new Date().toISOString(), svc: "honcho", path: url.pathname, req: summarizeRequest(text) };
    const headers = new Headers(req.headers);
    headers.delete("host");
    headers.delete("content-length");
    headers.delete("accept-encoding");
    const res = await fetch(UPSTREAM + url.pathname + url.search, { method: req.method, headers, body: text || undefined });
    const out = await res.text();
    entry.status = res.status;
    entry.ms = Date.now() - started;
    entry.res = summarizeResponse(out);
    record(entry, text);
    const back = new Headers(res.headers);
    back.delete("content-encoding");
    back.delete("content-length");
    return new Response(out, { status: res.status, headers: back });
  },
});
