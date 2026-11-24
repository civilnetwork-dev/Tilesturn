//! Browser HTTP transport, ordered request headers, and stream weights.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use cookie::Cookie;
use http::header::{COOKIE, HeaderMap, HeaderName, HeaderValue, LOCATION, SET_COOKIE, USER_AGENT};
use http::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use super::header_order::{HeaderOrder, RequestKind, Slot};
use super::transport::{
    CookieJar, Transport, TransportError, TransportRequest, TransportResponse, TransportTlsInfo,
    TransportWebSocket, TransportWebSocketRequest,
};
use crate::engine::Engine;

const DEFAULT_TIMEOUT_MS: u32 = 30_000;
const MAX_REDIRECTS: u8 = 5;

/// HTTP client configured from the browser fingerprint. Shared by all session requests to
/// preserve cookies and transport settings.
pub struct BrowserClient {
    pub(super) transport: Arc<dyn Transport>,
    /// The cookies the browser holds, `cf_clearance` among them once a challenge page clears.
    pub(super) jar: Arc<CookieJar>,
    /// User agent and client hints from the profile, overridden by `transport.headers`.
    pub(super) own: HeaderMap,
    pub(super) requests: Requests,
    pub(super) engine: Engine,
    pub(super) tls: TransportTlsInfo,
    pub(super) proxy: Option<String>,
    pub(super) trusted_root: Option<Vec<u8>>,
}

/// Browser request settings beyond the transport profile, which vary by browser version.
#[derive(Clone, Debug, Deserialize)]
pub struct Requests {
    /// HTTP/2 stream weights, from 1 to 256, indexed by RFC 9218 urgency. Missing `priority`
    /// implies urgency 3. Unspecified urgencies, or an absent map, use the transport profile's
    /// weight.
    #[serde(default)]
    pub weights: BTreeMap<u8, u16>,
    /// The headers each kind of request carries, in the browser's order.
    pub headers: HeaderOrder,
}

impl Requests {
    /// Reads request settings from tracked profile text.
    #[cfg(test)]
    pub(crate) fn from_profile(profile: &str) -> Self {
        let profile: serde_json::Value =
            serde_json::from_str(profile).expect("a tracked profile is JSON");
        let requests = profile
            .pointer("/transport/requests")
            .expect("a tracked profile describes its requests");
        Self::deserialize(requests).expect("a tracked profile's requests are valid")
    }
}

/// Parameters for sending one request through [`BrowserClient`].
pub(super) struct RequestSpec {
    pub method: Method,
    pub url: Url,
    pub body: Option<Vec<u8>>,
    pub follow_redirects: bool,
    pub include_credentials: bool,
    pub preserve_encoding: bool,
    pub timeout_ms: u32,
}

