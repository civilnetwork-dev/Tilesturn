//! Builds requests used by the challenge flow. [`super::HeaderOrder`] supplies headers and
//! ordering; each request supplies values such as its referrer.

use anyhow::{Context, Result, ensure};
use http::StatusCode;
use http::header::LOCATION;
use url::Url;
use url::form_urlencoded::Serializer;

use super::client::{BrowserClient, RequestSpec};
use super::decode::{self, ResponseMetrics};
use super::header_order::RequestKind;
use super::referrer::{self, ReferrerPolicy};
use super::transport::{TransportError, TransportResponse, TransportWebSocket};
use crate::engine::Engine;

/// Fetched iframe URL, HTML, and navigation measurements.
#[derive(Debug)]
pub struct Iframe {
    pub url: Url,
    pub html: String,
    pub navigation: ResponseMetrics,
}

fn ensure_success(response: TransportResponse) -> Result<TransportResponse> {
    ensure!(
        !response.status.is_client_error() && !response.status.is_server_error(),
        "HTTP {} for {}",
        response.status,
        response.url
    );
    Ok(response)
}

fn response_text(response: TransportResponse) -> Result<String> {
    let (decoded, _) = decode::read_encoded(response)?;
    Ok(String::from_utf8_lossy(&decoded).into_owned())
}

/// Fetches the embedding API's iframe URL using the page-policy `referer`. Never caches
/// challenge options because every response creates a separate POST session.
pub async fn fetch_iframe(
    client: &BrowserClient,
    url: Url,
    referer: Option<&str>,
) -> Result<Iframe> {
    let mut spec = RequestSpec::get(url);
    spec.preserve_encoding = true;
    let response = ensure_success(
        client
            .send(spec, RequestKind::Iframe, &[("referer", referer)])
            .await?,
    )?;
    let url = response.url.clone();
    let (decoded, navigation) = decode::read_encoded(response)?;
    let html = String::from_utf8(decoded).context("iframe HTML is not UTF-8")?;
    Ok(Iframe {
        url,
        html,
        navigation,
    })
}

/// A fetched document and response metadata, regardless of status. Challenge pages use 403.
#[derive(Clone, Debug)]
pub struct PageDocument {
    /// Final navigation URL, used to resolve relative links.
    pub url: Url,
    pub status: StatusCode,
    pub html: String,
    /// Referrer policy applied to the page's outgoing requests.
    pub(crate) referrer_policy: ReferrerPolicy,
}

impl PageDocument {
    /// A document served at `url` with its `Referrer-Policy` header, if any.
    #[must_use]
    pub(crate) fn new(url: Url, status: StatusCode, html: String, policy: Option<&str>) -> Self {
        Self {
            referrer_policy: ReferrerPolicy::of_document(policy, &html),
            url,
            status,
            html,
        }
    }

    fn read(response: TransportResponse) -> Result<Self> {
        let url = response.url.clone();
        let status = response.status;
        let header = response
            .headers
            .get("referrer-policy")
            .map(decode::header_text);
        let html = response_text(response)?;
        Ok(Self::new(url, status, html, header.as_deref()))
    }
}

/// Navigates as if the user typed an address. Retries once if `Critical-CH` requires
/// high-entropy client hints and the profile supports that retry.
pub async fn navigate(client: &BrowserClient, url: &Url) -> Result<PageDocument> {
    let navigation = async |kind| {
        client
            .send(RequestSpec::get(url.clone()), kind, &[])
            .await
            .context("navigating to the page")
    };
    let mut response = navigation(RequestKind::Navigation).await?;
    if response.headers.contains_key("critical-ch")
        && client.orders(RequestKind::NavigationWithHints)
    {
        response = navigation(RequestKind::NavigationWithHints).await?;
    }
    PageDocument::read(response)
}

/// Loads a same-origin orchestrator script after the page has requested high-entropy hints.
/// `referer` is the page's temporary address during loading.
pub async fn fetch_orchestrator(
    client: &BrowserClient,
    url: &Url,
    referer: &Url,
) -> Result<String> {
    let supplied = [("referer", Some(referer.as_str()))];
    let response = ensure_success(
        client
            .send(
                RequestSpec::get(url.clone()),
                RequestKind::Orchestrator,
                &supplied,
            )
            .await
            .context("loading the orchestrator")?,
    )?;
    response_text(response)
}

