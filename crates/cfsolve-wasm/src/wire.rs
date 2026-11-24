//! Wire types crossing the WebAssembly ↔ JavaScript boundary.
//!
//! Uses JSON-compatible serialization so Rust maps always become plain JavaScript objects rather
//! than ES `Map` instances.

use cfsolve::challenge::ChallengePage;
use cfsolve::http::ReferrerPolicy;
use cfsolve::stage2::analysis::ProgramOutcome;
use cfsolve::vm::{Cause, Exit, Listing};
use cfsolve::{Clearance, Cookie, Error, Outcome, Stage, Widget};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// Typed error categories for non-solver analysis/utility functions.
#[derive(Clone, Copy)]
pub(crate) enum ErrorKind {
    Config,
    Unsupported,
}

impl ErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Serializes `value` via `serde_json` + `JSON.parse` to preserve exact key insertion order
/// (`IndexMap`), integer map keys, and round-trip float representation.
pub(crate) fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    let json = serde_json::to_string(value)
        .map_err(|error| wire_config_error(format!("serializing WASM output: {error}")))?;
    js_sys::JSON::parse(&json)
        .map_err(|error| wire_config_error(format!("parsing serialized WASM output: {error:?}")))
}

/// Deserializes `value` via `JSON.stringify` + `serde_json` so JS string keys coerce cleanly to
/// Rust integer map keys (`BTreeMap<usize, Stored>`, `BTreeMap<u8, u16>`) and preserve key order.
pub(crate) fn from_js<T: for<'de> Deserialize<'de>>(
    value: JsValue,
    what: &str,
) -> Result<T, JsValue> {
    let json = js_sys::JSON::stringify(&value)
        .map_err(|error| wire_config_error(format!("invalid {what}: {error:?}")))?
        .as_string()
        .ok_or_else(|| {
            wire_config_error(format!("invalid {what}: value is not JSON-serializable"))
        })?;
    serde_json::from_str(&json)
        .map_err(|error| wire_config_error(format!("invalid {what}: {error}")))
}

/// Structured error thrown across the WASM boundary and wrapped into `CfsolveError` by the SDK.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub chain: Vec<String>,
}

pub(crate) fn wire_error(error: &Error) -> JsValue {
    let (message, status, stage, chain) = match error {
        Error::Config(err) | Error::Network(err) | Error::Sink(err) | Error::Unsupported(err) => {
            (format!("{err:#}"), None, None, anyhow_chain(err))
        }
        Error::Blocked { status } | Error::Challenged { status } => (
            error.to_string(),
            Some(status.as_u16()),
            None,
            vec![error.to_string()],
        ),
        Error::NotChallenged => (error.to_string(), None, None, vec![error.to_string()]),
        Error::Rejected { stage, error: err } => (
            format!("{err:#}"),
            None,
            Some(stage_name(*stage).to_owned()),
            anyhow_chain(err),
        ),
        _ => (error.to_string(), None, None, vec![error.to_string()]),
    };
    let wire = WireError {
        code: error.code().to_owned(),
        message: message.clone(),
        retryable: error.is_retryable(),
        status,
        stage,
        chain,
    };
    to_js(&wire).unwrap_or_else(|_| JsValue::from_str(&message))
}

pub(crate) fn wire_anyhow(kind: ErrorKind, error: anyhow::Error) -> JsValue {
    let message = format!("{error:#}");
    let wire = WireError {
        code: kind.as_str().to_owned(),
        message: message.clone(),
        retryable: false,
        status: None,
        stage: None,
        chain: anyhow_chain(&error),
    };
    to_js(&wire).unwrap_or_else(|_| JsValue::from_str(&message))
}

pub(crate) fn wire_config_error(message: impl Into<String>) -> JsValue {
    let message = message.into();
    let wire = WireError {
        code: "config".to_owned(),
        message: message.clone(),
        retryable: false,
        status: None,
        stage: None,
        chain: vec![message.clone()],
    };
    to_js(&wire).unwrap_or_else(|_| JsValue::from_str(&message))
}

