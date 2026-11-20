//! Authenticated, loopback-only Mihomo HTTP API client.
//! Error chains intentionally omit request URLs and never retain controller secrets.
use serde_json::{Value, json};
use std::{backtrace::Backtrace, time::Duration};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

// A wrapper prevents thiserror from emitting unstable Error::provide code.
#[derive(Debug)]
struct CapturedTrace(Backtrace);

#[derive(Debug, thiserror::Error)]
#[error("{kind}")]
pub struct Error {
    #[source]
    kind: ErrorKind,
    trace: CapturedTrace,
}
impl Error {
    fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            trace: CapturedTrace(Backtrace::capture()),
        }
    }
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
    pub fn backtrace(&self) -> &Backtrace {
        &self.trace.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ErrorKind {
    #[error("Invalid controller request: {0}")]
    InvalidRequest(&'static str),
    #[error("Cannot reach controller; run clash-cli run or start its service")]
    Transport(#[source] reqwest::Error),
    #[error("Controller returned HTTP {0}")]
    HttpStatus(u16),
    #[error("Controller returned invalid JSON")]
    Json(#[source] serde_json::Error),
    #[error("Controller response exceeds size limit")]
    ResponseTooLarge,
    #[error(
        "Controller does not require authentication; refusing to modify an unidentified instance"
    )]
    UnidentifiedController,
}
fn transport(error: reqwest::Error) -> Error {
    Error::new(ErrorKind::Transport(error.without_url()))
}

/// Encodes one path segment or query value, including spaces, slashes and Unicode.
pub fn encode_component(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

pub struct Client {
    http: reqwest::Client,
    port: u16,
    secret: String,
}
impl Client {
    pub fn new(port: u16, secret: &str) -> Result<Self, Error> {
        if port == 0 {
            return Err(Error::new(ErrorKind::InvalidRequest(
                "port must be nonzero",
            )));
        }
        if secret.is_empty() || secret.chars().any(char::is_control) {
            return Err(Error::new(ErrorKind::InvalidRequest(
                "secret is empty or contains control characters",
            )));
        }
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(transport)?;
        Ok(Self {
            http,
            port,
            secret: secret.to_owned(),
        })
    }
    /// Verify that the listener both requires authentication and accepts our secret.
    /// A secretless foreign Mihomo accepts arbitrary bearer headers, so a successful
    /// authenticated request alone is insufficient before a mutation.
    pub async fn verify_controller(&self) -> Result<(), Error> {
        let response = self
            .http
            .get(format!("http://127.0.0.1:{}/version", self.port))
            .send()
            .await
            .map_err(transport)?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::new(ErrorKind::UnidentifiedController));
        }
        self.request("GET", "/version", None).await?;
        Ok(())
    }

    pub async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, Error> {
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains('#')
            || path.chars().any(char::is_control)
        {
            return Err(Error::new(ErrorKind::InvalidRequest(
                "path must be an encoded absolute API path",
            )));
        }
        let method = method
            .parse::<reqwest::Method>()
            .map_err(|_| Error::new(ErrorKind::InvalidRequest("invalid HTTP method")))?;
        let mut request = self
            .http
            .request(method, format!("http://127.0.0.1:{}{path}", self.port))
            .bearer_auth(&self.secret);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.map_err(transport)?;
        if !response.status().is_success() {
            return Err(Error::new(ErrorKind::HttpStatus(
                response.status().as_u16(),
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(Error::new(ErrorKind::ResponseTooLarge));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(Error::new(ErrorKind::ResponseTooLarge));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(json!({"ok":true}));
        }
        serde_json::from_slice(&bytes).map_err(|e| Error::new(ErrorKind::Json(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn mock(status: &str, body: &str) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let count = stream.read(&mut chunk).unwrap();
                assert_ne!(count, 0);
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (port, handle)
    }
    #[tokio::test]
    async fn authorization_and_escaped_node_path() {
        let (port, server) = mock("200 OK", r#"{"delay":12}"#);
        let client = Client::new(port, "test-secret").unwrap();
        let path = format!(
            "/proxies/{}/delay?url={}",
            encode_component("Hong Kong/香港"),
            encode_component("https://example.com/a?b=c")
        );
        assert_eq!(
            client.request("GET", &path, None).await.unwrap()["delay"],
            12
        );
        let request = server.join().unwrap();
        assert!(request.starts_with(&format!("GET {path} HTTP/1.1")));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-secret\r\n")
        );
    }
    #[tokio::test]
    async fn empty_response_is_success() {
        let (port, server) = mock("204 No Content", "");
        assert_eq!(
            Client::new(port, "secret")
                .unwrap()
                .request("PUT", "/configs", Some(json!({})))
                .await
                .unwrap(),
            json!({"ok":true})
        );
        server.join().unwrap();
    }
    #[tokio::test]
    async fn http_error_does_not_retain_sensitive_body() {
        let (port, server) = mock("401 Unauthorized", "SECRET BODY");
        let error = Client::new(port, "SECRET HEADER")
            .unwrap()
            .request("GET", "/version", None)
            .await
            .unwrap_err();
        assert!(matches!(error.kind(), ErrorKind::HttpStatus(401)));
        assert!(!format!("{error:?}").contains("SECRET"));
        server.join().unwrap();
    }
    #[tokio::test]
    async fn invalid_json_is_reported() {
        let (port, server) = mock("200 OK", "not json");
        let error = Client::new(port, "secret")
            .unwrap()
            .request("GET", "/version", None)
            .await
            .unwrap_err();
        assert!(matches!(error.kind(), ErrorKind::Json(_)));
        server.join().unwrap();
    }
    #[tokio::test]
    async fn rejects_redirects_and_invalid_paths() {
        let (port, server) = mock("302 Found", "");
        let client = Client::new(port, "secret").unwrap();
        assert!(matches!(
            client
                .request("GET", "/version", None)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::HttpStatus(302)
        ));
        server.join().unwrap();
        assert!(matches!(
            client
                .request("GET", "//remote.example", None)
                .await
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidRequest(_)
        ));
    }
    #[tokio::test]
    async fn refuses_foreign_controller_accepting_any_token() {
        let (port, server) = mock("200 OK", r#"{"version":"foreign"}"#);
        let error = Client::new(port, "our-secret")
            .unwrap()
            .verify_controller()
            .await
            .unwrap_err();
        assert!(matches!(error.kind(), ErrorKind::UnidentifiedController));
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /version HTTP/1.1"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }

    #[tokio::test]
    async fn verifies_authentication_is_required_and_secret_is_accepted() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for expected_auth in [false, true] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = stream.read(&mut chunk).unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(bytes).unwrap().to_ascii_lowercase();
                assert_eq!(
                    request.contains("authorization: bearer our-secret\r\n"),
                    expected_auth
                );
                let (status, body) = if expected_auth {
                    ("200 OK", r#"{"version":"test"}"#)
                } else {
                    ("401 Unauthorized", "")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        Client::new(port, "our-secret")
            .unwrap()
            .verify_controller()
            .await
            .unwrap();
        server.join().unwrap();
    }
}