/// Loads a same-origin image as the orchestrator would. Reads and discards the body, retaining
/// response timing.
pub async fn fetch_page_image(
    client: &BrowserClient,
    page: &Url,
    url: &Url,
) -> Result<ResponseMetrics> {
    let mut spec = RequestSpec::get(url.clone());
    spec.preserve_encoding = true;
    let supplied = [("referer", Some(page.as_str()))];
    let response = client
        .send(spec, RequestKind::PageImage, &supplied)
        .await
        .context("loading the page's image")?;
    let (_, metrics) = decode::read_encoded(response)?;
    Ok(metrics)
}

/// Posts an encoded challenge body to `post_url` from `document` using `referer`. Does not
/// follow redirects, preserving the response for the caller.
async fn encoded_post(
    client: &BrowserClient,
    kind: RequestKind,
    document: &Url,
    referer: &str,
    post_url: &Url,
    cf_chl: &str,
    encoded_body: String,
) -> Result<TransportResponse, TransportError> {
    let length = encoded_body.len().to_string();
    let origin = document.origin().ascii_serialization();
    let mut spec = RequestSpec::post(post_url.clone(), encoded_body.into_bytes());
    spec.follow_redirects = false;
    spec.preserve_encoding = true;
    let supplied = [
        ("content-length", Some(length.as_str())),
        ("cf-chl", Some(cf_chl)),
        ("origin", Some(origin.as_str())),
        ("referer", Some(referer)),
    ];
    client.send(spec, kind, &supplied).await
}

/// The orchestrator's XHR to its challenge URL on the page's own origin.
/// Like [`post_challenge`], the response is returned intact.
pub async fn post_orchestrator(
    client: &BrowserClient,
    page: &Url,
    post_url: &Url,
    cf_chl: &str,
    encoded_body: String,
) -> Result<TransportResponse> {
    encoded_post(
        client,
        RequestKind::OrchestratorPost,
        page,
        page.as_str(),
        post_url,
        cf_chl,
        encoded_body,
    )
    .await
    .context("sending the orchestrator POST")
}

/// Submits a URL-encoded form as a navigation from `referer` to `action`.
pub async fn submit_form(
    client: &BrowserClient,
    referer: &Url,
    action: &Url,
    fields: &[(String, String)],
) -> Result<PageDocument> {
    let body = Serializer::new(String::new()).extend_pairs(fields).finish();
    let length = body.len().to_string();
    let origin = referer.origin().ascii_serialization();
    let site = if action.origin() == referer.origin() {
        "same-origin"
    } else {
        "cross-site"
    };
    let spec = RequestSpec::post(action.clone(), body.into_bytes());
    let supplied = [
        ("content-length", Some(length.as_str())),
        ("origin", Some(origin.as_str())),
        ("sec-fetch-site", Some(site)),
        ("referer", Some(referer.as_str())),
    ];
    let response = client
        .send(spec, RequestKind::Form, &supplied)
        .await
        .context("submitting the form")?;
    PageDocument::read(response)
}

/// How a page loads a script from another site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptLoad {
    /// A classic `<script src>`.
    Classic,
    /// An anonymous cross-origin script request. Sends no cookies and reports no storage
    /// access.
    Anonymous,
}

