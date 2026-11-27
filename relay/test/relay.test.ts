// Runs against a relay and stub started here, or, with RELAY_URL and STUB_URL set, against
// the built image (test/image.sh).
import { afterAll, beforeAll, beforeEach, describe, expect, spyOn, test } from "bun:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { startRelay } from "../src/relay";
import { type ReceivedRequest, startStub, TRANSCRIPT } from "./stub";

const KEY = "placeholder-lemonade-key";
const JOINED = " Hello from the first segment, which ends mid-sentence. Bye.";

const servers: { stop(force: boolean): unknown }[] = [];
let relayUrl = process.env.RELAY_URL ?? "";
let stubUrl = process.env.STUB_URL ?? "";
let audioDir = "";
let webm: Blob;
let mp4: Blob;

async function generateAudio(name: string, codec: string): Promise<Blob> {
  const path = join(audioDir, name);
  const ffmpeg = Bun.spawn([
    "ffmpeg", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
    "-ar", "48000", "-ac", "2", "-c:a", codec, path,
  ]);
  if ((await ffmpeg.exited) !== 0) throw new Error(`ffmpeg couldn't write ${name}`);
  return new Blob([await Bun.file(path).arrayBuffer()]);
}

beforeAll(async () => {
  if (!relayUrl) {
    const stub = startStub(0);
    const relay = startRelay({ upstream: `http://127.0.0.1:${stub.port}`, apiKey: KEY, port: 0 });
    servers.push(stub, relay);
    stubUrl = `http://127.0.0.1:${stub.port}`;
    relayUrl = `http://127.0.0.1:${relay.port}`;
  }
  audioDir = await mkdtemp(join(tmpdir(), "relay-test-"));
  webm = await generateAudio("speech.webm", "libopus");
  mp4 = await generateAudio("speech.m4a", "aac");
});

afterAll(async () => {
  for (const server of servers) server.stop(true);
  await rm(audioDir, { recursive: true, force: true });
});

async function received(): Promise<ReceivedRequest[]> {
  const reply = await fetch(`${stubUrl}/__stub/requests`);
  return (await reply.json()) as ReceivedRequest[];
}

beforeEach(async () => {
  await received();
});

async function onlyReceived(): Promise<ReceivedRequest> {
  const requests = await received();
  expect(requests).toHaveLength(1);
  return requests[0]!;
}

function bodyOf(request: ReceivedRequest): Buffer {
  return Buffer.from(request.bodyBase64, "base64");
}

function formOf(request: ReceivedRequest) {
  return new Response(bodyOf(request), { headers: request.headers }).formData();
}

function transcribe(
  file: Blob,
  filename: string,
  fields: Record<string, string> = {},
  path = "/api/v1/audio/transcriptions",
): Promise<Response> {
  const form = new FormData();
  for (const [name, value] of Object.entries({ model: "Whisper-Small", ...fields })) {
    form.set(name, value);
  }
  form.set("file", file, filename);
  return fetch(relayUrl + path, { method: "POST", body: form });
}

function speak(body: string, path = "/api/v1/audio/speech"): Promise<Response> {
  return fetch(relayUrl + path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body,
  });
}

function wavInfo(wav: Buffer) {
  expect(wav.toString("ascii", 0, 4)).toBe("RIFF");
  expect(wav.toString("ascii", 8, 12)).toBe("WAVE");
  const chunks = new Map<string, Buffer>();
  for (let at = 12; at + 8 <= wav.length; ) {
    const size = wav.readUInt32LE(at + 4);
    chunks.set(wav.toString("ascii", at, at + 4), wav.subarray(at + 8, at + 8 + size));
    at += 8 + size + (size % 2);
  }
  const format = chunks.get("fmt ")!;
  return {
    encoding: format.readUInt16LE(0),
    channels: format.readUInt16LE(2),
    sampleRate: format.readUInt32LE(4),
    bitsPerSample: format.readUInt16LE(14),
    seconds: chunks.get("data")!.length / format.readUInt32LE(8),
  };
}

