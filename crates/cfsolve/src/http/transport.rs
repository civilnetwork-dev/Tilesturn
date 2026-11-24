//! Pluggable HTTP/WebSocket transport seam and pure-Rust RFC 6265 cookie jar.
//!
//! All network I/O crosses [`Transport`], allowing the core solver to run identically in native
//! tests, browsers (`fetch`/`WebSocket`), and TLS-impersonating host adapters without native TLS
//! or C dependencies.

use std::error::Error as StdError;
use std::fmt;
use std::sync::Mutex;

use cookie::Cookie;
use http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use super::client::ClientHello;
use crate::engine::Engine;
use crate::platform::{self, BoxFuture, MaybeSend};

/// Transport-layer failure mapped to [`crate::Error::Network`].
#[derive(Debug, Clone)]
pub struct TransportError(String);

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl StdError for TransportError {}

/// TLS and browser profile metadata forwarded to custom transports capable of TLS/HTTP2
/// impersonation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportTlsInfo {
    pub engine: Engine,
    pub profile: String,
    pub platform: String,
    pub client_hello: ClientHello,
}

/// One outgoing HTTP request built by [`super::BrowserClient`] with ordered headers.
#[derive(Clone, Debug)]
pub struct TransportRequest {
    pub method: Method,
    pub url: Url,
    /// Headers in the profile's exact browser order (including `Cookie` when credentials apply).
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub follow_redirects: bool,
    pub max_redirects: u8,
    pub include_credentials: bool,
    pub preserve_encoding: bool,
    pub timeout_ms: u32,
    pub kind: String,
    pub http2_weight: Option<u16>,
    pub proxy: Option<String>,
    pub trusted_root: Option<Vec<u8>>,
    pub tls: TransportTlsInfo,
}

/// One HTTP response returned by a [`Transport`].
#[derive(Clone, Debug)]
pub struct TransportResponse {
    /// Final URL after any redirects.
    pub url: Url,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    /// True when the transport already decompressed `Content-Encoding` (e.g. Web `fetch()`).
    pub body_decoded: bool,
    /// Optional compressed wire byte length if the transport already decompressed `body`.
    pub encoded_bytes: Option<u64>,
}

/// WebSocket handshake request built by [`super::BrowserClient`].
#[derive(Clone, Debug)]
pub struct TransportWebSocketRequest {
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub proxy: Option<String>,
    pub trusted_root: Option<Vec<u8>>,
    pub tls: TransportTlsInfo,
}

/// One WebSocket frame exchanged during the WebSocket echo probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}

/// An open WebSocket connection returned by [`Transport::open_websocket`].
pub trait TransportWebSocket: MaybeSend {
    fn send_text(&mut self, text: String) -> BoxFuture<'_, Result<(), TransportError>>;
    fn recv(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>>;
    fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>>;
}

/// Network adapter for HTTP and WebSocket operations.
pub trait Transport: MaybeSend {
    fn fetch(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'_, Result<TransportResponse, TransportError>>;

    fn open_websocket(
        &self,
        _request: TransportWebSocketRequest,
    ) -> BoxFuture<'_, Result<Box<dyn TransportWebSocket>, TransportError>> {
        Box::pin(async {
            Err(TransportError::new(
                "WebSocket transport is not supported by this adapter",
            ))
        })
    }
}

/// Pure-Rust RFC 6265 cookie jar stored per solve session.
#[derive(Default)]
pub struct CookieJar {
    entries: Mutex<Vec<StoredCookie>>,
}

#[derive(Clone, Debug)]
struct StoredCookie {
    name: String,
    value: String,
    domain: String,
    host_only: bool,
    path: String,
    secure: bool,
    http_only: bool,
    expires_at_ms: Option<i64>,
}

impl CookieJar {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a `Set-Cookie` header received from `url` and updates the jar.
    pub fn add_set_cookie(&self, header: &str, url: &Url) {
        let Ok(parsed) = Cookie::parse(header) else {
            return;
        };
        let Some(request_host) = url.host_str().map(str::to_ascii_lowercase) else {
            return;
        };
        let (domain, host_only) = match parsed.domain() {
            Some(raw) => {
                let candidate = raw.trim().trim_start_matches('.').to_ascii_lowercase();
                if candidate.is_empty() || !domain_matches(&request_host, &candidate) {
                    return;
                }
                (candidate, false)
            }
            None => (request_host, true),
        };
        let path = parsed
            .path()
            .filter(|path| path.starts_with('/'))
            .map(str::to_owned)
            .unwrap_or_else(|| default_cookie_path(url.path()));
        let now_ms = platform::epoch_us() / 1000;
        let expires_at_ms = if let Some(max_age) = parsed.max_age() {
            let delta_ms = max_age
                .whole_milliseconds()
                .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
            Some(now_ms.saturating_add(delta_ms))
        } else {
            parsed
                .expires_datetime()
                .map(|dt| (dt.unix_timestamp_nanos() / 1_000_000) as i64)
        };
        let mut entries = self.entries.lock().expect("cookie jar lock");
        entries.retain(|existing| {
            !(existing.name == parsed.name() && existing.domain == domain && existing.path == path)
        });
        if expires_at_ms.is_some_and(|exp| exp <= now_ms) {
            return;
        }
        entries.push(StoredCookie {
            name: parsed.name().to_owned(),
            value: parsed.value().to_owned(),
            domain,
            host_only,
            path,
            secure: parsed.secure().unwrap_or(false),
            http_only: parsed.http_only().unwrap_or(false),
            expires_at_ms,
        });
    }

