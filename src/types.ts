export type Engine = "chromium" | "webkit";

export type Stage =
    | "initial"
    | "probes"
    | "interaction"
    | "orchestrator"
    | "clearance";

export type ErrorCode =
    | "config"
    | "network"
    | "blocked"
    | "challenged"
    | "not_challenged"
    | "rejected"
    | "sink"
    | "unsupported";

export interface ClientHello {
    advertisedSigalgsPrefix?: number[];
    greaseSigalgs?: boolean;
    requestedTrustAnchors?: string | null;
}

export interface TransportTlsInfo {
    engine: Engine;
    profile: string;
    platform: string;
    clientHello: ClientHello;
}

export type HeaderEntry = readonly [name: string, value: string];

export type HeadersInput =
    | readonly HeaderEntry[]
    | Record<string, string | readonly string[]>;

export interface TransportRequest {
    method: string;
    url: string;
    headers: readonly HeaderEntry[];
    body?: Uint8Array;
    followRedirects: boolean;
    maxRedirects: number;
    includeCredentials: boolean;
    preserveEncoding: boolean;
    timeoutMs: number;
    kind: string;
    http2Weight?: number;
    proxy?: string;
    trustedRoot?: Uint8Array;
    signal?: AbortSignal;
    tls: TransportTlsInfo;
}

export interface TransportResponse {
    url?: string;
    status: number;
    headers: HeadersInput;
    body: Uint8Array | ArrayBuffer | string;
    /** Set true when the transport already decompressed `Content-Encoding` (e.g. Web `fetch()`). */
    bodyDecoded?: boolean;
    /** Wire byte size before decompression, when `bodyDecoded` is true and known. */
    encodedBytes?: number;
}

export interface TransportWebSocketRequest {
    url: string;
    headers: readonly HeaderEntry[];
    proxy?: string;
    trustedRoot?: Uint8Array;
    tls: TransportTlsInfo;
}

export type WsMessage =
    | { type: "text"; data: string }
    | { type: "binary"; data: Uint8Array }
    | { type: "ping"; data?: Uint8Array }
    | { type: "pong"; data?: Uint8Array }
    | { type: "close" };

export interface TransportWebSocket {
    sendText(text: string): Promise<void> | void;
    recv(): Promise<WsMessage | null>;
    close(): Promise<void> | void;
}

export interface Transport {
    fetch(request: TransportRequest): Promise<TransportResponse>;
    websocket?(request: TransportWebSocketRequest): Promise<TransportWebSocket>;
}

export interface Sink {
    artifact?(name: string, contents: Uint8Array): void;
    progress?(message: string): void;
}

export interface SolveOptions {
    transport?: Transport;
    proxy?: string;
    trustedRoot?: Uint8Array | string;
    /** Optional IANA time zone (e.g. `"America/New_York"`) overriding the host zone. */
    timeZone?: string;
    sink?: Sink;
    /** Optional AbortSignal forwarded to the default fetch transport. */
    signal?: AbortSignal;
}

export interface Widget {
    sitekey: string;
    action?: string;
    cdata?: string;
}

export type PageSource =
    | { kind: "navigate" }
    | { kind: "html"; html: string; referrerPolicy?: string }
    | { kind: "blank" };

export interface Turnstile {
    url: string | URL;
    page?: PageSource;
    widget?: Widget;
}

export interface FetchedPage {
    url: string | URL;
    status: number;
    html: string;
    referrerPolicy?: string;
    setCookies?: readonly string[];
}

export interface Cookie {
    name: string;
    value: string;
    domain?: string;
    path?: string;
    secure?: boolean;
    httpOnly?: boolean;
}

export interface PageDocument {
    url: string;
    status: number;
    html: string;
}

export interface Clearance {
    clearance: string;
    cookies: Cookie[];
    page: PageDocument;
}

export type Outcome =
    | { kind: "token"; token: string }
    | ({ kind: "cleared" } & Clearance);

export interface StoredAnswer {
    duration: number | null;
    fields: Record<string, unknown>;
    late?: string[];
}

