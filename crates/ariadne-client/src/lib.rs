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

pub mod endpoint;

/// Environment variable pointing at the daemon endpoint. Either a filesystem
/// path (unix socket) or an `http://host:port` URL (TCP).
pub const ENDPOINT_ENV: &str = "ARIADNE_SOCKET";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything that can go wrong talking to the daemon.
///
/// `Display` is the diagnostic spelling — transport detail included — and is
/// what logs and the MCP server surface. Anything a person reads at a terminal
/// goes through [`ClientError::human`] instead.
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

impl ClientError {
    /// One line for a human: no transport plumbing, no repetition of the
    /// error envelope the daemon already spelled out in prose. The advice, if
    /// there is any, is [`ClientError::hint`] — callers decide where to put it.
    pub fn human(&self) -> String {
        match self {
            // The hyper source ("client error (Connect)") says nothing a user
            // can act on; the endpoint does.
            Self::Unreachable { endpoint, .. } => {
                format!("cannot reach the ariadne daemon at {endpoint}")
            }
            // The daemon's message is already the human sentence; status and
            // code are the machine-readable half, kept for `--format json`.
            Self::Api {
                status, message, ..
            } if message.trim().is_empty() => {
                format!("daemon returned {status}")
            }
            Self::Api { message, .. } => message.clone(),
            other => other.to_string(),
        }
    }

    /// What to do about it, when there is a single obvious answer.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Unreachable { .. } => Some("is it running? try: ariadne daemon start"),
            _ => None,
        }
    }

    /// Stable slug for machine-readable output: the daemon's own code when it
    /// sent one, else the transport failure that stopped us reaching it.
    pub fn code(&self) -> &str {
        match self {
            Self::Unreachable { .. } => "daemon_unreachable",
            Self::Api { code, .. } => code,
            Self::Decode(_) => "invalid_response",
            Self::Timeout => "timeout",
        }
    }
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

    /// The socket `ariadned` listens on for a given home, resolved exactly as
    /// the daemon resolves it (see [`endpoint`]).
    pub fn socket_path(home_override: Option<PathBuf>) -> PathBuf {
        match endpoint::home(home_override) {
            Some(home) => endpoint::socket_path(&home),
            // No home directory at all: same lenient relative fallback the CLI
            // has always used, so we still name something.
            None => endpoint::default_socket_path(Path::new(".ariadne")),
        }
    }

    /// Resolve the daemon endpoint: an explicit endpoint (`--host`, else
    /// `ARIADNE_SOCKET`) wins, otherwise the socket of the resolved home
    /// (`--home` > `ARIADNE_HOME` > `~/.ariadne`, honouring that home's
    /// `config.toml`).
    pub fn resolve(endpoint_override: Option<&str>, home_override: Option<PathBuf>) -> Self {
        let explicit = endpoint_override
            .map(str::to_owned)
            .or_else(|| std::env::var(ENDPOINT_ENV).ok())
            .filter(|v| !v.is_empty());
        Self::from_parts(explicit, || Self::socket_path(home_override))
    }

    /// Build a client from the ambient environment (`ARIADNE_SOCKET`,
    /// `ARIADNE_HOME`), with no command-line overrides.
    pub fn from_env() -> Self {
        Self::resolve(None, None)
    }

    /// Client for the daemon of a home, deliberately deaf to endpoint
    /// overrides: `ariadned` is only ever told a home, so whoever starts one
    /// must address the socket that home resolves to and no other.
    pub fn for_home(home_override: Option<PathBuf>) -> Self {
        Self::unix(Self::socket_path(home_override))
    }

    fn from_parts(explicit: Option<String>, socket: impl FnOnce() -> PathBuf) -> Self {
        match explicit {
            Some(v) if v.starts_with("http://") || v.starts_with("https://") => Self::tcp(v),
            Some(v) => Self::unix(v),
            None => Self::unix(socket()),
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

    #[cfg(test)]
    fn is_tcp(&self) -> bool {
        matches!(self.transport, Transport::Tcp { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket() -> PathBuf {
        PathBuf::from("/scratch/home/ariadne.sock")
    }

    #[test]
    fn explicit_http_endpoint_uses_tcp() {
        let client = Client::from_parts(Some("http://127.0.0.1:7676".into()), socket);
        assert!(client.is_tcp());
        assert_eq!(client.endpoint(), "http://127.0.0.1:7676");
    }

    #[test]
    fn explicit_path_endpoint_beats_the_home_socket() {
        let client = Client::from_parts(Some("/override/ariadne.sock".into()), socket);
        assert!(!client.is_tcp());
        assert_eq!(client.endpoint(), "/override/ariadne.sock");
    }

    #[test]
    fn without_an_override_the_home_socket_is_used() {
        let client = Client::from_parts(None, socket);
        assert!(!client.is_tcp());
        assert_eq!(client.endpoint(), "/scratch/home/ariadne.sock");
    }

    #[test]
    fn socket_path_honours_the_homes_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "socket_path = \"/scratch/custom.sock\"\n",
        )
        .unwrap();
        assert_eq!(
            Client::socket_path(Some(dir.path().to_path_buf())),
            PathBuf::from("/scratch/custom.sock")
        );
    }

    /// `ariadne daemon start` used to poll whatever `--host` / `ARIADNE_SOCKET`
    /// named while spawning a daemon on the home's socket — reporting "already
    /// running" for a stranger's daemon, or timing out on its own.
    #[test]
    fn starting_a_daemon_is_deaf_to_endpoint_overrides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "socket_path = \"/scratch/custom.sock\"\n",
        )
        .unwrap();
        let home = Some(dir.path().to_path_buf());

        // Commands addressing a running daemon honour the override...
        let addressed = Client::resolve(Some("/tmp/other.sock"), home.clone());
        assert_eq!(addressed.endpoint(), "/tmp/other.sock");
        // ...but the daemon being started only ever hears about its home.
        let started = Client::for_home(home);
        assert_eq!(started.endpoint(), "/scratch/custom.sock");
    }

    /// The transport source is a dead end for a user ("client error
    /// (Connect)"): the human line names the endpoint and the way out.
    #[test]
    fn an_unreachable_daemon_reads_as_one_actionable_line() {
        let err = ClientError::Unreachable {
            endpoint: "/tmp/x.sock".into(),
            source: "client error (Connect)".into(),
        };
        assert_eq!(
            err.human(),
            "cannot reach the ariadne daemon at /tmp/x.sock"
        );
        assert_eq!(err.hint(), Some("is it running? try: ariadne daemon start"));
        assert_eq!(err.code(), "daemon_unreachable");
    }

    /// Status, code and message otherwise say the same thing three times.
    #[test]
    fn an_api_error_reads_as_the_daemons_message_alone() {
        let err = ClientError::Api {
            status: StatusCode::NOT_FOUND,
            code: "not_found".into(),
            message: "task not found: badid123".into(),
            details: None,
        };
        assert_eq!(err.human(), "task not found: badid123");
        assert_eq!(err.hint(), None);
        assert_eq!(err.code(), "not_found");
    }

    /// A body with nothing in it still has to say something.
    #[test]
    fn an_api_error_without_a_message_falls_back_to_the_status() {
        let err = ClientError::Api {
            status: StatusCode::BAD_GATEWAY,
            code: "unknown_error".into(),
            message: String::new(),
            details: None,
        };
        assert_eq!(err.human(), "daemon returned 502 Bad Gateway");
    }

    #[test]
    fn socket_path_defaults_within_the_home() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Client::socket_path(Some(dir.path().to_path_buf())),
            dir.path().join("ariadne.sock")
        );
    }
}
