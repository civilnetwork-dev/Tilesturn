import type { WasmBrowser } from "../pkg/cfsolve_wasm.js";
import { toBytes, urlString } from "./convert.js";
import { CfsolveError, toCfsolveError } from "./errors.js";
import { createFetchTransport } from "./transport.js";
import type {
    Answers,
    Clearance,
    Engine,
    FetchedPage,
    Outcome,
    Profile,
    ProfileBundle,
    SolveOptions,
    Turnstile,
    Widget,
} from "./types.js";
import { initWasm, requireWasm } from "./wasm.js";

export interface BrowserCreateOptions {
    answers?: Answers;
    challengePageAnswers?: Answers;
    /** Default options applied to every solve unless overridden per call. */
    defaultOptions?: SolveOptions;
}

function mergeSolveOptions(
    defaults: SolveOptions | undefined,
    overrides: SolveOptions | undefined,
): Record<string, unknown> {
    const signal = overrides?.signal ?? defaults?.signal;
    const transport =
        overrides?.transport ??
        defaults?.transport ??
        createFetchTransport({ signal });
    const proxy = overrides?.proxy ?? defaults?.proxy;
    const rawRoot = overrides?.trustedRoot ?? defaults?.trustedRoot;
    const trustedRoot = rawRoot === undefined ? undefined : toBytes(rawRoot);
    const timeZone = overrides?.timeZone ?? defaults?.timeZone;
    const sink = overrides?.sink ?? defaults?.sink;

    return {
        transport,
        proxy,
        trustedRoot,
        timeZone,
        sink,
        signal,
    };
}

function isProfileBundle(value: Profile | ProfileBundle): value is ProfileBundle {
    return (
        typeof value === "object" &&
        value !== null &&
        "profile" in value &&
        typeof value.profile === "object" &&
        value.profile !== null
    );
}

/**
 * Validated browser profile shared across concurrent Cloudflare Turnstile and
 * challenge-page solves. Each solve runs with its own cookie jar and session.
 */
export class Browser {
    #handle: WasmBrowser | null;
    readonly #defaultOptions?: SolveOptions;

    private constructor(handle: WasmBrowser, defaultOptions?: SolveOptions) {
        this.#handle = handle;
        this.#defaultOptions = defaultOptions;
    }

    /**
     * Initializes WebAssembly if needed and creates a validated `Browser` from
     * a `Profile` (or `ProfileBundle` containing `profile`, `answers`, and
     * `challengePageAnswers`).
     */
    static async create(
        profileOrBundle: Profile | ProfileBundle,
        options: BrowserCreateOptions = {},
    ): Promise<Browser> {
        await initWasm();
        return Browser.fromSync(profileOrBundle, options);
    }

    /**
     * Synchronously creates a validated `Browser`. Requires `initWasm()` or
     * `initWasmSync()` to have completed first.
     */
    static fromSync(
        profileOrBundle: Profile | ProfileBundle,
        options: BrowserCreateOptions = {},
    ): Browser {
        const wasm = requireWasm();
        const isBundle = isProfileBundle(profileOrBundle);
        const profile = isBundle ? profileOrBundle.profile : profileOrBundle;
        const answers =
            options.answers ??
            (isBundle ? profileOrBundle.answers : undefined);
        const challengePageAnswers =
            options.challengePageAnswers ??
            (isBundle ? profileOrBundle.challengePageAnswers : undefined);

        try {
            const handle = new wasm.WasmBrowser(
                profile,
                answers ?? null,
                challengePageAnswers ?? null,
            );
            return new Browser(handle, options.defaultOptions);
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    #requireHandle(): WasmBrowser {
        if (!this.#handle) {
            throw new CfsolveError("Browser instance has been disposed", {
                code: "config",
            });
        }
        return this.#handle;
    }

    get engine(): Engine {
        return this.#requireHandle().engine as Engine;
    }

    get profileName(): string {
        return this.#requireHandle().profileName;
    }

    get platformName(): string {
        return this.#requireHandle().platformName;
    }

    get userAgent(): string {
        // @ts-expect-error: Doesn't really matter at runtime; pointless
        return this.#requireHandle().userAgent;
    }

    get hasAnswers(): boolean {
        return this.#requireHandle().hasAnswers;
    }

    get hasChallengePageAnswers(): boolean {
        return this.#requireHandle().hasChallengePageAnswers;
    }

    get disposed(): boolean {
        return this.#handle === null;
    }

    /**
     * Solves a Cloudflare Turnstile widget and returns its token.
     */
    async solveTurnstile(
        turnstile: Turnstile,
        options?: SolveOptions,
    ): Promise<string> {
        const handle = this.#requireHandle();
        const wireTurnstile = {
            url: urlString(turnstile.url),
            page: turnstile.page ?? { kind: "navigate" },
            widget: turnstile.widget,
        };
        const wireOptions = mergeSolveOptions(this.#defaultOptions, options);
        try {
            return (await handle.solveTurnstile(
                wireTurnstile,
                wireOptions,
            )) as string;
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    /**
     * Navigates to `url` and clears the Cloudflare challenge page.
     */
    async clear(url: string | URL, options?: SolveOptions): Promise<Clearance> {
        const handle = this.#requireHandle();
        const wireOptions = mergeSolveOptions(this.#defaultOptions, options);
        try {
            return (await handle.clear(
                urlString(url),
                wireOptions,
            )) as Clearance;
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    /**
     * Clears a Cloudflare challenge page already fetched by the caller.
     */
    async clearFetched(
        page: FetchedPage,
        options?: SolveOptions,
    ): Promise<Clearance> {
        const handle = this.#requireHandle();
        const wirePage = {
            url: urlString(page.url),
            status: page.status,
            html: page.html,
            referrerPolicy: page.referrerPolicy,
            setCookies: page.setCookies ? [...page.setCookies] : [],
        };
        const wireOptions = mergeSolveOptions(this.#defaultOptions, options);
        try {
            return (await handle.clearFetched(
                wirePage,
                wireOptions,
            )) as Clearance;
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    /**
     * Navigates to `url` and either clears a challenge page or solves its
     * first Turnstile widget.
     */
    async solve(url: string | URL, options?: SolveOptions): Promise<Outcome> {
        const handle = this.#requireHandle();
        const wireOptions = mergeSolveOptions(this.#defaultOptions, options);
        try {
            return (await handle.solve(urlString(url), wireOptions)) as Outcome;
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    /**
     * Navigates to `url` and sends only the first challenge payload. Useful
     * with a diagnostic `Sink` to inspect the initial POST and returned program.
     */
    async sendFirstPayload(
        url: string | URL,
        widget?: Widget,
        options?: SolveOptions,
    ): Promise<void> {
        const handle = this.#requireHandle();
        const wireOptions = mergeSolveOptions(this.#defaultOptions, options);
        try {
            await handle.sendFirstPayload(
                urlString(url),
                widget ?? null,
                wireOptions,
            );
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    /**
     * Fetches `iframeUrl` and extracts its encoder and payload field map into
     * `options.sink` without sending any POST.
     */
    async prepare(
        iframeUrl: string | URL,
        options?: SolveOptions,
    ): Promise<void> {
        const handle = this.#requireHandle();
        const wireOptions = mergeSolveOptions(this.#defaultOptions, options);
        try {
            await handle.prepare(urlString(iframeUrl), wireOptions);
        } catch (err) {
            throw toCfsolveError(err);
        }
    }

    /** Releases the underlying WebAssembly memory handle. */
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
