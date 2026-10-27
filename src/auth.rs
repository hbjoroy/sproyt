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
    domain::{DisplayName, Handle, PrincipalKind, TextValidationError, User, UserId},
};

const LOGIN_TTL_SECONDS: u64 = 600;
const ACCESS_SESSION_MAX_TTL_SECONDS: u64 = 8 * 60 * 60;
const REFRESH_IDLE_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
const SESSION_REFRESH_LEAD_SECONDS: u64 = 60;
const MAX_EMAIL_LENGTH: usize = 254;
pub const LOGIN_COOKIE: &str = "sproyt_oidc_tx";
pub const SESSION_COOKIE: &str = "sproyt_session";
pub const REFRESH_COOKIE: &str = "sproyt_refresh";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedPrincipal {
    pub user: User,
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
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
        principal("urn:sproyt:development", &name, &name, Some(&name), None)
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
        self.login_with_enrollment(return_to, None)
    }

    /// Starts an OIDC login tied to a single enrollment token.  The token is
    /// kept only in the encrypted, short-lived OIDC transaction cookie; it is
    /// deliberately not included in the provider redirect or a return URL.
    pub fn login_enrollment(&self, token: String) -> Result<LoginStart, AuthError> {
        self.login_with_enrollment(None, Some(token))
    }

    fn login_with_enrollment(
        &self,
        return_to: Option<String>,
        enrollment_token: Option<String>,
    ) -> Result<LoginStart, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unsupported("OIDC is disabled".to_owned())),
            Self::Oidc(service) => service.login(return_to, enrollment_token),
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

    /// Native callers own a separate OIDC grant and must fence uncertain refresh outcomes.
    pub(crate) async fn renew_native_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<SessionRenewal, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unauthorized),
            Self::Oidc(service) => service.renew_session_inner(cookie_header, true).await,
        }
    }

    pub(crate) fn native_session_expires_at(&self, cookie_header: &str) -> Result<u64, AuthError> {
        let Self::Oidc(service) = self else {
            return Err(AuthError::Unauthorized);
        };
        let value =
            read_cookie(Some(cookie_header), SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let claims: SessionClaims = service.codec.open(value)?;
        validate_session_claims(&claims, &service.issuer)?;
        Ok(claims.expires_at)
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
    pub enrollment_token: Option<String>,
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

    fn login(
        &self,
        return_to: Option<String>,
        enrollment_token: Option<String>,
    ) -> Result<LoginStart, AuthError> {
        let client = self.current_client()?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let authorization = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                OidcNonce::new_random,
            )
            .add_scope(Scope::new("profile".to_owned()))
            .add_scope(Scope::new("email".to_owned()))
            .add_scope(Scope::new("offline_access".to_owned()))
            .set_pkce_challenge(challenge);
        // An invitation can be opened while a different account has an SSO
        // session. Make the identity explicit instead of silently redeeming
        // against that session. The verified-email match still happens below
        // and in the repository before the invitation can be consumed.
        let authorization = if enrollment_token.is_some() {
            authorization.add_extra_param("prompt", "login")
        } else {
            authorization
        };
        let (url, state, nonce) = authorization.url();
        let transaction = LoginTransaction {
            state: state.secret().to_owned(),
            nonce: nonce.secret().to_owned(),
            pkce_verifier: verifier.secret().to_owned(),
            expires_at: now_seconds() + LOGIN_TTL_SECONDS,
            return_to,
            enrollment_token,
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
        let preferred_username = claims
            .preferred_username()
            .map(|username| username.as_str());
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
        // The ID-token signature, issuer, audience, and nonce were verified
        // above; do not source identity data from an unverified endpoint.
        let email = normalized_email(claims.email().map(|email| email.as_str()));
        // A signed email claim is not proof that the user controls the address.
        // Only enrollment requires the provider's address-bound verification;
        // ordinary login remains compatible with legacy unverified accounts.
        // Acceptance separately matches this address to the invitation's hash.
        if transaction.enrollment_token.is_some()
            && (claims.email_verified() != Some(true) || email.is_none())
        {
            return Err(AuthError::Unauthorized);
        }
        let principal = principal(
            &self.issuer,
            &subject,
            display_name,
            preferred_username,
            email,
        )?;
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
                        #[allow(clippy::option_as_ref_deref)]
                        preferred_username: principal
                            .user
                            .handle
                            .as_ref()
                            .map(|handle| handle.to_string()),
                        email: principal.email.clone(),
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
            preferred_username: principal
                .user
                .handle
                .as_ref()
                .map(|handle| handle.to_string()),
            email: principal.email.clone(),
            // Do not put the provider access token in a browser cookie.  Real
            // Authentik access tokens can be large enough that adding normal
            // profile claims pushes the encrypted cookie past the browser's
            // 4 KiB per-cookie limit, which makes a fresh login loop.
            // Refresh credentials remain in their dedicated cookie.
            access_token: None,
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
            enrollment_token: transaction.enrollment_token,
        })
    }

    async fn authenticate_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let value = read_cookie(cookie_header, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let claims: SessionClaims = self.codec.open(value)?;
        validate_session_claims(&claims, &self.issuer)?;
        principal(
            &claims.issuer,
            &claims.subject,
            &claims.display_name,
            claims.preferred_username.as_deref(),
            claims.email,
        )
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
            return principal(
                &claims.issuer,
                &claims.subject,
                &claims.display_name,
                claims.preferred_username.as_deref(),
                claims.email,
            );
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
        let preferred_username = user_info
            .preferred_username()
            .map(|username| username.as_str())
            .or(claims.preferred_username.as_deref());
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
        principal(
            &claims.issuer,
            &claims.subject,
            display_name,
            preferred_username,
            claims.email,
        )
    }

    async fn renew_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<SessionRenewal, AuthError> {
        self.renew_session_inner(cookie_header, false).await
    }

    async fn renew_session_inner(
        &self,
        cookie_header: Option<&str>,
        preserve_uncertain: bool,
    ) -> Result<SessionRenewal, AuthError> {
        let (issuer, subject, display_name, preferred_username, email, refresh_token) =
            if let Some(value) = read_cookie(cookie_header, REFRESH_COOKIE) {
                let claims: RefreshClaims = self.codec.open(value)?;
                validate_refresh_claims(&claims.issuer, claims.expires_at, &self.issuer)?;
                (
                    claims.issuer,
                    claims.subject,
                    claims.display_name,
                    claims.preferred_username,
                    claims.email,
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
                    claims.preferred_username,
                    claims.email,
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
            .map_err(|error| {
                if preserve_uncertain && !matches!(&error, openidconnect::RequestTokenError::ServerResponse(response) if response.error() == &openidconnect::core::CoreErrorResponseType::InvalidGrant) {
                    AuthError::External("native provider refresh outcome uncertain".into())
                } else { AuthError::Unauthorized }
            })?;
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
            preferred_username,
            email: email.clone(),
            // See the callback path: an access token is not needed to resume
            // the local session and must not bloat the browser cookie.
            access_token: None,
            refresh_token: None,
            expires_at: now_seconds().saturating_add(access_max_age),
            refresh_expires_at: Some(refresh_expires_at),
        };
        let value = self.codec.seal(&renewed)?;
        let refresh_value = self.codec.seal(&RefreshClaims {
            issuer,
            subject,
            display_name: renewed.display_name.clone(),
            preferred_username: renewed.preferred_username.clone(),
            email,
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
    #[serde(default)]
    enrollment_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SessionClaims {
    issuer: String,
    subject: String,
    display_name: String,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
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
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
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
    preferred_username: Option<&str>,
    email: Option<String>,
) -> Result<AuthenticatedPrincipal, AuthError> {
    Ok(AuthenticatedPrincipal {
        user: User {
            id: UserId::from_oidc(issuer, subject),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new(display_name)?,
            handle: preferred_username.map(Handle::from_external),
            external_provider: Some(issuer.to_owned()),
            external_subject: Some(subject.to_owned()),
            created_at: chrono::Utc::now(),
        },
        issuer: issuer.to_owned(),
        subject: subject.to_owned(),
        email,
    })
}

fn normalized_email(email: Option<&str>) -> Option<String> {
    let email = email?.trim();
    (!email.is_empty() && email.len() <= MAX_EMAIL_LENGTH && !email.chars().any(char::is_control))
        .then(|| email.to_lowercase())
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
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Clone)]
    struct TestProvider {
        issuer: String,
        client_id: String,
        client_secret: String,
        nonce: Arc<Mutex<String>>,
        email_verified: Arc<Mutex<Option<bool>>>,
        email: Arc<Mutex<Option<String>>>,
        signing_key: Arc<Mutex<(String, RsaPrivateKey)>>,
        active: Arc<AtomicBool>,
        native_refresh_failure: Arc<AtomicBool>,
        refresh_requests: Arc<std::sync::atomic::AtomicUsize>,
        last_refresh: Arc<Mutex<String>>,
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
        let mut claims = serde_json::json!({
            "iss":provider.issuer,
            "sub":"authentik-user-1",
            "aud":provider.client_id,
            "exp":now+300,
            "iat":now,
            "nonce":provider.nonce.lock().unwrap().clone(),
            "name":"Authentik Test User",
            "preferred_username":"ignored-fallback"
        });
        if let Some(email) = provider.email.lock().unwrap().as_ref() {
            claims["email"] = serde_json::json!(email);
        }
        if let Some(verified) = *provider.email_verified.lock().unwrap() {
            claims["email_verified"] = serde_json::json!(verified);
        }
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
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
    ) -> axum::response::Response {
        assert!(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("Basic ")),
            "OIDC client did not authenticate at the token endpoint"
        );
        if form.get("grant_type").map(String::as_str) == Some("refresh_token") {
            provider.refresh_requests.fetch_add(1, Ordering::SeqCst);
            *provider.last_refresh.lock().unwrap() = form.get("refresh_token").unwrap().clone();
            if provider.native_refresh_failure.load(Ordering::SeqCst) {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error":"server_error"})),
                )
                    .into_response();
            }
        }
        let refresh_token = match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                assert!(matches!(
                    form.get("code").map(String::as_str),
                    Some("valid-code" | "native-code")
                ));
                assert!(
                    form.get("code_verifier")
                        .is_some_and(|value| !value.is_empty())
                );
                if form.get("code").map(String::as_str) == Some("native-code") {
                    "native-refresh-a"
                } else {
                    "refresh-a"
                }
            }
            Some("refresh_token") => match form.get("refresh_token").map(String::as_str) {
                Some("refresh-a") => "refresh-b",
                Some("native-refresh-a") => "native-refresh-b",
                _ => panic!("unexpected fixture refresh grant"),
            },
            grant => panic!("unexpected grant type {grant:?}"),
        };
        Json(serde_json::json!({
            "access_token":"test-access-token",
            "token_type":"Bearer",
            "expires_in":300,
            "id_token":signed_id_token(&provider),
            "refresh_token":refresh_token
        }))
        .into_response()
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
            email_verified: Arc::new(Mutex::new(Some(false))),
            email: Arc::new(Mutex::new(Some("  Authentik.User@Example.Test  ".into()))),
            signing_key: Arc::new(Mutex::new(generate_signing_key("key-a"))),
            active: Arc::new(AtomicBool::new(true)),
            native_refresh_failure: Arc::new(AtomicBool::new(false)),
            refresh_requests: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            last_refresh: Arc::new(Mutex::new(String::new())),
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
        assert!(first.email.is_none());
    }

    #[test]
    fn normalized_email_trims_lowercases_and_rejects_unsafe_values() {
        assert_eq!(
            normalized_email(Some("  User.Name@Example.Test  ")),
            Some("user.name@example.test".to_owned())
        );
        assert_eq!(normalized_email(Some("user\n@example.test")), None);
        assert_eq!(
            normalized_email(Some(&"a".repeat(MAX_EMAIL_LENGTH + 1))),
            None
        );
    }

    #[test]
    fn encrypted_cookie_round_trips_and_rejects_tampering() {
        let key = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let codec = CookieCodec::new(&key, &[]).unwrap();
        let value = SessionClaims {
            issuer: "issuer".to_owned(),
            subject: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            preferred_username: None,
            email: None,
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
    fn new_session_cookie_omits_large_access_tokens_and_stays_below_browser_limit() {
        let key = URL_SAFE_NO_PAD.encode([6_u8; 32]);
        let codec = CookieCodec::new(&key, &[]).unwrap();
        let preferred_username =
            "early-adopter-with-a-perfectly-ordinary-but-long-authentik-username";
        let base = || SessionClaims {
            issuer: "https://auth.example".to_owned(),
            subject: "authentik-user-with-a-long-subject".to_owned(),
            display_name: "An Authentik User With A Normal Display Name".to_owned(),
            preferred_username: Some(preferred_username.to_owned()),
            email: Some("user.with.a.long.local.part@example.test".to_owned()),
            access_token: None,
            refresh_token: None,
            expires_at: now_seconds() + 300,
            refresh_expires_at: Some(now_seconds() + REFRESH_IDLE_TTL_SECONDS),
        };

        // Authentik can issue JWT access tokens of this size.  The pre-fix
        // session cookie stored that token and exceeded the 4 KiB browser
        // limit after profile data was added.
        let mut oversized_legacy = base();
        oversized_legacy.access_token = Some("a".repeat(3_000));
        let oversized_cookie = secure_cookie(
            SESSION_COOKIE,
            &codec.seal(&oversized_legacy).unwrap(),
            "/",
            300,
        );
        assert!(oversized_cookie.len() > 4_096);

        let new_cookie = secure_cookie(SESSION_COOKIE, &codec.seal(&base()).unwrap(), "/", 300);
        assert!(new_cookie.len() < 3_800);
        let claims: SessionClaims = codec
            .open(read_cookie(Some(&new_cookie), SESSION_COOKIE).unwrap())
            .unwrap();
        assert_eq!(
            claims.preferred_username.as_deref(),
            Some(preferred_username)
        );
        assert!(claims.access_token.is_none());
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
        assert!(opened.email.is_none());
        assert!(opened.access_token.is_none());

        #[derive(Serialize)]
        struct PreviousRefreshClaims {
            issuer: String,
            subject: String,
            display_name: String,
            refresh_token: String,
            expires_at: u64,
        }

        let sealed = codec
            .seal(&PreviousRefreshClaims {
                issuer: "issuer".to_owned(),
                subject: "alice".to_owned(),
                display_name: "Alice".to_owned(),
                refresh_token: "refresh-token".to_owned(),
                expires_at: now_seconds() + 60,
            })
            .unwrap();
        let opened: RefreshClaims = codec.open(&sealed).unwrap();
        assert!(opened.email.is_none());
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
            preferred_username: None,
            email: None,
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
            preferred_username: None,
            email: None,
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
    async fn enrollment_requires_verified_email_without_restricting_ordinary_login() {
        let (config, provider, server) = test_oidc_provider().await;
        let auth = AuthService::oidc(&config).await.unwrap();
        for verified in [Some(false), None, Some(true)] {
            *provider.email_verified.lock().unwrap() = verified;
            for enrollment in [false, true] {
                let login = if enrollment {
                    auth.login_enrollment("test-enrollment-capability".into())
                } else {
                    auth.login(None)
                }
                .unwrap();
                let authorization = reqwest::Url::parse(&login.authorization_url).unwrap();
                assert_eq!(
                    authorization
                        .query_pairs()
                        .find(|(key, _)| key == "prompt")
                        .map(|(_, value)| value.into_owned()),
                    enrollment.then(|| "login".to_owned())
                );
                assert!(
                    !login
                        .authorization_url
                        .contains("test-enrollment-capability")
                );
                *provider.nonce.lock().unwrap() =
                    authorization_parameter(&login.authorization_url, "nonce");
                let result = auth
                    .callback(
                        "valid-code".into(),
                        authorization_parameter(&login.authorization_url, "state"),
                        Some(&login.set_cookie),
                    )
                    .await;
                if enrollment && verified != Some(true) {
                    assert!(matches!(result, Err(AuthError::Unauthorized)));
                } else {
                    assert!(result.is_ok());
                }
            }
        }
        *provider.email.lock().unwrap() = None;
        let login = auth
            .login_enrollment("test-enrollment-capability".into())
            .unwrap();
        *provider.nonce.lock().unwrap() =
            authorization_parameter(&login.authorization_url, "nonce");
        assert!(matches!(
            auth.callback(
                "valid-code".into(),
                authorization_parameter(&login.authorization_url, "state"),
                Some(&login.set_cookie),
            )
            .await,
            Err(AuthError::Unauthorized)
        ));
        server.abort();
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
                preferred_username: None,
                email: None,
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
                preferred_username: None,
                email: None,
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
                preferred_username: None,
                email: None,
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
        assert_eq!(complete.enrollment_token, None);
        assert_eq!(complete.principal.subject, "authentik-user-1");
        assert_eq!(complete.principal.issuer, provider.issuer);
        assert_eq!(
            complete.principal.email.as_deref(),
            Some("authentik.user@example.test")
        );
        assert_eq!(
            complete.principal.user.display_name.to_string(),
            "Authentik Test User"
        );
        assert!(complete.clear_transaction_cookie.contains("Max-Age=0"));

        *provider.email_verified.lock().unwrap() = Some(true);
        let enrollment_login = auth
            .login_enrollment("enrollment-token_123456".to_owned())
            .unwrap();
        let enrollment_state =
            authorization_parameter(&enrollment_login.authorization_url, "state");
        assert_eq!(
            authorization_parameter(&enrollment_login.authorization_url, "prompt"),
            "login"
        );
        let enrollment_nonce =
            authorization_parameter(&enrollment_login.authorization_url, "nonce");
        *provider.nonce.lock().unwrap() = enrollment_nonce;
        let enrollment_complete = auth
            .callback(
                "valid-code".to_owned(),
                enrollment_state,
                Some(&enrollment_login.set_cookie),
            )
            .await
            .unwrap();
        assert_eq!(enrollment_complete.return_to, "/");
        assert_eq!(
            enrollment_complete.enrollment_token.as_deref(),
            Some("enrollment-token_123456")
        );
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
        assert_eq!(
            restored.email.as_deref(),
            Some("authentik.user@example.test")
        );
        let callback_session: SessionClaims = service
            .codec
            .open(read_cookie(Some(&complete.set_cookie), SESSION_COOKIE).unwrap())
            .unwrap();
        assert!(callback_session.access_token.is_none());
        let revalidated = auth
            .revalidate_request(None, Some(&complete.set_cookie))
            .await
            .unwrap();
        assert_eq!(
            revalidated.user.display_name.to_string(),
            "Authentik Test User"
        );
        assert_eq!(
            revalidated.email.as_deref(),
            Some("authentik.user@example.test")
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
        assert!(renewed.access_token.is_none());
        assert_eq!(
            renewed.email.as_deref(),
            Some("authentik.user@example.test")
        );
        let renewed_refresh: RefreshClaims = service
            .codec
            .open(read_cookie(Some(&renewal.set_refresh_cookie), REFRESH_COOKIE).unwrap())
            .unwrap();
        assert_eq!(renewed_refresh.refresh_token, "refresh-b");
        assert_eq!(
            renewed_refresh.email.as_deref(),
            Some("authentik.user@example.test")
        );
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
        assert!(
            auth.revalidate_request(None, Some(&renewed_cookie))
                .await
                .is_ok()
        );

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
    #[tokio::test]
    async fn native_dedicated_grant_refresh_and_uncertain_provider_fence() {
        use crate::native_sessions::{
            CancelRequest, ExchangeRequest, LoginRequest, NativeError, NativeSessions,
            RefreshRequest,
        };
        let (config, provider, server) = test_oidc_provider().await;
        let auth = AuthService::oidc(&config).await.unwrap();
        let browser = auth.login(None).unwrap();
        *provider.nonce.lock().unwrap() =
            authorization_parameter(&browser.authorization_url, "nonce");
        let browser_complete = auth
            .callback(
                "valid-code".into(),
                authorization_parameter(&browser.authorization_url, "state"),
                Some(&browser.set_cookie),
            )
            .await
            .unwrap();
        let browser_cookies = format!(
            "{}; {}",
            browser_complete.set_cookie,
            browser_complete.set_refresh_cookie.as_ref().unwrap()
        );
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let native = NativeSessions::for_test(pool, config.issuer(), config.session_key())
            .await
            .unwrap();
        let verifier = URL_SAFE_NO_PAD.encode([19u8; 32]);
        let started = native
            .login(
                &auth,
                LoginRequest {
                    challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
                    destination_id: "windows-loopback".into(),
                    callback_port: 19192,
                    expected_user_id: None,
                },
            )
            .await
            .unwrap();
        let (upstream, cookie) = native.browser(&started.transaction_id).await.unwrap();
        assert!(
            authorization_parameter(&upstream, "state")
                != authorization_parameter(&browser.authorization_url, "state")
        );
        *provider.nonce.lock().unwrap() = authorization_parameter(&upstream, "nonce");
        // A browser's unrelated LOGIN_COOKIE cannot replace the native per-attempt cookie.
        assert!(
            native
                .callback(
                    &auth,
                    "native-code".into(),
                    authorization_parameter(&upstream, "state"),
                    Some(&browser.set_cookie)
                )
                .await
                .is_err()
        );
        let confirmation = native
            .callback(
                &auth,
                "native-code".into(),
                authorization_parameter(&upstream, "state"),
                Some(&cookie),
            )
            .await
            .unwrap();
        let target = native
            .confirm(&started.transaction_id, &confirmation.nonce, Some(&cookie))
            .await
            .unwrap();
        let target = reqwest::Url::parse(&target).unwrap();
        let code = target
            .query_pairs()
            .find(|(key, _)| key == "code")
            .unwrap()
            .1
            .into_owned();
        let grant = native
            .exchange(ExchangeRequest {
                transaction_id: started.transaction_id.clone(),
                code,
                verifier: verifier.clone(),
                redemption_id: "a".repeat(32),
                environment: "canary".into(),
                destination_id: "windows-loopback".into(),
            })
            .await
            .unwrap()
            .native_session;
        assert!(
            grant.display_name == "Authentik Test User" && grant.expires_at <= now_seconds() + 300
        );
        assert!(
            native
                .authenticate(&auth, &grant.access_token)
                .await
                .unwrap()
                .user
                .id
                == browser_complete.principal.user.id
        );
        let refreshed = native
            .refresh(
                &auth,
                &grant.refresh_token,
                RefreshRequest {
                    operation_id: "b".repeat(32),
                    generation: 0,
                },
            )
            .await
            .unwrap();
        assert!(provider.last_refresh.lock().unwrap().as_str() == "native-refresh-a");
        assert!(
            native
                .refresh(
                    &auth,
                    &grant.refresh_token,
                    RefreshRequest {
                        operation_id: "b".repeat(32),
                        generation: 0
                    }
                )
                .await
                .unwrap()
                == refreshed
        );
        assert!(provider.refresh_requests.load(Ordering::SeqCst) == 1);
        provider
            .native_refresh_failure
            .store(true, Ordering::SeqCst);
        assert!(
            native
                .refresh(
                    &auth,
                    &refreshed.refresh_token,
                    RefreshRequest {
                        operation_id: "c".repeat(32),
                        generation: 1
                    }
                )
                .await
                == Err(NativeError::Unavailable)
        );
        assert!(provider.last_refresh.lock().unwrap().as_str() == "native-refresh-b");
        assert!(
            native
                .refresh(
                    &auth,
                    &refreshed.refresh_token,
                    RefreshRequest {
                        operation_id: "c".repeat(32),
                        generation: 1
                    }
                )
                .await
                .is_err()
        );
        assert!(provider.refresh_requests.load(Ordering::SeqCst) == 2);
        assert!(
            auth.authenticate_session(Some(&browser_cookies))
                .await
                .is_ok()
        );
        native
            .cancel_login(CancelRequest {
                transaction_id: started.transaction_id,
                verifier,
                environment: "canary".into(),
                destination_id: "windows-loopback".into(),
            })
            .await
            .unwrap();
        server.abort();
    }
    #[tokio::test]
    async fn push_subscription_check_requires_oidc_session() {
        let (config, _provider, provider_server) = test_oidc_provider().await;
        let auth = AuthService::oidc(&config).await.unwrap();
        let state = crate::web::native::test_state(auth, None).await;
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        for query in ["", "?participant=someone-else"] {
            let response = client
                .post(format!(
                    "http://{address}/api/v1/me/push-subscriptions/check{query}"
                ))
                .json(&serde_json::json!({"endpoint": "https://fcm.googleapis.com/test-device"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        }
        server.abort();
        provider_server.abort();
    }

    #[tokio::test]
    async fn native_http_callback_and_same_origin_account_confirmation() {
        let (config, provider, provider_server) = test_oidc_provider().await;
        let auth = AuthService::oidc(&config).await.unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let native = crate::native_sessions::NativeSessions::for_test(
            pool,
            config.issuer(),
            config.session_key(),
        )
        .await
        .unwrap();
        let state = crate::web::native::test_state(auth, Some(native)).await;
        let app = crate::web::native::routes()
            .route("/auth/callback", get(crate::web::auth::auth_callback))
            .route("/api/v1/media/{id}", get(crate::web::media::download_media))
            .route(
                "/api/v1/media/{id}/preview",
                get(crate::web::media::download_media_preview),
            )
            .route(
                "/api/v1/media/{id}/status",
                get(crate::web::media::media_preview_status),
            )
            .layer(axum::middleware::from_fn(
                crate::web::system::add_security_headers,
            ))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let verifier = URL_SAFE_NO_PAD.encode([29u8; 32]);
        let response=client.post(format!("{origin}/api/v1/native/login")).json(&serde_json::json!({"challenge":URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),"destination_id":"windows-loopback","callback_port":19192})).send().await.unwrap();
        assert!(response.status().is_success());
        let started: serde_json::Value = response.json().await.unwrap();
        let id = started["transaction_id"].as_str().unwrap();
        assert!(
            started["authorization_url"]
                == format!("https://sproyt-canary.bjoroy.me/auth/native/browser/{id}")
        );
        let entry = client
            .get(format!("{origin}/auth/native/browser/{id}"))
            .send()
            .await
            .unwrap();
        assert!(entry.status() == axum::http::StatusCode::FOUND);
        let upstream = entry.headers()["location"].to_str().unwrap();
        *provider.nonce.lock().unwrap() = authorization_parameter(upstream, "nonce");
        let cookie = entry.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert!(
            !cookie.starts_with(SESSION_COOKIE)
                && !cookie.starts_with(REFRESH_COOKIE)
                && !cookie.starts_with(LOGIN_COOKIE)
        );
        let callback = client
            .get(format!("{origin}/auth/callback"))
            .query(&[
                ("code", "native-code"),
                ("state", authorization_parameter(upstream, "state").as_str()),
            ])
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert!(callback.status().is_success() && !callback.headers().contains_key("set-cookie"));
        assert!(callback.headers()["referrer-policy"] == "origin");
        let policy = callback.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(
            policy.contains("form-action 'self' http://127.0.0.1:19192")
                && !policy.contains("127.0.0.1:*")
        );
        let html = callback.text().await.unwrap();
        assert!(html.contains("Authentik Test User"));
        let nonce = html
            .split("name=nonce value='")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap();
        let confirmation = format!("{origin}/auth/native/confirm/{id}");
        let denied = client
            .post(&confirmation)
            .header("Cookie", &cookie)
            .header("Origin", "https://wrong.example")
            .form(&[("nonce", nonce)])
            .send()
            .await
            .unwrap();
        assert!(denied.status() == axum::http::StatusCode::UNAUTHORIZED);
        let accepted = client
            .post(&confirmation)
            .header("Cookie", &cookie)
            .header("Origin", "https://sproyt-canary.bjoroy.me")
            .form(&[("nonce", nonce)])
            .send()
            .await
            .unwrap();
        assert!(
            accepted.status() == axum::http::StatusCode::FOUND
                && !accepted.headers().contains_key("set-cookie")
        );
        let target = reqwest::Url::parse(accepted.headers()["location"].to_str().unwrap()).unwrap();
        assert!(
            target.host_str() == Some("127.0.0.1")
                && target.port() == Some(19192)
                && target.path() == "/return"
        );
        let code = target
            .query_pairs()
            .find(|(key, _)| key == "code")
            .unwrap()
            .1
            .into_owned();
        let response=client.post(format!("{origin}/api/v1/native/exchange")).json(&serde_json::json!({"transaction_id":id,"code":code,"verifier":verifier,"redemption_id":"a".repeat(32),"environment":"canary","destination_id":"windows-loopback"})).send().await.unwrap();
        assert!(response.status().is_success());
        let result: serde_json::Value = response.json().await.unwrap();
        assert!(
            result["native_session"]["scope"] == "sproyt_native_v1"
                && result["display_name"] == "Authentik Test User"
        );
        let access = result["native_session"]["access_token"].as_str().unwrap();
        let actor = state
            .native_sessions
            .as_ref()
            .unwrap()
            .authenticate(&state.auth, access)
            .await
            .unwrap();
        let other = AuthService::development()
            .authenticate_request(Some("native-media-other".into()), None)
            .await
            .unwrap();
        let mut media_ids = Vec::new();
        for (principal, slug) in [(&actor, "native-media-own"), (&other, "native-media-other")] {
            state
                .chat
                .ensure_user(principal.user.clone())
                .await
                .unwrap();
            let channel = state
                .chat
                .create_channel(
                    principal.user.id.clone(),
                    crate::domain::ChannelSlug::new(slug).unwrap(),
                    crate::domain::DisplayName::new(slug).unwrap(),
                    crate::domain::ChannelKind::Private,
                    None,
                )
                .await
                .unwrap();
            let media = state
                .chat
                .store_media(crate::domain::MediaUpload {
                    actor: principal.user.id.clone(),
                    channel_id: channel.id,
                    filename: "native.mp4".into(),
                    content_type: "video/mp4".into(),
                    content: vec![7; 10],
                    dimensions: None,
                    preview: None,
                })
                .await
                .unwrap();
            media_ids.push(media.id);
        }
        let media_url = format!("{origin}/api/v1/media/{}", media_ids[0]);
        let bytes = client
            .get(&media_url)
            .bearer_auth(access)
            .send()
            .await
            .unwrap();
        assert_eq!(bytes.status(), axum::http::StatusCode::OK);
        assert_eq!(bytes.bytes().await.unwrap().as_ref(), &[7; 10]);
        let range = client
            .get(&media_url)
            .bearer_auth(access)
            .header("Range", "bytes=2-5")
            .send()
            .await
            .unwrap();
        assert_eq!(range.status(), axum::http::StatusCode::PARTIAL_CONTENT);
        assert_eq!(range.bytes().await.unwrap().len(), 4);
        let head = client
            .head(&media_url)
            .bearer_auth(access)
            .send()
            .await
            .unwrap();
        assert_eq!(head.status(), axum::http::StatusCode::OK);
        assert!(head.bytes().await.unwrap().is_empty());
        for suffix in ["", "/preview", "/status"] {
            let denied = client
                .get(format!("{origin}/api/v1/media/{}{suffix}", media_ids[1]))
                .bearer_auth(access)
                .send()
                .await
                .unwrap();
            assert!(matches!(
                denied.status(),
                axum::http::StatusCode::FORBIDDEN | axum::http::StatusCode::NOT_FOUND
            ));
            let mixed = client
                .get(format!("{media_url}{suffix}"))
                .bearer_auth(access)
                .header("Cookie", "sproyt_session=browser")
                .send()
                .await
                .unwrap();
            assert_eq!(mixed.status(), axum::http::StatusCode::UNAUTHORIZED);
        }
        let mixed = client
            .get(format!("{media_url}?participant=other"))
            .bearer_auth(access)
            .send()
            .await
            .unwrap();
        assert_eq!(mixed.status(), axum::http::StatusCode::UNAUTHORIZED);
        let response = client
            .get(format!("{origin}/api/v1/native/session"))
            .bearer_auth(access)
            .header("Cookie", "sproyt_session=browser")
            .send()
            .await
            .unwrap();
        assert!(response.status() == axum::http::StatusCode::UNAUTHORIZED);
        let response = client
            .get(format!("{origin}/api/v1/native/session"))
            .bearer_auth(access)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let refresh = result["native_session"]["refresh_token"].as_str().unwrap();
        let revoke = result["native_session"]["revoke_token"].as_str().unwrap();
        let refreshed = client
            .post(format!("{origin}/api/v1/native/refresh"))
            .bearer_auth(refresh)
            .json(&serde_json::json!({"operation_id":"b".repeat(32),"generation":0}))
            .send()
            .await
            .unwrap();
        assert!(refreshed.status().is_success());
        let refreshed: serde_json::Value = refreshed.json().await.unwrap();
        let repeated = client
            .post(format!("{origin}/api/v1/native/refresh"))
            .bearer_auth(refresh)
            .json(&serde_json::json!({"operation_id":"b".repeat(32),"generation":0}))
            .send()
            .await
            .unwrap();
        assert!(repeated.status().is_success());
        assert!(repeated.json::<serde_json::Value>().await.unwrap() == refreshed);
        assert!(provider.refresh_requests.load(Ordering::SeqCst) == 1);
        for _ in 0..2 {
            let response = client
                .post(format!("{origin}/api/v1/native/revoke"))
                .bearer_auth(revoke)
                .send()
                .await
                .unwrap();
            assert!(response.status() == axum::http::StatusCode::NO_CONTENT);
        }
        let revoked_media = client
            .get(&media_url)
            .bearer_auth(refreshed["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(revoked_media.status(), axum::http::StatusCode::UNAUTHORIZED);
        let response = client
            .get(format!("{origin}/api/v1/native/session"))
            .bearer_auth(refreshed["access_token"].as_str().unwrap())
            .send()
            .await
            .unwrap();
        assert!(response.status() == axum::http::StatusCode::UNAUTHORIZED);
        let response = client
            .post(format!("{origin}/api/v1/native/refresh"))
            .bearer_auth(refresh)
            .json(&serde_json::json!({"operation_id":"b".repeat(32),"generation":0}))
            .send()
            .await
            .unwrap();
        assert!(response.status() == axum::http::StatusCode::UNAUTHORIZED);
        server.abort();
        provider_server.abort();
    }
}
