//! WebAssembly bindings exposing the full `cfsolve` solver, deobfuscator, VM disassembler,
//! stage-2 probe analyzer, payload encoder, and HTTP codec suite to JavaScript/TypeScript.

mod bridge;
mod wire;

use std::sync::Arc;

use cfsolve::challenge::program::{ExtractedProgram, PreparedSession, SessionEncoder};
use cfsolve::challenge::{self, ChallengePage};
use cfsolve::http::{self, ReferrerPolicy};
use cfsolve::js::Payload;
use cfsolve::script::Source;
use cfsolve::stage2::analysis::{self, ProgramOutcome};
use cfsolve::stage2::interaction;
use cfsolve::stage2::replay::Stylesheet;
use cfsolve::stage2;
use cfsolve::{Answers, Browser, FetchedPage, PageSource, StatusCode, Turnstile, Url, Widget};
use js_sys::{Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::bridge::parse_options;
use crate::wire::{
    ErrorKind, WireChallengePage, WireClearance, WireDeobfuscated, WireFetchedPage, WireListing,
    WireOutcome, WirePageSource, WireProgramOutcome, WireReferrerResolution, WireTurnstile,
    WireUnconverged, WireWidget, from_js, policy_name, to_js, wire_anyhow, wire_config_error,
    wire_error,
};

/// A validated browser profile shared across concurrent Cloudflare solves.
#[wasm_bindgen]
pub struct WasmBrowser {
    inner: Arc<Browser>,
}

#[wasm_bindgen]
impl WasmBrowser {
    /// Constructs and validates a `Browser` from a `Profile` JSON object and optional stored
    /// `Answers`.
    #[wasm_bindgen(constructor)]
    pub fn new(
        profile: JsValue,
        answers: JsValue,
        challenge_page_answers: JsValue,
    ) -> Result<Self, JsValue> {
        let profile: serde_json::Value = from_js(profile, "profile")?;
        let answers: Option<Answers> = if answers.is_undefined() || answers.is_null() {
            None
        } else {
            Some(from_js(answers, "stored answers")?)
        };
        let challenge_page_answers: Option<Answers> =
            if challenge_page_answers.is_undefined() || challenge_page_answers.is_null() {
                None
            } else {
                Some(from_js(
                    challenge_page_answers,
                    "challenge page stored answers",
                )?)
            };
        let inner = Browser::try_new(profile, answers, challenge_page_answers)
            .map_err(|error| wire_error(&error))?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    #[wasm_bindgen(getter)]
    pub fn engine(&self) -> String {
        match self.inner.engine() {
            cfsolve::Engine::Chromium => "chromium".to_owned(),
            cfsolve::Engine::WebKit => "webkit".to_owned(),
        }
    }

    #[wasm_bindgen(getter, js_name = profileName)]
    pub fn profile_name(&self) -> String {
        self.inner.profile_name().to_owned()
    }

    #[wasm_bindgen(getter, js_name = platformName)]
    pub fn platform_name(&self) -> String {
        self.inner.platform_name().to_owned()
    }

    #[wasm_bindgen(getter, js_name = userAgent)]
    pub fn user_agent(&self) -> String {
        self.inner.user_agent().to_owned()
    }

    #[wasm_bindgen(getter, js_name = hasAnswers)]
    pub fn has_answers(&self) -> bool {
        self.inner.has_answers()
    }

    #[wasm_bindgen(getter, js_name = hasChallengePageAnswers)]
    pub fn has_challenge_page_answers(&self) -> bool {
        self.inner.has_challenge_page_answers()
    }

    /// Solves a Turnstile widget and resolves to its token string.
    #[wasm_bindgen(js_name = solveTurnstile)]
    pub fn solve_turnstile(&self, turnstile: JsValue, options: JsValue) -> Promise {
        let browser = Arc::clone(&self.inner);
        future_to_promise(async move {
            let wire: WireTurnstile = from_js(turnstile, "turnstile options")?;
            let url = parse_url(&wire.url, "turnstile.url")?;
            let page = match wire.page.unwrap_or(WirePageSource::Navigate) {
                WirePageSource::Navigate => PageSource::Navigate,
                WirePageSource::Html {
                    html,
                    referrer_policy,
                } => PageSource::Html {
                    html,
                    referrer_policy,
                },
                WirePageSource::Blank => PageSource::Blank,
            };
            let turnstile = Turnstile {
                url,
                page,
                widget: wire.widget.map(Widget::from),
            };
            let options = parse_options(options)?;
            let token = browser
                .solve_turnstile(turnstile, &options)
                .await
                .map_err(|error| wire_error(&error))?;
            Ok(JsValue::from_str(&token))
        })
    }

    /// Navigates to `url` and clears the Cloudflare challenge page.
    pub fn clear(&self, url: String, options: JsValue) -> Promise {
        let browser = Arc::clone(&self.inner);
        future_to_promise(async move {
            let url = parse_url(&url, "url")?;
            let options = parse_options(options)?;
            let cleared = browser
                .clear(&url, &options)
                .await
                .map_err(|error| wire_error(&error))?;
            to_js(&WireClearance::from(cleared))
        })
    }

    /// Clears a challenge page already fetched by the caller.
    #[wasm_bindgen(js_name = clearFetched)]
    pub fn clear_fetched(&self, page: JsValue, options: JsValue) -> Promise {
        let browser = Arc::clone(&self.inner);
        future_to_promise(async move {
            let wire: WireFetchedPage = from_js(page, "fetched page")?;
            let url = parse_url(&wire.url, "page.url")?;
            let status = StatusCode::from_u16(wire.status)
                .map_err(|err| wire_config_error(format!("invalid page.status: {err}")))?;
            let page = FetchedPage {
                url,
                status,
                html: wire.html,
                referrer_policy: wire.referrer_policy,
                set_cookies: wire.set_cookies,
            };
            let options = parse_options(options)?;
            let cleared = browser
                .clear_fetched(page, &options)
                .await
                .map_err(|error| wire_error(&error))?;
            to_js(&WireClearance::from(cleared))
        })
    }

    /// Navigates to `url` and either clears a challenge page or solves its first Turnstile widget.
    pub fn solve(&self, url: String, options: JsValue) -> Promise {
        let browser = Arc::clone(&self.inner);
        future_to_promise(async move {
            let url = parse_url(&url, "url")?;
            let options = parse_options(options)?;
            let outcome = browser
                .solve(&url, &options)
                .await
                .map_err(|error| wire_error(&error))?;
            to_js(&WireOutcome::from(outcome))
        })
    }

    /// Navigates to `url` and sends only the first payload (diagnostic helper).
    #[wasm_bindgen(js_name = sendFirstPayload)]
    pub fn send_first_payload(&self, url: String, widget: JsValue, options: JsValue) -> Promise {
        let browser = Arc::clone(&self.inner);
        future_to_promise(async move {
            let url = parse_url(&url, "url")?;
            let widget: Option<Widget> = if widget.is_undefined() || widget.is_null() {
                None
            } else {
                Some(from_js::<WireWidget>(widget, "widget")?.into())
            };
            let options = parse_options(options)?;
            browser
                .send_first_payload(&url, widget.as_ref(), &options)
                .await
                .map_err(|error| wire_error(&error))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Fetches `iframe_url` and emits encoder and payload field artifacts to `options.sink`.
    pub fn prepare(&self, iframe_url: String, options: JsValue) -> Promise {
        let browser = Arc::clone(&self.inner);
        future_to_promise(async move {
            let iframe_url = parse_url(&iframe_url, "iframeUrl")?;
            let options = parse_options(options)?;
            browser
                .prepare(iframe_url, &options)
                .await
                .map_err(|error| wire_error(&error))?;
            Ok(JsValue::UNDEFINED)
        })
    }
}

/// Extracted session encoding parameters paired with a live session encoder seed.
#[wasm_bindgen]
pub struct WasmSessionEncoder {
    program: ExtractedProgram,
    encoder: SessionEncoder,
}

#[wasm_bindgen]
impl WasmSessionEncoder {
    /// Returns the extracted `encoder.json` metadata (URL, options, alphabet, modulus, key secret,
    /// bootstrap source, and payload template fields).
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        to_js(&self.program.encoder_json())
    }

    /// Encodes a plaintext payload object into Cloudflare's encrypted POST envelope string.
    pub fn encode(&self, payload: JsValue) -> Result<String, JsValue> {
        let payload: Payload = from_js(payload, "payload object")?;
        Ok(self.encoder.encode(&payload))
    }
}

/// Deobfuscates a Cloudflare challenge or orchestrator script using Oxc multi-round rewrites.
#[wasm_bindgen]
pub fn deobfuscate(javascript: &str) -> Result<JsValue, JsValue> {
    let result = cfsolve::deobfuscate(javascript.to_owned())
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let wire = WireDeobfuscated {
        text: result.text,
        unconverged: result.unconverged.map(|unconverged| WireUnconverged {
            rounds: unconverged.rounds,
            message: unconverged.to_string(),
        }),
    };
    to_js(&wire)
}

/// Extracts the inline `<script>` JavaScript from a challenge or iframe HTML document.
#[wasm_bindgen(js_name = extractInlineJavascript)]
pub fn extract_inline_javascript(html: &str) -> Result<String, JsValue> {
    challenge::inline_javascript(html).map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))
}

/// Checks whether a URL path matches Cloudflare's challenge POST endpoint shape.
#[wasm_bindgen(js_name = isPostPath)]
pub fn is_post_path(path: &str) -> bool {
    challenge::is_post_path(path)
}

/// Extracts the Cloudflare ray identifier from a challenge POST URL or path.
#[wasm_bindgen(js_name = extractRay)]
pub fn extract_ray(url_or_path: &str) -> Result<String, JsValue> {
    let url = Url::parse(url_or_path)
        .or_else(|_| {
            Url::parse("https://challenges.cloudflare.com")
                .expect("base URL")
                .join(url_or_path)
        })
        .map_err(|error| wire_config_error(format!("invalid url {url_or_path:?}: {error}")))?;
    challenge::ray(&url)
        .map(str::to_owned)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))
}

