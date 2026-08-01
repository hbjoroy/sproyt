use std::{
    fmt,
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use std::sync::Mutex;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use openidconnect::{
    AccessToken, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndSessionUrl,
    EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, LogoutRequest, Nonce as OidcNonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, PostLogoutRedirectUrl,
    ProviderMetadataWithLogout, RedirectUrl, RefreshToken, Scope, SubjectIdentifier, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreUserInfoClaims},
    reqwest,
};
use serde::{Deserialize, Serialize};

use crate::{
    config::OidcConfig,
    domain::{DisplayName, PrincipalKind, TextValidationError, User, UserId},
};

const LOGIN_TTL_SECONDS: u64 = 600;
const ACCESS_SESSION_MAX_TTL_SECONDS: u64 = 8 * 60 * 60;
const REFRESH_IDLE_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
const SESSION_REFRESH_LEAD_SECONDS: u64 = 60;
pub const LOGIN_COOKIE: &str = "sproyt_oidc_tx";
pub const SESSION_COOKIE: &str = "sproyt_session";
pub const REFRESH_COOKIE: &str = "sproyt_refresh";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedPrincipal {
    pub user: User,
    pub issuer: String,
    pub subject: String,
}

#[derive(Clone)]
pub enum AuthService {
    Development,
    Oidc(Arc<OidcService>),
}

impl fmt::Debug for AuthService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Development => formatter.write_str("AuthService::Development"),
            Self::Oidc(_) => formatter.write_str("AuthService::Oidc"),
        }
    }
}

impl AuthService {
    pub fn development() -> Self {
        Self::Development
    }

    pub async fn oidc(config: &OidcConfig) -> Result<Self, AuthError> {
        OidcService::discover(config)
            .await
            .map(|service| Self::Oidc(Arc::new(service)))
    }

    pub fn authenticate_development(
        &self,
        requested_name: Option<String>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        if matches!(self, Self::Oidc(_)) {
            return Err(AuthError::Unauthorized);
        }
        let name = requested_name.unwrap_or_else(|| "guest".to_owned());
        principal("urn:sproyt:development", &name, &name)
    }

    pub async fn authenticate_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unauthorized),
            Self::Oidc(service) => service.authenticate_session(cookie_header).await,
        }
    }

    pub async fn authenticate_request(
        &self,
        requested_name: Option<String>,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        match self {
            Self::Development => self.authenticate_development(requested_name),
            Self::Oidc(_) => self.authenticate_session(cookie_header).await,
        }
    }

    #[allow(dead_code)] // Explicit provider revalidation remains available for sensitive future operations.
    pub async fn revalidate_request(
        &self,
        requested_name: Option<String>,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        match self {
            Self::Development => self.authenticate_development(requested_name),
            Self::Oidc(service) => service.revalidate_session(cookie_header).await,
        }
    }

    pub fn login(&self, return_to: Option<String>) -> Result<LoginStart, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unsupported("OIDC is disabled".to_owned())),
            Self::Oidc(service) => service.login(return_to),
        }
    }

    pub async fn callback(
        &self,
        code: String,
        state: String,
        cookie_header: Option<&str>,
    ) -> Result<LoginComplete, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unsupported("OIDC is disabled".to_owned())),
            Self::Oidc(service) => service.callback(code, state, cookie_header).await,
        }
    }

    pub async fn renew_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<SessionRenewal, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unsupported("OIDC is disabled".to_owned())),
            Self::Oidc(service) => service.renew_session(cookie_header).await,
        }
    }

    pub fn session_refresh_after(&self, cookie_header: Option<&str>) -> Result<u64, AuthError> {
        match self {
            Self::Development => Ok(300),
            Self::Oidc(service) => service.session_refresh_after(cookie_header),
        }
    }

    pub fn logout(&self) -> Logout {
        match self {
            Self::Development => Logout {
                redirect_url: "/".to_owned(),
                clear_cookie: clear_cookie(SESSION_COOKIE, "/"),
                clear_refresh_cookie: clear_cookie(REFRESH_COOKIE, "/"),
                clear_legacy_refresh_cookie: clear_cookie(REFRESH_COOKIE, "/auth/refresh"),
            },
            Self::Oidc(service) => Logout {
                redirect_url: service.logout_redirect_url(),
                clear_cookie: clear_cookie(SESSION_COOKIE, "/"),
                clear_refresh_cookie: clear_cookie(REFRESH_COOKIE, "/"),
                clear_legacy_refresh_cookie: clear_cookie(REFRESH_COOKIE, "/auth/refresh"),
            },
        }
    }
}

pub struct LoginStart {
    pub authorization_url: String,
    pub set_cookie: String,
}

pub struct LoginComplete {
    pub principal: AuthenticatedPrincipal,
    pub set_cookie: String,
    pub set_refresh_cookie: Option<String>,
    pub clear_transaction_cookie: String,
    pub return_to: String,
}

pub struct SessionRenewal {
    pub set_cookie: String,
    pub set_refresh_cookie: String,
    pub refresh_after_seconds: u64,
}

fn refresh_after_seconds(max_age: u64) -> u64 {
    if max_age > SESSION_REFRESH_LEAD_SECONDS {
        max_age - SESSION_REFRESH_LEAD_SECONDS
    } else {
        (max_age / 2).max(1)
    }
}

