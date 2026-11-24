import type {
    HeaderEntry,
    Sink,
    Transport,
    TransportRequest,
    TransportResponse,
    TransportWebSocket,
    TransportWebSocketRequest,
    WsMessage,
} from "./types.js";

export type FetchLike = (
    input: string | URL | Request,
    init?: RequestInit,
) => Promise<Response>;

export interface FetchTransportOptions {
    /** Custom WHATWG fetch implementation (defaults to global `fetch`). */
    fetch?: FetchLike;
    /** Custom WebSocket constructor (defaults to global `WebSocket`). */
    WebSocket?: typeof globalThis.WebSocket;
    /** Optional AbortSignal applied to every request made by this transport. */
    signal?: AbortSignal;
    /** Additional request headers merged only when not already present. */
    defaultHeaders?: Record<string, string>;
}

const textDecoder = new TextDecoder();

/**
 * Creates a universal `Transport` backed by standard Web `fetch` and `WebSocket`.
 *
 * Note: browser `fetch()` enforces forbidden-header rules (`User-Agent`, `Referer`,
 * `Origin`, `Sec-Fetch-*`), returns opaque redirects when `followRedirects` is false,
 * and cannot customize TLS ClientHello or proxy routing. For full TLS and header-order
 * impersonation, supply a custom `Transport` backed by a native/proxy client.
 */
export function createFetchTransport(
    options: FetchTransportOptions = {},
): Transport {
    const fetchImpl = options.fetch ?? globalThis.fetch;
    const WebSocketImpl = options.WebSocket ?? globalThis.WebSocket;
    const baseSignal = options.signal;
    const defaultHeaders = options.defaultHeaders;

    return {
        async fetch(request: TransportRequest): Promise<TransportResponse> {
            if (typeof fetchImpl !== "function") {
                throw new Error(
                    "Global `fetch` is unavailable; pass a custom `Transport` or `fetch` option",
                );
            }

            const headers = new Headers();
            for (const [name, value] of request.headers) {
                if (name.toLowerCase() === "content-length") {
                    continue;
                }
                try {
                    headers.append(name, value);
                } catch {
                    // Forbidden or runtime-restricted headers are skipped in browser fetch.
                }
            }
            if (defaultHeaders) {
                for (const [name, value] of Object.entries(defaultHeaders)) {
                    if (!headers.has(name)) {
                        headers.set(name, value);
                    }
                }
            }

            const timeoutSignal =
                request.timeoutMs > 0
                    ? AbortSignal.timeout(request.timeoutMs)
                    : undefined;
            const signal = combineSignals(
                baseSignal,
                request.signal,
                timeoutSignal,
            );

            const bodyBytes =
                request.body && request.body.byteLength > 0
                    ? new Uint8Array(request.body)
                    : undefined;
            const init: RequestInit = {
                method: request.method,
                headers,
                body: bodyBytes,
                redirect: request.followRedirects ? "follow" : "manual",
                credentials: request.includeCredentials ? "include" : "omit",
                signal,
            };

            let response: Response;
            try {
                response = await fetchImpl(request.url, init);
            } catch (err) {
                if (err instanceof DOMException && err.name === "TimeoutError") {
                    throw new Error(
                        `Request to ${request.url} timed out after ${request.timeoutMs}ms`,
                        { cause: err },
                    );
                }
                throw err;
            }

            if (response.type === "opaqueredirect" || response.status === 0) {
                throw new Error(
                    `Opaque redirect for ${request.url} (browser fetch cannot inspect manual 3xx responses; use a custom Transport)`,
                );
            }

            const responseHeaders = extractResponseHeaders(response.headers);
            const rawBuffer = await response.arrayBuffer();
            const body = new Uint8Array(rawBuffer);
            const contentLength = response.headers.get("content-length");
            const parsedLength =
                contentLength !== null
                    ? Number.parseInt(contentLength, 10)
                    : NaN;

            return {
                url: response.url || request.url,
                status: response.status,
                headers: responseHeaders,
                body,
                bodyDecoded: true,
                encodedBytes:
                    Number.isFinite(parsedLength) && parsedLength >= 0
                        ? parsedLength
                        : body.byteLength,
            };
        },

        async websocket(
            request: TransportWebSocketRequest,
        ): Promise<TransportWebSocket> {
            if (typeof WebSocketImpl !== "function") {
                throw new Error(
                    "Global `WebSocket` is unavailable; supply a custom `Transport.websocket`",
                );
            }
            return openWebSocket(WebSocketImpl, request.url);
        },
    };
}

function combineSignals(
    ...candidates: (AbortSignal | undefined)[]
): AbortSignal | undefined {
    const active = candidates.filter(
        (signal): signal is AbortSignal => signal !== undefined,
    );
    if (active.length === 0) {
        return undefined;
    }
    if (active.length === 1) {
        return active[0];
    }
    return AbortSignal.any(active);
}

