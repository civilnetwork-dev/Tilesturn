//! HTTP transport and response handling for challenge sessions.

mod client;
mod decode;
mod header_order;
mod referrer;
mod requests;
pub mod transport;

pub use client::{BrowserClient, ClientHello, Requests};
pub(crate) use client::default_user_agent;
pub use decode::{
    ResponseMetrics, decode_body, encode_body, header_text, read_encoded, text_headers,
};
pub use header_order::HeaderOrder;
pub use referrer::ReferrerPolicy;
pub use requests::{
    FetchOptions, Iframe, PageDocument, ScriptLoad, fetch_from_worker, fetch_iframe, fetch_image,
    fetch_orchestrator, fetch_page_image, fetch_script, navigate, open_websocket, post_challenge,
    post_orchestrator, submit_form,
};
pub use transport::{
    CookieJar, Transport, TransportError, TransportRequest, TransportResponse, TransportTlsInfo,
    TransportWebSocket, TransportWebSocketRequest, WsMessage,
};
