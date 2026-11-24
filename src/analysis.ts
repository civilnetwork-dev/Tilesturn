import type { WasmSessionEncoder } from "../pkg/cfsolve_wasm.js";
import { CfsolveError, toCfsolveError } from "./errors.ts";
import type {
    Answers,
    ChallengePage,
    Deobfuscated,
    EncoderMetadata,
    Listing,
    ProbeAnalysisArtifact,
    ReferrerResolution,
} from "./types.ts";
import { requireWasm } from "./wasm.ts";

const textEncoder = new TextEncoder();

function urlString(value: string | URL): string {
    return value instanceof URL ? value.toString() : value;
}

function toBytes(input: Uint8Array | string): Uint8Array {
    return typeof input === "string" ? textEncoder.encode(input) : input;
}

/**
 * Extracted session encoder holding the challenge's custom base64 alphabet,
 * RSA modulus, key secret, and per-session random seed.
 */
export class SessionEncoder {
    #handle: WasmSessionEncoder | null;

    /** @internal */
    constructor(handle: WasmSessionEncoder) {
        this.#handle = handle;
    }

    #requireHandle(): WasmSessionEncoder {
        if (!this.#handle) {
            throw new CfsolveError(
                "SessionEncoder instance has been disposed",
                { code: "config" },
            );
        }
        return this.#handle;
    }

    metadata(): EncoderMetadata {
        try {
            return this.#requireHandle().metadata() as EncoderMetadata;
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    encode(payload: Record<string, unknown>): string {
        try {
            return this.#requireHandle().encode(payload);
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    dispose(): void {
        if (this.#handle) {
            this.#handle.free();
            this.#handle = null;
        }
    }

    [Symbol.dispose](): void {
        this.dispose();
    }
}

/**
 * Deobfuscates a Cloudflare challenge or orchestrator script using Oxc
 * multi-round AST rewrites.
 */
export function deobfuscate(javascript: string): Deobfuscated {
    try {
        return requireWasm().deobfuscate(javascript) as Deobfuscated;
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Extracts the inline `<script>` JavaScript from a challenge or iframe document.
 */
export function extractInlineJavascript(html: string): string {
    try {
        return requireWasm().extractInlineJavascript(html);
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/** Checks whether a URL path matches Cloudflare's challenge POST endpoint. */
export function isPostPath(path: string): boolean {
    return requireWasm().isPostPath(path);
}

/** Extracts the Cloudflare ray identifier from a challenge POST URL or path. */
export function extractRay(urlOrPath: string | URL): string {
    try {
        return requireWasm().extractRay(urlString(urlOrPath));
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/** Unmasks a challenge POST response body using the session ray token. */
export function decodeResponse(
    body: Uint8Array | string,
    token: string,
): Uint8Array {
    try {
        return requireWasm().decodeResponse(toBytes(body), token);
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Parses a deobfuscated iframe or orchestrator script and extracts its
 * `SessionEncoder` and payload template metadata.
 */
export function prepareSession(
    deobfuscatedJs: string,
    iframeUrl: string | URL,
): SessionEncoder {
    try {
        const handle = requireWasm().prepareSession(
            deobfuscatedJs,
            urlString(iframeUrl),
        );
        return new SessionEncoder(handle);
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Symbolically disassembles a returned VM bytecode program using the VM
 * definition recovered from `deobfuscatedJs`.
 */
export function disassemble(
    deobfuscatedJs: string,
    program: Uint8Array,
): Listing {
    try {
        return requireWasm().disassemble(deobfuscatedJs, program) as Listing;
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Analyzes a stage-2 VM program to recover its ordered probes, vocabulary
 * mappings, record envelope fields, and payload layout.
 */
export function analyzeProbes(
    deobfuscatedJs: string,
    program: Uint8Array,
): ProbeAnalysisArtifact {
    try {
        return requireWasm().analyzeProbes(
            deobfuscatedJs,
            program,
        ) as ProbeAnalysisArtifact;
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Builds reusable stored `Answers` from a captured deobfuscated iframe script,
 * probe program bytecode, and second POST plaintext payload.
 */
export function captureAnswers(
    deobfuscatedJs: string,
    program: Uint8Array,
    secondPayload: Record<string, unknown>,
): Answers {
    try {
        return requireWasm().captureAnswers(
            deobfuscatedJs,
            program,
            secondPayload,
        ) as Answers;
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/** Validates stored `Answers` against the embedded reference build digest. */
export function validateAnswers(answers: Answers): void {
    try {
        requireWasm().validateAnswers(answers);
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Parses a Cloudflare challenge page ("Just a moment..."), or returns `null`
 * if `html` is not a challenge page.
 */
export function parseChallengePage(
    html: string,
    url: string | URL,
): ChallengePage | null {
    try {
        const result = requireWasm().parseChallengePage(html, urlString(url));
        return (result ?? null) as ChallengePage | null;
    } catch (err) {
        throw toCfsolveError(err);
    }
}

export interface ResolveReferrerOptions {
    policyHeader?: string;
    html: string;
    pageUrl: string | URL;
    targetUrl: string | URL;
}

/**
 * Resolves a document's W3C Referrer Policy (`Referrer-Policy` header and
 * `<meta name="referrer">`) and computes the `Referer` header sent to `targetUrl`.
 */
export function resolveReferrer(
    options: ResolveReferrerOptions,
): ReferrerResolution {
    try {
        return requireWasm().resolveReferrer(
            options.policyHeader ?? null,
            options.html,
            urlString(options.pageUrl),
            urlString(options.targetUrl),
        ) as ReferrerResolution;
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Decodes an HTTP body according to `Content-Encoding` (`gzip`, `deflate`,
 * `br`, `zstd`, `identity`, or comma-separated chains).
 */
export function decodeHttpBody(
    body: Uint8Array,
    contentEncoding: string,
): Uint8Array {
    try {
        return requireWasm().decodeHttpBody(body, contentEncoding);
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/**
 * Encodes an HTTP body according to `Content-Encoding` (`gzip`, `deflate`,
 * `br`, `zstd`, `identity`, or comma-separated chains).
 */
export function encodeHttpBody(
    body: Uint8Array | string,
    contentEncoding: string,
): Uint8Array {
    try {
        return requireWasm().encodeHttpBody(toBytes(body), contentEncoding);
    } catch (err) {
        throw toCfsolveError(err);
    }
}

/** Returns the embedded stage-2 reference build digest expected by `Answers`. */
export function referenceDigest(): string {
    return requireWasm().referenceDigest();
}
