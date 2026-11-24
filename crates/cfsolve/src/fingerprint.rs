//! Reads browser facts from caller-supplied fingerprint JSON. Unknown keys are ignored, and each
//! program reads only the sections it needs.

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use http::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
#[cfg(not(target_arch = "wasm32"))]
use serde::de::DeserializeOwned;

use crate::engine::Engine;
use crate::http::{BrowserClient, ClientHello, Requests, Transport as HttpTransport};

/// Reads the JSON file at `path` as `T`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("reading fingerprint JSON {}", path.display()))
}

/// The fingerprint's `transport` section.
#[derive(Deserialize)]
pub(crate) struct TransportSection {
    profile: String,
    platform: String,
    /// Headers that replace the emulated profile's headers.
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    client_hello: ClientHello,
    requests: Requests,
}

/// Transport settings from the fingerprint's `engine` and `transport` sections.
pub struct Transport {
    pub engine: Engine,
    pub profile: String,
    pub platform: String,
    /// `transport.headers`, which replace those of the emulated profile.
    pub headers: HeaderMap,
    pub client_hello: ClientHello,
    pub requests: Requests,
}

impl Transport {
    /// Reads only the transport sections of the fingerprint at `path`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(path: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        struct Sections {
            engine: Engine,
            transport: TransportSection,
        }
        let sections: Sections = read(path)?;
        Self::try_new(sections.engine, sections.transport)
    }

    pub(crate) fn try_new(engine: Engine, transport: TransportSection) -> Result<Self> {
        ensure!(
            !transport.profile.trim().is_empty(),
            "fingerprint `transport.profile` must not be empty"
        );
        ensure!(
            !transport.platform.trim().is_empty(),
            "fingerprint `transport.platform` must not be empty"
        );
        let headers: Result<HeaderMap> = transport
            .headers
            .iter()
            .map(|(name, value)| {
                Ok((
                    HeaderName::from_bytes(name.as_bytes())?,
                    HeaderValue::from_str(value)?,
                ))
            })
            .collect();
        Ok(Self {
            engine,
            profile: transport.profile,
            platform: transport.platform,
            headers: headers.context("reading fingerprint `transport.headers`")?,
            client_hello: transport.client_hello,
            requests: transport.requests,
        })
    }

    /// Creates a client with this transport, backed by `adapter`, optionally using `proxy` and
    /// trusting only the PEM certificate `trusted_root`.
    pub fn client(
        &self,
        adapter: Arc<dyn HttpTransport>,
        proxy: Option<&str>,
        trusted_root: Option<&[u8]>,
    ) -> Result<BrowserClient> {
        BrowserClient::try_new(
            self.profile.clone(),
            self.platform.clone(),
            self.headers.clone(),
            &self.client_hello,
            self.requests.clone(),
            self.engine,
            proxy,
            trusted_root,
            adapter,
        )
    }
}
