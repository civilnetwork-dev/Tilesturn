//! JavaScript ↔ Rust adapters for [`cfsolve::Transport`], [`cfsolve::TransportWebSocket`],
//! and [`cfsolve::Sink`].

use cfsolve::Options;
use wasm_bindgen::JsValue;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::sync::Arc;

    use anyhow::{Result, anyhow};
    use cfsolve::platform::BoxFuture;
    use cfsolve::{
        Options, Sink, Transport, TransportError, TransportRequest, TransportResponse,
        TransportWebSocket, TransportWebSocketRequest, WsMessage,
    };
    use http::StatusCode;
    use http::header::{HeaderMap, HeaderName, HeaderValue};
    use js_sys::{Array, ArrayBuffer, Function, Object, Promise, Reflect, Uint8Array};
    use serde::Serialize;
    use url::Url;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    use crate::wire::{to_js, wire_config_error};

    fn js_error_message(error: JsValue) -> String {
        if let Some(text) = error.as_string() {
            return text;
        }
        if let Ok(message) = Reflect::get(&error, &JsValue::from_str("message"))
            && let Some(text) = message.as_string()
        {
            return text;
        }
        format!("{error:?}")
    }

    fn get_prop(target: &JsValue, name: &str) -> Option<JsValue> {
        let value = Reflect::get(target, &JsValue::from_str(name)).ok()?;
        (!value.is_undefined() && !value.is_null()).then_some(value)
    }

    fn get_optional_fn(
        target: &JsValue,
        name: &str,
        error_label: &str,
    ) -> Result<Option<Function>, JsValue> {
        match get_prop(target, name) {
            Some(value) => value
                .dyn_into::<Function>()
                .map(Some)
                .map_err(|_| wire_config_error(format!("{error_label} must be a function"))),
            None => Ok(None),
        }
    }

    fn get_required_fn(
        target: &JsValue,
        name: &str,
        error_label: &str,
    ) -> Result<Function, JsValue> {
        get_optional_fn(target, name, error_label)?
            .ok_or_else(|| wire_config_error(format!("{error_label} must be a function")))
    }

    struct JsTransport {
        target: JsValue,
        fetch_fn: Function,
        websocket_fn: Option<Function>,
        signal: Option<JsValue>,
    }

    impl JsTransport {
        fn from_js(target: JsValue, signal: Option<JsValue>) -> Result<Arc<dyn Transport>, JsValue> {
            let fetch_fn = get_required_fn(&target, "fetch", "options.transport.fetch")?;
            let websocket_fn = get_optional_fn(&target, "websocket", "options.transport.websocket")?;
            Ok(Arc::new(Self {
                target,
                fetch_fn,
                websocket_fn,
                signal,
            }))
        }
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct JsFetchRequestMeta<'a> {
        method: &'a str,
        url: &'a str,
        headers: &'a [(String, String)],
        follow_redirects: bool,
        max_redirects: u8,
        include_credentials: bool,
        preserve_encoding: bool,
        timeout_ms: u32,
        kind: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        http2_weight: Option<u16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        proxy: Option<&'a str>,
        tls: &'a cfsolve::TransportTlsInfo,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct JsWsRequestMeta<'a> {
        url: &'a str,
        headers: &'a [(String, String)],
        #[serde(skip_serializing_if = "Option::is_none")]
        proxy: Option<&'a str>,
        tls: &'a cfsolve::TransportTlsInfo,
    }

    fn build_js_fetch_request(
        request: &TransportRequest,
        signal: Option<&JsValue>,
    ) -> Result<JsValue, TransportError> {
        let meta = JsFetchRequestMeta {
            method: request.method.as_str(),
            url: request.url.as_str(),
            headers: &request.headers,
            follow_redirects: request.follow_redirects,
            max_redirects: request.max_redirects,
            include_credentials: request.include_credentials,
            preserve_encoding: request.preserve_encoding,
            timeout_ms: request.timeout_ms,
            kind: &request.kind,
            http2_weight: request.http2_weight,
            proxy: request.proxy.as_deref(),
            tls: &request.tls,
        };
        let obj = to_js(&meta).map_err(|err| TransportError::new(js_error_message(err)))?;
        if let Some(body) = &request.body {
            let bytes = Uint8Array::from(body.as_slice());
            Reflect::set(&obj, &JsValue::from_str("body"), &bytes)
                .map_err(|err| TransportError::new(js_error_message(err)))?;
        }
        if let Some(root) = &request.trusted_root {
            let bytes = Uint8Array::from(root.as_slice());
            Reflect::set(&obj, &JsValue::from_str("trustedRoot"), &bytes)
                .map_err(|err| TransportError::new(js_error_message(err)))?;
        }
        if let Some(signal) = signal {
            Reflect::set(&obj, &JsValue::from_str("signal"), signal)
                .map_err(|err| TransportError::new(js_error_message(err)))?;
        }
        Ok(obj)
    }

    fn parse_js_headers(value: Option<JsValue>) -> Result<HeaderMap, TransportError> {
        let mut headers = HeaderMap::new();
        let Some(value) = value else {
            return Ok(headers);
        };
        if Array::is_array(&value) {
            let entries = Array::from(&value);
            for index in 0..entries.length() {
                let entry = entries.get(index);
                if Array::is_array(&entry) {
                    let pair = Array::from(&entry);
                    if let (Some(name), Some(val)) =
                        (pair.get(0).as_string(), pair.get(1).as_string())
                    {
                        append_header(&mut headers, &name, &val);
                    }
                }
            }
            return Ok(headers);
        }
        if let Some(object) = value.dyn_ref::<Object>() {
            let keys = Object::keys(object);
            for index in 0..keys.length() {
                let Some(name) = keys.get(index).as_string() else {
                    continue;
                };
                let Ok(raw) = Reflect::get(object, &JsValue::from_str(&name)) else {
                    continue;
                };
                if Array::is_array(&raw) {
                    let list = Array::from(&raw);
                    for item_index in 0..list.length() {
                        if let Some(val) = list.get(item_index).as_string() {
                            append_header(&mut headers, &name, &val);
                        }
                    }
                } else if let Some(val) = raw.as_string() {
                    append_header(&mut headers, &name, &val);
                }
            }
        }
        Ok(headers)
    }

    fn append_header(headers: &mut HeaderMap, name: &str, value: &str) {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.append(name, value);
        }
    }

    fn parse_js_body(value: Option<JsValue>) -> Result<Vec<u8>, TransportError> {
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        if let Some(array) = value.dyn_ref::<Uint8Array>() {
            return Ok(array.to_vec());
        }
        if let Some(buffer) = value.dyn_ref::<ArrayBuffer>() {
            return Ok(Uint8Array::new(buffer).to_vec());
        }
        if let Some(text) = value.as_string() {
            return Ok(text.into_bytes());
        }
        if Array::is_array(&value) {
            let array = Array::from(&value);
            let mut bytes = Vec::with_capacity(array.length() as usize);
            for index in 0..array.length() {
                if let Some(num) = array.get(index).as_f64() {
                    bytes.push(num as u8);
                }
            }
            return Ok(bytes);
        }
        Err(TransportError::new(
            "transport response `body` must be Uint8Array, ArrayBuffer, or string",
        ))
    }

    fn parse_js_response(
        value: JsValue,
        fallback_url: &Url,
    ) -> Result<TransportResponse, TransportError> {
        let url = match get_prop(&value, "url").and_then(|v| v.as_string()) {
            Some(raw) => Url::parse(&raw).map_err(|err| {
                TransportError::new(format!("invalid response url {raw:?}: {err}"))
            })?,
            None => fallback_url.clone(),
        };
        let status_num = get_prop(&value, "status")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| TransportError::new("transport response missing numeric `status`"))?;
        if !status_num.is_finite()
            || status_num.fract() != 0.0
            || !(100.0..=599.0).contains(&status_num)
        {
            return Err(TransportError::new(format!(
                "invalid HTTP status {status_num} (expected integer 100..=599)"
            )));
        }
        let status = StatusCode::from_u16(status_num as u16)
            .map_err(|err| TransportError::new(format!("invalid HTTP status {status_num}: {err}")))?;
        let headers = parse_js_headers(get_prop(&value, "headers"))?;
        let body = parse_js_body(get_prop(&value, "body"))?;
        let body_decoded = get_prop(&value, "bodyDecoded")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let encoded_bytes = get_prop(&value, "encodedBytes")
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64);
        Ok(TransportResponse {
            url,
            status,
            headers,
            body,
            body_decoded,
            encoded_bytes,
        })
    }

    /// Resolves any JS value, Promise, or cross-realm thenable via `Promise.resolve`.
    async fn resolve_js_result(value: JsValue) -> Result<JsValue, TransportError> {
        JsFuture::from(Promise::resolve(&value))
            .await
            .map_err(|err| TransportError::new(js_error_message(err)))
    }

    impl Transport for JsTransport {
        fn fetch(
            &self,
            request: TransportRequest,
        ) -> BoxFuture<'_, Result<TransportResponse, TransportError>> {
            Box::pin(async move {
                let fallback_url = request.url.clone();
                let js_req = build_js_fetch_request(&request, self.signal.as_ref())?;
                let raw = self
                    .fetch_fn
                    .call1(&self.target, &js_req)
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                let resolved = resolve_js_result(raw).await?;
                parse_js_response(resolved, &fallback_url)
            })
        }

        fn open_websocket(
            &self,
            request: TransportWebSocketRequest,
        ) -> BoxFuture<'_, Result<Box<dyn TransportWebSocket>, TransportError>> {
            Box::pin(async move {
                let Some(ws_fn) = &self.websocket_fn else {
                    return Err(TransportError::new(
                        "transport does not implement `websocket`",
                    ));
                };
                let meta = JsWsRequestMeta {
                    url: request.url.as_str(),
                    headers: &request.headers,
                    proxy: request.proxy.as_deref(),
                    tls: &request.tls,
                };
                let js_req =
                    to_js(&meta).map_err(|err| TransportError::new(js_error_message(err)))?;
                if let Some(root) = &request.trusted_root {
                    let bytes = Uint8Array::from(root.as_slice());
                    Reflect::set(&js_req, &JsValue::from_str("trustedRoot"), &bytes)
                        .map_err(|err| TransportError::new(js_error_message(err)))?;
                }
                let raw = ws_fn
                    .call1(&self.target, &js_req)
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                let socket = resolve_js_result(raw).await?;
                let send_fn = get_required_fn(&socket, "sendText", "websocket.sendText")
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                let recv_fn = get_required_fn(&socket, "recv", "websocket.recv")
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                let close_fn = get_required_fn(&socket, "close", "websocket.close")
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                Ok(Box::new(JsWebSocket {
                    target: socket,
                    send_fn,
                    recv_fn,
                    close_fn,
                }) as Box<dyn TransportWebSocket>)
            })
        }
    }

    struct JsWebSocket {
        target: JsValue,
        send_fn: Function,
        recv_fn: Function,
        close_fn: Function,
    }

    impl TransportWebSocket for JsWebSocket {
        fn send_text(&mut self, text: String) -> BoxFuture<'_, Result<(), TransportError>> {
            Box::pin(async move {
                let raw = self
                    .send_fn
                    .call1(&self.target, &JsValue::from_str(&text))
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                let _ = resolve_js_result(raw).await?;
                Ok(())
            })
        }

        fn recv(&mut self) -> BoxFuture<'_, Option<Result<WsMessage, TransportError>>> {
            Box::pin(async move {
                let raw = match self.recv_fn.call0(&self.target) {
                    Ok(raw) => raw,
                    Err(err) => return Some(Err(TransportError::new(js_error_message(err)))),
                };
                let msg = match resolve_js_result(raw).await {
                    Ok(msg) => msg,
                    Err(err) => return Some(Err(err)),
                };
                if msg.is_null() || msg.is_undefined() {
                    return None;
                }
                let kind = get_prop(&msg, "type").and_then(|v| v.as_string());
                match kind.as_deref() {
                    Some("text") => {
                        let data = get_prop(&msg, "data")
                            .and_then(|v| v.as_string())
                            .unwrap_or_default();
                        Some(Ok(WsMessage::Text(data)))
                    }
                    Some("binary") => match parse_js_body(get_prop(&msg, "data")) {
                        Ok(bytes) => Some(Ok(WsMessage::Binary(bytes))),
                        Err(err) => Some(Err(err)),
                    },
                    Some("ping") => Some(Ok(WsMessage::Ping(
                        parse_js_body(get_prop(&msg, "data")).unwrap_or_default(),
                    ))),
                    Some("pong") => Some(Ok(WsMessage::Pong(
                        parse_js_body(get_prop(&msg, "data")).unwrap_or_default(),
                    ))),
                    Some("close") => Some(Ok(WsMessage::Close)),
                    other => Some(Err(TransportError::new(format!(
                        "unknown websocket frame type {other:?}"
                    )))),
                }
            })
        }

        fn close(&mut self) -> BoxFuture<'_, Result<(), TransportError>> {
            Box::pin(async move {
                let raw = self
                    .close_fn
                    .call0(&self.target)
                    .map_err(|err| TransportError::new(js_error_message(err)))?;
                let _ = resolve_js_result(raw).await?;
                Ok(())
            })
        }
    }

    struct JsSink {
        target: JsValue,
        artifact_fn: Option<Function>,
        progress_fn: Option<Function>,
    }

    impl JsSink {
        fn from_js(target: JsValue) -> Result<Arc<dyn Sink>, JsValue> {
            let artifact_fn = get_optional_fn(&target, "artifact", "options.sink.artifact")?;
            let progress_fn = get_optional_fn(&target, "progress", "options.sink.progress")?;
            Ok(Arc::new(Self {
                target,
                artifact_fn,
                progress_fn,
            }))
        }
    }

    impl Sink for JsSink {
        fn artifact(&self, name: &str, contents: &[u8]) -> Result<()> {
            let Some(callback) = &self.artifact_fn else {
                return Ok(());
            };
            let bytes = Uint8Array::from(contents);
            callback
                .call2(&self.target, &JsValue::from_str(name), &bytes)
                .map(|_| ())
                .map_err(|err| anyhow!("{}", js_error_message(err)))
        }

        fn progress(&self, message: &str) {
            if let Some(callback) = &self.progress_fn {
                let _ = callback.call1(&self.target, &JsValue::from_str(message));
            }
        }
    }

    pub(super) fn parse_options_wasm(value: JsValue) -> Result<Options, JsValue> {
        if value.is_undefined() || value.is_null() {
            return Ok(Options::default());
        }
        let signal = get_prop(&value, "signal");
        let transport = match get_prop(&value, "transport") {
            Some(transport) => Some(JsTransport::from_js(transport, signal)?),
            None => None,
        };
        let proxy = get_prop(&value, "proxy").and_then(|v| v.as_string());
        let time_zone = get_prop(&value, "timeZone").and_then(|v| v.as_string());
        let trusted_root = match get_prop(&value, "trustedRoot") {
            Some(raw) => Some(
                parse_js_body(Some(raw))
                    .map_err(|err| wire_config_error(format!("invalid options.trustedRoot: {err}")))?,
            ),
            None => None,
        };
        let sink = match get_prop(&value, "sink") {
            Some(sink) => Some(JsSink::from_js(sink)?),
            None => None,
        };
        Ok(Options {
            transport,
            proxy,
            trusted_root,
            time_zone,
            sink,
        })
    }
}

/// Parses JS solve `options` into [`cfsolve::Options`].
#[cfg(target_arch = "wasm32")]
pub(crate) fn parse_options(value: JsValue) -> Result<Options, JsValue> {
    wasm::parse_options_wasm(value)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn parse_options(_value: JsValue) -> Result<Options, JsValue> {
    Ok(Options::default())
}
