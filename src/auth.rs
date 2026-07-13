use std::{
    fmt,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce as OidcNonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest,
};
use serde::{Deserialize, Serialize};

use crate::{
    config::OidcConfig,
    domain::{DisplayName, PrincipalKind, TextValidationError, User, UserId},
};

const LOGIN_TTL_SECONDS: u64 = 600;
const SESSION_TTL_SECONDS: u64 = 8 * 60 * 60;
pub const LOGIN_COOKIE: &str = "sproyt_oidc_tx";
pub const SESSION_COOKIE: &str = "sproyt_session";

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

    pub fn authenticate_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unauthorized),
            Self::Oidc(service) => service.authenticate_session(cookie_header),
        }
    }

    pub fn authenticate_request(
        &self,
        requested_name: Option<String>,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        match self {
            Self::Development => self.authenticate_development(requested_name),
            Self::Oidc(_) => self.authenticate_session(cookie_header),
        }
    }

    pub fn login(&self) -> Result<LoginStart, AuthError> {
        match self {
            Self::Development => Err(AuthError::Unsupported("OIDC is disabled".to_owned())),
            Self::Oidc(service) => service.login(),
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

    pub fn logout(&self) -> Logout {
        match self {
            Self::Development => Logout {
                redirect_url: "/".to_owned(),
                clear_cookie: clear_cookie(SESSION_COOKIE, "/"),
            },
            Self::Oidc(service) => Logout {
                redirect_url: service.post_logout_redirect_url.clone(),
                clear_cookie: clear_cookie(SESSION_COOKIE, "/"),
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
    pub clear_transaction_cookie: String,
}

pub struct Logout {
    pub redirect_url: String,
    pub clear_cookie: String,
}

pub struct OidcService {
    client: DiscoveredCoreClient,
    http_client: reqwest::Client,
    codec: CookieCodec,
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

impl OidcService {
    async fn discover(config: &OidcConfig) -> Result<Self, AuthError> {
        let issuer = IssuerUrl::new(config.issuer().to_owned()).map_err(AuthError::external)?;
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(AuthError::external)?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &http_client)
            .await
            .map_err(AuthError::external)?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.client_id().to_owned()),
            Some(ClientSecret::new(config.client_secret().to_owned())),
        )
        .set_redirect_uri(
            RedirectUrl::new(config.redirect_url().to_owned()).map_err(AuthError::external)?,
        );
        Ok(Self {
            client,
            http_client,
            codec: CookieCodec::new(config.session_key(), config.session_previous_keys())?,
            issuer: config.issuer().to_owned(),
            post_logout_redirect_url: config.post_logout_redirect_url().to_owned(),
        })
    }

    fn login(&self) -> Result<LoginStart, AuthError> {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                OidcNonce::new_random,
            )
            .add_scope(Scope::new("openid".to_owned()))
            .add_scope(Scope::new("profile".to_owned()))
            .add_scope(Scope::new("email".to_owned()))
            .set_pkce_challenge(challenge)
            .url();
        let transaction = LoginTransaction {
            state: state.secret().to_owned(),
            nonce: nonce.secret().to_owned(),
            pkce_verifier: verifier.secret().to_owned(),
            expires_at: now_seconds() + LOGIN_TTL_SECONDS,
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
        let token = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(AuthError::external)?
            .set_pkce_verifier(PkceCodeVerifier::new(transaction.pkce_verifier))
            .request_async(&self.http_client)
            .await
            .map_err(AuthError::external)?;
        let id_token = token.id_token().ok_or(AuthError::Unauthorized)?;
        let nonce = OidcNonce::new(transaction.nonce);
        let claims = id_token
            .claims(&self.client.id_token_verifier(), &nonce)
            .map_err(AuthError::external)?;
        let subject = claims.subject().as_str().to_owned();
        let principal = principal(&self.issuer, &subject, &subject)?;
        let session = SessionClaims {
            issuer: self.issuer.clone(),
            subject,
            display_name: principal.user.display_name.to_string(),
            expires_at: now_seconds() + SESSION_TTL_SECONDS,
        };
        let value = self.codec.seal(&session)?;
        Ok(LoginComplete {
            principal,
            set_cookie: secure_cookie(SESSION_COOKIE, &value, "/", SESSION_TTL_SECONDS),
            clear_transaction_cookie: clear_cookie(LOGIN_COOKIE, "/auth/callback"),
        })
    }

    fn authenticate_session(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<AuthenticatedPrincipal, AuthError> {
        let value = read_cookie(cookie_header, SESSION_COOKIE).ok_or(AuthError::Unauthorized)?;
        let claims: SessionClaims = self.codec.open(value)?;
        if claims.expires_at < now_seconds() || claims.issuer != self.issuer {
            return Err(AuthError::Unauthorized);
        }
        principal(&claims.issuer, &claims.subject, &claims.display_name)
    }
}

#[derive(Serialize, Deserialize)]
struct LoginTransaction {
    state: String,
    nonce: String,
    pkce_verifier: String,
    expires_at: u64,
}

#[derive(Serialize, Deserialize)]
struct SessionClaims {
    issuer: String,
    subject: String,
    display_name: String,
    expires_at: u64,
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
            expires_at: now_seconds() + 60,
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
    fn rotated_cookie_key_reads_old_sessions_but_writes_only_with_primary() {
        let old_key = URL_SAFE_NO_PAD.encode([3_u8; 32]);
        let new_key = URL_SAFE_NO_PAD.encode([9_u8; 32]);
        let old_codec = CookieCodec::new(&old_key, &[]).unwrap();
        let rotated = CookieCodec::new(&new_key, std::slice::from_ref(&old_key)).unwrap();
        let claims = SessionClaims {
            issuer: "issuer".to_owned(),
            subject: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            expires_at: now_seconds() + 60,
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
}
