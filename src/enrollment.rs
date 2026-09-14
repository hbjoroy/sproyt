use std::{env, fmt, time::Duration};

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const DEFAULT_API_URL: &str = "http://authentik-server.authentik.svc.cluster.local";
const DEFAULT_PUBLIC_URL: &str = "https://sproyt-security.bjoroy.me";
const DEFAULT_FLOW_SLUG: &str = "sproyt-invitation-enrollment";
const DEFAULT_SPROYT_URL: &str = "https://sproyt.bjoroy.me";
pub const INVITATION_LIFETIME_HOURS: i64 = 48;

#[derive(Clone)]
pub struct EnrollmentService {
    client: reqwest::Client,
    api_url: Url,
    public_url: Url,
    sproyt_url: Url,
    flow_id: Uuid,
    flow_slug: String,
    token: String,
}

impl fmt::Debug for EnrollmentService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnrollmentService")
            .field("api_url", &self.api_url)
            .field("public_url", &self.public_url)
            .field("sproyt_url", &self.sproyt_url)
            .field("flow_id", &self.flow_id)
            .field("flow_slug", &self.flow_slug)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EnrollmentInvitation {
    pub url: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvisionedEnrollment {
    pub invitation: EnrollmentInvitation,
    pub authentik_invitation_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum EnrollmentError {
    #[error("invalid onboarding configuration: {0}")]
    Configuration(&'static str),
    #[error("invalid invitation details: {0}")]
    Validation(&'static str),
    #[error("identity provider is temporarily unavailable")]
    Unavailable,
    #[error("identity provider rejected the invitation request")]
    Rejected,
    #[error("identity provider returned an invalid invitation")]
    InvalidResponse,
}

#[derive(Serialize)]
struct CreateInvitationRequest<'a> {
    name: String,
    expires: &'a str,
    fixed_data: serde_json::Value,
    single_use: bool,
    flow: Uuid,
}

#[derive(Deserialize)]
struct CreateInvitationResponse {
    pk: String,
}

#[derive(Serialize)]
struct SendInvitationEmailRequest<'a> {
    email_addresses: [&'a str; 1],
}

impl EnrollmentService {
    pub fn from_env() -> Result<Option<Self>, EnrollmentError> {
        let Some(token) = optional_env("SPROYT_AUTHENTIK_API_TOKEN") else {
            return Ok(None);
        };
        let flow_id = required_env("SPROYT_AUTHENTIK_ENROLLMENT_FLOW_ID")?
            .parse()
            .map_err(|_| EnrollmentError::Configuration("invalid enrollment flow ID"))?;
        Self::new(
            env::var("SPROYT_AUTHENTIK_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned()),
            env::var("SPROYT_AUTHENTIK_PUBLIC_URL")
                .unwrap_or_else(|_| DEFAULT_PUBLIC_URL.to_owned()),
            env::var("SPROYT_AUTHENTIK_ENROLLMENT_FLOW_SLUG")
                .unwrap_or_else(|_| DEFAULT_FLOW_SLUG.to_owned()),
            env::var("SPROYT_PUBLIC_URL").unwrap_or_else(|_| DEFAULT_SPROYT_URL.to_owned()),
            flow_id,
            token,
        )
        .map(Some)
    }

    pub fn new(
        api_url: impl AsRef<str>,
        public_url: impl AsRef<str>,
        flow_slug: impl Into<String>,
        sproyt_url: impl AsRef<str>,
        flow_id: Uuid,
        token: impl Into<String>,
    ) -> Result<Self, EnrollmentError> {
        let api_url = service_url(api_url.as_ref(), false)?;
        let public_url = service_url(public_url.as_ref(), true)?;
        let sproyt_url = service_url(sproyt_url.as_ref(), true)?;
        let flow_slug = flow_slug.into();
        if flow_slug.is_empty()
            || flow_slug.len() > 80
            || !flow_slug
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(EnrollmentError::Configuration(
                "invalid enrollment flow slug",
            ));
        }
        let token = token.into();
        if token.trim().is_empty() {
            return Err(EnrollmentError::Configuration("empty API token"));
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| EnrollmentError::Configuration("cannot build HTTP client"))?;
        Ok(Self {
            client,
            api_url,
            public_url,
            sproyt_url,
            flow_id,
            flow_slug,
            token,
        })
    }

    pub async fn create(
        &self,
        enrollment_token: &str,
        email: &str,
        display_name: Option<&str>,
        expires_at: DateTime<Utc>,
    ) -> Result<ProvisionedEnrollment, EnrollmentError> {
        let (email, display_name) = validate_invitee(email, display_name)?;
        let remaining = expires_at.signed_duration_since(Utc::now());
        if remaining <= chrono::Duration::zero()
            || remaining > chrono::Duration::hours(INVITATION_LIFETIME_HOURS)
        {
            return Err(EnrollmentError::Validation("ugyldig utløpstid"));
        }
        let expires = expires_at.to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut fixed_data = serde_json::Map::new();
        fixed_data.insert("email".to_owned(), email.into());
        if let Some(display_name) = display_name {
            fixed_data.insert("name".to_owned(), display_name.into());
        }
        let request = CreateInvitationRequest {
            name: format!("sproyt-{}", Uuid::now_v7().simple()),
            expires: &expires,
            fixed_data: fixed_data.into(),
            single_use: true,
            flow: self.flow_id,
        };
        let endpoint = self
            .api_url
            .join("api/v3/stages/invitation/invitations/")
            .map_err(|_| EnrollmentError::Configuration("invalid API URL"))?;
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.token)
            .json(&request)
            .send()
            .await
            .map_err(|_| EnrollmentError::Unavailable)?;
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "Authentik rejected enrollment invitation");
            return Err(if response.status().is_server_error() {
                EnrollmentError::Unavailable
            } else {
                EnrollmentError::Rejected
            });
        }
        let invitation: CreateInvitationResponse = response
            .json()
            .await
            .map_err(|_| EnrollmentError::InvalidResponse)?;
        let authentik_invitation_id =
            Uuid::parse_str(&invitation.pk).map_err(|_| EnrollmentError::InvalidResponse)?;
        let mut next = self
            .sproyt_url
            .join("auth/login")
            .map_err(|_| EnrollmentError::Configuration("invalid Sprøyt URL"))?;
        next.query_pairs_mut()
            .append_pair("enrollment", enrollment_token);
        let mut url = self
            .public_url
            .join(&format!("if/flow/{}/", self.flow_slug))
            .map_err(|_| EnrollmentError::Configuration("invalid public Authentik URL"))?;
        url.query_pairs_mut()
            .append_pair("itoken", &invitation.pk)
            .append_pair("next", next.as_str());
        Ok(ProvisionedEnrollment {
            invitation: EnrollmentInvitation {
                url: url.into(),
                expires_at: expires,
            },
            authentik_invitation_id,
        })
    }

    pub async fn revoke(&self, invitation_id: Uuid) -> Result<(), EnrollmentError> {
        let endpoint = self
            .api_url
            .join(&format!(
                "api/v3/stages/invitation/invitations/{invitation_id}/"
            ))
            .map_err(|_| EnrollmentError::Configuration("invalid API URL"))?;
        let response = self
            .client
            .delete(endpoint)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|_| EnrollmentError::Unavailable)?;
        if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            tracing::warn!(status = %response.status(), "Authentik rejected enrollment invitation cleanup");
            Err(if response.status().is_server_error() {
                EnrollmentError::Unavailable
            } else {
                EnrollmentError::Rejected
            })
        }
    }

    pub async fn send_email(
        &self,
        invitation_id: Uuid,
        email: &str,
    ) -> Result<(), EnrollmentError> {
        let email = validate_email(email)?;
        // Authentik builds the URL in its invitation email from the incoming
        // request. The API call uses the internal service address, so forward
        // the validated public origin explicitly to avoid leaking an
        // unreachable cluster hostname to the recipient.
        let public_host = url_authority(&self.public_url)?;
        let endpoint = self
            .api_url
            .join(&format!(
                "api/v3/stages/invitation/invitations/{invitation_id}/send_email/"
            ))
            .map_err(|_| EnrollmentError::Configuration("invalid API URL"))?;
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.token)
            .header(reqwest::header::HOST, public_host)
            .header("x-forwarded-proto", "https")
            .json(&SendInvitationEmailRequest {
                email_addresses: [email],
            })
            .send()
            .await
            .map_err(|_| EnrollmentError::Unavailable)?;
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(());
        }
        tracing::warn!(status = %response.status(), "Authentik rejected enrollment invitation email");
        Err(if response.status().is_server_error() {
            EnrollmentError::Unavailable
        } else {
            EnrollmentError::Rejected
        })
    }
}