/// Unmasks a challenge POST response body using the session ray `token`.
#[wasm_bindgen(js_name = decodeResponse)]
pub fn decode_response(body: &[u8], token: &str) -> Result<Uint8Array, JsValue> {
    let decoded = challenge::decode_response(body, token)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    Ok(Uint8Array::from(decoded.as_slice()))
}

/// Parses a deobfuscated iframe/orchestrator script and extracts its session encoder and payload
/// template.
#[wasm_bindgen(js_name = prepareSession)]
pub fn prepare_session(
    deobfuscated_js: &str,
    iframe_url: &str,
) -> Result<WasmSessionEncoder, JsValue> {
    let url = parse_url(iframe_url, "iframeUrl")?;
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let prepared = PreparedSession::try_new(&source, &url)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let program = prepared.into_program();
    let encoder = program
        .session_encoder()
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    Ok(WasmSessionEncoder { program, encoder })
}

/// Disassembles a returned VM bytecode `program` using the VM recovered from `deobfuscated_js`.
#[wasm_bindgen]
pub fn disassemble(deobfuscated_js: &str, program: &[u8]) -> Result<JsValue, JsValue> {
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let listing = source
        .disassemble(program)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    to_js(&WireListing::from(&listing))
}

/// Infers the challenge outcome (completion token, interaction required, or other strings) from a
/// returned VM bytecode `program`.
#[wasm_bindgen(js_name = programOutcome)]
pub fn program_outcome(deobfuscated_js: &str, program: &[u8]) -> Result<JsValue, JsValue> {
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let listing = source
        .disassemble(program)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    to_js(&WireProgramOutcome::from(ProgramOutcome::of(&listing)))
}

