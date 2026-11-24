const textEncoder = new TextEncoder();

export function urlString(value: string | URL): string {
    return value instanceof URL ? value.toString() : value;
}

export function toBytes(input: Uint8Array | string): Uint8Array {
    return typeof input === "string" ? textEncoder.encode(input) : input;
}