describe("transcription uploads", () => {
  test.each([
    ["WebM/Opus", () => webm, "speech.webm"],
    ["MP4/AAC", () => mp4, "speech.m4a"],
  ])("%s reaches upstream as 16 kHz mono 16-bit WAV", async (_, audio, filename) => {
    const fields = { response_format: "json", language: "en", prompt: "Zoë,\r\nRakazo" };
    const reply = await transcribe(audio(), filename, fields);
    expect(reply.status).toBe(200);

    const form = await formOf(await onlyReceived());
    const file = form.get("file") as File;
    expect(file.name).toBe("audio.wav");
    const info = wavInfo(Buffer.from(await file.arrayBuffer()));
    expect(info).toMatchObject({ encoding: 1, channels: 1, sampleRate: 16000, bitsPerSample: 16 });
    expect(info.seconds).toBeCloseTo(1, 0);
    for (const [name, value] of Object.entries({ model: "Whisper-Small", ...fields })) {
      expect(form.get(name)).toBe(value);
    }
  });

  test("an upload ffmpeg can't read is refused and not forwarded", async () => {
    const reply = await transcribe(new Blob(["not audio"]), "speech.webm");
    expect(reply.status).toBe(400);
    expect(await received()).toHaveLength(0);
  });
});

describe("transcription replies", () => {
  test.each(["json", "verbose_json"])("%s text has its newlines joined", async (format) => {
    const reply = await transcribe(webm, "speech.webm", { response_format: format });
    expect(await reply.json()).toEqual({ text: JOINED, task: "transcribe" });
  });

  test("the default format has its newlines joined, on /v1 too", async () => {
    const reply = await transcribe(webm, "speech.webm", {}, "/v1/audio/transcriptions");
    expect(((await reply.json()) as { text: string }).text).toBe(JOINED);
  });

  test("plain text has its newlines joined", async () => {
    const reply = await transcribe(webm, "speech.webm", { response_format: "text" });
    expect(reply.headers.get("content-type")).toStartWith("text/plain");
    expect(await reply.text()).toBe(JOINED);
  });

  test.each(["srt", "vtt"])("%s keeps its line structure", async (format) => {
    const reply = await transcribe(webm, "speech.webm", { response_format: format });
    expect(await reply.text()).toBe(TRANSCRIPT);
  });

  test("an upstream error passes through", async () => {
    const reply = await transcribe(
      webm,
      "speech.webm",
      {},
      "/api/v1/audio/transcriptions?status=500",
    );
    expect(reply.status).toBe(500);
    expect(reply.headers.get("x-stub")).toBe("echo");
  });

  test(
    "a reply that takes 60 s before its first byte gets through",
    async () => {
      // Bun's server idle timeout cuts off a waiting GET, but not a POST whose body it has read.
      const [transcription, models] = await Promise.all([
        transcribe(webm, "speech.webm", {}, "/api/v1/audio/transcriptions?delay=60000"),
        fetch(`${relayUrl}/api/v1/models?delay=60000`),
      ]);
      expect(((await transcription.json()) as { text: string }).text).toBe(JOINED);
      expect(models.status).toBe(200);
    },
    90_000,
  );
});

describe("lang_code on /audio/speech", () => {
  test.each([
    ["a", "en-us"],
    ["b", "en-gb-x-rp"],
    ["e", "es"],
    ["f", "fr-fr"],
    ["h", "hi"],
    ["i", "it"],
    ["j", "ja"],
    ["p", "pt-br"],
    ["z", "cmn"],
  ])("%s becomes %s", async (code, espeak) => {
    const speech = { model: "kokoro-v1", input: "Hello", voice: "af_heart" };
    await speak(JSON.stringify({ ...speech, lang_code: code }));
    const forwarded = JSON.parse(bodyOf(await onlyReceived()).toString());
    expect(forwarded).toEqual({ ...speech, lang_code: espeak });
  });

  test("is rewritten on /v1 too", async () => {
    await speak('{"lang_code":"a"}', "/v1/audio/speech");
    expect(bodyOf(await onlyReceived()).toString()).toBe('{"lang_code":"en-us"}');
  });

  test.each([
    '{ "lang_code": "en-us" }',
    '{ "lang_code": "x" }',
    '{ "lang_code": "A" }',
    '{ "lang_code": "constructor" }',
    '{ "lang_code": ["a"] }',
    '{ "input": "a" }',
    "not json",
  ])("%s passes through byte for byte", async (body) => {
    await speak(body);
    expect(bodyOf(await onlyReceived()).toString()).toBe(body);
  });
});

