// Preloaded into Rakazo's api and worker with NODE_OPTIONS=--import. Observes OpenRouter calls only.
import { record, summarizeRequest, summarizeResponse } from "./summarize.mjs";

const original = globalThis.fetch;
const svc = process.env.HOOK_SVC ?? "rakazo";

globalThis.fetch = async function (input, init) {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  if (!url.includes("openrouter.ai")) return original(input, init);
  let body = init?.body;
  if (body === undefined && input instanceof Request) body = await input.clone().text();
  const text = typeof body === "string" ? body : body instanceof Uint8Array ? new TextDecoder().decode(body) : "";
  const started = Date.now();
  const entry = { t: new Date().toISOString(), svc, path: new URL(url).pathname, req: summarizeRequest(text) };
  const res = await original(input, init);
  entry.status = res.status;
  if (!res.body) {
    record(entry, text);
    return res;
  }
  const [mine, theirs] = res.body.tee();
  new Response(theirs)
    .text()
    .then((out) => {
      entry.ms = Date.now() - started;
      entry.res = summarizeResponse(out);
      record(entry, text);
    })
    .catch((error) => record({ ...entry, teeError: String(error) }, text));
  return new Response(mine, { status: res.status, statusText: res.statusText, headers: res.headers });
};
