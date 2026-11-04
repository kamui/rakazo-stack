// Shared by the Rakazo fetch hook and the Honcho-side proxy. Logs shapes and usage, never keys.
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";

const DIR = process.env.OR_LOG_DIR ?? "/orlog";
let seq = 0;

export function summarizeRequest(text) {
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return { unparsed: String(text).slice(0, 200) };
  }
  const { messages, tools, input, ...rest } = body;
  const out = { ...rest };
  if (Array.isArray(messages)) {
    out.messages = messages.length;
    out.chars = JSON.stringify(messages).length;
  }
  if (Array.isArray(tools)) out.tools = tools.map((t) => t.function?.name ?? t.name);
  if (input !== undefined) out.input = Array.isArray(input) ? input.length : 1;
  if (out.response_format) out.response_format = out.response_format.type;
  return out;
}

export function summarizeResponse(text) {
  const out = {};
  const take = (obj) => {
    if (!obj || typeof obj !== "object") return;
    if (obj.id) out.id = obj.id;
    if (obj.provider) out.provider = obj.provider;
    if (obj.model) out.model = obj.model;
    if (obj.usage) out.usage = obj.usage;
    if (obj.error) out.error = obj.error;
    const choice = obj.choices?.[0];
    if (choice) {
      if (choice.finish_reason) out.finish_reason = choice.finish_reason;
      const m = choice.message ?? choice.delta ?? {};
      if (typeof m.content === "string") out.content_chars = (out.content_chars ?? 0) + m.content.length;
      const r = m.reasoning ?? m.reasoning_content;
      if (typeof r === "string") out.reasoning_chars = (out.reasoning_chars ?? 0) + r.length;
      if (m.tool_calls) out.tool_calls = (out.tool_calls ?? []).concat(m.tool_calls.map((c) => c.function?.name).filter(Boolean));
      if (choice.message && typeof m.content === "string") out.content_head = m.content.slice(0, 300);
    }
    if (Array.isArray(obj.data) && obj.data[0]?.embedding) out.embeddings = obj.data.length;
  };
  if (text.includes("\ndata: ") || text.startsWith("data: ")) {
    out.stream = true;
    for (const line of text.split("\n")) {
      if (!line.startsWith("data: ") || line === "data: [DONE]") continue;
      try {
        take(JSON.parse(line.slice(6)));
      } catch {}
    }
  } else {
    try {
      take(JSON.parse(text));
    } catch {
      out.unparsed = text.slice(0, 300);
    }
  }
  return out;
}

export function record(entry, fullRequest) {
  mkdirSync(DIR, { recursive: true });
  const n = ++seq;
  entry.n = n;
  appendFileSync(`${DIR}/${entry.svc}.jsonl`, JSON.stringify(entry) + "\n");
  // Keep the first few full request bodies per process for inspection.
  if (fullRequest && n <= 6) writeFileSync(`${DIR}/${entry.svc}-${process.pid}-req${n}.json`, fullRequest);
}