fn anyhow_chain(error: &anyhow::Error) -> Vec<String> {
    error.chain().map(ToString::to_string).collect()
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Initial => "initial",
        Stage::Probes => "probes",
        Stage::Interaction => "interaction",
        Stage::Orchestrator => "orchestrator",
        Stage::Clearance => "clearance",
        _ => "unknown",
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireWidget {
    pub sitekey: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub cdata: Option<String>,
}

impl From<WireWidget> for Widget {
    fn from(wire: WireWidget) -> Self {
        Self {
            sitekey: wire.sitekey,
            action: wire.action,
            cdata: wire.cdata,
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WirePageSource {
    Navigate,
    Html {
        html: String,
        #[serde(default, rename = "referrerPolicy")]
        referrer_policy: Option<String>,
    },
    Blank,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireTurnstile {
    pub url: String,
    #[serde(default)]
    pub page: Option<WirePageSource>,
    #[serde(default)]
    pub widget: Option<WireWidget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireFetchedPage {
    pub url: String,
    pub status: u16,
    pub html: String,
    #[serde(default)]
    pub referrer_policy: Option<String>,
    #[serde(default)]
    pub set_cookies: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireCookie {
    pub name: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_only: Option<bool>,
}

impl From<&Cookie<'static>> for WireCookie {
    fn from(cookie: &Cookie<'static>) -> Self {
        Self {
            name: cookie.name().to_owned(),
            value: cookie.value().to_owned(),
            domain: cookie.domain().map(str::to_owned),
            path: cookie.path().map(str::to_owned),
            secure: cookie.secure(),
            http_only: cookie.http_only(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WirePageDocument {
    pub url: String,
    pub status: u16,
    pub html: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireClearance {
    pub clearance: String,
    pub cookies: Vec<WireCookie>,
    pub page: WirePageDocument,
}

impl From<Clearance> for WireClearance {
    fn from(cleared: Clearance) -> Self {
        Self {
            clearance: cleared.clearance,
            cookies: cleared.cookies.iter().map(WireCookie::from).collect(),
            page: WirePageDocument {
                url: cleared.page.url.to_string(),
                status: cleared.page.status.as_u16(),
                html: cleared.page.html,
            },
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WireOutcome {
    Token {
        token: String,
    },
    Cleared {
        clearance: String,
        cookies: Vec<WireCookie>,
        page: WirePageDocument,
    },
}

impl From<Outcome> for WireOutcome {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Token(token) => Self::Token { token },
            Outcome::Cleared(cleared) => {
                let wire = WireClearance::from(cleared);
                Self::Cleared {
                    clearance: wire.clearance,
                    cookies: wire.cookies,
                    page: wire.page,
                }
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireDeobfuscated {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unconverged: Option<WireUnconverged>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireUnconverged {
    pub rounds: usize,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireChallengePage {
    pub url: String,
    pub orchestrator: String,
    pub loading: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered: Option<String>,
    pub inline: String,
}

impl From<ChallengePage> for WireChallengePage {
    fn from(page: ChallengePage) -> Self {
        Self {
            url: page.url.to_string(),
            orchestrator: page.orchestrator.to_string(),
            loading: page.loading.to_string(),
            answered: page.answered.map(|url| url.to_string()),
            inline: page.inline,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireReferrerResolution {
    pub policy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
}

pub(crate) fn policy_name(policy: ReferrerPolicy) -> &'static str {
    match policy {
        ReferrerPolicy::NoReferrer => "no-referrer",
        ReferrerPolicy::NoReferrerWhenDowngrade => "no-referrer-when-downgrade",
        ReferrerPolicy::SameOrigin => "same-origin",
        ReferrerPolicy::Origin => "origin",
        ReferrerPolicy::StrictOrigin => "strict-origin",
        ReferrerPolicy::OriginWhenCrossOrigin => "origin-when-cross-origin",
        ReferrerPolicy::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
        ReferrerPolicy::UnsafeUrl => "unsafe-url",
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WireProgramOutcome {
    Complete { token: String },
    Interaction,
    Other { strings: Vec<String> },
}

impl From<ProgramOutcome> for WireProgramOutcome {
    fn from(outcome: ProgramOutcome) -> Self {
        match outcome {
            ProgramOutcome::Complete { token } => Self::Complete { token },
            ProgramOutcome::Interaction => Self::Interaction,
            ProgramOutcome::Other(strings) => Self::Other { strings },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireListing {
    pub text: String,
    pub instruction_count: usize,
    pub instructions: Vec<WireInstruction>,
    pub functions: Vec<WirePosition>,
    pub failures: Vec<WireFailure>,
    pub hidden: Vec<WireHidden>,
    pub encoded: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WirePosition {
    pub pc: u32,
    pub key: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireInstruction {
    pub pc: u32,
    pub key: u8,
    pub opcode: u8,
    pub paths: Vec<WirePath>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WirePath {
    pub conditions: Vec<WireCondition>,
    pub effects: Vec<String>,
    pub exit: WireExit,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireCondition {
    pub expression: String,
    pub branch: bool,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WireExit {
    Next { pc: u32, key: u8 },
    Return,
    Throw,
    Unfollowed { pc: u32, key: u8, reason: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireFailure {
    pub pc: u32,
    pub key: u8,
    pub message: String,
    pub cause: WireCause,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WireCause {
    PastEnd,
    Undecoded { reason: String },
    NoHandler { opcode: u8 },
    Faults { opcode: u8, reasons: Vec<String> },
    TooManyPaths { opcode: u8, limit: usize },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WireHidden {
    pub compare: u32,
    pub string: String,
    pub target: WirePosition,
}

impl From<&Listing> for WireListing {
    fn from(listing: &Listing) -> Self {
        use std::fmt::Write as _;

        let mut text = String::new();
        for inst in listing.instructions.values() {
            let _ = write!(text, "{inst}");
        }
        let instructions = listing
            .instructions
            .values()
            .map(|inst| WireInstruction {
                pc: inst.pc,
                key: inst.key,
                opcode: inst.opcode,
                paths: inst
                    .paths
                    .iter()
                    .map(|path| WirePath {
                        conditions: path
                            .conditions
                            .iter()
                            .map(|(term, branch)| WireCondition {
                                expression: term.to_string(),
                                branch: *branch,
                            })
                            .collect(),
                        effects: path.effects.iter().map(ToString::to_string).collect(),
                        exit: match &path.exit {
                            Exit::Next { pc, key } => WireExit::Next { pc: *pc, key: *key },
                            Exit::Return => WireExit::Return,
                            Exit::Throw => WireExit::Throw,
                            Exit::Unfollowed { pc, key, reason } => WireExit::Unfollowed {
                                pc: *pc,
                                key: *key,
                                reason: reason.clone(),
                            },
                        },
                    })
                    .collect(),
            })
            .collect();
        let functions = listing
            .functions
            .iter()
            .map(|pos| WirePosition {
                pc: pos.pc,
                key: pos.key,
            })
            .collect();
        let failures = listing
            .failures
            .iter()
            .map(|failure| WireFailure {
                pc: failure.pc,
                key: failure.key,
                message: failure.to_string(),
                cause: match &failure.cause {
                    Cause::PastEnd => WireCause::PastEnd,
                    Cause::Undecoded { reason } => WireCause::Undecoded {
                        reason: reason.clone(),
                    },
                    Cause::NoHandler { opcode } => WireCause::NoHandler { opcode: *opcode },
                    Cause::Faults { opcode, reasons } => WireCause::Faults {
                        opcode: *opcode,
                        reasons: reasons.clone(),
                    },
                    Cause::TooManyPaths { opcode, limit } => WireCause::TooManyPaths {
                        opcode: *opcode,
                        limit: *limit,
                    },
                },
            })
            .collect();
        let hidden = listing
            .hidden
            .iter()
            .map(|entry| WireHidden {
                compare: entry.compare,
                string: entry.string.clone(),
                target: WirePosition {
                    pc: entry.target.pc,
                    key: entry.target.key,
                },
            })
            .collect();
        Self {
            text,
            instruction_count: listing.instructions.len(),
            instructions,
            functions,
            failures,
            hidden,
            encoded: listing.encoded.iter().cloned().collect(),
        }
    }
}