describe("forwarding", () => {
  test.each([
    ["GET", "/api/v1/models"],
    ["GET", "/api/v1/audio/voices?model=kokoro-v1"],
    ["POST", "/api/v1/pull"],
    ["POST", "/api/v1/audio/speech"],
    ["DELETE", "/anything/else?a=1&a=2"],
  ])("%s %s arrives unchanged, with the key", async (method, path) => {
    const body = method === "GET" ? undefined : new Uint8Array([0, 255, 10, 13, 128]);
    const reply = await fetch(relayUrl + path, {
      method,
      body,
      headers: { authorization: "Bearer someone-else", "x-custom": "kept" },
    });
    expect(reply.status).toBe(200);
    expect(new Uint8Array(await reply.arrayBuffer())).toEqual(body ?? new Uint8Array());

    const request = await onlyReceived();
    expect(request.method).toBe(method);
    expect(request.path).toBe(path);
    expect(request.headers.authorization).toBe(`Bearer ${KEY}`);
    expect(request.headers["x-custom"]).toBe("kept");
  });

  test("a transcription carries the key", async () => {
    await transcribe(webm, "speech.webm");
    expect((await onlyReceived()).headers.authorization).toBe(`Bearer ${KEY}`);
  });

  test("the upstream status, headers and body come back", async () => {
    const reply = await fetch(`${relayUrl}/api/v1/pull?status=418`, {
      method: "POST",
      body: "teapot",
    });
    expect(reply.status).toBe(418);
    expect(reply.headers.get("x-stub")).toBe("echo");
    expect(await reply.text()).toBe("teapot");
  });
});

describe("the API key", () => {
  test("never appears in the logs", async () => {
    const lines: unknown[][] = [];
    const spies = (["log", "info", "warn", "error", "debug"] as const).map((level) =>
      spyOn(console, level).mockImplementation((...line) => void lines.push(line)),
    );
    const upstream = startStub(0);
    const gone = `http://127.0.0.1:${upstream.port}`;
    await upstream.stop(true);
    const relay = startRelay({ upstream: gone, apiKey: KEY, port: 0 });
    try {
      const unreachable = await fetch(`http://127.0.0.1:${relay.port}/api/v1/models`);
      expect(unreachable.status).toBe(502);
      const form = new FormData();
      form.set("file", new Blob(["not audio"]), "speech.webm");
      await fetch(`http://127.0.0.1:${relay.port}/api/v1/audio/transcriptions`, {
        method: "POST",
        body: form,
      });
    } finally {
      await relay.stop(true);
      for (const spy of spies) spy.mockRestore();
    }
    expect(lines).toHaveLength(2);
    expect(JSON.stringify(lines)).not.toContain(KEY);
  });

  test("is not quoted when it is refused at startup", async () => {
    const relay = Bun.spawn([process.execPath, join(import.meta.dir, "../src/relay.ts")], {
      env: { UPSTREAM: "http://example:13305", LEMONADE_API_KEY: `${KEY}\nsecond-line` },
      stdout: "pipe",
      stderr: "pipe",
    });
    const [code, stdout, stderr] = await Promise.all([
      relay.exited,
      relay.stdout.text(),
      relay.stderr.text(),
    ]);
    expect(code).not.toBe(0);
    expect(stderr).toContain("LEMONADE_API_KEY");
    expect(stdout + stderr).not.toContain(KEY);
    expect(stdout + stderr).not.toContain("second-line");
  });
});