/// Loads a cross-origin script into `page`, using the page policy's `referer` as for the widget
/// loader.
pub async fn fetch_script(
    client: &BrowserClient,
    url: &Url,
    load: ScriptLoad,
    page: &Url,
    referer: Option<&str>,
) -> Result<TransportResponse> {
    let origin = page.origin().ascii_serialization();
    let (kind, supplied): (RequestKind, &[(&str, Option<&str>)]) = match load {
        ScriptLoad::Classic => (RequestKind::Script, &[("referer", referer)]),
        ScriptLoad::Anonymous => (
            RequestKind::CorsScript,
            &[("origin", Some(origin.as_str())), ("referer", referer)],
        ),
    };
    // Follow the redirect manually when the browser uses a separate header order for redirected
    // scripts.
    let reorders_redirect = client.orders(RequestKind::ScriptRedirect);
    let mut spec = RequestSpec::get(url.clone());
    spec.preserve_encoding = true;
    spec.include_credentials = load != ScriptLoad::Anonymous;
    if reorders_redirect {
        spec.follow_redirects = false;
    }
    let response = client.send(spec, kind, supplied).await?;
    if !reorders_redirect || !response.status.is_redirection() {
        return ensure_success(response);
    }
    let location = response
        .headers
        .get(LOCATION)
        .context("the loader's redirect has no location")?;
    let target = url.join(location.to_str()?)?;
    let redirected = match load {
        ScriptLoad::Classic => RequestKind::ScriptRedirect,
        // No redirected CORS load has been observed, so preserve its original order.
        ScriptLoad::Anonymous => kind,
    };
    let mut next_spec = RequestSpec::get(target);
    next_spec.preserve_encoding = true;
    next_spec.include_credentials = load != ScriptLoad::Anonymous;
    ensure_success(client.send(next_spec, redirected, supplied).await?)
}

/// The iframe's XHR of a challenge's encoded body to `post_url`. The response
/// is returned intact, so callers can read Cloudflare's error body too.
pub async fn post_challenge(
    client: &BrowserClient,
    iframe: &Iframe,
    post_url: &Url,
    cf_chl: &str,
    encoded_body: String,
) -> Result<TransportResponse> {
    let referer = iframe_referrer(client, iframe);
    encoded_post(
        client,
        RequestKind::ChallengePost,
        &iframe.url,
        &referer,
        post_url,
        cf_chl,
        encoded_body,
    )
    .await
    .context("sending the challenge POST")
}

/// Loads an image in the iframe document as an `<img>` element would.
pub async fn fetch_image(
    client: &BrowserClient,
    iframe: &Iframe,
    url: &Url,
) -> Result<TransportResponse> {
    let mut spec = RequestSpec::get(url.clone());
    spec.preserve_encoding = true;
    let referer = iframe_referrer(client, iframe);
    let supplied = [("referer", Some(referer.as_str()))];
    client
        .send(spec, RequestKind::Image, &supplied)
        .await
        .context("loading a challenge image")
}

/// Returns the iframe request's referrer. WebKit exposes only the iframe URL's origin.
fn iframe_referrer(client: &BrowserClient, iframe: &Iframe) -> String {
    match client.engine {
        Engine::Chromium => iframe.url.to_string(),
        Engine::WebKit => referrer::origin_referrer(&iframe.url),
    }
}

/// Worker fetch options that affect request headers and cookies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchOptions {
    /// `{cache: "no-cache"}`: revalidate whatever the cache holds.
    NoCache,
    /// `{cache: "no-store", credentials: "omit"}`: bypasses the cache, sends no cookies, and
    /// ignores response cookies.
    NoStoreWithoutCredentials,
}

/// Fetches from an iframe worker using its `fetch` options. Worker requests send neither a
/// referrer nor client hints.
pub async fn fetch_from_worker(
    client: &BrowserClient,
    iframe: &Iframe,
    url: &Url,
    options: FetchOptions,
) -> Result<TransportResponse> {
    let site = if url.origin() == iframe.url.origin() {
        "same-origin"
    } else {
        "same-site"
    };
    let mut spec = RequestSpec::get(url.clone());
    let kind = match options {
        FetchOptions::NoCache => RequestKind::WorkerFetch,
        FetchOptions::NoStoreWithoutCredentials => {
            spec.include_credentials = false;
            RequestKind::WorkerFetchWithoutCredentials
        }
    };
    let supplied = [("sec-fetch-site", Some(site))];
    client
        .send(spec, kind, &supplied)
        .await
        .context("fetching from the challenge worker")
}

