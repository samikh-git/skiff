//! OAuth flow selection and redirect URI helpers.

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use url::Url;

use crate::error::{Error, Result};
use crate::paths::cache_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthFlow {
    Auto,
    AuthorizationCode,
    ClientCredentials,
}

impl OAuthFlow {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AuthorizationCode => "authorization_code",
            Self::ClientCredentials => "client_credentials",
        }
    }
}

pub fn parse_oauth_flow(s: &str) -> Result<OAuthFlow> {
    match s {
        "auto" => Ok(OAuthFlow::Auto),
        "authorization_code" => Ok(OAuthFlow::AuthorizationCode),
        "client_credentials" => Ok(OAuthFlow::ClientCredentials),
        other => Err(Error::usage(format!(
            "invalid --oauth-flow {other:?}; expected auto|authorization_code|client_credentials"
        ))),
    }
}

/// Resolved OAuth options after secret resolution and validation.
#[derive(Debug, Clone)]
pub struct OAuthOptions {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_name: String,
    pub scope: Option<String>,
    pub redirect_uri: Option<String>,
    pub flow: OAuthFlow,
}

impl OAuthOptions {
    /// True when auto or explicit client-credentials and both credentials present.
    pub fn use_client_credentials(&self) -> bool {
        match self.flow {
            OAuthFlow::ClientCredentials => true,
            OAuthFlow::Auto => self.client_id.is_some() && self.client_secret.is_some(),
            OAuthFlow::AuthorizationCode => false,
        }
    }
}

/// Validate `--oauth-redirect-uri`: http, loopback host, explicit port.
pub fn validate_redirect_uri(uri: &str) -> Result<Url> {
    let parsed = Url::parse(uri).map_err(|e| Error::usage(format!("invalid redirect URI: {e}")))?;
    if parsed.scheme() != "http" {
        return Err(Error::usage(
            "oauth redirect URI must use http:// (local callback server)",
        ));
    }
    let host = parsed.host_str().unwrap_or("");
    if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return Err(Error::usage(format!(
            "oauth redirect URI host must be loopback (localhost/127.0.0.1/::1), got {host:?}"
        )));
    }
    if parsed.port().is_none() {
        return Err(Error::usage(
            "oauth redirect URI must include an explicit port",
        ));
    }
    Ok(parsed)
}

pub fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| Error::runtime(format!("cannot bind free port: {e}")))?;
    Ok(listener
        .local_addr()
        .map_err(|e| Error::runtime(e.to_string()))?
        .port())
}

pub fn port_available(host: &str, port: u16) -> bool {
    let addr = match format!("{host}:{port}").parse::<SocketAddr>() {
        Ok(a) => a,
        Err(_) => {
            // IPv6 localhost
            if host == "::1" {
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port))
            } else {
                return false;
            }
        }
    };
    TcpListener::bind(addr).is_ok()
}

pub fn server_hash(server_url: &str) -> String {
    let digest = Sha256::digest(server_url.as_bytes());
    hex::encode(&digest[..8])
}

pub fn oauth_dir_for_server(server_url: &str) -> PathBuf {
    cache_dir().join("oauth").join(server_hash(server_url))
}

/// Resolve redirect URI: explicit validated URI, sticky cached, or fresh free port.
pub fn resolve_redirect_uri(
    explicit: Option<&str>,
    sticky_uri: Option<&str>,
) -> Result<String> {
    if let Some(uri) = explicit {
        validate_redirect_uri(uri)?;
        return Ok(uri.to_string());
    }
    if let Some(uri) = sticky_uri {
        if let Ok(parsed) = Url::parse(uri) {
            if parsed.scheme() == "http" {
                let host = parsed.host_str().unwrap_or("127.0.0.1");
                if let Some(port) = parsed.port() {
                    if port_available(host, port) {
                        return Ok(uri.to_string());
                    }
                }
            }
        }
    }
    let port = find_free_port()?;
    Ok(format!("http://127.0.0.1:{port}/callback"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_parse() {
        assert_eq!(parse_oauth_flow("auto").unwrap(), OAuthFlow::Auto);
        assert_eq!(
            parse_oauth_flow("authorization_code").unwrap(),
            OAuthFlow::AuthorizationCode
        );
        assert!(parse_oauth_flow("bogus").is_err());
    }

    #[test]
    fn redirect_validation() {
        assert!(validate_redirect_uri("http://127.0.0.1:18080/callback").is_ok());
        assert!(validate_redirect_uri("http://localhost:9999/cb").is_ok());
        assert!(validate_redirect_uri("https://127.0.0.1:18080/callback").is_err());
        assert!(validate_redirect_uri("http://127.0.0.1/callback").is_err());
        assert!(validate_redirect_uri("http://example.com:8080/callback").is_err());
    }

    #[test]
    fn auto_picks_cc_when_both_creds() {
        let opts = OAuthOptions {
            client_id: Some("id".into()),
            client_secret: Some("sec".into()),
            client_name: "mcp2cli".into(),
            scope: None,
            redirect_uri: None,
            flow: OAuthFlow::Auto,
        };
        assert!(opts.use_client_credentials());
        let opts2 = OAuthOptions {
            client_secret: None,
            flow: OAuthFlow::Auto,
            ..opts.clone()
        };
        assert!(!opts2.use_client_credentials());
        let opts3 = OAuthOptions {
            flow: OAuthFlow::AuthorizationCode,
            ..opts
        };
        assert!(!opts3.use_client_credentials());
    }

    #[test]
    fn server_hash_stable() {
        assert_eq!(server_hash("http://x"), server_hash("http://x"));
        assert_ne!(server_hash("http://x"), server_hash("http://y"));
        assert_eq!(server_hash("http://x").len(), 16);
    }

    #[test]
    fn sticky_redirect_reused_when_port_free() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let sticky = format!("http://127.0.0.1:{port}/callback");
        let resolved = resolve_redirect_uri(None, Some(&sticky)).unwrap();
        assert_eq!(resolved, sticky);
    }

    #[test]
    fn explicit_redirect_wins_over_sticky() {
        let explicit = "http://127.0.0.1:18080/callback";
        let sticky = "http://127.0.0.1:18081/callback";
        assert_eq!(
            resolve_redirect_uri(Some(explicit), Some(sticky)).unwrap(),
            explicit
        );
    }
}