/// Recovers the checkbox interaction and proof-of-work plan from a returned interaction VM
/// bytecode `program`.
#[wasm_bindgen(js_name = recoverInteraction)]
pub fn recover_interaction(deobfuscated_js: &str, program: &[u8]) -> Result<JsValue, JsValue> {
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let listing = source
        .disassemble(program)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let plan = interaction::recover(&listing)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    to_js(&plan)
}

/// Analyzes a returned stage-2 VM `program` to recover ordered probes, vocabulary mappings,
/// record envelope fields, and payload layout (`ProbePlan`).
#[wasm_bindgen(js_name = analyzeProbes)]
pub fn analyze_probes(deobfuscated_js: &str, program: &[u8]) -> Result<JsValue, JsValue> {
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let analyzed = analysis::analyze(&source, program)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    to_js(&analyzed.artifact())
}

/// Builds reusable stored [`Answers`] from a captured deobfuscated iframe script, probe program
/// bytecode, and second POST plaintext payload.
#[wasm_bindgen(js_name = captureAnswers)]
pub fn capture_answers(
    deobfuscated_js: &str,
    program: &[u8],
    second_payload: JsValue,
) -> Result<JsValue, JsValue> {
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    let payload: Payload = from_js(second_payload, "second payload")?;
    let answers = Answers::capture(&source, program, &payload)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    to_js(&answers)
}

