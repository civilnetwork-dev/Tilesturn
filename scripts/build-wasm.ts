import { spawnSync } from "node:child_process";
import { mkdirSync, statSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const debug = process.argv.includes("--debug");
const offline = process.argv.includes("--offline");
const profile = debug ? "debug" : "release";

function run(command: string, args: string[]): void {
    const result = spawnSync(command, args, {
        cwd: root,
        stdio: "inherit",
    });
    if (result.status !== 0) {
        throw new Error(
            `Command failed (${result.status ?? "unknown"}): ${command} ${args.join(" ")}`,
        );
    }
}

const cargoArgs = [
    "build",
    "-p",
    "cfsolve-wasm",
    "--target",
    "wasm32-unknown-unknown",
];
if (offline) {
    cargoArgs.push("--offline");
}
if (!debug) {
    cargoArgs.push("--release");
}
run("cargo", cargoArgs);

const wasmInput = resolve(
    root,
    "target",
    "wasm32-unknown-unknown",
    profile,
    "cfsolve_wasm.wasm",
);
const outDir = resolve(root, "pkg");
mkdirSync(outDir, { recursive: true });

const wasmBindgen = process.env.WASM_BINDGEN_BIN || "wasm-bindgen";
run(wasmBindgen, [
    "--target",
    "web",
    "--out-dir",
    outDir,
    "--out-name",
    "cfsolve_wasm",
    wasmInput,
]);

const wasmOutput = resolve(outDir, "cfsolve_wasm_bg.wasm");
const sizeBytes = statSync(wasmOutput).size;
console.log(
    `Built pkg/cfsolve_wasm_bg.wasm (${(sizeBytes / 1024).toFixed(1)} KiB)`,
);