function extractResponseHeaders(headers: Headers): HeaderEntry[] {
    const entries: HeaderEntry[] = [];
    const setCookies =
        typeof headers.getSetCookie === "function"
            ? headers.getSetCookie()
            : [];
    const hasMultiSetCookie = setCookies.length > 0;

    headers.forEach((value, name) => {
        if (hasMultiSetCookie && name.toLowerCase() === "set-cookie") {
            return;
        }
        entries.push([name, value]);
    });
    for (const cookie of setCookies) {
        entries.push(["set-cookie", cookie]);
    }
    return entries;
}

async function openWebSocket(
    WebSocketImpl: typeof globalThis.WebSocket,
    url: string,
): Promise<TransportWebSocket> {
    const openGate = Promise.withResolvers<void>();
    const socket = new WebSocketImpl(url);
    socket.binaryType = "arraybuffer";

    const queue: (WsMessage | null)[] = [];
    const waiters: {
        resolve: (value: WsMessage | null) => void;
        reject: (err: unknown) => void;
    }[] = [];
    let closed = false;
    let opened = false;
    let terminalError: Error | null = null;
    let blobChain: Promise<void> = Promise.resolve();

    const push = (message: WsMessage | null): void => {
        const waiter = waiters.shift();
        if (waiter) {
            waiter.resolve(message);
        } else {
            queue.push(message);
        }
    };

    const closeWaiters = (): void => {
        while (waiters.length > 0) {
            waiters.shift()!.resolve(null);
        }
    };

    const failWaiters = (error: Error): void => {
        terminalError = error;
        while (waiters.length > 0) {
            waiters.shift()!.reject(error);
        }
    };

    socket.onopen = () => {
        opened = true;
        openGate.resolve();
    };

    socket.onmessage = event => {
        const { data } = event;
        if (typeof data === "string") {
            push({ type: "text", data });
            return;
        }
        if (data instanceof ArrayBuffer) {
            push({ type: "binary", data: new Uint8Array(data) });
            return;
        }
        if (ArrayBuffer.isView(data)) {
            push({
                type: "binary",
                data: new Uint8Array(
                    data.buffer,
                    data.byteOffset,
                    data.byteLength,
                ),
            });
            return;
        }
        if (typeof Blob !== "undefined" && data instanceof Blob) {
            // Chain Blob reads so frames are never reordered or dropped silently.
            blobChain = blobChain.then(
                () =>
                    data.arrayBuffer().then(buffer => {
                        push({ type: "binary", data: new Uint8Array(buffer) });
                    }),
                err => {
                    failWaiters(
                        err instanceof Error ? err : new Error(String(err)),
                    );
                },
            );
        }
    };

    socket.onerror = () => {
        const err = new Error(`WebSocket error on ${url}`);
        if (!opened) {
            openGate.reject(err);
        } else if (!closed) {
            closed = true;
            failWaiters(err);
        }
    };

    socket.onclose = () => {
        if (!opened) {
            openGate.reject(
                new Error(`WebSocket closed before opening: ${url}`),
            );
            return;
        }
        if (!closed) {
            closed = true;
            push({ type: "close" });
            push(null);
            closeWaiters();
        }
    };

    await openGate.promise;

    return {
        sendText(text: string): void {
            socket.send(text);
        },
        recv(): Promise<WsMessage | null> {
            if (queue.length > 0) {
                return Promise.resolve(queue.shift() ?? null);
            }
            if (terminalError) {
                return Promise.reject(terminalError);
            }
            if (closed) {
                return Promise.resolve(null);
            }
            const next = Promise.withResolvers<WsMessage | null>();
            waiters.push(next);
            return next.promise;
        },
        close(): void {
            if (!closed) {
                closed = true;
                closeWaiters();
                socket.close();
            }
        },
    };
}

export interface MemorySink extends Sink {
    readonly artifacts: Map<string, Uint8Array>;
    readonly progressMessages: string[];
    text(name: string): string | undefined;
    json<T = unknown>(name: string): T | undefined;
}

/**
 * Creates an in-memory diagnostic `Sink` capturing all emitted session artifacts
 * and progress messages.
 */
export function createMemorySink(): MemorySink {
    const artifacts = new Map<string, Uint8Array>();
    const progressMessages: string[] = [];

    return {
        artifacts,
        progressMessages,
        artifact(name: string, contents: Uint8Array): void {
            artifacts.set(name, new Uint8Array(contents));
        },
        progress(message: string): void {
            progressMessages.push(message);
        },
        text(name: string): string | undefined {
            const bytes = artifacts.get(name);
            return bytes ? textDecoder.decode(bytes) : undefined;
        },
        json<T = unknown>(name: string): T | undefined {
            const raw = this.text(name);
            return raw === undefined ? undefined : (JSON.parse(raw) as T);
        },
    };
}
