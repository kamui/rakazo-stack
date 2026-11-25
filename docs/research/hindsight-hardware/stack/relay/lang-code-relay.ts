// Kokoros passes `lang_code` straight to espeak, but Rakazo (like Kokoro-FastAPI and
// mlx-audio) sends Kokoro's one-letter codes, which espeak rejects with near-empty audio.
// Same table as Kokoros' get_language_code() in kokoros-openai/src/lib.rs.
const ESPEAK_BY_KOKORO_LANG: Record<string, string> = {
  a: "en-us",
  b: "en-gb-x-rp",
  e: "es",
  f: "fr-fr",
  h: "hi",
  i: "it",
  j: "ja",
  p: "pt-br",
  z: "cmn",
};

const upstream = new URL(Bun.env.UPSTREAM ?? "http://lemonade:13305");
const port = Number(Bun.env.PORT ?? 13306);
const lemonadeApiKey = Bun.env.LEMONADE_API_KEY;

async function speechBody(req: Request): Promise<BodyInit> {
  const raw = await req.text();
  let body: unknown;
  try {
    body = JSON.parse(raw);
  } catch {
    return raw;
  }
  if (!body || typeof body !== "object" || Array.isArray(body)) return raw;
  const record = body as Record<string, unknown>;
  const lang = record.lang_code;
  if (typeof lang !== "string") return raw;
  const espeak = ESPEAK_BY_KOKORO_LANG[lang.trim().toLowerCase()];
  if (!espeak) return raw;
  record.lang_code = espeak;
  return JSON.stringify(record);
}

Bun.serve({
  port,
  hostname: "0.0.0.0",
  // Transcription can take tens of seconds before the first response byte.
  idleTimeout: 255,
  async fetch(req) {
    const url = new URL(req.url);
    const target = new URL(url.pathname + url.search, upstream);
    const headers = new Headers(req.headers);
    headers.delete("host");
    headers.delete("content-length");
    if (lemonadeApiKey) headers.set("authorization", `Bearer ${lemonadeApiKey}`);

    let body: BodyInit | undefined;
    if (req.method !== "GET" && req.method !== "HEAD") {
      const isSpeech =
        req.method === "POST" &&
        url.pathname.endsWith("/audio/speech") &&
        (req.headers.get("content-type") ?? "").includes("json");
      body = isSpeech ? await speechBody(req) : await req.arrayBuffer();
    }

    try {
      const res = await fetch(target, {
        method: req.method,
        headers,
        body,
        redirect: "manual",
        decompress: false,
      });
      return new Response(res.body, {
        status: res.status,
        statusText: res.statusText,
        headers: res.headers,
      });
    } catch (error) {
      console.error(`${req.method} ${url.pathname} -> upstream failed:`, error);
      return Response.json(
        { error: { message: "Lemonade is unreachable from the relay", type: "bad_gateway" } },
        { status: 502 },
      );
    }
  },
});

console.log(`lang-code relay listening on :${port} -> ${upstream.origin}`);