pub fn validate_invitee<'a>(
    email: &'a str,
    display_name: Option<&'a str>,
) -> Result<(&'a str, Option<&'a str>), EnrollmentError> {
    Ok((validate_email(email)?, validate_display_name(display_name)?))
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn required_env(name: &'static str) -> Result<String, EnrollmentError> {
    optional_env(name).ok_or(EnrollmentError::Configuration(name))
}

fn service_url(value: &str, require_https: bool) -> Result<Url, EnrollmentError> {
    let mut url = Url::parse(value).map_err(|_| EnrollmentError::Configuration("invalid URL"))?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (require_https && url.scheme() != "https")
        || (!require_https && !matches!(url.scheme(), "http" | "https"))
    {
        return Err(EnrollmentError::Configuration("invalid service URL"));
    }
    url.set_path("/");
    Ok(url)
}

fn url_authority(url: &Url) -> Result<String, EnrollmentError> {
    let mut authority = url
        .host()
        .ok_or(EnrollmentError::Configuration("public URL has no host"))?
        .to_string();
    if let Some(port) = url.port() {
        authority.push(':');
        authority.push_str(&port.to_string());
    }
    Ok(authority)
}

fn validate_email(value: &str) -> Result<&str, EnrollmentError> {
    let value = value.trim();
    let Some((local, domain)) = value.split_once('@') else {
        return Err(EnrollmentError::Validation("ugyldig e-postadresse"));
    };
    if value.len() > 254
        || local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(EnrollmentError::Validation("ugyldig e-postadresse"));
    }
    Ok(value)
}