fn access_session_ttl(expires_in: Option<Duration>) -> u64 {
    expires_in
        .unwrap_or(Duration::from_secs(300))
        .min(Duration::from_secs(ACCESS_SESSION_MAX_TTL_SECONDS))
        .as_secs()
        .max(1)
}

pub struct Logout {
    pub redirect_url: String,
    pub clear_cookie: String,
    pub clear_refresh_cookie: String,
    pub clear_legacy_refresh_cookie: String,
}

pub struct OidcService {
    client: RwLock<DiscoveredCoreClient>,
    end_session_endpoint: RwLock<Option<EndSessionUrl>>,
    http_client: reqwest::Client,
    codec: CookieCodec,
    config: OidcConfig,
    issuer: String,
    post_logout_redirect_url: String,
}

type DiscoveredCoreClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

struct DiscoveredOidc {
    client: DiscoveredCoreClient,
    end_session_endpoint: Option<EndSessionUrl>,
}

async fn discover_client(
    config: &OidcConfig,
    http_client: &reqwest::Client,
) -> Result<DiscoveredOidc, AuthError> {
    let issuer = IssuerUrl::new(config.issuer().to_owned()).map_err(AuthError::external)?;
    let metadata = ProviderMetadataWithLogout::discover_async(issuer, http_client)
        .await
        .map_err(AuthError::external)?;
    let end_session_endpoint = metadata.additional_metadata().end_session_endpoint.clone();
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(config.client_id().to_owned()),
        Some(ClientSecret::new(config.client_secret().to_owned())),
    )
    .set_redirect_uri(
        RedirectUrl::new(config.redirect_url().to_owned()).map_err(AuthError::external)?,
    );
    Ok(DiscoveredOidc {
        client,
        end_session_endpoint,
    })
}

impl OidcService {
    async fn discover(config: &OidcConfig) -> Result<Self, AuthError> {
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(AuthError::external)?;
        let discovered = discover_client(config, &http_client).await?;
        Ok(Self {
            client: RwLock::new(discovered.client),
            end_session_endpoint: RwLock::new(discovered.end_session_endpoint),
            http_client,
            codec: CookieCodec::new(config.session_key(), config.session_previous_keys())?,
            config: config.clone(),
            issuer: config.issuer().to_owned(),
            post_logout_redirect_url: config.post_logout_redirect_url().to_owned(),
        })
    }

    async fn refreshed_client(&self) -> Result<DiscoveredOidc, AuthError> {
        discover_client(&self.config, &self.http_client).await
    }

    fn current_client(&self) -> Result<DiscoveredCoreClient, AuthError> {
        self.client
            .read()
            .map(|client| client.clone())
            .map_err(|_| AuthError::External("OIDC client lock poisoned".to_owned()))
    }

    fn replace_client(&self, discovered: DiscoveredOidc) -> Result<(), AuthError> {
        *self
            .client
            .write()
            .map_err(|_| AuthError::External("OIDC client lock poisoned".to_owned()))? =
            discovered.client;
        *self
            .end_session_endpoint
            .write()
            .map_err(|_| AuthError::External("OIDC logout lock poisoned".to_owned()))? =
            discovered.end_session_endpoint;
        Ok(())
    }

    fn logout_redirect_url(&self) -> String {
        let endpoint = self
            .end_session_endpoint
            .read()
            .ok()
            .and_then(|endpoint| endpoint.clone());
        let redirect = PostLogoutRedirectUrl::new(self.post_logout_redirect_url.clone());
        match (endpoint, redirect) {
            (Some(endpoint), Ok(redirect)) => LogoutRequest::from(endpoint)
                .set_client_id(ClientId::new(self.config.client_id().to_owned()))
                .set_post_logout_redirect_uri(redirect)
                .http_get_url()
                .to_string(),
            _ => self.post_logout_redirect_url.clone(),
        }
    }

