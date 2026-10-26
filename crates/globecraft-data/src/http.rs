use crate::paths::cache_dir;
use std::{path::PathBuf, time::Duration};
const MAX_CACHE_FILENAME_CHARS: usize = 120;
const JSON_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const JSON_READ_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) fn geo_cache_path(url: &str) -> PathBuf {
    let clean: String = url
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let start = clean.len().saturating_sub(MAX_CACHE_FILENAME_CHARS);
    cache_dir().join("geo").join(&clean[start..])
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Policy {
    Tile,
    Boundary,
    Wayback,
    Search,
    Suggest,
    Cloud,
    Radar,
    Aircraft,
}
pub(crate) fn client(policy: Policy) -> Result<reqwest::Client, crate::Error> {
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
    };
    static CLIENTS: OnceLock<Mutex<HashMap<Policy, reqwest::Client>>> = OnceLock::new();
    let mut clients = CLIENTS
        .get_or_init(Default::default)
        .lock()
        .expect("HTTP client pool poisoned");
    if let Some(client) = clients.get(&policy) {
        return Ok(client.clone());
    }
    let mut builder = reqwest::Client::builder();
    let agent = match policy {
        Policy::Tile | Policy::Search => {
            "GlobeCraft/0.2 (educational 3D globe viewer; +https://github.com/FireflyTheReal/GlobeCraft)"
        }
        Policy::Boundary => "GlobeCraft/0.1 (map boundary overlay)",
        Policy::Wayback => "GlobeCraft/0.2 (wayback release list)",
        Policy::Suggest => "GlobeCraft/0.2 (search suggestions)",
        Policy::Cloud => "GlobeCraft/1.0 (attribution: Matt Eason, EUMETSAT)",
        Policy::Radar => "GlobeCraft/1.0 radar viewer",
        Policy::Aircraft => "GlobeCraft/1.0 (live aircraft viewer; OpenSky / ADSB.lol)",
    };
    builder = builder.user_agent(agent);
    match policy {
        Policy::Tile => builder = builder.timeout(Duration::from_secs(15)),
        Policy::Boundary => {
            builder = builder
                .connect_timeout(JSON_CONNECT_TIMEOUT)
                .read_timeout(JSON_READ_TIMEOUT)
        }
        Policy::Wayback => {
            builder = builder
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(60))
        }
        Policy::Suggest => builder = builder.timeout(Duration::from_secs(8)),
        Policy::Cloud => builder = builder.timeout(Duration::from_secs(65)),
        Policy::Radar => builder = builder.timeout(Duration::from_secs(12)),
        Policy::Aircraft => builder = builder.timeout(Duration::from_secs(25)),
        Policy::Search => {}
    }
    let client = builder.build()?;
    clients.insert(policy, client.clone());
    Ok(client)
}
pub(crate) async fn body(
    request: reqwest::RequestBuilder,
    limit: u64,
) -> Result<Vec<u8>, crate::Error> {
    let mut response = request.send().await?.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let remaining = (limit as usize).saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if bytes.len() as u64 >= limit {
            break;
        }
    }
    Ok(bytes)
}
/// A response body, or how long the server asked us to wait (429 Too Many Requests or
/// 503 Service Unavailable, with `Retry-After` in seconds when given).
pub(crate) enum Fetched {
    Body(Vec<u8>),
    RetryAfter(Duration),
}
/// Waited when a server throttles without saying for how long.
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(10);
/// The longest wait honoured; tiles are retried after it whatever the server says.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// `body`, but a throttling answer comes back as `Fetched::RetryAfter` instead of an error.
pub(crate) async fn body_or_retry_after(
    request: reqwest::RequestBuilder,
    limit: u64,
) -> Result<Fetched, crate::Error> {
    let mut response = request.send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
    {
        let wait = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map_or(DEFAULT_RETRY_AFTER, Duration::from_secs);
        return Ok(Fetched::RetryAfter(wait.min(MAX_RETRY_AFTER)));
    }
    response = response.error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let remaining = (limit as usize).saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if bytes.len() as u64 >= limit {
            break;
        }
    }
    Ok(Fetched::Body(bytes))
}
pub(crate) async fn parse_json(bytes: Vec<u8>) -> Result<serde_json::Value, crate::Error> {
    tokio::task::spawn_blocking(move || serde_json::from_slice(&bytes))
        .await?
        .map_err(Into::into)
}
/// Fetch boundary JSON with bounded retries and disk-cache fallback.
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn server(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let job = tokio::spawn(async move {
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await.unwrap();
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
        });
        (url, job)
    }
    #[tokio::test]
    async fn body_caps_reads_and_rejects_http_errors() {
        let (url, job) = server(vec![("200 OK", "hello world")]).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert_eq!(body(client.get(url), 4).await.unwrap(), b"hell");
        job.await.unwrap();
        let (url, job) = server(vec![("500 Server Error", "bad")]).await;
        assert!(matches!(
            body(client.get(url), 100).await,
            Err(crate::Error::Http(_))
        ));
        job.await.unwrap();
    }

    #[tokio::test]
    async fn throttling_answers_say_how_long_to_wait() {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let (url, job) = server(vec![("429 Too Many Requests\r\nRetry-After: 7", "")]).await;
        assert!(matches!(
            body_or_retry_after(client.get(url), 100).await,
            Ok(Fetched::RetryAfter(wait)) if wait == Duration::from_secs(7)
        ));
        job.await.unwrap();
        let (url, job) = server(vec![("503 Service Unavailable", "")]).await;
        assert!(matches!(
            body_or_retry_after(client.get(url), 100).await,
            Ok(Fetched::RetryAfter(wait)) if wait == DEFAULT_RETRY_AFTER
        ));
        job.await.unwrap();
        let (url, job) = server(vec![("200 OK", "tile")]).await;
        assert!(matches!(
            body_or_retry_after(client.get(url), 100).await,
            Ok(Fetched::Body(bytes)) if bytes == b"tile"
        ));
        job.await.unwrap();
    }
}
