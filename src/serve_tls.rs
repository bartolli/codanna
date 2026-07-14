//! Single source of truth for the persisted self-signed TLS cert/key paths,
//! and for the ONE cert-pinning HTTP client used to dial a `--https` backing
//! MCP server.
//!
//! This lives at crate root, not under `src/mcp/`: it is consumed by
//! [`crate::serve_discovery`] (root, for the `/health` probe against an
//! `--https` backing server) and by `mcp::proxy` / `mcp::https_server`
//! (subsystem). Nesting it inside either subsystem's directory would force
//! the other subsystem to depend on it through an unrelated module, which
//! is exactly the layering violation this module is meant to avoid.
//!
//! # Cert trust
//!
//! [`pinned_client`] pins the persisted server certificate via
//! [`reqwest::ClientBuilder::tls_certs_only`]: only that one certificate is
//! trusted, system/native roots are excluded entirely, and there is no
//! verification bypass on any error path. This is deliberate and
//! non-negotiable -- do not replace `tls_certs_only` with
//! `add_root_certificate` (deprecated, merges with native roots) or with any
//! `danger_accept_invalid_certs`-style escape hatch.

use std::path::PathBuf;

use thiserror::Error;

/// Absolute paths to the persisted self-signed server cert and key.
///
/// This is the SINGLE definition of
/// `dirs::config_dir()/codanna/certs/{server.pem,server.key}`. Both the
/// writer (`mcp::https_server::get_or_create_certificate`, which generates
/// and persists the cert/key) and every reader (this module's
/// [`pinned_client`]) go through this function rather than recomputing the
/// join; otherwise the writer and a reader can silently drift onto different
/// files and the pin would point at the wrong certificate.
///
/// Returns `None` if the platform config directory cannot be determined
/// (mirrors [`dirs::config_dir`]'s own `None` case, e.g. no
/// `HOME`/`XDG_CONFIG_HOME` on Unix).
pub fn cert_paths() -> Option<(PathBuf, PathBuf)> {
    let cert_dir = dirs::config_dir()?.join("codanna").join("certs");
    Some((cert_dir.join("server.pem"), cert_dir.join("server.key")))
}

/// Errors constructing the cert-pinning client used to dial a `--https`
/// backing MCP server.
#[derive(Debug, Error)]
pub enum TlsClientError {
    #[error(
        "persisted server certificate not found at '{path}'; start the backing server at least once with 'codanna serve --https' so it generates and persists a self-signed certificate before a client can pin it"
    )]
    CertNotFound { path: PathBuf },

    #[error("failed to read persisted server certificate '{path}': {source}")]
    CertRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error(
        "failed to parse persisted server certificate '{path}' as PEM: {source}; delete the certs directory and restart 'codanna serve --https' to regenerate it"
    )]
    CertParse {
        path: PathBuf,
        #[source]
        source: rmcp_reqwest::Error,
    },

    #[error("failed to build the cert-pinned HTTPS client: {source}")]
    ClientBuild {
        #[source]
        source: rmcp_reqwest::Error,
    },

    #[error(
        "codanna was built without the 'https-server' feature; rebuild with --features https-server to dial a '--https' backing server"
    )]
    HttpsSupportNotCompiled,
}

/// Result alias for this module's fallible operations (§RS.3).
pub type TlsResult<T> = Result<T, TlsClientError>;

