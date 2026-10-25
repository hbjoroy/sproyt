use openidconnect::{
    OAuth2TokenResponse, RefreshToken, RequestTokenError,
    core::{CoreJsonWebKeySet, CoreProviderMetadata, CoreTokenResponse},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub issuer: String,
    pub client_id: String,
    pub client_secret_file: PathBuf,
    pub listen_port: u16,
}
impl Config {
    pub fn read(path: &Path) -> Result<Self, ()> {
        let bytes = std::fs::read(path).map_err(|_| ())?;
        if bytes.len() > 8192 {
            return Err(());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| ())?;
        let url = local_url(&config.issuer)?;
        if url.path() != "/application/o/n01-lab/"
            || config.client_id != "n01-lab"
            || config.listen_port == 0
            || url.port_or_known_default() == Some(config.listen_port)
        {
            return Err(());
        }
        Ok(config)
    }
    pub fn secret(&self) -> Result<String, ()> {
        let bytes = std::fs::read(&self.client_secret_file).map_err(|_| ())?;
        if bytes.len() > 4096 {
            return Err(());
        }
        let value = String::from_utf8(bytes).map_err(|_| ())?;
        let value = value.trim_end_matches(['\r', '\n']);
        if value.len() < 16 || value.chars().any(char::is_control) {
            return Err(());
        }
        Ok(value.to_owned())
    }
    pub async fn metadata(&self, http: &reqwest::Client) -> Result<CoreProviderMetadata, ()> {
        let issuer = local_url(&self.issuer)?;
        let discovery = issuer
            .join(".well-known/openid-configuration")
            .map_err(|_| ())?;
        let mut response = http.get(discovery).send().await.map_err(|_| ())?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(());
            }
            bytes.extend_from_slice(&chunk);
        }
        let metadata: CoreProviderMetadata = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if metadata.issuer().as_str() != self.issuer {
            return Err(());
        }
        self.endpoint(metadata.authorization_endpoint().as_str())?;
        self.endpoint(metadata.token_endpoint().ok_or(())?.as_str())?;
        self.endpoint(metadata.jwks_uri().as_str())?;
        if let Some(endpoint) = metadata.userinfo_endpoint() {
            self.endpoint(endpoint.as_str())?;
        }
        // Discovery's JWKS URL is checked before the first request to it.
        let keys = CoreJsonWebKeySet::fetch_async(metadata.jwks_uri(), http)
            .await
            .map_err(|_| ())?;
        Ok(metadata.set_jwks(keys))
    }
    fn endpoint(&self, value: &str) -> Result<(), ()> {
        let endpoint = local_url(value)?;
        if endpoint.origin() != local_url(&self.issuer)?.origin() {
            return Err(());
        }
        Ok(())
    }
}
fn local_url(value: &str) -> Result<reqwest::Url, ()> {
    let url = reqwest::Url::parse(value).map_err(|_| ())?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none_or(|port| port == 0)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
    {
        return Err(());
    }
    Ok(url)
}

pub struct Upstream {
    refresh: Option<String>,
    access: String,
}
pub type Sessions = tokio::sync::Mutex<HashMap<(String, String), Upstream>>;
pub fn session(token: &CoreTokenResponse) -> Upstream {
    Upstream {
        refresh: token.refresh_token().map(|value| value.secret().clone()),
        access: token.access_token().secret().clone(),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub user_id: String,
    pub transaction_id: String,
    pub action: String,
}
pub async fn check(app: &crate::App, input: Check) -> Result<serde_json::Value, crate::ProbeError> {
    if !app.external
        || !matches!(
            input.action.as_str(),
            "refresh"
                | "drop-refresh-result"
                | "retry-old"
                | "introspect-access"
                | "introspect-refresh"
                | "revoke-refresh"
        )
    {
        return Err(crate::ProbeError);
    }
    let mut sessions = app.upstream.lock().await;
    let current = sessions
        .get_mut(&(input.transaction_id, input.user_id))
        .ok_or(crate::ProbeError)?;
    if matches!(
        input.action.as_str(),
        "introspect-access" | "introspect-refresh" | "revoke-refresh"
    ) {
        let control = app.control.get().ok_or(crate::ProbeError)?;
        return control
            .check(&app.http, current, &input.action)
            .await
            .map_err(|_| crate::ProbeError);
    }
    let Some(refresh) = current.refresh.clone() else {
        return Ok(
            serde_json::json!({"accepted":false,"refresh_rotated":false,"expires_in":null,"requires_reauthentication":true}),
        );
    };
    let client = app.client.get().ok_or(crate::ProbeError)?;
    let token = match client
        .exchange_refresh_token(&RefreshToken::new(refresh.clone()))
        .map_err(|_| crate::ProbeError)?
        .request_async(&app.http)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            let reauthenticate = matches!(&error, RequestTokenError::ServerResponse(response) if response.error() == &openidconnect::core::CoreErrorResponseType::InvalidGrant);
            return Ok(
                serde_json::json!({"accepted":false,"refresh_rotated":false,"expires_in":null,"requires_reauthentication":reauthenticate}),
            );
        }
    };
    let rotated = token
        .refresh_token()
        .is_some_and(|value| value.secret() != &refresh);
    let expires = token.expires_in().map(|value| value.as_secs());
    // Both lost-result and retry-old deliberately discard the returned token.
    // A retry-old reply records the provider observation; it never repairs the grant.
    if input.action == "refresh" {
        let mut replacement = session(&token);
        if replacement.refresh.is_none() {
            replacement.refresh = current.refresh.clone();
        }
        *current = replacement;
    }
    Ok(
        serde_json::json!({"accepted":true,"refresh_rotated":rotated,"expires_in":expires,"requires_reauthentication":false}),
    )
}

