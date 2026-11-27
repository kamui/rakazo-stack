// Run by `rakazo-stack setup` inside Rakazo's `api` container (`node -e`), so every call takes
// the path Rakazo's own voice calls take: through the relay, which adds Lemonade's key.
// Reads one JSON request on stdin and writes JSON lines on stdout; the last line is the result.
// A failure exits 1 with its message as the last line on stderr.
const BASE = "http://lemonade-relay:13306/api/v1";
const PROGRESS_EVERY_MS = 5000;

function say(line) {
  process.stdout.write(`${JSON.stringify(line)}\n`);
}

async function failure(reply) {
  const text = await reply.text();
  let message = text.trim();
  try {
    const body = JSON.parse(text);
    message = body?.error?.message ?? body?.error ?? body?.message ?? message;
  } catch {}
  return new Error(`${reply.status} ${String(message).slice(0, 300)}`.trim());
}

async function call(path, init) {
  const reply = await fetch(BASE + path, init);
  if (!reply.ok) throw await failure(reply);
  return reply;
}

function json(body) {
  return { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) };
}

async function installed() {
  const reply = await call("/models");
  const { data } = await reply.json();
  say({ installed: data.map((model) => model.id) });
}

// Lemonade streams progress as server-sent events, which also keeps the relay's idle timeout
// from ending a long download.
async function pull(model) {
  const reply = await call("/pull", json({ model_name: model, stream: true }));
  const decoder = new TextDecoder();
  let buffer = "";
  let shown = 0;
  for await (const chunk of reply.body) {
    buffer += decoder.decode(chunk, { stream: true });
    let end;
    while ((end = buffer.indexOf("\n\n")) >= 0) {
      const block = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      const event = /^event: *(.*)$/m.exec(block)?.[1]?.trim();
      const data = /^data: *(.*)$/m.exec(block)?.[1];
      const fields = data ? JSON.parse(data) : {};
      if (event === "error") throw new Error(fields.error ?? "the download failed");
      if (event === "complete") return say({ pulled: model });
      if (event === "progress" && Date.now() - shown >= PROGRESS_EVERY_MS) {
        shown = Date.now();
        say({ progress: { file: fields.file_index, files: fields.total_files, percent: fields.percent } });
      }
    }
  }
  throw new Error("the download ended without finishing");
}

async function load(model) {
  await call("/load", json({ model_name: model }));
  say({ loaded: model });
}

// One speech and one transcription of that speech, so each model's first-use cost is paid now.
async function warm(speech, transcription) {
  let started = Date.now();
  const reply = await call(
    "/audio/speech",
    json({ model: speech, voice: "af_heart", input: "Voice is ready.", lang_code: "a" }),
  );
  const audio = await reply.blob();
  const speechMs = Date.now() - started;
  const form = new FormData();
  form.set("model", transcription);
  form.set("file", audio, "speech.mp3");
  started = Date.now();
  await (await call("/audio/transcriptions", { method: "POST", body: form })).json();
  say({ warmed: { speech_ms: speechMs, transcription_ms: Date.now() - started } });
}

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
});
process.stdin.on("end", async () => {
  try {
    const request = JSON.parse(input);
    if (request.op === "installed") await installed();
    else if (request.op === "pull") await pull(request.model);
    else if (request.op === "load") await load(request.model);
    else if (request.op === "warm") await warm(request.speech, request.transcription);
    else throw new Error(`unknown request ${request.op}`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
});