/// One-time, idempotent install of the `ring` rustls crypto provider as the
/// process default.
///
/// `reqwest`'s `rustls-no-provider` backend (the one this module uses via
/// [`pinned_client`]) requires a default [`rustls::crypto::CryptoProvider`]
/// to already be installed before the FIRST `rmcp_reqwest::Client` is built
/// anywhere in the process, or that build panics. This crate already pins
/// the shared `rustls` dependency to the `ring` backend only (see the
/// `[features].https-server` comment in `Cargo.toml`), so installing `ring`
/// here is consistent with, not additional to, that existing pin.
/// `install_default` failing (a provider is already installed, e.g. by the
/// HTTPS server binding via `axum-server`) is expected and ignored: either
/// provider being `ring` is equally correct for this process.
#[cfg(feature = "https-server")]
fn ensure_crypto_provider_installed() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// The ONE TLS-trusting client used to dial a `--https` backing server.
///
/// Used by [`crate::serve_discovery`]'s `/health` probe and by `mcp::proxy`
/// (as the `rmcp` `StreamableHttpClient`). Trust is `tls_certs_only([persisted
/// cert])`: no system roots, no verification bypass.
///
/// Pool/redirect settings mirror `rmcp`'s own
/// `StreamableHttpClientTransport::default_http_client`: `pool_max_idle_per_host(0)`
/// avoids a ~40ms stall from TCP Delayed ACK when a streamed SSE response body
/// was not fully consumed before the pool tries to reuse the connection, and
/// `redirect::Policy::none()` stops a redirect target from replaying
/// caller-supplied custom headers (e.g. the bearer token). Dropping either
/// silently changes transport behavior, not just decoration.
#[cfg(feature = "https-server")]
pub fn pinned_client() -> TlsResult<rmcp_reqwest::Client> {
    let (cert_path, _key_path) = cert_paths().ok_or_else(|| TlsClientError::CertNotFound {
        path: PathBuf::from("<unresolvable config directory>/codanna/certs/server.pem"),
    })?;

    if !cert_path.is_file() {
        return Err(TlsClientError::CertNotFound { path: cert_path });
    }

    let pem = std::fs::read(&cert_path).map_err(|source| TlsClientError::CertRead {
        path: cert_path.clone(),
        source,
    })?;

    let cert =
        rmcp_reqwest::Certificate::from_pem(&pem).map_err(|source| TlsClientError::CertParse {
            path: cert_path.clone(),
            source,
        })?;

    ensure_crypto_provider_installed();

    rmcp_reqwest::Client::builder()
        .tls_certs_only([cert])
        .pool_max_idle_per_host(0)
        .redirect(rmcp_reqwest::redirect::Policy::none())
        .build()
        .map_err(|source| TlsClientError::ClientBuild { source })
}

/// Without the `https-server` feature there is no TLS backend compiled into
/// the pinned client's `reqwest` dependency, so a `--https` backing server
/// can never be dialed from this build.
#[cfg(not(feature = "https-server"))]
pub fn pinned_client() -> TlsResult<rmcp_reqwest::Client> {
    Err(TlsClientError::HttpsSupportNotCompiled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// On Linux, `dirs::config_dir()` prefers `XDG_CONFIG_HOME` over
    /// `HOME/.config`. Overriding only `HOME` does not isolate this test from
    /// whatever `XDG_CONFIG_HOME` is already set to in the ambient
    /// environment (e.g. a real persisted cert from a developer's own
    /// `codanna serve --https` run), so both must be pointed at the temp dir.
    #[test]
    fn pinned_client_errors_when_cert_absent() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");

        let original_home = std::env::var("HOME").ok();
        let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            std::env::set_var("HOME", temp_dir.path());
            std::env::set_var("XDG_CONFIG_HOME", temp_dir.path().join(".config"));
        }

        let result = pinned_client();

        unsafe {
            match original_home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
            match original_xdg_config_home {
                Some(xdg) => std::env::set_var("XDG_CONFIG_HOME", xdg),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }

        match result {
            #[cfg(feature = "https-server")]
            Err(TlsClientError::CertNotFound { path }) => {
                let rendered = TlsClientError::CertNotFound { path: path.clone() }.to_string();
                assert!(
                    rendered.contains(&path.display().to_string()),
                    "rendered CertNotFound message must contain the cert path, got: {rendered}"
                );
            }
            #[cfg(not(feature = "https-server"))]
            Err(TlsClientError::HttpsSupportNotCompiled) => {}
            other => panic!(
                "expected CertNotFound (https-server) or HttpsSupportNotCompiled (no https-server), got: {other:?}"
            ),
        }
    }
}
