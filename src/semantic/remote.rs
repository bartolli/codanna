//! Remote embedding backend — OpenAI-compatible HTTP endpoint.
//!
//! Replaces local fastembed when `semantic_search.remote_url` is configured
//! (or the `CODANNA_EMBED_URL` environment variable is set).
//!
//! Compatible with Infinity, OpenAI, vLLM, and any server that serves
//! POST /v1/embeddings with the OpenAI request/response schema.

use std::future::Future;
use std::time::Duration;

use reqwest::Client;

/// Run an async future from a sync context.
///
/// `block_in_place` requires a multi-thread Tokio runtime. If the current
/// runtime is single-threaded (or there is no runtime), we fall back to
/// spawning a temporary `current_thread` runtime on this thread instead.
pub(crate) fn run_async<F, T>(f: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            // block_in_place is valid only on multi-thread schedulers.
            // Detect single-thread by attempting a spawn; if it would block,
            // we have a multi-thread runtime and can use block_in_place.
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
                tokio::task::block_in_place(|| handle.block_on(f))
            } else {
                // Current-thread runtime: we cannot block_in_place.
                // Spawn a sibling thread with its own runtime instead.
                std::thread::scope(|s| {
                    s.spawn(|| {
                        tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("failed to build tokio runtime")
                            .block_on(f)
                    })
                    .join()
                    .expect("async worker thread panicked")
                })
            }
        }
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
            .block_on(f),
    }
}
use serde::{Deserialize, Serialize};

use super::SemanticSearchError;

// ── Request / Response types ───────────────────────────────────────────────

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    index: usize,
    embedding: Vec<f32>,
}

// ── RemoteEmbedder ─────────────────────────────────────────────────────────

/// Embedding client for an OpenAI-compatible HTTP server.
///
/// All requests are batched in chunks of `BATCH_SIZE` to avoid hitting
/// server request-size limits. Each request has a 30-second timeout.
#[derive(Clone)]
pub struct RemoteEmbedder {
    client: Client,
    url: String,
    model: String,
    dim: usize,
    api_key: Option<String>,
}

const BATCH_SIZE: usize = 64;
const REQUEST_TIMEOUT_SECS: u64 = 30;
const MAX_TEXT_CHARS: usize = 2000;