impl RequestSpec {
    pub(super) fn get(url: Url) -> Self {
        Self {
            method: Method::GET,
            url,
            body: None,
            follow_redirects: true,
            include_credentials: true,
            preserve_encoding: false,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }

    pub(super) fn post(url: Url, body: Vec<u8>) -> Self {
        Self {
            method: Method::POST,
            url,
            body: Some(body),
            follow_redirects: true,
            include_credentials: true,
            preserve_encoding: false,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

impl BrowserClient {
    /// Creates a client backed by `transport`. `headers` replace the profile's defaults,
    /// `hello` carries additional ClientHello metadata, and `proxy` / `trusted_root` are
    /// forwarded to the transport.
    #[expect(clippy::too_many_arguments)]
    pub fn try_new(
        profile: String,
        platform: String,
        headers: HeaderMap,
        hello: &ClientHello,
        requests: Requests,
        engine: Engine,
        proxy: Option<&str>,
        trusted_root: Option<&[u8]>,
        transport: Arc<dyn Transport>,
    ) -> Result<Self> {
        if let Some(ids) = &hello.requested_trust_anchors {
            let _ = decode_hex(ids).context("trust anchor IDs must be hex")?;
        }
        for weight in requests.weights.values() {
            ensure!(
                (1..=256).contains(weight),
                "HTTP/2 weight {weight} is not from 1 to 256"
            );
        }
        let mut own = default_profile_headers(&platform, engine);
        for (name, value) in &headers {
            own.insert(name, value.clone());
        }
        requests.headers.check_own(&own)?;
        Ok(Self {
            transport,
            jar: Arc::new(CookieJar::new()),
            own,
            requests,
            engine,
            tls: TransportTlsInfo {
                engine,
                profile,
                platform,
                client_hello: hello.clone(),
            },
            proxy: proxy.map(str::to_owned),
            trusted_root: trusted_root.map(<[u8]>::to_vec),
        })
    }

    #[must_use]
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// The cookies the browser would send to `url`.
    #[must_use]
    pub fn cookies(&self, url: &Url) -> Vec<Cookie<'static>> {
        self.jar.matches(url)
    }

    /// Stores the cookie a `Set-Cookie` header from `url` sets.
    pub fn add_cookie(&self, set_cookie: &str, url: &Url) {
        self.jar.add_set_cookie(set_cookie, url);
    }

    /// Creates a test browser backed by `transport` using the tracked iOS Safari 26 request profile.
    #[cfg(test)]
    pub(super) fn for_test(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            jar: Arc::new(CookieJar::new()),
            own: HeaderMap::from_iter([(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"))]),
            requests: Requests::from_profile(include_str!(
                "../../../../profiles/safari-26-ios.json"
            )),
            engine: Engine::WebKit,
            tls: TransportTlsInfo {
                engine: Engine::WebKit,
                profile: "safari_ios_26.2".into(),
                platform: "ios".into(),
                client_hello: ClientHello::default(),
            },
            proxy: None,
            trusted_root: None,
        }
    }

    /// Checks whether the profile defines a header order for `kind`. Required request kinds
    /// must have one.
    #[must_use]
    pub(super) fn orders(&self, kind: RequestKind) -> bool {
        self.requests.headers.get(kind).is_some()
    }

    /// Sends a request with the profile's header order and stream weight for `kind`.
    /// Manages session cookies and follows redirects up to [`MAX_REDIRECTS`] when requested and
    /// not already followed by the transport.
    pub(super) async fn send(
        &self,
        mut spec: RequestSpec,
        mut kind: RequestKind,
        supplied: &[(&str, Option<&str>)],
    ) -> Result<TransportResponse, TransportError> {
        let mut hops = 0u8;
        let mut active_supplied: Vec<(&str, Option<&str>)> = supplied.to_vec();
        loop {
            let mut headers = self.ordered_headers(kind, &active_supplied);
            if spec.include_credentials
                && let Some(cookie) = self.jar.cookie_header(&spec.url)
            {
                headers.push((COOKIE.as_str().to_owned(), cookie));
            }
            let urgency = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("priority"))
                .map_or(3, |(_, value)| urgency(value));
            let http2_weight = self.requests.weights.get(&urgency).copied();
            let request = TransportRequest {
                method: spec.method.clone(),
                url: spec.url.clone(),
                headers,
                body: spec.body.clone(),
                follow_redirects: spec.follow_redirects,
                max_redirects: MAX_REDIRECTS,
                include_credentials: spec.include_credentials,
                preserve_encoding: spec.preserve_encoding,
                timeout_ms: spec.timeout_ms,
                kind: kind.to_string(),
                http2_weight,
                proxy: self.proxy.clone(),
                trusted_root: self.trusted_root.clone(),
                tls: self.tls.clone(),
            };
            let response = self.transport.fetch(request).await?;
            if spec.include_credentials {
                for set_cookie in response.headers.get_all(SET_COOKIE) {
                    if let Ok(value) = set_cookie.to_str() {
                        self.jar.add_set_cookie(value, &response.url);
                    }
                }
            }
            if spec.follow_redirects
                && response.status.is_redirection()
                && hops < MAX_REDIRECTS
                && let Some(location) = response
                    .headers
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                && let Ok(next_url) = response.url.join(location)
            {
                hops += 1;
                if matches!(
                    response.status,
                    StatusCode::SEE_OTHER | StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND
                ) && spec.method == Method::POST
                {
                    // Drop POST body headers when converting a redirected form POST to GET.
                    spec.method = Method::GET;
                    spec.body = None;
                    kind = RequestKind::Navigation;
                    active_supplied.clear();
                }
                spec.url = next_url;
                continue;
            }
            return Ok(response);
        }
    }

    /// Opens a WebSocket to `url` using the profile's handshake headers.
    pub(super) async fn websocket(
        &self,
        url: &Url,
        supplied: &[(&str, Option<&str>)],
    ) -> Result<Box<dyn TransportWebSocket>, TransportError> {
        let mut headers = self.ordered_headers(RequestKind::WebSocket, supplied);
        if let Some(cookie) = self.jar.cookie_header(url) {
            headers.push((COOKIE.as_str().to_owned(), cookie));
        }
        let request = TransportWebSocketRequest {
            url: url.clone(),
            headers,
            proxy: self.proxy.clone(),
            trusted_root: self.trusted_root.clone(),
            tls: self.tls.clone(),
        };
        self.transport.open_websocket(request).await
    }

    /// Builds headers in the profile's order for `kind`. Takes values from `supplied`, omitting
    /// headers whose value is `None`.
    pub(super) fn ordered_headers(
        &self,
        kind: RequestKind,
        supplied: &[(&str, Option<&str>)],
    ) -> Vec<(String, String)> {
        debug_assert!(
            supplied.len() == kind.supplied().len()
                && supplied
                    .iter()
                    .all(|(name, _)| kind.supplied().contains(name)),
            "a `{kind}` request supplies {:?}, not {supplied:?}",
            kind.supplied()
        );
        let order = self
            .requests
            .headers
            .get(kind)
            .unwrap_or_else(|| panic!("the profile orders no `{kind}` requests"));
        let mut headers = Vec::with_capacity(order.len());
        for slot in order {
            match slot {
                Slot::Fixed(name, value) => {
                    if let Ok(text) = value.to_str() {
                        headers.push((name.as_str().to_owned(), text.to_owned()));
                    }
                }
                // Request-supplied values override browser defaults, including an explicit omission.
                Slot::Named(name) => match supplied.iter().find(|(given, _)| name == given) {
                    Some((_, Some(value))) => {
                        headers.push((name.as_str().to_owned(), (*value).to_owned()));
                    }
                    Some((_, None)) => {}
                    None => {
                        if let Some(value) = self.own.get(name)
                            && let Ok(text) = value.to_str()
                        {
                            headers.push((name.as_str().to_owned(), text.to_owned()));
                        }
                    }
                },
            }
        }
        headers
    }
}

/// Supplies fallback `User-Agent` (and Chromium client hints) when a profile relies on transport
/// defaults rather than repeating them in `transport.headers`. Explicit `transport.headers`
/// always take precedence.
pub(crate) fn default_profile_headers(platform: &str, engine: Engine) -> HeaderMap {
    let mut map = HeaderMap::new();
    let ua = default_user_agent(platform, engine);
    map.insert(USER_AGENT, HeaderValue::from_static(ua));
    if engine == Engine::Chromium {
        let mobile = if platform.eq_ignore_ascii_case("android") {
            "?1"
        } else {
            "?0"
        };
        let sec_platform = match platform.to_ascii_lowercase().as_str() {
            "macos" => "\"macOS\"",
            "linux" => "\"Linux\"",
            "android" => "\"Android\"",
            _ => "\"Windows\"",
        };
        map.insert(
            HeaderName::from_static("sec-ch-ua"),
            HeaderValue::from_static(
                "\"Chromium\";v=\"136\", \"Google Chrome\";v=\"136\", \"Not.A/Brand\";v=\"99\"",
            ),
        );
        map.insert(
            HeaderName::from_static("sec-ch-ua-mobile"),
            HeaderValue::from_static(mobile),
        );
        map.insert(
            HeaderName::from_static("sec-ch-ua-platform"),
            HeaderValue::from_static(sec_platform),
        );
    }
    map
}

pub(crate) fn default_user_agent(platform: &str, engine: Engine) -> &'static str {
    match (engine, platform.to_ascii_lowercase().as_str()) {
        (Engine::WebKit, "ios") => {
            "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.2 Mobile/15E148 Safari/604.1"
        }
        (Engine::WebKit, _) => {
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15"
        }
        (Engine::Chromium, "macos") => {
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36"
        }
        (Engine::Chromium, "linux") => {
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36"
        }
        (Engine::Chromium, "android") => {
            "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Mobile Safari/537.36"
        }
        (Engine::Chromium, _) => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36"
        }
    }
}

/// Reads RFC 9218 urgency from `priority`, defaulting to 3.
fn urgency(priority: &str) -> u8 {
    priority
        .split(',')
        .find_map(|item| item.trim().strip_prefix("u=")?.parse().ok())
        .unwrap_or(3)
}

/// Additional ClientHello fields from the fingerprint's browser, beyond standard transport profiles.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClientHello {
    /// Signature algorithm code points prepended to the profile's list, such as Chrome's ML-DSA
    /// values. They are advertised but never accepted.
    #[serde(alias = "advertised_sigalgs_prefix")]
    pub advertised_sigalgs_prefix: Vec<u16>,
    /// Whether `signature_algorithms` starts with a GREASE value.
    #[serde(alias = "grease_sigalgs")]
    pub grease_sigalgs: bool,
    /// Hex-encoded, length-prefixed list of IDs for the `trust_anchors` extension.
    #[serde(alias = "requested_trust_anchors")]
    pub requested_trust_anchors: Option<String>,
}

/// Bytes written as pairs of hexadecimal digits.
fn decode_hex(text: &str) -> Result<Vec<u8>> {
    ensure!(
        text.len().is_multiple_of(2) && text.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{text:?} is not pairs of hex digits"
    );
    let byte = |start: usize| u8::from_str_radix(&text[start..start + 2], 16).expect("hex digits");
    Ok((0..text.len()).step_by(2).map(byte).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgency_reads_the_priority_dictionary_and_defaults_to_three() {
        assert_eq!(urgency("u=0, i"), 0);
        assert_eq!(urgency("i, u=4"), 4);
        assert_eq!(urgency("u=1"), 1);
        assert_eq!(urgency("i"), 3);
        assert_eq!(urgency(""), 3);
    }
}
