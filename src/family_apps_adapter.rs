//! Trusted server-to-server Open Drinks authorization transport.
//! No database handle is accepted: callers finish network I/O before their
//! transaction rechecks policy and atomically consumes the pending state/proof.
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{Client, Url, header::HeaderValue};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "/api/v1/integrations/sproyt";
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub(crate) struct OpenDrinksAdapter {
    origin: Url,
    credential: HeaderValue,
    client: Client,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AdapterError {
    #[error("invalid Open Drinks server configuration")]
    Configuration,
    #[error("Open Drinks authorization service unavailable")]
    Unavailable,
    #[error("Open Drinks authorization rejected")]
    Rejected,
    #[error("invalid Open Drinks authorization response")]
    InvalidProof,
}

/// Context is read from Sprøyt's pending authorization, never the callback body.
#[derive(Clone, Serialize)]
pub(crate) struct ApprovalRequest {
    pub state: String,
    pub sproyt_user_id: String,
    pub channel_id: String,
    pub session_id: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub(crate) struct ExchangeRequest {
    pub approval_code: String,
    pub state: String,
    pub sproyt_user_id: String,
    pub channel_id: String,
    pub session_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalCreated {
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireAuthorization {
    authorization_id: String,
    app_user_id: String,
    state: String,
    sproyt_user_id: String,
    channel_id: String,
    session_id: String,
    session_name: String,
    session_admin: bool,
    verified_email: Option<String>,
    expires_at: DateTime<Utc>,
}

/// Deliberately not Deserialize: only a validated server exchange creates this.
pub(crate) struct VerifiedAuthorization {
    pub authorization_id: String,
    pub app_user_id: String,
    pub session_id: String,
    pub session_name: String,
    pub verified_email: Option<String>,
    pub expires_at: DateTime<Utc>,
}

impl OpenDrinksAdapter {
    /// `origin` is operator configuration, never a URL provided by a user/app.
    pub(crate) fn new(origin: &str, credential: &str) -> Result<Self, AdapterError> {
        let origin = trusted_origin(origin)?;
        if credential.is_empty() || credential.len() > 4096 {
            return Err(AdapterError::Configuration);
        }
        let mut credential = HeaderValue::from_str(&format!("Bearer {credential}"))
            .map_err(|_| AdapterError::Configuration)?;
        credential.set_sensitive(true);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .https_only(true)
            .build()
            .map_err(|_| AdapterError::Configuration)?;
        Ok(Self {
            origin,
            credential,
            client,
        })
    }

    pub(crate) async fn prepare(&self, request: &ApprovalRequest) -> Result<Url, AdapterError> {
        if !valid_context(
            &request.state,
            &request.sproyt_user_id,
            &request.channel_id,
            &request.session_id,
        ) || request.expires_at <= Utc::now()
            || request.expires_at > Utc::now() + chrono::Duration::minutes(5)
        {
            return Err(AdapterError::InvalidProof);
        }
        let response: ApprovalCreated = self.post("/authorization-requests", request).await?;
        if !valid_id(&response.request_id) {
            return Err(AdapterError::InvalidProof);
        }
        let mut url = self.origin.clone();
        url.set_path("/integrations/sproyt/authorize");
        url.query_pairs_mut()
            .append_pair("request_id", &response.request_id);
        Ok(url)
    }

    pub(crate) async fn exchange(
        &self,
        request: &ExchangeRequest,
    ) -> Result<VerifiedAuthorization, AdapterError> {
        if !valid_id(&request.approval_code)
            || !valid_context(
                &request.state,
                &request.sproyt_user_id,
                &request.channel_id,
                &request.session_id,
            )
        {
            return Err(AdapterError::InvalidProof);
        }
        let response: WireAuthorization = self.post("/authorizations/exchange", request).await?;
        validate_authorization(response, request, Utc::now())
    }

    async fn post<T: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        suffix: &str,
        body: &T,
    ) -> Result<R, AdapterError> {
        let mut url = self.origin.clone();
        url.set_path(&format!("{PREFIX}{suffix}"));
        let mut response = self
            .client
            .post(url)
            .header(reqwest::header::AUTHORIZATION, self.credential.clone())
            .json(body)
            .send()
            .await
            .map_err(|_| AdapterError::Unavailable)?;
        // In particular, never follow a Location with our service credential.
        if !response.status().is_success() {
            return Err(if response.status().is_server_error() {
                AdapterError::Unavailable
            } else {
                AdapterError::Rejected
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AdapterError::Unavailable)?
        {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(AdapterError::InvalidProof);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| AdapterError::InvalidProof)
    }
}

fn trusted_origin(value: &str) -> Result<Url, AdapterError> {
    let url = Url::parse(value).map_err(|_| AdapterError::Configuration)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AdapterError::Configuration);
    }
    Ok(url)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

fn valid_context(state: &str, user: &str, channel: &str, session: &str) -> bool {
    state.len() >= 32 && [state, user, channel, session].into_iter().all(valid_id)
}

fn validate_authorization(
    response: WireAuthorization,
    request: &ExchangeRequest,
    now: DateTime<Utc>,
) -> Result<VerifiedAuthorization, AdapterError> {
    if response.state != request.state
        || response.sproyt_user_id != request.sproyt_user_id
        || response.channel_id != request.channel_id
        || response.session_id != request.session_id
        || !response.session_admin
        || response.expires_at <= now
        || response.expires_at > now + chrono::Duration::minutes(5)
        || ![
            response.authorization_id.as_str(),
            response.app_user_id.as_str(),
            response.session_name.as_str(),
        ]
        .into_iter()
        .all(valid_id)
        || response.verified_email.as_deref().is_some_and(|email| {
            email.is_empty() || email.len() > 320 || email.chars().any(char::is_control)
        })
    {
        return Err(AdapterError::InvalidProof);
    }
    Ok(VerifiedAuthorization {
        authorization_id: response.authorization_id,
        app_user_id: response.app_user_id,
        session_id: response.session_id,
        session_name: response.session_name,
        verified_email: response.verified_email,
        expires_at: response.expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_trusted_https_origins_are_accepted() {
        for origin in [
            "http://open-drink.bjoroy.me",
            "https://u:p@example.com",
            "https://example.com/path",
            "https://example.com/?target=evil",
            "https://example.com/#fragment",
        ] {
            assert!(OpenDrinksAdapter::new(origin, "secret").is_err());
        }
        assert!(OpenDrinksAdapter::new("https://open-drink.bjoroy.me", "secret").is_ok());
        assert!(
            OpenDrinksAdapter::new("https://example.com", "secret\r\ninjected: header").is_err()
        );
    }

    fn request() -> ExchangeRequest {
        ExchangeRequest {
            approval_code: "code".into(),
            state: "01234567890123456789012345678901".into(),
            sproyt_user_id: "user".into(),
            channel_id: "channel".into(),
            session_id: "session".into(),
        }
    }

    fn proof() -> WireAuthorization {
        WireAuthorization {
            authorization_id: "auth".into(),
            app_user_id: "od-user".into(),
            state: request().state,
            sproyt_user_id: "user".into(),
            channel_id: "channel".into(),
            session_id: "session".into(),
            session_name: "Tasting".into(),
            session_admin: true,
            verified_email: Some("moderator@example.com".into()),
            expires_at: Utc::now() + chrono::Duration::minutes(2),
        }
    }

    #[test]
    fn proof_requires_matching_context_current_session_admin_and_bounded_expiry() {
        let now = Utc::now();
        assert!(validate_authorization(proof(), &request(), now).is_ok());
        for field in [
            "state", "user", "channel", "session", "admin", "expired", "future",
        ] {
            let mut proof = proof();
            match field {
                "state" => proof.state = "other".into(),
                "user" => proof.sproyt_user_id = "other".into(),
                "channel" => proof.channel_id = "other".into(),
                "session" => proof.session_id = "other".into(),
                "admin" => proof.session_admin = false,
                "expired" => proof.expires_at = now,
                "future" => proof.expires_at = now + chrono::Duration::minutes(6),
                _ => unreachable!(),
            }
            assert!(
                validate_authorization(proof, &request(), now).is_err(),
                "{field}"
            );
        }
    }
}
