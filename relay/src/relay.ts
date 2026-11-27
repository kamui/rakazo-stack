import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const AUDIO_ROUTE = /^\/(?:api\/)?v[01]\/audio\/(speech|transcriptions)$/;

// Kokoros hands lang_code to espeak as is (lucasjinreal/Kokoros#144).
const LANG_CODES = new Map([
  ["a", "en-us"],
  ["b", "en-gb-x-rp"],
  ["e", "es"],
  ["f", "fr-fr"],
  ["h", "hi"],
  ["i", "it"],
  ["j", "ja"],
  ["p", "pt-br"],
  ["z", "cmn"],
]);

const HOP_BY_HOP_HEADERS = [
  "connection",
  "keep-alive",
  "proxy-authorization",
  "proxy-connection",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
];

export interface RelayConfig {
  upstream: string;
  apiKey: string;
  port: number;
}

export function startRelay({ upstream, apiKey, port }: RelayConfig) {
  // Anything else makes Headers throw an error that quotes the whole key.
  if (!/^[\x21-\x7e]+$/.test(apiKey)) {
    throw new Error("LEMONADE_API_KEY must be visible ASCII with no spaces.");
  }
  const base = new URL(upstream).href.replace(/\/$/, "");

  async function forward(
    request: Request,
    body: ArrayBuffer | string | FormData | undefined,
  ): Promise<Response> {
    const { pathname, search } = new URL(request.url);
    const headers = new Headers(request.headers);
    for (const name of [...HOP_BY_HOP_HEADERS, "host", "content-length", "expect"]) {
      headers.delete(name);
    }
    if (body instanceof FormData) headers.delete("content-type");
    headers.set("accept-encoding", "identity");
    headers.set("authorization", `Bearer ${apiKey}`);

    let reply: Response;
    try {
      reply = await fetch(base + pathname + search, {
        method: request.method,
        headers,
        body,
        redirect: "manual",
        signal: request.signal,
        decompress: false,
      });
    } catch (error) {
      if (!request.signal.aborted) {
        console.error(`${request.method} ${pathname}: upstream failed: ${describe(error)}`);
      }
      return errorReply(502, "Lemonade didn't answer.");
    }
    const replyHeaders = new Headers(reply.headers);
    for (const name of HOP_BY_HOP_HEADERS) replyHeaders.delete(name);
    return new Response(reply.body, { status: reply.status, headers: replyHeaders });
  }

  async function transcribe(request: Request): Promise<Response> {
    const form = await request.formData().catch(() => undefined);
    if (!form) return errorReply(400, "The upload isn't a multipart form.");
    const upload = form.get("file");
    if (upload instanceof Blob) {
      const wav = await toWav(upload);
      if (!wav) return errorReply(400, "The upload couldn't be converted to WAV.");
      // Lemonade picks the audio type from the file name's extension.
      form.set("file", wav, "audio.wav");
    }

    const reply = await forward(request, form);
    const format = form.get("response_format");
    // Line breaks are part of the subtitle formats.
    if (!reply.ok || format === "srt" || format === "vtt") return reply;
    return joinReplyLines(reply);
  }

  return Bun.serve({
    port,
    // Bun's maximum. A transcription can take tens of seconds before its first byte.
    idleTimeout: 255,
    development: false,
    async fetch(request) {
      const route =
        request.method === "POST" ? AUDIO_ROUTE.exec(new URL(request.url).pathname)?.[1] : undefined;
      if (route === "transcriptions") return transcribe(request);
      const body = request.body ? await request.arrayBuffer() : undefined;
      return forward(request, route === "speech" && body ? rewriteLangCode(body) : body);
    },
    error(error) {
      console.error(`relay failed: ${describe(error)}`);
      return errorReply(500, "The relay failed.");
    },
  });
}

async function toWav(upload: Blob): Promise<Blob | undefined> {
  // MP4 needs a seekable input and WAV a seekable output, so neither can be a pipe.
  const dir = await mkdtemp(join(tmpdir(), "relay-"));
  try {
    const input = join(dir, "in");
    const output = join(dir, "out.wav");
    await Bun.write(input, upload);
    const ffmpeg = Bun.spawn(
      [
        "ffmpeg", "-nostdin", "-v", "error", "-i", input,
        "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", output,
      ],
      { stdout: "ignore", stderr: "pipe" },
    );
    const [code, stderr] = await Promise.all([ffmpeg.exited, ffmpeg.stderr.text()]);
    if (code !== 0) {
      console.error(`ffmpeg exited with ${code}: ${stderr.trim()}`);
      return undefined;
    }
    return new Blob([await Bun.file(output).arrayBuffer()], { type: "audio/wav" });
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

function rewriteLangCode(body: ArrayBuffer): ArrayBuffer | string {
  let speech: unknown;
  try {
    speech = JSON.parse(new TextDecoder().decode(body));
  } catch {
    return body;
  }
  if (typeof speech !== "object" || speech === null || !("lang_code" in speech)) return body;
  const mapped = typeof speech.lang_code === "string" && LANG_CODES.get(speech.lang_code);
  return mapped ? JSON.stringify({ ...speech, lang_code: mapped }) : body;
}

/** Whisper breaks lines at its segment boundaries, even mid-sentence. */
function joinLines(text: string): string {
  return text.replace(/\s+/g, (run) => (/[\r\n]/.test(run) ? " " : run));
}

async function joinReplyLines(reply: Response): Promise<Response> {
  const type = reply.headers.get("content-type")?.split(";")[0]?.trim().toLowerCase();
  if (type !== "application/json" && type !== "text/plain") return reply;
  const headers = new Headers(reply.headers);
  headers.delete("content-length");
  let body = await reply.text();
  if (type === "text/plain") {
    body = joinLines(body);
  } else {
    const transcript: unknown = JSON.parse(body);
    if (
      typeof transcript === "object" &&
      transcript !== null &&
      "text" in transcript &&
      typeof transcript.text === "string"
    ) {
      body = JSON.stringify({ ...transcript, text: joinLines(transcript.text) });
    }
  }
  return new Response(body, { status: reply.status, headers });
}

function errorReply(status: number, message: string): Response {
  return Response.json({ error: { message } }, { status });
}

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function requiredEnv(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set.`);
  return value;
}

if (import.meta.main) {
  const upstream = requiredEnv("UPSTREAM");
  const server = startRelay({ upstream, apiKey: requiredEnv("LEMONADE_API_KEY"), port: 13306 });
  // The relay is PID 1 in its container, where an unhandled SIGTERM is ignored.
  process.on("SIGTERM", () => process.exit(0));
  console.log(`lemonade-relay listening on ${server.port}, forwarding to ${new URL(upstream).origin}`);
}
