import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Browser, type BrowserCreateOptions } from "./browser.js";
import { CfsolveError } from "./errors.js";
import type { Answers, Profile, ProfileBundle } from "./types.js";
import { initWasmSync, type WasmModule } from "./wasm.js";

/**
 * Loads a profile JSON file and resolves any relative `answers` and
 * `challenge_page_answers` files named inside it, matching Rust `Browser::read`.
 */
export async function loadProfileFromFile(
    profilePath: string,
): Promise<ProfileBundle> {
    const resolvedPath = resolve(profilePath);
    const directory = dirname(resolvedPath);
    let profile: Profile;
    try {
        profile = JSON.parse(await readFile(resolvedPath, "utf8")) as Profile;
    } catch (err) {
        throw new CfsolveError(
            `Failed to read profile ${resolvedPath}: ${err instanceof Error ? err.message : String(err)}`,
            { code: "config", cause: err },
        );
    }

    const [answers, challengePageAnswers] = await Promise.all([
        readOptionalAnswers(directory, profile.answers),
        readOptionalAnswers(directory, profile.challenge_page_answers),
    ]);

    return {
        profile: structuredClone(profile),
        answers,
        challengePageAnswers,
    };
}

async function readOptionalAnswers(
    directory: string,
    relativePath: string | undefined,
): Promise<Answers | undefined> {
    if (!relativePath) {
        return undefined;
    }
    const target = resolve(directory, relativePath);
    try {
        return JSON.parse(await readFile(target, "utf8")) as Answers;
    } catch (err) {
        throw new CfsolveError(
            `Failed to read stored answers ${target}: ${err instanceof Error ? err.message : String(err)}`,
            { code: "config", cause: err },
        );
    }
}

/**
 * Loads the tracked iOS Safari 26 profile bundled with Tilesturn, including
 * its widget and challenge-page stored answers.
 */
export async function loadSafari26IosProfile(): Promise<ProfileBundle> {
    const profileUrl = new URL(
        "../profiles/safari-26-ios.json",
        import.meta.url,
    );
    return loadProfileFromFile(fileURLToPath(profileUrl));
}

/**
 * Loads a profile file and constructs a validated `Browser` in one step.
 */
export async function createBrowserFromFile(
    profilePath: string,
    options: Omit<
        BrowserCreateOptions,
        "answers" | "challengePageAnswers"
    > = {},
): Promise<Browser> {
    const bundle = await loadProfileFromFile(profilePath);
    return Browser.create(bundle, options);
}

/**
 * Initializes the WebAssembly module directly from a local `.wasm` file path.
 */
export async function initWasmFromFile(wasmPath?: string): Promise<WasmModule> {
    const resolved = wasmPath
        ? resolve(wasmPath)
        : fileURLToPath(
              new URL("../pkg/cfsolve_wasm_bg.wasm", import.meta.url),
          );
    const bytes = await readFile(resolved);
    return initWasmSync({ module: bytes });
}
