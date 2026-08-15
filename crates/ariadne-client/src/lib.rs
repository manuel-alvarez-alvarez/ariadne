//! REST client for the Ariadne daemon.
//!
//! Talks to `ariadned` over its unix socket (default, docker-style) or over
//! TCP when the daemon exposes one. Used by the CLI and the MCP server.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client as HyperClient;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use hyperlocal::{UnixClientExt, UnixConnector, Uri as UnixUri};
use serde::Serialize;
use serde::de::DeserializeOwned;

use ariadne_api::error::ErrorBody;
use ariadne_api::{HealthResponse, VersionResponse};

/// Environment variable pointing at the daemon endpoint. Either a filesystem
/// path (unix socket) or an `http://host:port` URL (TCP).
pub const ENDPOINT_ENV: &str = "ARIADNE_SOCKET";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("cannot reach the ariadne daemon at {endpoint}: {source}")]
    Unreachable {
        endpoint: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("daemon returned {status}: {code}: {message}")]
    Api {
        status: StatusCode,
        code: String,
        message: String,
        details: Option<serde_json::Value>,
    },
    #[error("failed to decode daemon response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("request timed out after {}s", REQUEST_TIMEOUT.as_secs())]
    Timeout,
}

enum Transport {
    Unix {
        client: HyperClient<UnixConnector, Full<Bytes>>,
        socket: PathBuf,
    },
    Tcp {
        client: HyperClient<HttpConnector, Full<Bytes>>,
        base: String,
    },
}

/// REST client for `ariadned`.
pub struct Client {
    transport: Transport,
    endpoint: String,
    /// When set, sent as `X-Ariadne-Session` so the daemon can derive the
    /// caller's role/task scope (agent-originated calls).
    session_id: Option<String>,
}

impl Client {
    /// Connect over a unix socket.
    pub fn unix(socket: impl AsRef<Path>) -> Self {
        let socket = socket.as_ref().to_path_buf();
        Self {
            endpoint: socket.display().to_string(),
            transport: Transport::Unix {
                client: HyperClient::unix(),
                socket,
            },
            session_id: None,
        }
    }

    /// Connect over TCP; `base` is e.g. `http://127.0.0.1:7676`.
    pub fn tcp(base: impl Into<String>) -> Self {
        let base = base.into().trim_end_matches('/').to_string();
        Self {
            endpoint: base.clone(),
            transport: Transport::Tcp {
                client: HyperClient::builder(TokioExecutor::new()).build_http(),
                base,
            },
            session_id: None,
        }
    }

    /// Attach an agent-session identity to every request.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Default socket path: `~/.ariadne/ariadne.sock`.
    pub fn default_socket_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".ariadne")
            .join("ariadne.sock")
    }

    /// Build a client from `ARIADNE_SOCKET` (path or http URL), falling back
    /// to the default unix socket.
    pub fn from_env() -> Self {
        match std::env::var(ENDPOINT_ENV) {
            Ok(v) if v.starts_with("http://") || v.starts_with("https://") => Self::tcp(v),
            Ok(v) if !v.is_empty() => Self::unix(v),
            _ => Self::unix(Self::default_socket_path()),
        }
    }

    /// The endpoint this client targets (socket path or base URL).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    // ---- typed endpoints -------------------------------------------------

    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        self.request(Method::GET, "/v1/health", None::<&()>).await
    }

    pub async fn version(&self) -> Result<VersionResponse, ClientError> {
        self.request(Method::GET, "/v1/version", None::<&()>).await
    }

    // ---- generic verbs ---------------------------------------------------

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        self.request(Method::GET, path, None::<&()>).await
    }

    pub async fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        self.request(Method::POST, path, None::<&()>).await
    }

    pub async fn put_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        self.request(Method::PUT, path, Some(body)).await
    }

    pub async fn patch_json<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    /// GET a plain-text endpoint (e.g. task diffs) without JSON decoding.
    pub async fn get_text(&self, path: &str) -> Result<String, ClientError> {
        let bytes = self.request_raw(Method::GET, path, None::<&()>).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Send a request and ignore the (possibly empty) response body.
    pub async fn send_no_content<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(), ClientError> {
        // Reuses `request` with a Value target, tolerating empty bodies.
        match self
            .request::<serde_json::Value, B>(method, path, body)
            .await
        {
            Ok(_) => Ok(()),
            Err(ClientError::Decode(_)) => Ok(()), // 2xx with empty body
            Err(e) => Err(e),
        }
    }

    // ---- plumbing --------------------------------------------------------

    /// Send a request and decode the JSON response. `path` must start with `/`.
    pub async fn request<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, ClientError> {
        let bytes = self.request_raw(method, path, body).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Send a request; return the raw success body, mapping non-2xx onto the
    /// uniform error envelope.
    async fn request_raw<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<Bytes, ClientError> {
        let payload = match body {
            Some(b) => Bytes::from(serde_json::to_vec(b)?),
            None => Bytes::new(),
        };

        let mut builder = Request::builder().method(method);
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(session) = &self.session_id {
            builder = builder.header(ariadne_api::SESSION_HEADER, session);
        }

        let response = match &self.transport {
            Transport::Unix { client, socket } => {
                let uri: http::Uri = UnixUri::new(socket, path).into();
                let req = builder
                    .uri(uri)
                    .body(Full::new(payload))
                    .expect("valid request");
                tokio::time::timeout(REQUEST_TIMEOUT, client.request(req))
                    .await
                    .map_err(|_| ClientError::Timeout)?
                    .map_err(|e| ClientError::Unreachable {
                        endpoint: self.endpoint.clone(),
                        source: Box::new(e),
                    })?
            }
            Transport::Tcp { client, base } => {
                let uri: http::Uri = format!("{base}{path}").parse().expect("valid TCP uri");
                let req = builder
                    .uri(uri)
                    .body(Full::new(payload))
                    .expect("valid request");
                tokio::time::timeout(REQUEST_TIMEOUT, client.request(req))
                    .await
                    .map_err(|_| ClientError::Timeout)?
                    .map_err(|e| ClientError::Unreachable {
                        endpoint: self.endpoint.clone(),
                        source: Box::new(e),
                    })?
            }
        };

        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| ClientError::Unreachable {
                endpoint: self.endpoint.clone(),
                source: Box::new(e),
            })?
            .to_bytes();

        if status.is_success() {
            Ok(bytes)
        } else {
            // Try the uniform error envelope, fall back to raw text.
            match serde_json::from_slice::<ErrorBody>(&bytes) {
                Ok(body) => Err(ClientError::Api {
                    status,
                    code: body.error.code,
                    message: body.error.message,
                    details: body.error.details,
                }),
                Err(_) => Err(ClientError::Api {
                    status,
                    code: "unknown_error".into(),
                    message: String::from_utf8_lossy(&bytes).into_owned(),
                    details: None,
                }),
            }
        }
    }
}
