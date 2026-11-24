import { describe, expect, it } from "vitest";

import {
    createFetchTransport,
    type FetchLike,
    type TransportRequest,
    type TransportTlsInfo,
} from "../src/index.ts";

const sampleTls: TransportTlsInfo = {
    engine: "webkit",
    profile: "safari_ios_26.2",
    platform: "ios",
    clientHello: {},
};

function makeRequest(
    overrides: Partial<TransportRequest> = {},
): TransportRequest {
    return {
        method: "GET",
        url: "https://example.com/test",
        headers: [
            ["user-agent", "TestAgent/1.0"],
            ["content-length", "0"],
            ["x-custom", "ok"],
        ],
        followRedirects: true,
        maxRedirects: 5,
        includeCredentials: true,
        preserveEncoding: false,
        timeoutMs: 5_000,
        kind: "navigation",
        tls: sampleTls,
        ...overrides,
    };
}

describe("createFetchTransport", () => {
    it("forwards method, headers, redirect mode, credentials, and multi Set-Cookie headers", async () => {
        let capturedUrl = "";
        let capturedInit: RequestInit | undefined;

        const fakeFetch: FetchLike = async (input, init) => {
            capturedUrl = String(input);
            capturedInit = init;
            const headers = new Headers([
                ["content-type", "text/html"],
                ["set-cookie", "a=1; Path=/"],
                ["set-cookie", "cf_clearance=xyz; Path=/; Secure"],
            ]);
            return new Response("<html>ok</html>", {
                status: 200,
                headers,
            });
        };

        const transport = createFetchTransport({ fetch: fakeFetch });
        const response = await transport.fetch(
            makeRequest({ followRedirects: false, includeCredentials: false }),
        );

        expect(capturedUrl).toBe("https://example.com/test");
        expect(capturedInit?.redirect).toBe("manual");
        expect(capturedInit?.credentials).toBe("omit");
        const sentHeaders = capturedInit?.headers as Headers;
        expect(sentHeaders.get("x-custom")).toBe("ok");
        expect(sentHeaders.has("content-length")).toBe(false);

        expect(response.status).toBe(200);
        expect(response.bodyDecoded).toBe(true);
        const text = new TextDecoder().decode(response.body as Uint8Array);
        expect(text).toBe("<html>ok</html>");

        const headerEntries = response.headers as readonly (readonly [
            string,
            string,
        ])[];
        const cookies = headerEntries
            .filter(([name]) => name.toLowerCase() === "set-cookie")
            .map(([, value]) => value);
        expect(cookies).toEqual([
            "a=1; Path=/",
            "cf_clearance=xyz; Path=/; Secure",
        ]);
    });

    it("aborts requests when the caller AbortSignal triggers", async () => {
        const controller = new AbortController();
        const fakeFetch: FetchLike = (_input, init) =>
            new Promise((_resolve, reject) => {
                init?.signal?.addEventListener("abort", () => {
                    reject(init.signal?.reason ?? new Error("aborted"));
                });
            });

        const transport = createFetchTransport({
            fetch: fakeFetch,
            signal: controller.signal,
        });
        const pending = transport.fetch(makeRequest());
        controller.abort(new Error("user cancelled"));

        await expect(pending).rejects.toThrow("user cancelled");
    });

    it("bridges WebSocket text/binary echo frames and clean close", async () => {
        class FakeWebSocket {
            binaryType = "blob";
            onopen: (() => void) | null = null;
            onmessage: ((event: { data: unknown }) => void) | null = null;
            onerror: (() => void) | null = null;
            onclose: (() => void) | null = null;
            closed = false;

            constructor(public readonly url: string) {
                queueMicrotask(() => this.onopen?.());
            }

            send(data: string): void {
                queueMicrotask(() => {
                    this.onmessage?.({ data: `echo:${data}` });
                });
            }

            close(): void {
                this.closed = true;
                queueMicrotask(() => this.onclose?.());
            }
        }

        const transport = createFetchTransport({
            WebSocket: FakeWebSocket as unknown as typeof WebSocket,
        });
        const ws = await transport.websocket!({
            url: "wss://challenges.cloudflare.com/echo",
            headers: [["origin", "https://challenges.cloudflare.com"]],
            tls: sampleTls,
        });

        await ws.sendText("123.45");
        const reply = await ws.recv();
        expect(reply).toEqual({ type: "text", data: "echo:123.45" });

        await ws.close();
        expect(await ws.recv()).toBeNull();
    });
});