    /// Returns all unexpired cookies matching `url`, ordered with longest paths first per RFC 6265.
    #[must_use]
    pub fn matches(&self, url: &Url) -> Vec<Cookie<'static>> {
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
            return Vec::new();
        };
        let is_secure = matches!(url.scheme(), "https" | "wss");
        let path = url.path();
        let now_ms = platform::epoch_us() / 1000;
        let mut entries = self.entries.lock().expect("cookie jar lock");
        entries.retain(|cookie| cookie.expires_at_ms.is_none_or(|exp| exp > now_ms));
        let mut matched: Vec<&StoredCookie> = entries
            .iter()
            .filter(|cookie| {
                let host_ok = if cookie.host_only {
                    host == cookie.domain
                } else {
                    domain_matches(&host, &cookie.domain)
                };
                host_ok && path_matches(path, &cookie.path) && (!cookie.secure || is_secure)
            })
            .collect();
        // RFC 6265 §5.4 step 2: longer paths precede shorter paths while preserving creation order.
        matched.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
        matched
            .into_iter()
            .map(|cookie| {
                let mut builder = Cookie::build((cookie.name.clone(), cookie.value.clone()))
                    .path(cookie.path.clone())
                    .secure(cookie.secure)
                    .http_only(cookie.http_only);
                if !cookie.host_only {
                    builder = builder.domain(cookie.domain.clone());
                }
                builder.build()
            })
            .collect()
    }

    /// Formats the `Cookie` request header for `url`, if any cookies match.
    #[must_use]
    pub fn cookie_header(&self, url: &Url) -> Option<String> {
        let cookies = self.matches(url);
        if cookies.is_empty() {
            return None;
        }
        let header = cookies
            .iter()
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ");
        Some(header)
    }
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || (host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

fn default_cookie_path(request_path: &str) -> String {
    if !request_path.starts_with('/') || request_path == "/" {
        return "/".to_owned();
    }
    match request_path.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(idx) => request_path[..idx].to_owned(),
    }
}

fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    if request_path == cookie_path {
        return true;
    }
    if request_path.starts_with(cookie_path) {
        return cookie_path.ends_with('/')
            || request_path.as_bytes().get(cookie_path.len()) == Some(&b'/');
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_jar_respects_domain_path_secure_and_expiry() {
        let jar = CookieJar::new();
        let origin = Url::parse("https://app.example.com/a/b/page").unwrap();
        jar.add_set_cookie("host=1", &origin);
        jar.add_set_cookie(
            "cf_clearance=abc; Domain=.example.com; Path=/; Secure; HttpOnly",
            &origin,
        );
        jar.add_set_cookie("expired=gone; Max-Age=0", &origin);

        let sub = Url::parse("https://sub.example.com/x").unwrap();
        let sub_cookies = jar.matches(&sub);
        assert_eq!(sub_cookies.len(), 1);
        assert_eq!(sub_cookies[0].name(), "cf_clearance");
        assert_eq!(sub_cookies[0].value(), "abc");
        assert_eq!(sub_cookies[0].http_only(), Some(true));

        // Secure cookies are not sent over plain HTTP.
        let insecure = Url::parse("http://sub.example.com/x").unwrap();
        assert!(jar.matches(&insecure).is_empty());

        // Longest cookie path precedes shorter path per RFC 6265 §5.4.
        assert_eq!(
            jar.cookie_header(&origin).as_deref(),
            Some("host=1; cf_clearance=abc")
        );
        let sibling = Url::parse("https://app.example.com/other").unwrap();
        assert_eq!(
            jar.cookie_header(&sibling).as_deref(),
            Some("cf_clearance=abc")
        );

        // Replacing with Max-Age=0 deletes an existing cookie.
        jar.add_set_cookie(
            "cf_clearance=; Domain=example.com; Path=/; Max-Age=0",
            &origin,
        );
        assert!(jar.matches(&sub).is_empty());
    }
}
