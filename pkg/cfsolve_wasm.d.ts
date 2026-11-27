/* tslint:disable */
/* eslint-disable */

/**
 * A validated browser profile shared across concurrent Cloudflare solves.
 */
export class WasmBrowser {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Clears a challenge page already fetched by the caller.
     */
    clearFetched(page: any, options: any): Promise<any>;
    /**
     * Navigates to `url` and clears the Cloudflare challenge page.
     */
    clear(url: string, options: any): Promise<any>;
    /**
     * Constructs and validates a `Browser` from a `Profile` JSON object and optional stored
     * `Answers`.
     */
    constructor(profile: any, answers: any, challenge_page_answers: any);
    /**
     * Fetches `iframe_url` and emits encoder and payload field artifacts to `options.sink`.
     */
    prepare(iframe_url: string, options: any): Promise<any>;
    /**
     * Navigates to `url` and sends only the first payload (diagnostic helper).
     */
    sendFirstPayload(url: string, widget: any, options: any): Promise<any>;
    /**
     * Solves a Turnstile widget and resolves to its token string.
     */
    solveTurnstile(turnstile: any, options: any): Promise<any>;
    /**
     * Navigates to `url` and either clears a challenge page or solves its first Turnstile widget.
     */
    solve(url: string, options: any): Promise<any>;
    readonly engine: string;
    readonly hasAnswers: boolean;
    readonly hasChallengePageAnswers: boolean;
    readonly platformName: string;
    readonly profileName: string;
    readonly userAgent: string;
}

/**
 * Extracted session encoding parameters paired with a live session encoder seed.
 */
export class WasmSessionEncoder {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Encodes a plaintext payload object into Cloudflare's encrypted POST envelope string.
     */
    encode(payload: any): string;
    /**
     * Returns the extracted `encoder.json` metadata (URL, options, alphabet, modulus, key secret,
     * bootstrap source, and payload template fields).
     */
    metadata(): any;
}

/**
 * Analyzes a returned stage-2 VM `program` to recover ordered probes, vocabulary mappings,
 * record envelope fields, and payload layout (`ProbePlan`).
 */
export function analyzeProbes(deobfuscated_js: string, program: Uint8Array | number[]): any;

/**
 * Builds reusable stored [`Answers`] from a captured deobfuscated iframe script, probe program
 * bytecode, and second POST plaintext payload.
 */
export function captureAnswers(deobfuscated_js: string, program: Uint8Array | number[], second_payload: any): any;

/**
 * Decodes an HTTP response body according to `Content-Encoding` (`gzip`, `deflate`, `br`, `zstd`,
 * `identity`, or comma-separated chains).
 */
export function decodeHttpBody(body: Uint8Array | number[], encoding: string): Uint8Array;

/**
 * Unmasks a challenge POST response body using the session ray `token`.
 */
export function decodeResponse(body: Uint8Array | number[], token: string): Uint8Array;

/**
 * Deobfuscates a Cloudflare challenge or orchestrator script using Oxc multi-round rewrites.
 */
export function deobfuscate(javascript: string): any;

/**
 * Disassembles a returned VM bytecode `program` using the VM recovered from `deobfuscated_js`.
 */
export function disassemble(deobfuscated_js: string, program: Uint8Array | number[]): any;

/**
 * Encodes an HTTP body according to `Content-Encoding` (`gzip`, `deflate`, `br`, `zstd`,
 * `identity`, or comma-separated chains).
 */
export function encodeHttpBody(body: Uint8Array | number[], encoding: string): Uint8Array;

/**
 * Extracts the inline `<script>` JavaScript from a challenge or iframe HTML document.
 */
export function extractInlineJavascript(html: string): string;

/**
 * Extracts the Cloudflare ray identifier from a challenge POST URL or path.
 */
export function extractRay(url_or_path: string): string;

/**
 * Checks whether a URL path matches Cloudflare's challenge POST endpoint shape.
 */
export function isPostPath(path: string): boolean;

/**
 * Attaches verified CSSOM stylesheet measurements to stored [`Answers`] after confirming they
 * reproduce the capture's reported stylesheet hash.
 */
export function measureStylesheets(answers: any, deobfuscated_js: string, program: Uint8Array | number[], html: string, templates: any): any;

/**
 * Parses a Cloudflare challenge page ("Just a moment...") or returns `null` if `html` is not one.
 */
