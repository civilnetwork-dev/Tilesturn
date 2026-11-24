# Tilesturn (cfsolve WebAssembly)

Solves Cloudflare Turnstile widgets and challenge pages natively in WebAssembly, and exposes a TypeScript/JavaScript SDK for browser, server, and edge runtimes.

## Language

**Profile**:
A validated JSON description of one impersonated browser (engine, transport headers, timelines, interaction viewport, and probe timings).
_Avoid_: Fingerprint config, device spec

**Answers**:
Recorded responses to browser probes keyed to a reference build digest, replayed and refreshed per session.
_Avoid_: Snapshots, cached probes

**Browser**:
A reusable solver configured from one Profile and its optional Answers; each solve starts an isolated session and cookie jar.
_Avoid_: Client, solver instance

**Turnstile**:
A solve target describing a page URL, its PageSource, and an optional Widget selector or explicit render configuration.
_Avoid_: Captcha, widget task

**PageSource**:
Where a Turnstile solve obtains its embedding page: live navigation, caller-supplied HTML, or a blank page hosting an explicit widget.
_Avoid_: Input mode, HTML source

**Widget**:
A Turnstile sitekey with optional action and cData parameters.
_Avoid_: Container, challenge config

**ChallengePage**:
Cloudflare's interstitial page ("Just a moment...") that loads an origin orchestrator script before rendering an embedded widget and clearance form.
_Avoid_: Interstitial, 403 block page

**Clearance**:
The successful result of clearing a ChallengePage: the `cf_clearance` token, all matching session cookies, and the protected page document.
_Avoid_: Session result, cookie bundle

**Outcome**:
The result of an auto-detected solve: either a Turnstile token or a ChallengePage Clearance.
_Avoid_: SolveResult

**Stage**:
One challenge POST exchange in a session (`initial`, `probes`, `interaction`, `orchestrator`, or `clearance`).
_Avoid_: Phase, step

**Transport**:
The host-supplied network adapter that executes ordered HTTP requests and WebSocket echo exchanges on behalf of the WebAssembly core.
_Avoid_: Fetcher, HTTP backend, driver

**Sink**:
An optional diagnostic observer receiving named session artifacts and human-readable progress messages during a solve.
_Avoid_: Logger, recorder, debugger

**Listing**:
The disassembled instruction graph of a Cloudflare challenge VM program.
_Avoid_: Bytecode dump, disassembly

**ProbePlan**:
The recovered ordered set of browser probes, record envelope fields, payload layout, and vocabulary mappings extracted from a returned VM program.
_Avoid_: Stage2 plan, probe manifest
