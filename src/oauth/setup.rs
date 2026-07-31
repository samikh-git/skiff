//! Build and run OAuth flows via `rmcp::transport::auth`.

use std::time::Duration;

use rmcp::transport::auth::{
    AuthClient, AuthorizationRequest, ClientCredentialsConfig, CredentialStore, OAuthState,
};

use crate::cli::args::GlobalArgs;
use crate::coerce::resolve_secret;
use crate::error::{Error, Result};
use crate::oauth::browser::open_authorization_url;
use crate::oauth::callback::wait_for_callback;
use crate::oauth::config::{
    parse_oauth_flow, resolve_redirect_uri, OAuthFlow, OAuthOptions,
};
use crate::oauth::store::{clear_server_credentials, FileCredentialStore};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Ready-to-use OAuth session wrapping rmcp `AuthClient`.
pub struct OAuthReady {
    auth_client: AuthClient<reqwest::Client>,
}

impl OAuthReady {
    pub fn auth_client(&self) -> &AuthClient<reqwest::Client> {
        &self.auth_client
    }

    pub fn into_auth_client(self) -> AuthClient<reqwest::Client> {
        self.auth_client
    }

    pub async fn access_token(&self) -> Result<String> {
        self.auth_client
            .get_access_token()
            .await
            .map_err(|e| Error::runtime(format!("OAuth get_access_token: {e}")))
    }
}

/// Whether any OAuth flag requests auth.
pub fn oauth_wanted(args: &GlobalArgs) -> bool {
    args.oauth
        || args.oauth_client_id.is_some()
        || args.oauth_client_secret.is_some()
}

/// Validate CLI oauth flags and resolve secrets into [`OAuthOptions`].
pub fn options_from_args(args: &GlobalArgs) -> Result<Option<OAuthOptions>> {
    if !oauth_wanted(args) {
        return Ok(None);
    }
    if args.mcp_stdio.is_some() {
        return Err(Error::usage(
            "OAuth is not supported with --mcp-stdio (HTTP discovery required)",
        ));
    }
    if args.oauth_client_secret.is_some() && args.oauth_client_id.is_none() {
        return Err(Error::usage(
            "--oauth-client-secret requires --oauth-client-id",
        ));
    }

    let flow = parse_oauth_flow(&args.oauth_flow)?;
    let client_id = args
        .oauth_client_id
        .as_deref()
        .map(resolve_secret)
        .transpose()?;
    let client_secret = args
        .oauth_client_secret
        .as_deref()
        .map(resolve_secret)
        .transpose()?;

    if flow == OAuthFlow::ClientCredentials && (client_id.is_none() || client_secret.is_none()) {
        return Err(Error::usage(
            "--oauth-flow client_credentials requires both --oauth-client-id and --oauth-client-secret",
        ));
    }

    if let Some(uri) = &args.oauth_redirect_uri {
        crate::oauth::config::validate_redirect_uri(uri)?;
    }

    Ok(Some(OAuthOptions {
        client_id,
        client_secret,
        client_name: args.oauth_client_name.clone(),
        scope: args.oauth_scope.clone(),
        redirect_uri: args.oauth_redirect_uri.clone(),
        flow,
    }))
}

/// Discoverable server URL for OAuth metadata (MCP URL, or OpenAPI HTTP base).
pub fn discovery_url_from_args(args: &GlobalArgs) -> Result<String> {
    if let Some(url) = &args.mcp {
        return Ok(url.clone());
    }
    if let Some(gql) = &args.graphql {
        return Ok(gql.clone());
    }
    if let Some(spec) = &args.spec {
        if spec.starts_with("http://") || spec.starts_with("https://") {
            return Ok(spec.clone());
        }
        if let Some(base) = &args.base_url {
            return Ok(base.clone());
        }
        return Err(Error::usage(
            "OAuth with a local --spec requires --base-url (or an HTTP --spec URL)",
        ));
    }
    Err(Error::usage(
        "OAuth requires --mcp, --graphql, or an HTTP --spec/--base-url",
    ))
}

