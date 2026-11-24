# Transport Seam and Pure-Rust WebAssembly Runtime

Status: accepted

Upstream `cfsolve` couples its solver to BoringSSL (`wreq` / `btls-sys`), `tokio` multi-threaded threads, C `zstd`, and host OS syscalls (`clock_gettime`, `QueryPerformanceCounter`, `iana-time-zone`). None of those compile cleanly to `wasm32-unknown-unknown`.

We decided to isolate all network I/O behind a small async `Transport` seam (ordered HTTP requests + WebSocket echo) while keeping an RFC 6265 cookie jar, header ordering, response decompression (`flate2`, `brotli`, pure-Rust `ruzstd`), deobfuscation, VM disassembly, probe analysis, and payload crypto inside the Rust core compiled to WebAssembly. On `wasm32`, OS primitives (clocks, timers, timezone resolution, single-threaded interleaved proof search) live behind `platform.rs`, and the WASM binary links with an 8 MiB linear stack (`-zstack-size=8388608`) to match upstream's dedicated deobfuscation stack.

## Consequences

- Zero C/C++ build dependencies: the Rust core compiles and runs its entire unit test suite natively on Windows/Linux/macOS and compiles identically to `wasm32-unknown-unknown`.
- Callers on browsers, Bun, Node, or edge runtimes can use the built-in Web `fetch` + `WebSocket` adapter out of the box, or plug in a TLS-impersonating backend (such as `epoxy-tls`, `libcurl`, or a server-side proxy) through the same `Transport` interface using the TLS/ClientHello/HTTP2 metadata attached to every request.
- Standard Web `fetch()` cannot set forbidden headers (`User-Agent`, `Referer`, `Origin`, `Sec-Fetch-*`) or spoof TLS ClientHello; production Cloudflare solves that require browser TLS/header parity must supply a custom `Transport` adapter that forwards the ordered headers and TLS profile metadata.