// These are fixed Authentik 2026.8.2 endpoints, not URLs supplied by a caller
// or by an unchecked discovery extension. Credentials and token bodies stay private.
pub struct Control {
    introspect: reqwest::Url,
    revoke: reqwest::Url,
    client_id: String,
    secret: String,
}
impl Control {
    pub fn new(config: &Config, secret: &str) -> Result<Self, ()> {
        let issuer = local_url(&config.issuer)?;
        let introspect = issuer.join("/application/o/introspect/").map_err(|_| ())?;
        let revoke = issuer.join("/application/o/revoke/").map_err(|_| ())?;
        config.endpoint(introspect.as_str())?;
        config.endpoint(revoke.as_str())?;
        Ok(Self {
            introspect,
            revoke,
            client_id: config.client_id.clone(),
            secret: secret.to_owned(),
        })
    }
    async fn check(
        &self,
        http: &reqwest::Client,
        grant: &Upstream,
        action: &str,
    ) -> Result<serde_json::Value, ()> {
        let (endpoint, token, hint) = match action {
            "introspect-access" => (&self.introspect, grant.access.as_str(), "access_token"),
            "introspect-refresh" => (
                &self.introspect,
                grant.refresh.as_deref().ok_or(())?,
                "refresh_token",
            ),
            "revoke-refresh" => (
                &self.revoke,
                grant.refresh.as_deref().ok_or(())?,
                "refresh_token",
            ),
            _ => return Err(()),
        };
        let response = http
            .post(endpoint.clone())
            .basic_auth(&self.client_id, Some(&self.secret))
            .form(&[("token", token), ("token_type_hint", hint)])
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|_| ())?;
        let accepted = response.status() == reqwest::StatusCode::OK;
        let bytes = bounded_response(response).await?;
        if action == "revoke-refresh" {
            // HTTP 200 also covers nonexistent tokens. Keep the local token unchanged
            // so later introspection/refresh records actual upstream invalidation.
            return Ok(serde_json::json!({"accepted":accepted}));
        }
        if !accepted {
            return Err(());
        }
        introspection_observation(&bytes)
    }
}
async fn bounded_response(mut response: reqwest::Response) -> Result<Vec<u8>, ()> {
    const MAXIMUM: usize = 16384;
    if response
        .content_length()
        .is_some_and(|length| length > MAXIMUM as u64)
    {
        return Err(());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len() + chunk.len() > MAXIMUM {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn introspection_observation(bytes: &[u8]) -> Result<serde_json::Value, ()> {
    // Never forward introspection's ID-token claims, subject, scope, or client data.
    let response: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    let active = response
        .get("active")
        .and_then(serde_json::Value::as_bool)
        .ok_or(())?;
    Ok(serde_json::json!({"active":active}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn introspection_strips_all_provider_claims_and_requires_boolean_active() {
        let observation = introspection_observation(br#"{"active":true,"sub":"synthetic-subject","token":"synthetic-secret","scope":"openid","exp":123}"#).unwrap();
        assert_eq!(observation, serde_json::json!({"active":true}));
        assert_eq!(
            introspection_observation(br#"{"active":false}"#).unwrap(),
            serde_json::json!({"active":false})
        );
        for bytes in [
            b"{}".as_slice(),
            br#"{"active":"true"}"#,
            b"not-json",
            b"[]",
        ] {
            assert!(introspection_observation(bytes).is_err());
        }
        assert!(serde_json::from_str::<Check>(r#"{"user_id":"alice","transaction_id":"one","action":"revoke-refresh","token":"injected"}"#).is_err());
    }
    #[tokio::test]
    async fn controls_use_fixed_paths_and_reject_oversized_responses() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let routes = axum::Router::new()
            .route(
                "/application/o/introspect/",
                axum::routing::post(|| async {
                    axum::Json(serde_json::json!({"active":true,"sub":"must-not-escape"}))
                }),
            )
            .route(
                "/application/o/revoke/",
                axum::routing::post(|| async { "x".repeat(16385) }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, routes).await.unwrap();
        });
        let config = Config {
            issuer: format!("http://127.0.0.1:{port}/application/o/n01-lab/"),
            client_id: "n01-lab".into(),
            client_secret_file: PathBuf::new(),
            listen_port: 19191,
        };
        let control = Control::new(&config, "synthetic-client-secret").unwrap();
        assert_eq!(
            control.introspect.as_str(),
            format!("http://127.0.0.1:{port}/application/o/introspect/")
        );
        assert_eq!(
            control.revoke.as_str(),
            format!("http://127.0.0.1:{port}/application/o/revoke/")
        );
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let grant = Upstream {
            refresh: Some("synthetic-refresh".into()),
            access: "synthetic-access".into(),
        };
        assert_eq!(
            control
                .check(&http, &grant, "introspect-access")
                .await
                .unwrap(),
            serde_json::json!({"active":true})
        );
        assert!(
            control
                .check(&http, &grant, "revoke-refresh")
                .await
                .is_err()
        );
        assert!(control.check(&http, &grant, "unknown").await.is_err());
        server.abort();
    }
    #[test]
    fn grants_are_bound_to_both_transaction_and_user() {
        let mut sessions = HashMap::new();
        for (transaction, user, marker) in [
            ("login-a", "alice", "first"),
            ("login-b", "alice", "second"),
            ("login-c", "bob", "third"),
        ] {
            sessions.insert(
                (transaction.to_owned(), user.to_owned()),
                Upstream {
                    refresh: Some(marker.into()),
                    access: "synthetic".into(),
                },
            );
        }
        assert_eq!(sessions.len(), 3);
        assert_eq!(
            sessions
                .get(&("login-a".into(), "alice".into()))
                .unwrap()
                .refresh
                .as_deref(),
            Some("first")
        );
        assert_eq!(
            sessions
                .get(&("login-b".into(), "alice".into()))
                .unwrap()
                .refresh
                .as_deref(),
            Some("second")
        );
        for (transaction, user) in [
            ("login-a", "bob"),
            ("login-b", "bob"),
            ("login-c", "alice"),
            ("missing", "alice"),
        ] {
            assert!(!sessions.contains_key(&(transaction.into(), user.into())));
        }
    }
    #[test]
    fn local_origin_has_no_aliases_or_extra_components() {
        assert!(local_url("http://127.0.0.1:19090/application/o/n01-lab/").is_ok());
        for value in [
            "https://127.0.0.1:19090/application/o/n01-lab/",
            "http://localhost:19090/application/o/n01-lab/",
            "http://[::1]:19090/application/o/n01-lab/",
            "http://127.0.0.1:19090/application/o/n01-lab/?x=1",
            "http://127.0.0.1:19090/application/o/n01-lab/#x",
            "http://user@127.0.0.1:19090/application/o/n01-lab/",
            "http://127.0.0.1/application/o/n01-lab/",
            "http://127.0.0.1:0/application/o/n01-lab/",
        ] {
            assert!(local_url(value).is_err());
        }
    }
    #[test]
    fn every_endpoint_must_stay_on_the_configured_origin() {
        let config = Config {
            issuer: "http://127.0.0.1:19090/application/o/n01-lab/".into(),
            client_id: "n01-lab".into(),
            client_secret_file: PathBuf::new(),
            listen_port: 19191,
        };
        assert!(
            config
                .endpoint("http://127.0.0.1:19090/application/o/token/")
                .is_ok()
        );
        for value in [
            "http://127.0.0.1:19191/application/o/token/",
            "http://127.0.0.2:19090/application/o/token/",
            "https://example.com/token",
            "http://127.0.0.1:19090/token?redirect=1",
        ] {
            assert!(config.endpoint(value).is_err());
        }
    }
}