fn validate_display_name(value: Option<&str>) -> Result<Option<&str>, EnrollmentError> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    if value.is_some_and(|value| value.chars().count() > 80 || value.chars().any(char::is_control))
    {
        return Err(EnrollmentError::Validation("ugyldig namn"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{delete, post},
    };
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Option<(HeaderMap, serde_json::Value)>>>);

    async fn create_invitation(
        State(capture): State<Capture>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        *capture.0.lock().unwrap() = Some((headers, body));
        Json(serde_json::json!({"pk":"dcde5ce9-ca43-4003-8d0c-762e8554650c"}))
    }

    async fn revoke_invitation() -> axum::http::StatusCode {
        axum::http::StatusCode::NO_CONTENT
    }

    async fn send_invitation_email(
        State(capture): State<Capture>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> StatusCode {
        *capture.0.lock().unwrap() = Some((headers, body));
        StatusCode::NO_CONTENT
    }

    async fn reject_invitation_email() -> StatusCode {
        StatusCode::FORBIDDEN
    }

    #[tokio::test]
    async fn creates_single_use_invitation_and_safe_return_url() {
        let capture = Capture::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/api/v3/stages/invitation/invitations/",
                post(create_invitation),
            )
            .with_state(capture.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = EnrollmentService::new(
            format!("http://{address}"),
            "https://identity.example",
            "sproyt-invitation-enrollment",
            "https://sproyt.example",
            Uuid::parse_str("dcde5ce9-ca43-4003-8d0c-762e8554650c").unwrap(),
            "secret-service-token",
        )
        .unwrap();
        let result = service
            .create(
                "enrollment_token-A_B",
                " ny@example.com ",
                Some(" Ny Brukar "),
                Utc::now() + chrono::Duration::hours(24),
            )
            .await
            .unwrap();
        let url = Url::parse(&result.invitation.url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.path(), "/if/flow/sproyt-invitation-enrollment/");
        assert_eq!(query["itoken"], "dcde5ce9-ca43-4003-8d0c-762e8554650c");
        assert_eq!(
            query["next"],
            "https://sproyt.example/auth/login?enrollment=enrollment_token-A_B"
        );
        assert_eq!(
            result.authentik_invitation_id,
            Uuid::parse_str("dcde5ce9-ca43-4003-8d0c-762e8554650c").unwrap()
        );
        let captured = capture.0.lock().unwrap();
        let (headers, body) = captured.as_ref().unwrap();
        assert_eq!(headers["authorization"], "Bearer secret-service-token");
        assert_eq!(body["single_use"], true);
        assert_eq!(body["flow"], "dcde5ce9-ca43-4003-8d0c-762e8554650c");
        assert_eq!(body["fixed_data"]["email"], "ny@example.com");
        assert_eq!(body["fixed_data"]["name"], "Ny Brukar");
        server.abort();
    }

    #[tokio::test]
    async fn revokes_authentik_invitation_and_accepts_already_removed_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/api/v3/stages/invitation/invitations/dcde5ce9-ca43-4003-8d0c-762e8554650c/",
            delete(revoke_invitation),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = EnrollmentService::new(
            format!("http://{address}"),
            "https://identity.example",
            "sproyt-invitation-enrollment",
            "https://sproyt.example",
            Uuid::nil(),
            "secret-service-token",
        )
        .unwrap();
        service
            .revoke(Uuid::parse_str("dcde5ce9-ca43-4003-8d0c-762e8554650c").unwrap())
            .await
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn queues_invitation_email_with_authenticated_expected_recipient() {
        let capture = Capture::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/api/v3/stages/invitation/invitations/dcde5ce9-ca43-4003-8d0c-762e8554650c/send_email/",
                post(send_invitation_email),
            )
            .with_state(capture.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = EnrollmentService::new(
            format!("http://{address}"),
            "https://identity.example",
            "sproyt-invitation-enrollment",
            "https://sproyt.example",
            Uuid::nil(),
            "secret-service-token",
        )
        .unwrap();
        service
            .send_email(
                Uuid::parse_str("dcde5ce9-ca43-4003-8d0c-762e8554650c").unwrap(),
                " ny@example.com ",
            )
            .await
            .unwrap();
        let captured = capture.0.lock().unwrap();
        let (headers, body) = captured.as_ref().unwrap();
        assert_eq!(headers["authorization"], "Bearer secret-service-token");
        assert_eq!(headers["host"], "identity.example");
        assert_eq!(headers["x-forwarded-proto"], "https");
        assert_eq!(
            body["email_addresses"],
            serde_json::json!(["ny@example.com"])
        );
        server.abort();
    }

    #[tokio::test]
    async fn reports_failed_invitation_email_delivery() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/api/v3/stages/invitation/invitations/dcde5ce9-ca43-4003-8d0c-762e8554650c/send_email/",
            post(reject_invitation_email),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let service = EnrollmentService::new(
            format!("http://{address}"),
            "https://identity.example",
            "sproyt-invitation-enrollment",
            "https://sproyt.example",
            Uuid::nil(),
            "secret-service-token",
        )
        .unwrap();
        assert!(matches!(
            service
                .send_email(
                    Uuid::parse_str("dcde5ce9-ca43-4003-8d0c-762e8554650c").unwrap(),
                    "ny@example.com",
                )
                .await,
            Err(EnrollmentError::Rejected)
        ));
        server.abort();
    }

    #[test]
    fn rejects_unsafe_configuration_and_invitee_fields() {
        let id = Uuid::nil();
        assert!(
            EnrollmentService::new(
                "file:///tmp",
                "https://id.example",
                "flow",
                "https://sproyt.example",
                id,
                "x"
            )
            .is_err()
        );
        assert!(
            EnrollmentService::new(
                "http://id",
                "http://id.example",
                "flow",
                "https://sproyt.example",
                id,
                "x"
            )
            .is_err()
        );
        assert!(validate_email("not-an-email").is_err());
        assert!(validate_email("person@example.com").is_ok());
        assert!(validate_display_name(Some("\u{0000}")).is_err());
    }
}
