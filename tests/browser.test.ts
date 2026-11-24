import { fileURLToPath } from "node:url";

import { beforeAll, describe, expect, it } from "vitest";

import {
    Browser,
    CfsolveError,
    createMemorySink,
    initWasm,
    isCfsolveError,
    type ProfileBundle,
    type Transport,
    type TransportRequest,
    type TransportResponse,
} from "../src/index.ts";
import { createBrowserFromFile, loadSafari26IosProfile } from "../src/node.ts";

let safariBundle: ProfileBundle;

beforeAll(async () => {
    await initWasm();
    safariBundle = await loadSafari26IosProfile();
});

function createMockTransport(
    handler: (
        request: TransportRequest,
    ) => Promise<TransportResponse> | TransportResponse,
): { transport: Transport; requests: TransportRequest[] } {
    const requests: TransportRequest[] = [];
    const transport: Transport = {
        async fetch(request: TransportRequest): Promise<TransportResponse> {
            requests.push(request);
            return handler(request);
        },
    };
    return { transport, requests };
}

describe("Browser profile validation & lifecycle", () => {
    it("loads the tracked iOS Safari 26 profile with widget and challenge-page answers", async () => {
        const browser = await Browser.create(safariBundle);
        try {
            expect(browser.engine).toBe("webkit");
            expect(browser.profileName).toBe("safari_ios_26.2");
            expect(browser.platformName).toBe("ios");
            expect(browser.userAgent).toContain("Safari/604.1");
            expect(browser.hasAnswers).toBe(true);
            expect(browser.hasChallengePageAnswers).toBe(true);
            expect(browser.disposed).toBe(false);
        } finally {
            browser.dispose();
        }
        expect(browser.disposed).toBe(true);
    });

    it("rejects missing required profile fields before any network access", async () => {
        const broken = structuredClone(safariBundle.profile);
        delete (broken as Record<string, unknown>).engine;

        await expect(Browser.create(broken)).rejects.toSatisfy(err => {
            expect(isCfsolveError(err)).toBe(true);
            const cfErr = err as CfsolveError;
            expect(cfErr.code).toBe("config");
            expect(cfErr.retryable).toBe(false);
            expect(cfErr.message).toContain("missing field `engine`");
            return true;
        });
    });

    it("rejects mismatched answers reference digest", async () => {
        const badAnswers = structuredClone(safariBundle.answers!);
        badAnswers.reference = "0000000000000000";

        await expect(
            Browser.create(safariBundle.profile, { answers: badAnswers }),
        ).rejects.toSatisfy(err => {
            const cfErr = err as CfsolveError;
            expect(cfErr.code).toBe("config");
            expect(cfErr.message).toContain("reference build");
            return true;
        });
    });

    it("throws a config error if used after dispose()", async () => {
        const browser = await Browser.create(safariBundle);
        browser.dispose();

        expect(() => browser.engine).toThrow(CfsolveError);
        await expect(
            browser.solve("https://example.com/"),
        ).rejects.toMatchObject({
            code: "config",
            retryable: false,
        });
    });
});