/// Attaches verified CSSOM stylesheet measurements to stored [`Answers`] after confirming they
/// reproduce the capture's reported stylesheet hash.
#[wasm_bindgen(js_name = measureStylesheets)]
pub fn measure_stylesheets(
    answers: JsValue,
    deobfuscated_js: &str,
    program: &[u8],
    html: &str,
    templates: JsValue,
) -> Result<JsValue, JsValue> {
    let mut answers: Answers = from_js(answers, "stored answers")?;
    let templates: Vec<Stylesheet> = from_js(templates, "stylesheet templates")?;
    let source = Source::try_new(deobfuscated_js)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    answers
        .measure_stylesheets(&source, program, html, templates)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    to_js(&answers)
}

/// Validates stored `Answers` JSON against the embedded reference build digest.
#[wasm_bindgen(js_name = validateAnswers)]
pub fn validate_answers(answers: JsValue) -> Result<(), JsValue> {
    let answers: Answers = from_js(answers, "stored answers")?;
    answers
        .check()
        .map_err(|error| wire_anyhow(ErrorKind::Config, error))
}

/// Parses a Cloudflare challenge page ("Just a moment...") or returns `null` if `html` is not one.
#[wasm_bindgen(js_name = parseChallengePage)]
pub fn parse_challenge_page(html: &str, url: &str) -> Result<JsValue, JsValue> {
    let url = parse_url(url, "url")?;
    let parsed = ChallengePage::parse(html, &url)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    match parsed {
        Some(page) => to_js(&WireChallengePage::from(page)),
        None => Ok(JsValue::NULL),
    }
}

/// Resolves the W3C Referrer Policy of a document (`Referrer-Policy` header + `<meta name="referrer">`)
/// and computes the `Referer` header sent from `page_url` to `target_url`.
#[wasm_bindgen(js_name = resolveReferrer)]
pub fn resolve_referrer(
    policy_header: Option<String>,
    html: &str,
    page_url: &str,
    target_url: &str,
) -> Result<JsValue, JsValue> {
    let page = parse_url(page_url, "pageUrl")?;
    let target = parse_url(target_url, "targetUrl")?;
    let policy = ReferrerPolicy::of_document(policy_header.as_deref(), html);
    let referrer = policy.referrer(&page, &target);
    to_js(&WireReferrerResolution {
        policy: policy_name(policy),
        referrer,
    })
}

/// Decodes an HTTP response body according to `Content-Encoding` (`gzip`, `deflate`, `br`, `zstd`,
/// `identity`, or comma-separated chains).
#[wasm_bindgen(js_name = decodeHttpBody)]
pub fn decode_http_body(body: &[u8], encoding: &str) -> Result<Uint8Array, JsValue> {
    let decoded = http::decode_body(body, encoding)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    Ok(Uint8Array::from(decoded.as_slice()))
}

/// Encodes an HTTP body according to `Content-Encoding` (`gzip`, `deflate`, `br`, `zstd`,
/// `identity`, or comma-separated chains).
#[wasm_bindgen(js_name = encodeHttpBody)]
pub fn encode_http_body(body: &[u8], encoding: &str) -> Result<Uint8Array, JsValue> {
    let encoded = http::encode_body(body, encoding)
        .map_err(|error| wire_anyhow(ErrorKind::Unsupported, error))?;
    Ok(Uint8Array::from(encoded.as_slice()))
}

/// Returns the embedded stage-2 reference build digest expected by stored `Answers`.
#[wasm_bindgen(js_name = referenceDigest)]
pub fn reference_digest() -> String {
    stage2::reference_digest().to_owned()
}

fn parse_url(raw: &str, field: &str) -> Result<Url, JsValue> {
    Url::parse(raw).map_err(|error| wire_config_error(format!("invalid {field} {raw:?}: {error}")))
}
