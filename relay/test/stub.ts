// A stand-in for Lemonade. It records what reaches it and hands that back on /__stub/requests.

export interface ReceivedRequest {
  method: string;
  path: string;
  headers: Record<string, string>;
  bodyBase64: string;
}

export const TRANSCRIPT = " Hello from the first segment,\n which ends mid-sentence.\r\n\n Bye.";

export function startStub(port: number) {
  const received: ReceivedRequest[] = [];
  return Bun.serve({
    port,
    idleTimeout: 255,
    async fetch(request) {
      const url = new URL(request.url);
      if (url.pathname === "/__stub/requests") return Response.json(received.splice(0));

      const body = await request.arrayBuffer();
      received.push({
        method: request.method,
        path: url.pathname + url.search,
        headers: Object.fromEntries(request.headers),
        bodyBase64: Buffer.from(body).toString("base64"),
      });
      await Bun.sleep(Number(url.searchParams.get("delay") ?? 0));
      const status = Number(url.searchParams.get("status") ?? 200);

      if (url.pathname.endsWith("/audio/transcriptions") && status === 200) {
        const form = await new Response(body, { headers: request.headers }).formData();
        const format = form.get("response_format");
        if (format === "text" || format === "srt" || format === "vtt") {
          return new Response(TRANSCRIPT, { headers: { "content-type": "text/plain" } });
        }
        return Response.json({ text: TRANSCRIPT, task: "transcribe" });
      }

      return new Response(body, {
        status,
        headers: {
          "content-type": request.headers.get("content-type") ?? "application/octet-stream",
          "x-stub": "echo",
        },
      });
    },
  });
}

if (import.meta.main) startStub(13305);
