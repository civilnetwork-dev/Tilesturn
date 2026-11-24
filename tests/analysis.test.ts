import { beforeAll, describe, expect, it } from "vitest";

import {
    analyzeProbes,
    CfsolveError,
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
} from "../src/index.ts";
import { loadSafari26IosProfile } from "../src/node.ts";

beforeAll(async () => {
    await initWasm();
});

describe("deobfuscate and script extraction", () => {
    it("simplifies obfuscated JavaScript expressions and constant control flow", () => {
        const obfuscated =
            "function f(){var a=1+2;if(false){return 99;}return a;}";
        const result = deobfuscate(obfuscated);
        expect(result.unconverged).toBeUndefined();
        expect(result.text).toContain("return 3");
        expect(result.text).not.toContain("99");
    });

    it("extracts raw inline JavaScript while skipping external and JSON scripts", () => {
        const html = `<!doctype html><html><head>
            <script src="/external.js"></script>
            <script type="application/json">{"ignored":true}</script>
            <script nonce="abc">window._cf_chl_opt = { cRay: "1234567890abcdef" };</script>
        </head><body></body></html>`;
        const inline = extractInlineJavascript(html);
        expect(inline).toContain("window._cf_chl_opt");
        expect(inline).not.toContain("ignored");
    });
});

describe("challenge path, response unmasking, and challenge page parsing", () => {
    it("recognizes challenge POST paths and extracts the ray identifier", () => {
        const path =
            "/cdn-cgi/challenge-platform/h/b/fo/123456:1790000000:sig/9abc1234567890ef/token";
        expect(isPostPath(path)).toBe(true);
        expect(isPostPath("/login")).toBe(false);
        expect(extractRay(path)).toBe("9abc1234567890ef");
        expect(extractRay(`https://challenges.cloudflare.com${path}`)).toBe(
            "9abc1234567890ef",
        );
    });

    it("unmasks a Cloudflare response body using the ray token", () => {
        const token = "abcdef0123456789";
        const keyBytes = new TextEncoder().encode(`${token}_0`);
        let mask = 32;
        for (const byte of keyBytes) {
            mask ^= byte;
        }
        const plaintext = new Uint8Array([72, 101, 108, 108, 111, 33]);
        const masked = new Uint8Array(plaintext.length);
        for (let i = 0; i < plaintext.length; i += 1) {
            masked[i] = (plaintext[i]! + mask + (i % 65_535)) % 255;
        }
        const b64 = Buffer.from(masked).toString("base64");
        const decoded = decodeResponse(b64, token);
        expect(Array.from(decoded)).toEqual(Array.from(plaintext));
    });

    it("parses a Cloudflare challenge page and returns null for ordinary pages", () => {
        const html = `<html><body><script nonce="n">(function(){window._cf_chl_opt = {cH: 'token', cRay: 'ray', cUPMDTk: "/protected?__cf_chl_tk=answered"};var a = document.createElement('script');a.src = '/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=ray';history.replaceState(null, null, "/protected?__cf_chl_rt_tk=loading");document.getElementsByTagName('head')[0].appendChild(a);}());</script></body></html>`;
        const parsed = parseChallengePage(
            html,
            "https://example.com/protected",
        );
        expect(parsed).not.toBeNull();
        expect(parsed!.orchestrator).toBe(
            "https://example.com/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=ray",
        );
        expect(parsed!.loading).toBe(
            "https://example.com/protected?__cf_chl_rt_tk=loading",
        );
        expect(parsed!.answered).toBe(
            "https://example.com/protected?__cf_chl_tk=answered",
        );

        expect(
            parseChallengePage(
                "<html><body>Hello</body></html>",
                "https://example.com/",
            ),
        ).toBeNull();
    });
});

describe("HTTP codec, referrer policy, and reference answers", () => {
    it("round-trips gzip, deflate, brotli, zstd, and chained encodings in pure Rust WASM", () => {
        const payload =
            "Cloudflare Turnstile WASM body round-trip test payload 0123456789";
        const decoder = new TextDecoder();

        for (const encoding of ["gzip", "deflate", "br", "zstd", "identity"]) {
            const encoded = encodeHttpBody(payload, encoding);
            const decoded = decodeHttpBody(encoded, encoding);
            expect(decoder.decode(decoded)).toBe(payload);
        }

        const chained = encodeHttpBody(payload, "zstd, gzip");
        expect(decoder.decode(decodeHttpBody(chained, "ZSTD, GZip"))).toBe(
            payload,
        );
    });

    it("resolves W3C Referrer Policy headers and meta overrides", () => {
        const crossOrigin = resolveReferrer({
            html: "<p>hello</p>",
            pageUrl: "https://site.example/login?next=1#frag",
            targetUrl: "https://challenges.cloudflare.com/turnstile/v0/api.js",
        });
        expect(crossOrigin).toEqual({
            policy: "strict-origin-when-cross-origin",
            referrer: "https://site.example/",
        });

        const overridden = resolveReferrer({
            policyHeader: "unsafe-url",
            html: '<head><meta name="referrer" content="no-referrer"></head>',
            pageUrl: "https://site.example/login",
            targetUrl: "https://site.example/next",
        });
        expect(overridden).toEqual({
            policy: "no-referrer",
            referrer: undefined,
        });
    });

    it("exposes the reference build digest and validates stored answers", async () => {
        const digest = referenceDigest();
        expect(digest).toMatch(/^[0-9a-f]{16}$/);

        const bundle = await loadSafari26IosProfile();
        expect(bundle.answers?.reference).toBe(digest);
        expect(() => validateAnswers(bundle.answers!)).not.toThrow();

        const stale = structuredClone(bundle.answers!);
        stale.reference = "deadbeefcafebabe";
        expect(() => validateAnswers(stale)).toThrow(CfsolveError);
    });

    it("throws structured CfsolveError for non-VM or non-session scripts", () => {
        expect(() =>
            prepareSession(
                "var x = 1;",
                "https://challenges.cloudflare.com/iframe",
            ),
        ).toThrow(CfsolveError);
        expect(() => disassemble("var x = 1;", new Uint8Array([0]))).toThrow(
            CfsolveError,
        );
        expect(() => analyzeProbes("var x = 1;", new Uint8Array([0]))).toThrow(
            CfsolveError,
        );
    });
});