/// Run OAuth (or restore cached tokens) and return an authorized client.
pub async fn authorize(server_url: &str, opts: &OAuthOptions) -> Result<OAuthReady> {
    let store = FileCredentialStore::for_server(server_url);
    let oauth_http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| Error::runtime(format!("oauth http client: {e}")))?;

    let mut state = OAuthState::new(server_url, Some(oauth_http))
        .await
        .map_err(|e| Error::runtime(format!("OAuth init: {e}")))?;

    attach_store(&mut state, store.clone())?;

    if try_restore(&mut state, server_url).await? {
        return finish(state);
    }

    if opts.use_client_credentials() {
        let client_id = opts
            .client_id
            .clone()
            .ok_or_else(|| Error::usage("client credentials requires --oauth-client-id"))?;
        let client_secret = opts
            .client_secret
            .clone()
            .ok_or_else(|| Error::usage("client credentials requires --oauth-client-secret"))?;
        let scopes = scopes_vec(opts);
        let config = ClientCredentialsConfig::ClientSecret {
            client_id,
            client_secret,
            scopes,
            resource: Some(server_url.to_string()),
        };
        state
            .authenticate_client_credentials(config)
            .await
            .map_err(|e| Error::runtime(format!("client credentials auth failed: {e}")))?;
        return finish(state);
    }

    // Authorization code + PKCE
    let sticky = store.load_sticky_redirect_uri();
    let redirect = resolve_redirect_uri(opts.redirect_uri.as_deref(), sticky.as_deref())?;
    if sticky.as_deref() != Some(redirect.as_str()) {
        let _ = store.clear().await;
    }
    store.save_sticky_redirect_uri(&redirect)?;

    let mut request = AuthorizationRequest::new(redirect.clone())
        .with_client_name(opts.client_name.clone());
    if let Some(id) = &opts.client_id {
        request = request.with_preregistered_client(id.clone());
        if let Some(sec) = &opts.client_secret {
            request = request.with_client_secret(sec.clone());
        }
    }
    if let Some(scope) = &opts.scope {
        request = request.with_scopes(scope.split_whitespace().map(|s| s.to_string()));
    }

    let redirect_for_cb = redirect.clone();
    let cb_handle =
        std::thread::spawn(move || wait_for_callback(&redirect_for_cb, CALLBACK_TIMEOUT));

    state
        .start_authorization(request)
        .await
        .map_err(|e| Error::runtime(format!("start authorization failed: {e}")))?;

    let auth_url = state
        .get_authorization_url()
        .await
        .map_err(|e| Error::runtime(format!("get authorization URL: {e}")))?;
    open_authorization_url(&auth_url);

    let cb = cb_handle
        .join()
        .map_err(|_| Error::runtime("callback thread panicked"))??;
    let code = cb.code.as_deref().unwrap_or("");
    let csrf = cb.state.as_deref().unwrap_or("");
    state
        .handle_callback_with_issuer(code, csrf, cb.iss.as_deref())
        .await
        .map_err(|e| Error::runtime(format!("OAuth callback exchange failed: {e}")))?;

    finish(state)
}

fn attach_store(state: &mut OAuthState, store: FileCredentialStore) -> Result<()> {
    match state {
        OAuthState::Unauthorized(manager) => {
            manager.set_credential_store(store);
            Ok(())
        }
        _ => Err(Error::runtime(
            "OAuth state not Unauthorized at store attach",
        )),
    }
}

async fn try_restore(state: &mut OAuthState, server_url: &str) -> Result<bool> {
    match state {
        OAuthState::Unauthorized(manager) => {
            let ok = manager
                .initialize_from_store()
                .await
                .map_err(|e| Error::runtime(format!("restore OAuth credentials: {e}")))?;
            if !ok {
                return Ok(false);
            }
            match manager.get_access_token().await {
                Ok(_) => Ok(true),
                Err(e) => {
                    tracing::debug!("cached OAuth token unusable: {e}");
                    let _ = FileCredentialStore::for_server(server_url).clear().await;
                    Ok(false)
                }
            }
        }
        OAuthState::Authorized(_) => Ok(true),
        _ => Ok(false),
    }
}

fn finish(state: OAuthState) -> Result<OAuthReady> {
    let manager = match state {
        OAuthState::Authorized(m) | OAuthState::Unauthorized(m) => m,
        OAuthState::Session(_) => {
            return Err(Error::runtime("OAuth still in session state after auth"));
        }
        OAuthState::AuthorizedHttpClient(_) => {
            return Err(Error::runtime("unexpected AuthorizedHttpClient state"));
        }
        _ => {
            return Err(Error::runtime("unexpected OAuth state"));
        }
    };
    let auth_client = AuthClient::new(reqwest::Client::default(), manager);
    Ok(OAuthReady { auth_client })
}

fn scopes_vec(opts: &OAuthOptions) -> Vec<String> {
    opts.scope
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn clear_oauth_credentials(server_url: &str) -> Result<()> {
    clear_server_credentials(server_url)
}

/// Convenience: setup from GlobalArgs if oauth wanted.
pub async fn setup_from_args(args: &GlobalArgs) -> Result<Option<OAuthReady>> {
    let Some(opts) = options_from_args(args)? else {
        return Ok(None);
    };
    let url = discovery_url_from_args(args)?;
    Ok(Some(authorize(&url, &opts).await?))
}