describe("Browser solver seam and error classification", () => {
    it("requires a widget when Turnstile uses a blank page source", async () => {
        const browser = await Browser.create(safariBundle);
        const { transport, requests } = createMockTransport(() => ({
            status: 200,
            headers: [],
            body: "",
        }));

        try {
            await expect(
                browser.solveTurnstile(
                    {
                        url: "https://example.com/login",
                        page: { kind: "blank" },
                    },
                    { transport },
                ),
            ).rejects.toMatchObject({
                code: "config",
                retryable: false,
            });
            expect(requests).toHaveLength(0);
        } finally {
            browser.dispose();
        }
    });

    it("classifies HTTP 403 non-challenge response as blocked and forwards ordered Safari headers", async () => {
        const browser = await Browser.create(safariBundle);
        const { transport, requests } = createMockTransport(req => ({
            url: req.url,
            status: 403,
            headers: [["content-type", "text/html"]],
            body: "<html><body>Access Denied</body></html>",
        }));

        try {
            await expect(
                browser.solve("https://example.com/protected", {
                    transport,
                    proxy: "socks5h://127.0.0.1:1080",
                }),
            ).rejects.toMatchObject({
                code: "blocked",
                status: 403,
                retryable: false,
            });

            expect(requests).toHaveLength(1);
            const sent = requests[0]!;
            expect(sent.method).toBe("GET");
            expect(sent.kind).toBe("navigation");
            expect(sent.proxy).toBe("socks5h://127.0.0.1:1080");
            expect(sent.tls.engine).toBe("webkit");
            expect(sent.tls.profile).toBe("safari_ios_26.2");
            expect(sent.headers.map(([name]) => name)).toEqual([
                "sec-fetch-dest",
                "user-agent",
                "accept",
                "sec-fetch-site",
                "sec-fetch-mode",
                "accept-language",
                "priority",
                "accept-encoding",
            ]);
        } finally {
            browser.dispose();
        }
    });

    it("classifies a plain 200 page without challenge markup as not_challenged on clear()", async () => {
        const browser = await Browser.create(safariBundle);
        const { transport } = createMockTransport(req => ({
            url: req.url,
            status: 200,
            headers: [["content-type", "text/html"]],
            body: "<html><body>Normal page</body></html>",
        }));

        try {
            await expect(
                browser.clear("https://example.com/", { transport }),
            ).rejects.toMatchObject({
                code: "not_challenged",
                retryable: false,
            });
        } finally {
            browser.dispose();
        }
    });

    it("classifies a challenge page returned during solveTurnstile() as retryable challenged", async () => {
        const browser = await Browser.create(safariBundle);
        const challengeHtml = `<html><body><script nonce="n">(function(){window._cf_chl_opt = {cH: 'tok', cRay: 'abc1234567890def', cUPMDTk: "/login?__cf_chl_tk=ans"};var a = document.createElement('script');a.src = '/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=abc1234567890def';history.replaceState(null, null, "/login?__cf_chl_rt_tk=load");document.getElementsByTagName('head')[0].appendChild(a);}());</script></body></html>`;
        const { transport } = createMockTransport(req => ({
            url: req.url,
            status: 403,
            headers: [["content-type", "text/html"]],
            body: challengeHtml,
        }));

        try {
            await expect(
                browser.solveTurnstile(
                    {
                        url: "https://example.com/login",
                        page: { kind: "navigate" },
                    },
                    { transport },
                ),
            ).rejects.toMatchObject({
                code: "challenged",
                status: 403,
                retryable: true,
            });
        } finally {
            browser.dispose();
        }
    });

    it("classifies transport exceptions as retryable network errors", async () => {
        const browser = await Browser.create(safariBundle);
        const { transport } = createMockTransport(() => {
            throw new Error("ECONNRESET");
        });

        try {
            await expect(
                browser.solve("https://example.com/", { transport }),
            ).rejects.toMatchObject({
                code: "network",
                retryable: true,
            });
        } finally {
            browser.dispose();
        }
    });

    it("emits page.html to sink and classifies sink failures as sink errors", async () => {
        const browser = await Browser.create(safariBundle);
        const sink = createMemorySink();
        const { transport } = createMockTransport(req => ({
            url: req.url,
            status: 200,
            headers: [["content-type", "text/html"]],
            body: "<html><body><p>No widget here</p></body></html>",
        }));

        try {
            await expect(
                browser.solve("https://example.com/", { transport, sink }),
            ).rejects.toMatchObject({
                code: "unsupported",
            });
            expect(sink.text("page.html")).toContain("No widget here");

            await expect(
                browser.solve("https://example.com/", {
                    transport,
                    sink: {
                        artifact() {
                            throw new Error("disk full");
                        },
                    },
                }),
            ).rejects.toMatchObject({
                code: "sink",
                retryable: false,
            });
        } finally {
            browser.dispose();
        }
    });

    it("sends caller cookies from clearFetched() on subsequent orchestrator request", async () => {
        const browser = await Browser.create(safariBundle);
        const challengeHtml = `<html><body><script>(function(){window._cf_chl_opt = {cH: 'tok', cRay: 'abc1234567890def', cUPMDTk: "/?__cf_chl_tk=ans"};var a = document.createElement('script');a.src = '/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=abc1234567890def';history.replaceState(null, null, "/?__cf_chl_rt_tk=load");document.head.appendChild(a);}());</script></body></html>`;
        const { transport, requests } = createMockTransport(() => {
            throw new Error("stop after orchestrator request");
        });

        try {
            await expect(
                browser.clearFetched(
                    {
                        url: "https://example.com/",
                        status: 403,
                        html: challengeHtml,
                        setCookies: ["__cf_bm=testcookie; Path=/; Secure"],
                    },
                    { transport },
                ),
            ).rejects.toMatchObject({ code: "network" });

            expect(requests).toHaveLength(1);
            const orchestratorReq = requests[0]!;
            expect(orchestratorReq.kind).toBe("orchestrator");
            expect(orchestratorReq.url).toBe(
                "https://example.com/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1?ray=abc1234567890def",
            );
            const cookieHeader = orchestratorReq.headers.find(
                ([name]) => name === "cookie",
            );
            expect(cookieHeader?.[1]).toBe("__cf_bm=testcookie");
        } finally {
            browser.dispose();
        }
    });

    it("loads a profile directly via createBrowserFromFile() and renders blank Turnstile loader request", async () => {
        const profilePath = fileURLToPath(
            new URL("../profiles/safari-26-ios.json", import.meta.url),
        );
        const browser = await createBrowserFromFile(profilePath);
        const { transport, requests } = createMockTransport(() => {
            throw new Error("stop at api.js loader fetch");
        });

        try {
            await expect(
                browser.solveTurnstile(
                    {
                        url: "https://site.example/login?next=1",
                        page: { kind: "blank" },
                        widget: {
                            sitekey: "0x4AAAAAAA00000000000000",
                            action: "login",
                            cdata: "ctx",
                        },
                    },
                    { transport },
                ),
            ).rejects.toMatchObject({ code: "network", retryable: true });

            expect(requests).toHaveLength(1);
            const loaderReq = requests[0]!;
            expect(loaderReq.kind).toBe("script");
            expect(loaderReq.url).toBe(
                "https://challenges.cloudflare.com/turnstile/v0/api.js",
            );
            expect(loaderReq.followRedirects).toBe(false);
            const referer = loaderReq.headers.find(
                ([name]) => name === "referer",
            );
            expect(referer?.[1]).toBe("https://site.example/");
        } finally {
            browser.dispose();
        }
    });
});
