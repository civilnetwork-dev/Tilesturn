# Tilesturn

Full-featured WebAssembly (`wasm32-unknown-unknown`) port and JavaScript/TypeScript SDK for [cfsolve](https://github.com/chris124567/cfsolve).

Solves Cloudflare Turnstile widgets and challenge pages (`cf_clearance`) natively in WebAssembly, and exposes the underlying Oxc deobfuscator, VM disassembler, stage-2 probe analyzer, payload session encoder, response unmasker, challenge-page parser, referrer-policy resolver, and pure-Rust HTTP content codecs (`gzip`, `deflate`, `br`, `zstd`).

## Architecture

- **`crates/cfsolve`**: Pure-Rust port of `cfsolve` with zero C/C++ or BoringSSL dependencies. Replaces native `wreq`/`tokio`/OS syscalls with a pluggable async `Transport` seam, a pure-Rust RFC 6265 `CookieJar`, pure-Rust compression (`flate2`, `brotli`, `ruzstd`), IANA time-zone evaluation (`chrono-tz`), an 8 MiB WASM linear stack (`-zstack-size=8388608`), and a single-threaded interleaved proof-of-work worker scheduler on `wasm32`.
- **`crates/cfsolve-wasm`**: `wasm-bindgen` bindings compiled with `--target web` into `pkg/`, exposing `WasmBrowser`, `WasmSessionEncoder`, structured classified errors, and all analysis/crypto entry points.
- **`src/`**: TypeScript SDK (`tilesturn`, `tilesturn/node`, `tilesturn/wasm`) providing ergonomic async/sync WASM initialization, `Browser`, `CfsolveError`, `createFetchTransport`, `createMemorySink`, and full analysis helpers.

See [GLOSSARY.md](./GLOSSARY.md) for domain terms and [docs/adr/0001-transport-seam-and-wasm-runtime.md](./docs/adr/0001-transport-seam-and-wasm-runtime.md) for the transport seam decision.

## Quick start (TypeScript / JavaScript)

```ts
import { Browser, createFetchTransport, createMemorySink } from "tilesturn";
import { loadSafari26IosProfile } from "tilesturn/node";

const bundle = await loadSafari26IosProfile();
const browser = await Browser.create(bundle);

const sink = createMemorySink();
const token = await browser.solveTurnstile(
    {
        url: "https://example.com/login",
        page: { kind: "blank" },
        widget: { sitekey: "0x4AAAAAAA...", action: "login" },
    },
    {
        transport: createFetchTransport(),
        sink,
        timeZone: "America/New_York",
    },
);

const cleared = await browser.clear("https://example.com/", {
    transport: createFetchTransport(),
});
console.log(cleared.clearance, cleared.cookies.length);
```

### Custom TLS / proxy `Transport`

Standard Web `fetch()` enforces browser forbidden-header rules (`User-Agent`, `Referer`, `Origin`, `Sec-Fetch-*`) and cannot customize TLS ClientHello. Every outgoing request from the WebAssembly solver includes exact ordered headers, HTTP/2 urgency weights, proxy settings, and TLS profile metadata (`request.tls`), so you can plug in any custom backend (`epoxy-tls`, `libcurl`, `wreq`, `impit`, or a server-side proxy):

```ts
import type { Transport } from "tilesturn";

const customTransport: Transport = {
    async fetch(req) {
        // Forward req.method, req.url, req.headers (in order), req.body,
        // req.followRedirects, req.proxy, req.http2Weight, and req.tls.
        return {
            url: req.url,
            status: 200,
            headers: [["content-type", "text/html"]],
            body: new Uint8Array(),
            // Set bodyDecoded: false if returning raw compressed wire bytes
            // so the WASM core measures exact encoded/decoded sizes.
            bodyDecoded: false,
        };
    },
};
```

### Analysis & cryptographic API

```ts
import {
    analyzeProbes,
    captureAnswers,
    decodeHttpBody,
    decodeResponse,
    deobfuscate,
    disassemble,
    encodeHttpBody,
    extractInlineJavascript,
    extractRay,
    initWasm,
    isPostPath,
    parseChallengePage,
    prepareSession,
    referenceDigest,
    resolveReferrer,
    validateAnswers,
} from "tilesturn";

await initWasm();

const clean = deobfuscate(obfuscatedScript);
const page = parseChallengePage(html, "https://example.com/");
const { policy, referrer } = resolveReferrer({
    html,
    pageUrl: "https://example.com/login",
    targetUrl: "https://challenges.cloudflare.com/turnstile/v0/api.js",
});
```

### Structured errors

All failures reject or throw `CfsolveError` with a typed `code`, `retryable` flag, optional HTTP `status` or challenge `stage`, full error `chain`, and standard `Error.cause`:

| `code` | Meaning | `retryable` |
| --- | --- | --- |
| `config` | Invalid profile, answers, URL, or options (fails before network I/O). | `false` |
| `network` | Transport request/WebSocket error or truncated response body. | `true` |
| `blocked` | Target returned an error HTTP status with no challenge page. | `false` |
| `challenged` | `solveTurnstile` encountered a challenge page (`clear` first). | `true` |
| `not_challenged` | `clear` / `clearFetched` received a page without a challenge. | `false` |
| `rejected` | Cloudflare refused the payload at `stage`. | `true` |
| `sink` | Caller `Sink.artifact` callback threw an error. | `false` |
| `unsupported` | Unrecognized challenge build, script, or missing widget. | `false` |

## Development commands

```sh
bun install
bun run build:wasm        # Compile release wasm32 + wasm-bindgen into pkg/
bun run build:sdk         # Emit ESM + .d.ts into dist/
bun run build             # Build WASM + SDK
bun run test:rust         # Run all pure-Rust unit tests (crates/cfsolve)
bun run test              # Run TypeScript/WASM integration tests (vitest)
bun run typecheck         # TypeScript strict check
bun run format            # Format JS/TS with oxfmt
bun run format:rust       # Format Rust with rustfmt
bun run lint              # Lint JS/TS with oxlint --fix
bun run audit             # oxlint + oxfmt --check + cargo fmt --check + tsc --noEmit
```

## License

MIT (preserving upstream `cfsolve` attribution in [LICENSE](./LICENSE)).