export function parseChallengePage(html: string, url: string): any;

/**
 * Parses a deobfuscated iframe/orchestrator script and extracts its session encoder and payload
 * template.
 */
export function prepareSession(deobfuscated_js: string, iframe_url: string): WasmSessionEncoder;

/**
 * Infers the challenge outcome (completion token, interaction required, or other strings) from a
 * returned VM bytecode `program`.
 */
export function programOutcome(deobfuscated_js: string, program: Uint8Array | number[]): any;

/**
 * Recovers the checkbox interaction and proof-of-work plan from a returned interaction VM
 * bytecode `program`.
 */
export function recoverInteraction(deobfuscated_js: string, program: Uint8Array | number[]): any;

/**
 * Returns the embedded stage-2 reference build digest expected by stored `Answers`.
 */
export function referenceDigest(): string;

/**
 * Resolves the W3C Referrer Policy of a document (`Referrer-Policy` header + `<meta name="referrer">`)
 * and computes the `Referer` header sent from `page_url` to `target_url`.
 */
export function resolveReferrer(policy_header: string | null | undefined, html: string, page_url: string, target_url: string): any;

/**
 * Validates stored `Answers` JSON against the embedded reference build digest.
 */
export function validateAnswers(answers: any): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_wasmbrowser_free: (a: number, b: number) => void;
    readonly __wbg_wasmsessionencoder_free: (a: number, b: number) => void;
    readonly analyzeProbes: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly captureAnswers: (a: number, b: number, c: number, d: number, e: any) => [number, number, number];
    readonly decodeHttpBody: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly decodeResponse: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly deobfuscate: (a: number, b: number) => [number, number, number];
    readonly disassemble: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly encodeHttpBody: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly extractInlineJavascript: (a: number, b: number) => [number, number, number, number];
    readonly extractRay: (a: number, b: number) => [number, number, number, number];
    readonly isPostPath: (a: number, b: number) => number;
    readonly measureStylesheets: (a: any, b: number, c: number, d: number, e: number, f: number, g: number, h: any) => [number, number, number];
    readonly parseChallengePage: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly prepareSession: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly programOutcome: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly recoverInteraction: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly referenceDigest: () => [number, number];
    readonly resolveReferrer: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number];
    readonly validateAnswers: (a: any) => [number, number];
    readonly wasmbrowser_clear: (a: number, b: number, c: number, d: any) => any;
    readonly wasmbrowser_clearFetched: (a: number, b: any, c: any) => any;
    readonly wasmbrowser_engine: (a: number) => [number, number];
    readonly wasmbrowser_hasAnswers: (a: number) => number;
    readonly wasmbrowser_hasChallengePageAnswers: (a: number) => number;
    readonly wasmbrowser_new: (a: any, b: any, c: any) => [number, number, number];
    readonly wasmbrowser_platformName: (a: number) => [number, number];
    readonly wasmbrowser_prepare: (a: number, b: number, c: number, d: any) => any;
    readonly wasmbrowser_profileName: (a: number) => [number, number];
    readonly wasmbrowser_sendFirstPayload: (a: number, b: number, c: number, d: any, e: any) => any;
    readonly wasmbrowser_solve: (a: number, b: number, c: number, d: any) => any;
    readonly wasmbrowser_solveTurnstile: (a: number, b: any, c: any) => any;
    readonly wasmbrowser_userAgent: (a: number) => [number, number];
    readonly wasmsessionencoder_encode: (a: number, b: any) => [number, number, number, number];
    readonly wasmsessionencoder_metadata: (a: number) => [number, number, number];
    readonly wasm_bindgen_9a45dd5869c46998___convert__closures_____invoke___js_sys_4f479ebc062d42eb___Function_fn_wasm_bindgen_9a45dd5869c46998___JsValue_____wasm_bindgen_9a45dd5869c46998___sys__Undefined___js_sys_4f479ebc062d42eb___Function_fn_wasm_bindgen_9a45dd5869c46998___JsValue_____wasm_bindgen_9a45dd5869c46998___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_9a45dd5869c46998___convert__closures_____invoke___wasm_bindgen_9a45dd5869c46998___JsValue__core_d941ad8722425d2e___result__Result_____wasm_bindgen_9a45dd5869c46998___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_9a45dd5869c46998___convert__closures_____invoke_______true_: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