    fn login(&self, return_to: Option<String>) -> Result<LoginStart, AuthError> {
        let client = self.current_client()?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                OidcNonce::new_random,
            )
            .add_scope(Scope::new("profile".to_owned()))
            .add_scope(Scope::new("email".to_owned()))
            .add_scope(Scope::new("offline_access".to_owned()))
            .set_pkce_challenge(challenge)
            .url();
        let transaction = LoginTransaction {
            state: state.secret().to_owned(),
            nonce: nonce.secret().to_owned(),
            pkce_verifier: verifier.secret().to_owned(),
            expires_at: now_seconds() + LOGIN_TTL_SECONDS,
            return_to,
        };
        let value = self.codec.seal(&transaction)?;
        Ok(LoginStart {
            authorization_url: url.to_string(),
            set_cookie: secure_cookie(LOGIN_COOKIE, &value, "/auth/callback", LOGIN_TTL_SECONDS),
        })
    }

    async fn callback(
        &self,
        code: String,
        state: String,
        cookie_header: Option<&str>,
    ) -> Result<LoginComplete, AuthError> {
        let transaction_cookie =
            read_cookie(cookie_header, LOGIN_COOKIE).ok_or(AuthError::Unauthorized)?;
        let transaction: LoginTransaction = self.codec.open(transaction_cookie)?;
        if transaction.expires_at < now_seconds() || transaction.state != state {
            return Err(AuthError::Unauthorized);
        }
        let client = self.current_client()?;
        let token = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(AuthError::external)?
            .set_pkce_verifier(PkceCodeVerifier::new(transaction.pkce_verifier))
            .request_async(&self.http_client)
            .await
            .map_err(AuthError::external)?;
        let id_token = token.id_token().ok_or(AuthError::Unauthorized)?;
        let nonce = OidcNonce::new(transaction.nonce);
        let initial_verifier = client.id_token_verifier();
        let claims = match id_token.claims(&initial_verifier, &nonce) {
            Ok(claims) => claims,
            Err(_) => {
                let refreshed = self.refreshed_client().await?;
                let refreshed_verifier = refreshed.client.id_token_verifier();
                let claims = id_token
                    .claims(&refreshed_verifier, &nonce)
                    .map_err(AuthError::external)?;
                self.replace_client(DiscoveredOidc {
                    client: refreshed.client.clone(),
                    end_session_endpoint: refreshed.end_session_endpoint.clone(),
                })?;
                claims
            }
        };
        let subject = claims.subject().as_str().to_owned();
        let display_name = claims
            .name()
            .and_then(|name| name.get(None))
            .map(|name| name.as_str())
            .or_else(|| {
                claims
                    .preferred_username()
                    .map(|username| username.as_str())
            })
            .unwrap_or(&subject);
        let principal = principal(&self.issuer, &subject, display_name)?;
        let now = now_seconds();
        // The ID token expiry validates the authentication assertion. The
        // access token has its own lifetime, supplied by the token endpoint,
        // and that is the lifetime that must bound the application session.
        let access_max_age = access_session_ttl(token.expires_in());
        let expires_at = now.saturating_add(access_max_age);
        let refresh_expires_at = now.saturating_add(REFRESH_IDLE_TTL_SECONDS);
        let refresh_cookie = token
            .refresh_token()
            .map(|refresh_token| {
                self.codec
                    .seal(&RefreshClaims {
                        issuer: self.issuer.clone(),
                        subject: subject.clone(),
                        display_name: principal.user.display_name.to_string(),
                        refresh_token: refresh_token.secret().to_owned(),
                        expires_at: refresh_expires_at,
                    })
                    .map(|value| {
                        secure_cookie(REFRESH_COOKIE, &value, "/", REFRESH_IDLE_TTL_SECONDS)
                    })
            })
            .transpose()?;
        let session = SessionClaims {
            issuer: self.issuer.clone(),
            subject,
            display_name: principal.user.display_name.to_string(),
            access_token: Some(token.access_token().secret().to_owned()),
            refresh_token: None,
            expires_at,
            refresh_expires_at: Some(refresh_expires_at),
        };
        let value = self.codec.seal(&session)?;
        Ok(LoginComplete {
            principal,
            // Keep the encrypted refresh credential beyond the shorter access
            // token lifetime so a suspended mobile browser can renew on wake.
            set_cookie: secure_cookie(SESSION_COOKIE, &value, "/", expires_at - now),
            set_refresh_cookie: refresh_cookie,
            clear_transaction_cookie: clear_cookie(LOGIN_COOKIE, "/auth/callback"),
            return_to: transaction.return_to.unwrap_or_else(|| "/".to_owned()),
        })
    }

    async fn authenticate_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let value = read_cookie(cookie_header, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let claims: SessionClaims = self.codec.open(value)?;
        validate_session_claims(&claims, &self.issuer)?;
        principal(&claims.issuer, &claims.subject, &claims.display_name)
    }

    fn session_refresh_after(&self, cookie_header: Option<&str>) -> Result<u64, AuthError> {
        let value = read_cookie(cookie_header, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let claims: SessionClaims = self.codec.open(value)?;
        validate_session_claims(&claims, &self.issuer)?;
        Ok(refresh_after_seconds(
            claims.expires_at.saturating_sub(now_seconds()),
        ))
    }

    #[allow(dead_code)] // Kept separate from routine WebSocket expiry checks to avoid provider coupling.
    async fn revalidate_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let value = read_cookie(cookie_header, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let claims: SessionClaims = self.codec.open(value)?;
        validate_session_claims(&claims, &self.issuer)?;
        let Some(access_token) = claims.access_token else {
            // Expand/contract compatibility: sessions issued by the previous
            // release remain usable only until their already bounded expiry.
            return principal(&claims.issuer, &claims.subject, &claims.display_name);
        };
        let client = self.current_client()?;
        let user_info: CoreUserInfoClaims = client
            .user_info(
                AccessToken::new(access_token),
                Some(SubjectIdentifier::new(claims.subject.clone())),
            )
            .map_err(AuthError::external)?
            .request_async(&self.http_client)
            .await
            .map_err(|_| AuthError::Unauthorized)?;
        let display_name = user_info
            .name()
            .and_then(|name| name.get(None))
            .map(|name| name.as_str())
            .or_else(|| {
                user_info
                    .preferred_username()
                    .map(|username| username.as_str())
            })
            .unwrap_or(&claims.display_name);
        principal(&claims.issuer, &claims.subject, display_name)
    }

    async fn renew_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<SessionRenewal, AuthError> {
        let (issuer, subject, display_name, refresh_token) =
            if let Some(value) = read_cookie(cookie_header, REFRESH_COOKIE) {
                let claims: RefreshClaims = self.codec.open(value)?;
                validate_refresh_claims(&claims.issuer, claims.expires_at, &self.issuer)?;
                (
                    claims.issuer,
                    claims.subject,
                    claims.display_name,
                    claims.refresh_token,
                )
            } else {
                let value =
                    read_cookie(cookie_header, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
                let claims: SessionClaims = self.codec.open(value)?;
                let refresh_expires_at = claims.refresh_expires_at.unwrap_or(claims.expires_at);
                validate_refresh_claims(&claims.issuer, refresh_expires_at, &self.issuer)?;
                let refresh_token = claims.refresh_token.ok_or_else(|| {
                    AuthError::Unsupported("session has no refresh token".to_owned())
                })?;
                (
                    claims.issuer,
                    claims.subject,
                    claims.display_name,
                    refresh_token,
                )
            };
        // A provider may legitimately omit a refresh token even when
        // `offline_access` was requested. The sealed session remains valid
        // until its bounded expiry, so this is not an authentication failure.
        let client = self.current_client()?;
        let token = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
            .map_err(AuthError::external)?
            .request_async(&self.http_client)
            .await
            .map_err(|_| AuthError::Unauthorized)?;
        let access_token = token.access_token().secret().to_owned();
        // Refresh-token exchange already authenticates the grant. Do not make
        // renewal depend on a second userinfo round-trip: profile freshness is
        // handled by the explicit revalidation path, while background renewal
        // should remain reliable during a partial provider outage.
        // Successful use starts a fresh inactivity window. The provider can
        // still impose a shorter absolute lifetime on its refresh token.
        let refresh_expires_at = now_seconds().saturating_add(REFRESH_IDLE_TTL_SECONDS);
        let refresh_max_age = REFRESH_IDLE_TTL_SECONDS;
        let access_max_age = access_session_ttl(token.expires_in()).min(refresh_max_age);
        let renewed = SessionClaims {
            issuer: issuer.clone(),
            subject: subject.clone(),
            display_name,
            access_token: Some(access_token),
            refresh_token: None,
            expires_at: now_seconds().saturating_add(access_max_age),
            refresh_expires_at: Some(refresh_expires_at),
        };
        let value = self.codec.seal(&renewed)?;
        let refresh_value = self.codec.seal(&RefreshClaims {
            issuer,
            subject,
            display_name: renewed.display_name.clone(),
            refresh_token: token
                .refresh_token()
                .map_or(refresh_token, |token| token.secret().to_owned()),
            expires_at: refresh_expires_at,
        })?;
        Ok(SessionRenewal {
            set_cookie: secure_cookie(SESSION_COOKIE, &value, "/", access_max_age),
            set_refresh_cookie: secure_cookie(REFRESH_COOKIE, &refresh_value, "/", refresh_max_age),
            refresh_after_seconds: refresh_after_seconds(access_max_age),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct LoginTransaction {
    state: String,
    nonce: String,
    pkce_verifier: String,
    expires_at: u64,
    #[serde(default)]
    return_to: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SessionClaims {
    issuer: String,
    subject: String,
    display_name: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_at: u64,
    #[serde(default)]
    refresh_expires_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct RefreshClaims {
    issuer: String,
    subject: String,
    display_name: String,
    refresh_token: String,
    expires_at: u64,
}

fn validate_session_claims(claims: &SessionClaims, issuer: &str) -> Result<(), AuthError> {
    if claims.expires_at <= now_seconds() || claims.issuer != issuer {
        return Err(AuthError::Unauthorized);
    }
    Ok(())
}

fn validate_refresh_claims(
    claims_issuer: &str,
    refresh_expires_at: u64,
    issuer: &str,
) -> Result<(), AuthError> {
    if refresh_expires_at <= now_seconds() || claims_issuer != issuer {
        return Err(AuthError::Unauthorized);
    }
    Ok(())
}

struct CookieCodec {
    primary: Aes256Gcm,
    readers: Vec<Aes256Gcm>,
}

impl CookieCodec {
    fn new(encoded_key: &str, previous_keys: &[String]) -> Result<Self, AuthError> {
        let primary = decode_cookie_key(encoded_key)?;
        let readers = std::iter::once(encoded_key)
            .chain(previous_keys.iter().map(String::as_str))
            .map(decode_cookie_key)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { primary, readers })
    }

    fn seal<T: Serialize>(&self, value: &T) -> Result<String, AuthError> {
        let plaintext = serde_json::to_vec(value).map_err(AuthError::external)?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let encrypted = self
            .primary
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|_| AuthError::External("session encryption failed".to_owned()))?;
        let mut output = nonce.to_vec();
        output.extend_from_slice(&encrypted);
        Ok(URL_SAFE_NO_PAD.encode(output))
    }

    fn open<T: for<'de> Deserialize<'de>>(&self, value: &str) -> Result<T, AuthError> {
        let data = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| AuthError::Unauthorized)?;
        if data.len() <= 12 {
            return Err(AuthError::Unauthorized);
        }
        let (nonce, encrypted) = data.split_at(12);
        self.readers
            .iter()
            .find_map(|cipher| cipher.decrypt(Nonce::from_slice(nonce), encrypted).ok())
            .and_then(|plaintext| serde_json::from_slice(&plaintext).ok())
            .ok_or(AuthError::Unauthorized)
    }
}

fn decode_cookie_key(encoded_key: &str) -> Result<Aes256Gcm, AuthError> {
    let key = URL_SAFE_NO_PAD
        .decode(encoded_key)
        .map_err(AuthError::external)?;
    if key.len() != 32 {
        return Err(AuthError::InvalidSessionKey);
    }
    Aes256Gcm::new_from_slice(&key).map_err(|_| AuthError::InvalidSessionKey)
}

fn principal(
    issuer: &str,
    subject: &str,
    display_name: &str,
) -> Result<AuthenticatedPrincipal, AuthError> {
    Ok(AuthenticatedPrincipal {
        user: User {
            id: UserId::named(format!("{issuer}:{subject}")),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new(display_name)?,
            external_provider: Some(issuer.to_owned()),
            external_subject: Some(subject.to_owned()),
            created_at: chrono::Utc::now(),
        },
        issuer: issuer.to_owned(),
        subject: subject.to_owned(),
    })
}

fn secure_cookie(name: &str, value: &str, path: &str, max_age: u64) -> String {
    format!("{name}={value}; Path={path}; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age}")
}

fn clear_cookie(name: &str, path: &str) -> String {
    format!("{name}=; Path={path}; HttpOnly; Secure; SameSite=Lax; Max-Age=0")
}

fn read_cookie<'a>(header: Option<&'a str>, name: &str) -> Option<&'a str> {
    header?.split(';').map(str::trim).find_map(|cookie| {
        let (key, value) = cookie.split_once('=')?;
        (key == name).then_some(value)
    })
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthError {
    InvalidIdentity(TextValidationError),
    InvalidSessionKey,
    Unauthorized,
    Unsupported(String),
    External(String),
}

impl AuthError {
    fn external(error: impl fmt::Display) -> Self {
        Self::External(error.to_string())
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::InvalidIdentity(error) => error.to_string(),
            Self::InvalidSessionKey => "authentication configuration error".to_owned(),
            Self::Unauthorized => "authentication required".to_owned(),
            Self::Unsupported(_) => "authentication operation unsupported".to_owned(),
            Self::External(_) => "authentication service unavailable".to_owned(),
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(error) => error.fmt(formatter),
            Self::InvalidSessionKey => {
                formatter.write_str("SPROYT_SESSION_KEY must be URL-safe base64 for 32 bytes")
            }
            Self::Unauthorized => formatter.write_str("authentication required"),
            Self::Unsupported(message) | Self::External(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<TextValidationError> for AuthError {
    fn from(value: TextValidationError) -> Self {
        Self::InvalidIdentity(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Form, Json, Router,
        extract::State,
        http::HeaderMap,
        response::IntoResponse,
        routing::{get, post},
    };
    use rsa::{
        RsaPrivateKey, RsaPublicKey,
        pkcs1v15::SigningKey,
        rand_core::OsRng,
        signature::{SignatureEncoding, Signer},
        traits::PublicKeyParts,
    };
    use sha2::Sha256;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Clone)]
    struct TestProvider {
        issuer: String,
        client_id: String,
        client_secret: String,
        nonce: Arc<Mutex<String>>,
        signing_key: Arc<Mutex<(String, RsaPrivateKey)>>,
        active: Arc<AtomicBool>,
    }

    async fn provider_metadata(State(provider): State<TestProvider>) -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "issuer":provider.issuer,
            "authorization_endpoint":format!("{}/authorize",provider.issuer),
            "token_endpoint":format!("{}/token",provider.issuer),
            "userinfo_endpoint":format!("{}/userinfo",provider.issuer),
            "end_session_endpoint":format!("{}/logout",provider.issuer),
            "jwks_uri":format!("{}/jwks",provider.issuer),
            "response_types_supported":["code"],
            "subject_types_supported":["public"],
            "id_token_signing_alg_values_supported":["RS256"],
            "token_endpoint_auth_methods_supported":["client_secret_basic"],
            "scopes_supported":["openid","profile","email","offline_access"],
            "claims_supported":["iss","sub","aud","exp","iat","nonce","name","preferred_username"]
        }))
    }

    async fn provider_jwks(State(provider): State<TestProvider>) -> Json<serde_json::Value> {
        let signing_key = provider.signing_key.lock().unwrap();
        let public_key = RsaPublicKey::from(&signing_key.1);
        Json(serde_json::json!({"keys":[{
            "kty":"RSA",
            "use":"sig",
            "alg":"RS256",
            "kid":signing_key.0,
            "n":URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be()),
            "e":URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be())
        }]}))
    }

    fn signed_id_token(provider: &TestProvider) -> String {
        let signing_key = provider.signing_key.lock().unwrap();
        let header = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "alg":"RS256",
                "typ":"JWT",
                "kid":signing_key.0
            }))
            .unwrap(),
        );
        let now = now_seconds();
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "iss":provider.issuer,
                "sub":"authentik-user-1",
                "aud":provider.client_id,
                "exp":now+300,
                "iat":now,
                "nonce":provider.nonce.lock().unwrap().clone(),
                "name":"Authentik Test User",
                "preferred_username":"ignored-fallback"
            }))
            .unwrap(),
        );
        let signing_input = format!("{header}.{claims}");
        let signature =
            SigningKey::<Sha256>::new(signing_key.1.clone()).sign(signing_input.as_bytes());
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    fn generate_signing_key(kid: &str) -> (String, RsaPrivateKey) {
        (
            kid.to_owned(),
            RsaPrivateKey::new(&mut OsRng, 2048).unwrap(),
        )
    }

    async fn provider_token(
        State(provider): State<TestProvider>,
        headers: HeaderMap,
        Form(form): Form<std::collections::HashMap<String, String>>,
    ) -> Json<serde_json::Value> {
        assert!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("Basic ")),
            "OIDC client did not authenticate at the token endpoint"
        );
        let refresh_token = match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                assert_eq!(form.get("code").map(String::as_str), Some("valid-code"));
                assert!(
                    form.get("code_verifier")
                        .is_some_and(|value| !value.is_empty())
                );
                "refresh-a"
            }
            Some("refresh_token") => {
                assert_eq!(
                    form.get("refresh_token").map(String::as_str),
                    Some("refresh-a")
                );
                "refresh-b"
            }
            grant => panic!("unexpected grant type {grant:?}"),
        };
        Json(serde_json::json!({
            "access_token":"test-access-token",
            "token_type":"Bearer",
            "expires_in":300,
            "id_token":signed_id_token(&provider),
            "refresh_token":refresh_token
        }))
    }

    async fn provider_user_info(
        State(provider): State<TestProvider>,
        headers: HeaderMap,
    ) -> axum::response::Response {
        let valid_token = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some("Bearer test-access-token");
        if !provider.active.load(Ordering::SeqCst) || !valid_token {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
        Json(serde_json::json!({
            "sub":"authentik-user-1",
            "name":"Current Authentik User"
        }))
        .into_response()
    }

    async fn test_oidc_provider() -> (OidcConfig, TestProvider, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let provider = TestProvider {
            issuer: issuer.clone(),
            client_id: "sproyt-test".to_owned(),
            client_secret: "test-client-secret-with-enough-entropy".to_owned(),
            nonce: Arc::new(Mutex::new(String::new())),
            signing_key: Arc::new(Mutex::new(generate_signing_key("key-a"))),
            active: Arc::new(AtomicBool::new(true)),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(provider_metadata))
            .route("/jwks", get(provider_jwks))
            .route("/token", post(provider_token))
            .route("/userinfo", get(provider_user_info))
            .with_state(provider.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = OidcConfig::for_test(
            issuer,
            provider.client_id.clone(),
            provider.client_secret.clone(),
            "https://chat.example/auth/callback".to_owned(),
            "https://chat.example/".to_owned(),
            URL_SAFE_NO_PAD.encode([11_u8; 32]),
            Vec::new(),
        );
        (config, provider, server)
    }

    fn authorization_parameter(url: &str, name: &str) -> String {
        url.split_once('?')
            .unwrap()
            .1
            .split('&')
            .find_map(|parameter| {
                let (key, value) = parameter.split_once('=')?;
                (key == name).then(|| value.to_owned())
            })
            .unwrap()
    }

    #[test]
    fn development_identities_are_deterministic() {
        let auth = AuthService::development();
        let first = auth
            .authenticate_development(Some("alice".to_owned()))
            .unwrap();
        let second = auth
            .authenticate_development(Some("alice".to_owned()))
            .unwrap();
        assert_eq!(first.user.id, second.user.id);
        assert_eq!(first.issuer, "urn:sproyt:development");
    }

    #[test]
    fn encrypted_cookie_round_trips_and_rejects_tampering() {
        let key = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let codec = CookieCodec::new(&key, &[]).unwrap();
        let value = SessionClaims {
            issuer: "issuer".to_owned(),
            subject: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            access_token: Some("test-token".to_owned()),
            refresh_token: None,
            expires_at: now_seconds() + 60,
            refresh_expires_at: None,
        };
        let sealed = codec.seal(&value).unwrap();
        let opened: SessionClaims = codec.open(&sealed).unwrap();
        assert_eq!(opened.subject, "alice");
        let mut tampered = sealed.into_bytes();
        let last = tampered.len() - 1;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).unwrap();
        assert!(codec.open::<SessionClaims>(&tampered).is_err());
    }

    #[test]
    fn session_cookie_accepts_previous_release_shape_during_rollout() {
        #[derive(Serialize)]
        struct PreviousSessionClaims {
            issuer: String,
            subject: String,
            display_name: String,
            expires_at: u64,
        }

        let key = URL_SAFE_NO_PAD.encode([8_u8; 32]);
        let codec = CookieCodec::new(&key, &[]).unwrap();
        let sealed = codec
            .seal(&PreviousSessionClaims {
                issuer: "issuer".to_owned(),
                subject: "alice".to_owned(),
                display_name: "Alice".to_owned(),
                expires_at: now_seconds() + 60,
            })
            .unwrap();
        let opened: SessionClaims = codec.open(&sealed).unwrap();

        assert_eq!(opened.subject, "alice");
        assert!(opened.access_token.is_none());
    }

    #[test]
    fn rotated_cookie_key_reads_old_sessions_but_writes_only_with_primary() {
        let old_key = URL_SAFE_NO_PAD.encode([3_u8; 32]);
        let new_key = URL_SAFE_NO_PAD.encode([9_u8; 32]);
        let old_codec = CookieCodec::new(&old_key, &[]).unwrap();
        let rotated = CookieCodec::new(&new_key, std::slice::from_ref(&old_key)).unwrap();
        let claims = SessionClaims {
            issuer: "issuer".to_owned(),
            subject: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            access_token: Some("test-token".to_owned()),
            refresh_token: None,
            expires_at: now_seconds() + 60,
            refresh_expires_at: None,
        };

        let old_session = old_codec.seal(&claims).unwrap();
        assert_eq!(
            rotated.open::<SessionClaims>(&old_session).unwrap().subject,
            "alice"
        );

        let new_session = rotated.seal(&claims).unwrap();
        assert!(old_codec.open::<SessionClaims>(&new_session).is_err());
        assert_eq!(
            rotated.open::<SessionClaims>(&new_session).unwrap().subject,
            "alice"
        );
    }

    #[test]
    fn expired_session_is_rejected() {
        let claims = SessionClaims {
            issuer: "https://issuer.example".to_owned(),
            subject: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            access_token: Some("test-token".to_owned()),
            refresh_token: None,
            expires_at: now_seconds().saturating_sub(1),
            refresh_expires_at: None,
        };

        assert!(matches!(
            validate_session_claims(&claims, "https://issuer.example"),
            Err(AuthError::Unauthorized)
        ));
    }

    #[test]
    fn access_session_uses_access_token_lifetime_with_a_bounded_fallback() {
        assert_eq!(access_session_ttl(Some(Duration::from_secs(3_600))), 3_600);
        assert_eq!(access_session_ttl(None), 300);
        assert_eq!(
            access_session_ttl(Some(Duration::from_secs(24 * 60 * 60))),
            ACCESS_SESSION_MAX_TTL_SECONDS
        );
        assert_eq!(refresh_after_seconds(3_600), 3_540);
    }

    #[tokio::test]
    async fn oidc_discovery_callback_state_nonce_session_and_logout_contract() {
        let (config, provider, server) = test_oidc_provider().await;
        let auth = AuthService::oidc(&config).await.unwrap();

        let AuthService::Oidc(service) = &auth else {
            panic!("expected OIDC service")
        };
        let session_without_refresh = service
            .codec
            .seal(&SessionClaims {
                issuer: provider.issuer.clone(),
                subject: "authentik-user-without-refresh".to_owned(),
                display_name: "Authentik User".to_owned(),
                access_token: Some("valid-access-token".to_owned()),
                refresh_token: None,
                expires_at: now_seconds() + 60,
                refresh_expires_at: None,
            })
            .unwrap();
        assert!(matches!(
            auth.renew_session(Some(&format!("{SESSION_COOKIE}={session_without_refresh}")))
                .await,
            Err(AuthError::Unsupported(_))
        ));
        let expired_session = service
            .codec
            .seal(&SessionClaims {
                issuer: provider.issuer.clone(),
                subject: "authentik-user-1".to_owned(),
                display_name: "Expired Authentik User".to_owned(),
                access_token: Some("expired-access-token".to_owned()),
                refresh_token: Some("refresh-a".to_owned()),
                expires_at: now_seconds().saturating_sub(1),
                refresh_expires_at: None,
            })
            .unwrap();
        let expired_cookie = format!("{SESSION_COOKIE}={expired_session}");
        assert!(matches!(
            auth.renew_session(Some(&expired_cookie)).await,
            Err(AuthError::Unauthorized)
        ));
        let suspended_session = service
            .codec
            .seal(&SessionClaims {
                issuer: provider.issuer.clone(),
                subject: "authentik-user-1".to_owned(),
                display_name: "Suspended Authentik User".to_owned(),
                access_token: Some("expired-access-token".to_owned()),
                refresh_token: Some("refresh-a".to_owned()),
                expires_at: now_seconds().saturating_sub(1),
                refresh_expires_at: Some(now_seconds() + REFRESH_IDLE_TTL_SECONDS),
            })
            .unwrap();
        let resumed = auth
            .renew_session(Some(&format!("{SESSION_COOKIE}={suspended_session}")))
            .await
            .unwrap();
        assert_eq!(resumed.refresh_after_seconds, 240);

        let login = auth.login(Some("/?invite=safe-token".to_owned())).unwrap();
        assert!(
            login
                .authorization_url
                .starts_with(&format!("{}/authorize?", provider.issuer))
        );
        assert!(
            login
                .authorization_url
                .contains("code_challenge_method=S256")
        );
        assert!(login.authorization_url.contains("offline_access"));
        assert!(login.set_cookie.contains("HttpOnly; Secure; SameSite=Lax"));
        let state = authorization_parameter(&login.authorization_url, "state");
        let nonce = authorization_parameter(&login.authorization_url, "nonce");
        *provider.nonce.lock().unwrap() = nonce;
        assert!(matches!(
            auth.callback(
                "valid-code".to_owned(),
                "wrong-state".to_owned(),
                Some(&login.set_cookie),
            )
            .await,
            Err(AuthError::Unauthorized)
        ));

        *provider.signing_key.lock().unwrap() = generate_signing_key("key-b");

        let complete = auth
            .callback("valid-code".to_owned(), state, Some(&login.set_cookie))
            .await
            .unwrap();
        assert_eq!(complete.return_to, "/?invite=safe-token");
        assert_eq!(complete.principal.subject, "authentik-user-1");
        assert_eq!(complete.principal.issuer, provider.issuer);
        assert_eq!(
            complete.principal.user.display_name.to_string(),
            "Authentik Test User"
        );
        assert!(complete.clear_transaction_cookie.contains("Max-Age=0"));
        let max_age = complete
            .set_cookie
            .split(';')
            .map(str::trim)
            .find_map(|attribute| attribute.strip_prefix("Max-Age="))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!((1..=300).contains(&max_age));
        let refresh_cookie = complete.set_refresh_cookie.as_ref().unwrap();
        assert!(refresh_cookie.contains("sproyt_refresh="));
        assert!(refresh_cookie.contains("Path=/"));
        assert!(refresh_cookie.contains(&format!("Max-Age={REFRESH_IDLE_TTL_SECONDS}")));
        let restored = auth
            .authenticate_session(Some(&complete.set_cookie))
            .await
            .unwrap();
        assert_eq!(restored.user.id, complete.principal.user.id);
        assert_eq!(
            restored.user.display_name.to_string(),
            "Authentik Test User"
        );
        let revalidated = auth
            .revalidate_request(None, Some(&complete.set_cookie))
            .await
            .unwrap();
        assert_eq!(
            revalidated.user.display_name.to_string(),
            "Current Authentik User"
        );
        let cookie_header = format!("{}; {}", complete.set_cookie, refresh_cookie);
        let renewal = auth.renew_session(Some(&cookie_header)).await.unwrap();
        assert_eq!(renewal.refresh_after_seconds, 240);
        let renewed_cookie = renewal.set_cookie;
        let renewed: SessionClaims = service
            .codec
            .open(read_cookie(Some(&renewed_cookie), SESSION_COOKIE).unwrap())
            .unwrap();
        assert!(renewed.refresh_token.is_none());
        let renewed_refresh: RefreshClaims = service
            .codec
            .open(read_cookie(Some(&renewal.set_refresh_cookie), REFRESH_COOKIE).unwrap())
            .unwrap();
        assert_eq!(renewed_refresh.refresh_token, "refresh-b");
        let remaining_idle_lifetime = renewed_refresh.expires_at.saturating_sub(now_seconds());
        assert!(
            (REFRESH_IDLE_TTL_SECONDS - 2..=REFRESH_IDLE_TTL_SECONDS)
                .contains(&remaining_idle_lifetime)
        );
        assert!(
            renewal
                .set_refresh_cookie
                .contains(&format!("Max-Age={REFRESH_IDLE_TTL_SECONDS}"))
        );
        assert!(
            auth.authenticate_session(Some(&renewed_cookie))
                .await
                .is_ok()
        );
        provider.active.store(false, Ordering::SeqCst);
        assert!(
            auth.authenticate_session(Some(&renewed_cookie))
                .await
                .is_ok()
        );
        assert!(matches!(
            auth.revalidate_request(None, Some(&renewed_cookie)).await,
            Err(AuthError::Unauthorized)
        ));

        let nonce_login = auth.login(None).unwrap();
        let nonce_state = authorization_parameter(&nonce_login.authorization_url, "state");
        *provider.nonce.lock().unwrap() = "wrong-nonce".to_owned();
        assert!(
            auth.callback(
                "valid-code".to_owned(),
                nonce_state,
                Some(&nonce_login.set_cookie),
            )
            .await
            .is_err()
        );

        let logout = auth.logout();
        let logout_url = reqwest::Url::parse(&logout.redirect_url).unwrap();
        assert_eq!(
            logout_url.as_str().split('?').next().unwrap(),
            format!("{}/logout", provider.issuer)
        );
        let logout_parameters = logout_url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            logout_parameters
                .get("client_id")
                .map(|value| value.as_ref()),
            Some("sproyt-test")
        );
        assert_eq!(
            logout_parameters
                .get("post_logout_redirect_uri")
                .map(|value| value.as_ref()),
            Some("https://chat.example/")
        );
        assert!(logout.clear_cookie.contains("sproyt_session="));
        assert!(logout.clear_cookie.contains("Max-Age=0"));
        assert!(logout.clear_refresh_cookie.contains("sproyt_refresh="));
        assert!(logout.clear_refresh_cookie.contains("Path=/"));
        assert!(
            logout
                .clear_legacy_refresh_cookie
                .contains("Path=/auth/refresh")
        );
        server.abort();
    }
}
