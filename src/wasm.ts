import * as wasmBindings from "../pkg/cfsolve_wasm.js";
import { CfsolveError, toCfsolveError } from "./errors.js";

export type WasmModule = typeof wasmBindings;

export type WasmInitInput =
    | wasmBindings.InitInput
    | wasmBindings.SyncInitInput
    | Promise<wasmBindings.InitInput>;

let initialized = false;
let pendingInit: Promise<WasmModule> | null = null;

export function isWasmInitialized(): boolean {
    return initialized;
}

export function requireWasm(): WasmModule {
    if (!initialized) {
        throw new CfsolveError(
            "WebAssembly module is not initialized; call `await initWasm()` or `initWasmSync(...)` first",
            { code: "config" },
        );
    }
    return wasmBindings;
}

/**
 * Synchronously initializes the cfsolve WebAssembly module from a compiled
 * `WebAssembly.Module` or raw `.wasm` bytes.
 */
export function initWasmSync(
    module: wasmBindings.SyncInitInput | { module: wasmBindings.SyncInitInput },
): WasmModule {
    if (!initialized) {
        try {
            wasmBindings.initSync(module);
            initialized = true;
        } catch (err) {
            throw toCfsolveError(err);
        }
    }
    return wasmBindings;
}

/**
 * Initializes the cfsolve WebAssembly module once across browsers, Bun, Node,
 * Deno, and Workers. Concurrent calls share the same initialization promise.
 */
export async function initWasm(input?: WasmInitInput): Promise<WasmModule> {
    if (initialized) {
        return wasmBindings;
    }
    if (!pendingInit) {
        pendingInit = loadWasm(input).then(
            () => {
                initialized = true;
                return wasmBindings;
            },
            err => {
                pendingInit = null;
                throw toCfsolveError(err);
            },
        );
    }
    return pendingInit;
}

async function loadWasm(input?: WasmInitInput): Promise<void> {
    if (input !== undefined) {
        await wasmBindings.default({
            module_or_path: input as wasmBindings.InitInput,
        });
        return;
    }
    const defaultUrl = new URL("../pkg/cfsolve_wasm_bg.wasm", import.meta.url);
    // Node's global fetch cannot read file:// URLs, so read bytes directly on
    // server runtimes. Build the specifier dynamically so browser/edge bundlers
    // do not resolve `node:fs/promises` statically.
    if (defaultUrl.protocol === "file:" && typeof process !== "undefined") {
        const fsModule = "node:" + "fs/promises";
        const { readFile } = (await import(/* @vite-ignore */ fsModule)) as {
            readFile: (path: URL) => Promise<Uint8Array>;
        };
        const bytes = await readFile(defaultUrl);
        wasmBindings.initSync({ module: bytes });
        return;
    }
    await wasmBindings.default({ module_or_path: defaultUrl });
}