/// Opens the iframe's WebSocket to `url`.
pub async fn open_websocket(
    client: &BrowserClient,
    iframe: &Iframe,
    url: &Url,
) -> Result<Box<dyn TransportWebSocket>> {
    let origin = iframe.url.origin().ascii_serialization();
    client
        .websocket(url, &[("origin", Some(&origin))])
        .await
        .context("opening the iframe's WebSocket")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use http::header::{HeaderMap, HeaderName, HeaderValue};

    use super::*;
    use crate::http::transport::{Transport, TransportError, TransportRequest};
    use crate::platform::BoxFuture;

    struct QueueTransport {
        responses: Mutex<VecDeque<TransportResponse>>,
        recorded: Mutex<Vec<TransportRequest>>,
    }

    impl QueueTransport {
        fn new(responses: Vec<TransportResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into()),
                recorded: Mutex::new(Vec::new()),
            })
        }

        fn take_recorded(&self) -> Vec<TransportRequest> {
            std::mem::take(&mut *self.recorded.lock().unwrap())
        }
    }

    impl Transport for QueueTransport {
        fn fetch(
            &self,
            request: TransportRequest,
        ) -> BoxFuture<'_, Result<TransportResponse, TransportError>> {
            let url = request.url.clone();
            self.recorded.lock().unwrap().push(request);
            let next = self.responses.lock().unwrap().pop_front();
            Box::pin(async move {
                let mut response = next.ok_or_else(|| TransportError::new("unexpected request"))?;
                if response.url.as_str() == "https://placeholder.invalid/" {
                    response.url = url;
                }
                Ok(response)
            })
        }
    }

    fn resp(status: u16, headers: &[(&str, &str)], body: Vec<u8>) -> TransportResponse {
        let mut map = HeaderMap::new();
        for (k, v) in headers {
            map.append(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        TransportResponse {
            url: Url::parse("https://placeholder.invalid/").unwrap(),
            status: StatusCode::from_u16(status).unwrap(),
            headers: map,
            body,
            body_decoded: false,
            encoded_bytes: None,
        }
    }

    #[tokio::test]
    async fn iframe_fetch_preserves_compressed_sizes_and_server_timing() {
        let html = "<html><script>window._cf_chl_opt = {};</script></html>";
        let compressed = decode::encode_body(html.as_bytes(), "gzip").unwrap();
        let encoded_length = compressed.len();
        let transport = QueueTransport::new(vec![resp(
            200,
            &[
                ("content-encoding", "gzip"),
                ("server-timing", &format!("session;dur={}", html.len())),
            ],
            compressed,
        )]);
        let client = BrowserClient::for_test(transport);
        let iframe = fetch_iframe(
            &client,
            Url::parse("https://challenges.cloudflare.com/iframe").unwrap(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(iframe.html, html);
        assert_eq!(iframe.navigation.encoded_bytes, encoded_length as u64);
        assert_eq!(iframe.navigation.decoded_bytes, html.len() as u64);
        assert_eq!(
            iframe.navigation.server_timing,
            [("session".into(), html.len() as f64)]
        );
    }

    #[tokio::test]
    async fn webkit_requests_send_safaris_headers_in_its_order() {
        let body = b"<script>window._cf_chl_opt = {};</script>".to_vec();
        let transport = QueueTransport::new(vec![
            resp(200, &[], body.clone()),
            resp(302, &[("location", "/b/api.js")], Vec::new()),
            resp(200, &[], body.clone()),
            resp(200, &[], body.clone()),
            resp(200, &[], body.clone()),
            resp(200, &[], body.clone()),
            resp(200, &[], body),
        ]);
        let client = BrowserClient::for_test(Arc::clone(&transport) as Arc<dyn Transport>);
        let page = Url::parse("https://example.com/page").unwrap();
        let referer = Some("https://example.com/");
        navigate(&client, &page).await.unwrap();
        let script = page.join("/api.js").unwrap();
        fetch_script(&client, &script, ScriptLoad::Classic, &page, referer)
            .await
            .unwrap();
        let iframe = fetch_iframe(&client, page.join("/iframe").unwrap(), referer)
            .await
            .unwrap();
        let url = iframe.url.join("/resource").unwrap();
        post_challenge(&client, &iframe, &url, "token", "body".into())
            .await
            .unwrap();
        fetch_image(&client, &iframe, &url).await.unwrap();
        fetch_from_worker(&client, &iframe, &url, FetchOptions::NoCache)
            .await
            .unwrap();

        let recorded = transport.take_recorded();
        let names: Vec<String> = recorded
            .iter()
            .map(|req| {
                req.headers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let loaded = "sec-fetch-dest user-agent accept referer sec-fetch-site sec-fetch-mode \
                      accept-language priority accept-encoding";
        let expected = [
            "sec-fetch-dest user-agent accept sec-fetch-site sec-fetch-mode accept-language \
             priority accept-encoding",
            loaded,
            "accept sec-fetch-site priority sec-fetch-mode user-agent accept-language \
             sec-fetch-dest referer accept-encoding",
            loaded,
            "accept content-type sec-fetch-site origin cf-chl content-length sec-fetch-mode \
             user-agent referer sec-fetch-dest cf-chl-ra accept-language priority \
             accept-encoding",
            loaded,
            "sec-fetch-dest user-agent accept sec-fetch-site cache-control sec-fetch-mode \
             accept-language priority accept-encoding",
        ];
        assert_eq!(recorded[2].url.path(), "/b/api.js");
        for req in &recorded[4..6] {
            let sent_referer = req
                .headers
                .iter()
                .find(|(name, _)| name == "referer")
                .map(|(_, value)| value.as_str());
            assert_eq!(sent_referer, Some("https://example.com/"));
        }
        assert_eq!(names, expected);
    }

    #[tokio::test]
    async fn worker_fetches_that_omit_credentials_neither_send_nor_keep_cookies() {
        let transport = QueueTransport::new(vec![
            resp(200, &[("set-cookie", "session=kept")], Vec::new()),
            resp(200, &[("set-cookie", "omitted=dropped")], Vec::new()),
            resp(200, &[("set-cookie", "later=1")], Vec::new()),
        ]);
        let client = BrowserClient::for_test(Arc::clone(&transport) as Arc<dyn Transport>);
        let page = Url::parse("https://example.com/iframe").unwrap();
        navigate(&client, &page).await.unwrap();
        let iframe = Iframe {
            url: page,
            html: String::new(),
            navigation: ResponseMetrics::default(),
        };
        let url = iframe.url.join("/fetch").unwrap();
        for options in [
            FetchOptions::NoStoreWithoutCredentials,
            FetchOptions::NoCache,
        ] {
            fetch_from_worker(&client, &iframe, &url, options)
                .await
                .unwrap();
        }
        let recorded = transport.take_recorded();
        fn cookie_of(req: &TransportRequest) -> Option<&str> {
            req.headers
                .iter()
                .find(|(name, _)| name == "cookie")
                .map(|(_, value)| value.as_str())
        }
        assert_eq!(cookie_of(&recorded[1]), None);
        assert_eq!(cookie_of(&recorded[2]), Some("session=kept"));
    }

    #[tokio::test]
    async fn sends_encoded_bytes_and_session_headers_without_following_redirects() {
        let transport =
            QueueTransport::new(vec![resp(302, &[("location", "/unexpected")], Vec::new())]);
        let client = BrowserClient::for_test(Arc::clone(&transport) as Arc<dyn Transport>);
        let iframe = Iframe {
            url: Url::parse("https://example.com/iframe?lang=auto").unwrap(),
            html: String::new(),
            navigation: ResponseMetrics::default(),
        };
        let response = post_challenge(
            &client,
            &iframe,
            &iframe.url.join("/fo/build/ray/token").unwrap(),
            "token",
            "abc+$-123".into(),
        )
        .await
        .unwrap();
        assert_eq!(response.status.as_u16(), 302);
        let req = transport.take_recorded().remove(0);
        assert!(!req.follow_redirects);
        assert_eq!(req.body.as_deref(), Some(b"abc+$-123".as_slice()));
        let header = |name: &str| {
            req.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(header("content-type"), Some("text/plain;charset=UTF-8"));
        assert_eq!(header("cf-chl"), Some("token"));
        assert_eq!(header("cf-chl-ra"), Some("0"));
        assert_eq!(header("content-length"), Some("9"));
        assert_eq!(header("sec-fetch-site"), Some("same-origin"));
        assert_eq!(header("referer"), Some("https://example.com/"));
    }
}
