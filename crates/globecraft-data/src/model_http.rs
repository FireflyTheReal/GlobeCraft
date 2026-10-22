//! Requests to AI model providers for the assistant: streamed on the service runtime, with the
//! body exposed piece by piece for a live preview and the whole response delivered at the end.
//! Request and response types are plain data, so this module needs no assistant dependency.
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest silence between two pieces of a streamed reply (reasoning models pause).
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// A request to send, as the assistant built it.
pub struct ModelRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// Most bytes of the reply to read.
    pub limit: usize,
    /// Response headers to keep (lower case); others are dropped.
    pub keep_headers: &'static [&'static str],
}

pub struct ModelResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

/// Why a request produced no response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelFailure {
    Connection(String),
    Timeout(String),
    Interrupted(String),
}

#[derive(Default)]
struct Shared {
    chunks: Vec<u8>,
    result: Option<Result<ModelResponse, ModelFailure>>,
}

/// A request in flight. Dropping it cancels the request.
pub struct ModelCall {
    shared: Arc<Mutex<Shared>>,
    task: tokio::task::JoinHandle<()>,
}

impl ModelCall {
    /// Pieces of the body that arrived since the last call.
    pub fn take_chunks(&self) -> Vec<u8> {
        std::mem::take(&mut self.shared.lock().expect("model call poisoned").chunks)
    }
    /// The finished response, once.
    pub fn take_result(&self) -> Option<Result<ModelResponse, ModelFailure>> {
        self.shared
            .lock()
            .expect("model call poisoned")
            .result
            .take()
    }
}

impl Drop for ModelCall {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn client() -> Result<reqwest::Client, reqwest::Error> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "GlobeCraft/",
            env!("CARGO_PKG_VERSION"),
            " (AI assistant)"
        ))
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

fn failure(error: &reqwest::Error) -> ModelFailure {
    // Never include the URL: some providers carry keys in query strings.
    let text = error.without_url_ref().to_string();
    if error.is_timeout() {
        ModelFailure::Timeout(text)
    } else if error.is_connect() || error.is_request() {
        ModelFailure::Connection(text)
    } else {
        ModelFailure::Interrupted(text)
    }
}

trait WithoutUrl {
    fn without_url_ref(&self) -> String;
}
impl WithoutUrl for reqwest::Error {
    fn without_url_ref(&self) -> String {
        let mut text = self.to_string();
        if let Some(url) = self.url() {
            text = text.replace(url.as_str(), "<provider>");
        }
        text
    }
}

/// Sends `request` on `handle`, waking the interface as pieces arrive and when it ends.
pub fn send(
    handle: &tokio::runtime::Handle,
    request: ModelRequest,
    wake: impl Fn() + Send + Sync + 'static,
) -> ModelCall {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let worker = shared.clone();
    let task = handle.spawn(async move {
        let result = run(request, &worker, &wake).await;
        worker.lock().expect("model call poisoned").result = Some(result);
        wake();
    });
    ModelCall { shared, task }
}

async fn run(
    request: ModelRequest,
    shared: &Mutex<Shared>,
    wake: &(impl Fn() + Send + Sync),
) -> Result<ModelResponse, ModelFailure> {
    let client = client().map_err(|e| failure(&e))?;
    let method = reqwest::Method::from_bytes(request.method.as_bytes())
        .map_err(|e| ModelFailure::Connection(e.to_string()))?;
    let mut builder = client.request(method, &request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let mut response = builder.send().await.map_err(|e| failure(&e))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| request.keep_headers.contains(&name.as_str()))
        .filter_map(|(name, value)| {
            Some((name.as_str().to_string(), value.to_str().ok()?.to_string()))
        })
        .collect();
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > request.limit {
                    return Err(ModelFailure::Interrupted(format!(
                        "the reply exceeded {} MiB",
                        request.limit >> 20
                    )));
                }
                body.extend_from_slice(&chunk);
                shared
                    .lock()
                    .expect("model call poisoned")
                    .chunks
                    .extend_from_slice(&chunk);
                wake();
            }
            Ok(None) => break,
            Err(e) => return Err(failure(&e)),
        }
    }
    Ok(ModelResponse {
        status,
        body,
        headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn streams_pieces_and_keeps_only_listed_headers() {
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        let listener = runtime
            .handle()
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        runtime.spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await.unwrap();
            let body = "data: one\n\ndata: two\n\n";
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nrequest-id: r1\r\nset-cookie: secret\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        let call = send(
            runtime.handle(),
            ModelRequest {
                method: "POST",
                url: format!("http://{addr}/v1/messages"),
                headers: vec![("content-type".into(), "application/json".into())],
                body: Some("{}".into()),
                limit: 1 << 20,
                keep_headers: &["request-id"],
            },
            || {},
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let result = loop {
            if let Some(result) = call.take_result() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline, "no response");
            std::thread::sleep(Duration::from_millis(10));
        };
        let response = result.unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"data: one\n\ndata: two\n\n");
        assert_eq!(
            response.headers,
            [("request-id".to_string(), "r1".to_string())]
        );
        assert_eq!(call.take_chunks(), response.body);
    }

    #[test]
    fn refused_connections_are_classified() {
        let runtime = crate::runtime::ServiceRuntime::new().unwrap();
        // Bind and drop to find a closed local port.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let call = send(
            runtime.handle(),
            ModelRequest {
                method: "GET",
                url: format!("http://127.0.0.1:{port}/models?key=secret"),
                headers: vec![],
                body: None,
                limit: 1024,
                keep_headers: &[],
            },
            || {},
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let result = loop {
            if let Some(result) = call.take_result() {
                break result;
            }
            assert!(std::time::Instant::now() < deadline, "no response");
            std::thread::sleep(Duration::from_millis(10));
        };
        match result {
            Err(ModelFailure::Connection(text)) => assert!(!text.contains("secret"), "{text}"),
            Err(other) => panic!("unexpected {other:?}"),
            Ok(_) => panic!("closed port answered"),
        }
    }
}