impl RemoteEmbedder {
    /// Build a RemoteEmbedder, probing the server to confirm the dimension
    /// matches `expected_dim` when provided.
    pub async fn new(
        base_url: &str,
        model: &str,
        expected_dim: Option<usize>,
        api_key: Option<String>,
    ) -> Result<Self, SemanticSearchError> {
        // reqwest reads the process-default rustls provider and panics without
        // one. A lost install race means another thread set one; either serves.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        // Bundled roots merged into the platform store: a host with no system
        // CA certificates still builds a client.
        let bundled = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                SemanticSearchError::ModelInitError(format!(
                    "Bundled root certificate rejected: {e}"
                ))
            })?;
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .tls_certs_merge(bundled)
            .build()
            .map_err(|e| {
                SemanticSearchError::ModelInitError(format!("HTTP client build failed: {e}"))
            })?;

        let url = format!("{}/v1/embeddings", base_url.trim_end_matches('/'));

        // Probe with a single text to determine / validate dimension
        let probe = Self::request(
            &client,
            &url,
            model,
            &["probe".to_string()],
            api_key.as_deref(),
        )
        .await?;
        let actual_dim = probe.first().map(|v| v.len()).ok_or_else(|| {
            SemanticSearchError::ModelInitError(
                "Remote server returned empty embedding on probe".into(),
            )
        })?;

        if let Some(expected) = expected_dim {
            if actual_dim != expected {
                return Err(SemanticSearchError::ModelInitError(format!(
                    "Remote embedding dim mismatch: expected {expected}, server returned {actual_dim}"
                )));
            }
        }

        tracing::info!(
            target: "semantic",
            "Remote embedding backend ready: url={url} model={model} dim={actual_dim}"
        );

        Ok(Self {
            client,
            url,
            model: model.to_string(),
            dim: actual_dim,
            api_key,
        })
    }

    /// Output dimension of this embedding model.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Embed a batch of texts, truncating each to `MAX_TEXT_CHARS` characters.
    /// Sends requests in chunks of `BATCH_SIZE`.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, SemanticSearchError> {
        let mut results: Vec<(usize, Vec<f32>)> = Vec::with_capacity(texts.len());

        // Truncate by char count, not bytes, to avoid splitting multi-byte codepoints.
        let truncated: Vec<String> = texts
            .iter()
            .map(|t| {
                if t.chars().count() > MAX_TEXT_CHARS {
                    t.chars().take(MAX_TEXT_CHARS).collect()
                } else {
                    t.clone()
                }
            })
            .collect();

        for (chunk_start, chunk) in truncated.chunks(BATCH_SIZE).enumerate() {
            let embeddings = Self::request(
                &self.client,
                &self.url,
                &self.model,
                chunk,
                self.api_key.as_deref(),
            )
            .await?;

            if embeddings.len() != chunk.len() {
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Remote server returned {} embeddings for {} inputs",
                    embeddings.len(),
                    chunk.len()
                )));
            }

            for (i, emb) in embeddings.into_iter().enumerate() {
                if emb.len() != self.dim {
                    return Err(SemanticSearchError::EmbeddingError(format!(
                        "Remote embedding at index {} has dim {}, expected {}",
                        chunk_start * BATCH_SIZE + i,
                        emb.len(),
                        self.dim
                    )));
                }
                results.push((chunk_start * BATCH_SIZE + i, emb));
            }
        }

        // Sort by original index and return in order
        results.sort_by_key(|(i, _)| *i);
        Ok(results.into_iter().map(|(_, emb)| emb).collect())
    }

    async fn request(
        client: &Client,
        url: &str,
        model: &str,
        texts: &[String],
        api_key: Option<&str>,
    ) -> Result<Vec<Vec<f32>>, SemanticSearchError> {
        let body = EmbedRequest {
            model,
            input: texts,
        };

        let mut req = client.post(url).json(&body);
        if let Some(key) = api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await.map_err(|e| {
            SemanticSearchError::EmbeddingError(format!("Remote embed request failed: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(SemanticSearchError::EmbeddingError(format!(
                "Remote embed server returned {status}: {text}"
            )));
        }

        let parsed: EmbedResponse = resp.json().await.map_err(|e| {
            SemanticSearchError::EmbeddingError(format!("Failed to parse embed response: {e}"))
        })?;

        // Sort by index and validate contiguous range [0, len)
        let mut data = parsed.data;
        data.sort_by_key(|d| d.index);

        for (expected, d) in data.iter().enumerate() {
            if d.index != expected {
                return Err(SemanticSearchError::EmbeddingError(format!(
                    "Remote embed response has non-contiguous index: expected {expected}, got {}",
                    d.index
                )));
            }
        }

        Ok(data.into_iter().map(|d| d.embedding).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Answer `requests` POSTs on a loopback port with a fixed JSON body.
    fn stub_server(body: &'static str, requests: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(requests) {
                let mut stream = stream.unwrap();
                let mut seen = Vec::new();
                let mut buf = [0u8; 4096];
                let mut expected = None;
                while expected.is_none_or(|total| seen.len() < total) {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    seen.extend_from_slice(&buf[..n]);
                    if expected.is_none() {
                        if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
                            let length = head
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length:"))
                                .map_or(0, |v| v.trim().parse::<usize>().unwrap());
                            expected = Some(end + 4 + length);
                        }
                    }
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}")
    }

    // reqwest panics when the process has no rustls provider. `new` installs
    // one itself: a caller that set nothing up still gets a working client.
    #[test]
    fn new_builds_a_client_and_probes_without_caller_setup() {
        let url = stub_server(r#"{"data":[{"index":0,"embedding":[0.6,0.8]}]}"#, 1);
        let embedder =
            run_async(async move { RemoteEmbedder::new(&url, "stub", Some(2), None).await })
                .expect("client builds and the probe completes");
        assert_eq!(embedder.dim(), 2);
    }
}
