//! OAuth 2.1 for MCP HTTP / OpenAPI / GraphQL — wraps `rmcp::transport::auth`.
//!
//! Credentials and sticky redirect metadata live under
//! `$SKIFF_CACHE_DIR/oauth/<server-hash>/` with `0o700` dirs and `0o600` files.
//! Authorization-code callbacks bind loopback only; sticky URIs are re-validated.

mod browser;
mod callback;
mod config;
mod setup;
mod store;

pub use config::{
    find_free_port, oauth_dir_for_server, parse_oauth_flow, port_available, resolve_redirect_uri,
    server_hash, validate_redirect_uri, OAuthFlow, OAuthOptions,
};
pub use setup::{
    authorize, clear_oauth_credentials, discovery_url_from_args, oauth_wanted, options_from_args,
    setup_from_args, OAuthReady,
};
pub use store::FileCredentialStore;