export interface Answers {
    reference: string;
    probes: Record<string, StoredAnswer>;
    navigator: Record<string, unknown>;
    globals?: Record<string, unknown>;
    stylesheets?: unknown[];
}

export interface Profile {
    engine: Engine;
    transport: {
        profile: string;
        platform: string;
        headers?: Record<string, string>;
        client_hello?: ClientHello;
        requests: {
            weights?: Record<string, number>;
            headers: Record<string, string[]>;
        };
    };
    browser?: Record<string, unknown>;
    timeline: Record<string, number>;
    challenge_page_timeline?: Record<string, number>;
    intervals: Record<string, number>;
    orchestrator: Record<string, unknown>;
    resource_timing: Record<string, unknown>;
    interaction: Record<string, unknown>;
    second: Record<string, unknown>;
    answers?: string;
    challenge_page_answers?: string;
    [key: string]: unknown;
}

export interface ProfileBundle {
    profile: Profile;
    answers?: Answers;
    challengePageAnswers?: Answers;
}

export interface Unconverged {
    rounds: number;
    message: string;
}

export interface Deobfuscated {
    text: string;
    unconverged?: Unconverged;
}

export interface ChallengePage {
    url: string;
    orchestrator: string;
    loading: string;
    answered?: string;
    inline: string;
}

export type ReferrerPolicyName =
    | "no-referrer"
    | "no-referrer-when-downgrade"
    | "same-origin"
    | "origin"
    | "strict-origin"
    | "origin-when-cross-origin"
    | "strict-origin-when-cross-origin"
    | "unsafe-url";

export interface ReferrerResolution {
    policy: ReferrerPolicyName;
    referrer?: string;
}

export interface ExportedPayloadField {
    name: string;
    expression: string;
    bootstrap_value?: unknown;
}

export interface EncoderMetadata {
    url: string;
    iframe_url: string;
    cf_chl: string;
    options: Record<string, unknown>;
    encoder: string;
    key_transform: string;
    key_secret: string;
    initialize_key: string;
    bootstrap: string;
    alphabet: string;
    modulus: string;
    payload_fields: ExportedPayloadField[];
}

export interface Position {
    pc: number;
    key: number;
}

export interface InstructionCondition {
    expression: string;
    branch: boolean;
}

export type InstructionExit =
    | { kind: "next"; pc: number; key: number }
    | { kind: "return" }
    | { kind: "throw" }
    | { kind: "unfollowed"; pc: number; key: number; reason: string };

export interface InstructionPath {
    conditions: InstructionCondition[];
    effects: string[];
    exit: InstructionExit;
}

export interface Instruction {
    pc: number;
    key: number;
    opcode: number;
    paths: InstructionPath[];
}

export type ListingCause =
    | { kind: "pastEnd" }
    | { kind: "undecoded"; reason: string }
    | { kind: "noHandler"; opcode: number }
    | { kind: "faults"; opcode: number; reasons: string[] }
    | { kind: "tooManyPaths"; opcode: number; limit: number };

export interface ListingFailure {
    pc: number;
    key: number;
    message: string;
    cause: ListingCause;
}

export interface HiddenBranch {
    compare: number;
    string: string;
    target: Position;
}

export interface Listing {
    text: string;
    instructionCount: number;
    instructions: Instruction[];
    functions: Position[];
    failures: ListingFailure[];
    hidden: HiddenBranch[];
    encoded: string[];
}

export type ProgramOutcome =
    | { kind: "complete"; token: string }
    | { kind: "interaction" }
    | { kind: "other"; strings: string[] };

export interface InteractionPlan {
    click_delay_ms: number;
    initialization: Record<string, unknown>;
    click: Record<string, unknown>;
    diagnostics: Record<string, unknown>;
    ui: Record<string, unknown>;
    proof: Record<string, unknown>;
    proof_fields: Record<string, unknown>;
    payload: Record<string, unknown>;
    [key: string]: unknown;
}

export interface ProbePlan {
    record_fields: Record<string, unknown>;
    payload: Record<string, unknown>;
    probes: Record<string, unknown>[];
    controller_trace?: string | null;
    [key: string]: unknown;
}
