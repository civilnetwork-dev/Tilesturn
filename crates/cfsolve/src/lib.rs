//! Solves Cloudflare challenges natively (including in WebAssembly): Turnstile widgets and
//! challenge pages that block access to a site.
//!
//! Load a [`Browser`] profile once and reuse it for multiple solves. Each solve has its own
//! cookies, transport adapter, and proxy settings.

#![warn(unreachable_pub)]
#![cfg_attr(
    not(feature = "internals"),
    allow(dead_code, unreachable_pub, unused_imports)
)]

mod browser;
mod candidates;
mod deep_stack;
mod deobfuscate;
mod embedding_page;
mod engine;
mod error;
mod listing;
mod performance_clock;
pub mod platform;
mod profiling;
mod random_id;
mod resource_timing;
mod session;
mod solver;

// Analysis and protocol modules used by tools and the WebAssembly SDK.
#[doc(hidden)]
pub mod challenge;
#[doc(hidden)]
pub mod fingerprint;
#[doc(hidden)]
pub mod http;
#[doc(hidden)]
pub mod js;
#[doc(hidden)]
pub mod script;
#[doc(hidden)]
pub mod stage2;
#[doc(hidden)]
pub mod vm;

pub use ::cookie::Cookie;
pub use ::http::StatusCode;
pub use browser::Browser;
pub use deobfuscate::{Deobfuscated, Unconverged, deobfuscate};
pub use embedding_page::page::Widget;
pub use engine::Engine;
pub use error::Error;
pub use http::{
    CookieJar, PageDocument, ReferrerPolicy, ResponseMetrics, Transport, TransportError,
    TransportRequest, TransportResponse, TransportTlsInfo, TransportWebSocket,
    TransportWebSocketRequest, WsMessage, decode_body, encode_body,
};
pub use js::{JsObject, JsValue};
pub use session::{Clearance, Outcome, Sink, Stage};
pub use solver::{FetchedPage, Options, PageSource, Turnstile};
pub use stage2::replay::Answers;
pub use url::Url;
