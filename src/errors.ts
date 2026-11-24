import type { ErrorCode, Stage } from "./types.js";

export interface WireErrorShape {
    code: ErrorCode;
    message: string;
    retryable: boolean;
    status?: number;
    stage?: Stage;
    chain?: string[];
}

export interface CfsolveErrorOptions extends ErrorOptions {
    code: ErrorCode;
    retryable?: boolean;
    status?: number;
    stage?: Stage;
    chain?: readonly string[];
}

export class CfsolveError extends Error {
    readonly code: ErrorCode;
    readonly retryable: boolean;
    readonly status?: number;
    readonly stage?: Stage;
    readonly chain: readonly string[];

    constructor(message: string, options: CfsolveErrorOptions) {
        super(message, { cause: options.cause });
        this.name = "CfsolveError";
        this.code = options.code;
        this.retryable = options.retryable ?? false;
        this.status = options.status;
        this.stage = options.stage;
        this.chain = options.chain ? [...options.chain] : [message];
    }
}

export function isCfsolveError(error: unknown): error is CfsolveError {
    return error instanceof CfsolveError;
}

function isWireError(value: unknown): value is WireErrorShape {
    if (typeof value !== "object" || value === null) {
        return false;
    }
    const candidate = value as Record<string, unknown>;
    return (
        typeof candidate.code === "string" &&
        typeof candidate.message === "string" &&
        typeof candidate.retryable === "boolean"
    );
}

export function toCfsolveError(thrown: unknown): CfsolveError {
    if (thrown instanceof CfsolveError) {
        return thrown;
    }
    if (isWireError(thrown)) {
        return new CfsolveError(thrown.message, {
            code: thrown.code,
            retryable: thrown.retryable,
            status: thrown.status,
            stage: thrown.stage,
            chain: thrown.chain,
            cause: thrown,
        });
    }
    if (thrown instanceof Error) {
        return new CfsolveError(thrown.message, {
            code: "unsupported",
            retryable: false,
            cause: thrown,
        });
    }
    const message =
        typeof thrown === "string" ? thrown : "Unexpected cfsolve WASM error";
    return new CfsolveError(message, {
        code: "unsupported",
        retryable: false,
        cause: thrown,
    });
}
