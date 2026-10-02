//! Server-only GitHub App transport. Never log credentials or approved issue text.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use rsa::{
    RsaPrivateKey,
    pkcs1::DecodeRsaPrivateKey,
    pkcs1v15::SigningKey,
    pkcs8::DecodePrivateKey,
    signature::{SignatureEncoding, Signer},
};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub(crate) struct GitHub {
    http: reqwest::Client,
    base: String,
    app_id: String,
    key: Arc<SigningKey<Sha256>>,
}

#[derive(Clone)]
pub(crate) struct Destination {
    pub installation: i64,
    pub repository_id: i64,
    pub repository: String,
    pub bot_login: String,
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct Issue {
    pub id: i64,
    pub number: i64,
    pub url: String,
}

pub(crate) enum CreateError {
    Rejected,
    Uncertain,
}

impl GitHub {
    #[cfg(test)]
    pub(crate) fn test_client(base: String) -> Self {
        Self::new(
            "1".into(),
            RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap(),
            base,
        )
        .unwrap()
    }
    pub fn from_env() -> Result<Option<Self>, &'static str> {
        let app = std::env::var("SPROYT_GITHUB_APP_ID").ok();
        let path = std::env::var("SPROYT_GITHUB_PRIVATE_KEY_PATH").ok();
        match (app, path) {
            (None, None) => Ok(None),
            (Some(app), Some(path)) if app.parse::<u64>().is_ok_and(|id| id > 0) => {
                let pem = std::fs::read_to_string(path).map_err(|_| "GitHub key cannot be read")?;
                let key = RsaPrivateKey::from_pkcs1_pem(&pem)
                    .or_else(|_| RsaPrivateKey::from_pkcs8_pem(&pem))
                    .map_err(|_| "GitHub key is invalid")?;
                use rsa::traits::PublicKeyParts;
                if key.n().bits() < 2048 {
                    return Err("GitHub key is too small");
                }
                Ok(Some(Self::new(app, key, "https://api.github.com".into())?))
            }
            _ => Err("GitHub App ID and private key path must be configured together"),
        }
    }

    fn new(app_id: String, key: RsaPrivateKey, base: String) -> Result<Self, &'static str> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Sproyt-Work-Items/1.0")
            .build()
            .map_err(|_| "GitHub client cannot be configured")?;
        Ok(Self {
            http,
            base,
            app_id,
            key: Arc::new(SigningKey::new(key)),
        })
    }

    fn jwt(&self) -> String {
        let now = Utc::now().timestamp();
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = URL_SAFE_NO_PAD
            .encode(json!({"iat":now-60,"exp":now+300,"iss":self.app_id}).to_string());
        let input = format!("{header}.{claims}");
        let signature = self.key.sign(input.as_bytes());
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    fn request(&self, method: reqwest::Method, path: &str, token: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
    }

    async fn response(request: reqwest::RequestBuilder) -> Result<Value, &'static str> {
        let response = request
            .send()
            .await
            .map_err(|_| "GitHub transport unavailable")?;
        if !response.status().is_success() {
            return Err("GitHub rejected the request");
        }
        Self::decode_response(response).await
    }

    async fn decode_response(mut response: reqwest::Response) -> Result<Value, &'static str> {
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "GitHub response incomplete")?
        {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err("GitHub response too large");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "GitHub response invalid")
    }

    pub async fn token(&self, target: &Destination) -> Result<String, &'static str> {
        let response = Self::response(self.request(reqwest::Method::POST,
            &format!("/app/installations/{}/access_tokens", target.installation), &self.jwt())
            .json(&json!({"repository_ids":[target.repository_id],"permissions":{"issues":"write","metadata":"read"}}))).await?;
        let repositories = response["repositories"]
            .as_array()
            .ok_or("GitHub token scope missing")?;
        if repositories.len() != 1
            || repositories[0]["id"] != target.repository_id
            || response["permissions"]["issues"] != "write"
        {
            return Err("GitHub token scope mismatch");
        }
        let token = response["token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("GitHub token missing")?;
        Ok(token.into())
    }

    pub async fn verify(&self, target: &Destination, token: &str) -> Result<(), &'static str> {
        let repo = Self::response(self.request(
            reqwest::Method::GET,
            &format!("/repos/{}", target.repository),
            token,
        ))
        .await?;
        if repo["id"] != target.repository_id
            || repo["full_name"] != target.repository
            || repo["has_issues"] != true
        {
            return Err("GitHub destination mismatch");
        }
        Ok(())
    }

    pub async fn create(
        &self,
        target: &Destination,
        token: &str,
        title: &str,
        body: &str,
    ) -> Result<Issue, CreateError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/repos/{}/issues", target.repository),
                token,
            )
            .json(&json!({"title":title,"body":body}))
            .send()
            .await
            .map_err(|_| CreateError::Uncertain)?;
        if matches!(
            response.status().as_u16(),
            400 | 401 | 403 | 404 | 410 | 415 | 422 | 429
        ) {
            return Err(CreateError::Rejected);
        }
        if !response.status().is_success() {
            return Err(CreateError::Uncertain);
        }
        let value = Self::decode_response(response)
            .await
            .map_err(|_| CreateError::Uncertain)?;
        checked_issue(&value, target, title, body).map_err(|_| CreateError::Uncertain)
    }

    // A missing marker is not evidence that an uncertain POST did not succeed.
    // The caller must keep the receipt uncertain, never issue another blind POST.
    pub async fn find(
        &self,
        target: &Destination,
        token: &str,
        marker: &str,
        title: &str,
        body: &str,
    ) -> Result<Option<Issue>, &'static str> {
        let mut found = None;
        for page in 1..=10 {
            let value = Self::response(
                self.request(
                    reqwest::Method::GET,
                    &format!("/repos/{}/issues", target.repository),
                    token,
                )
                .query(&[
                    ("state", "all".to_string()),
                    ("sort", "created".to_string()),
                    ("direction", "desc".to_string()),
                    ("per_page", "100".to_string()),
                    ("page", page.to_string()),
                ]),
            )
            .await?;
            let issues = value.as_array().ok_or("GitHub issue list invalid")?;
            for value in issues {
                if value["body"]
                    .as_str()
                    .is_some_and(|text| text.contains(marker))
                {
                    if found.is_some() {
                        return Err("GitHub marker is ambiguous");
                    }
                    found = Some(checked_issue(value, target, title, body)?);
                }
            }
            if issues.len() < 100 {
                return Ok(found);
            }
        }
        Err("GitHub reconciliation needs an operator: issue list incomplete")
    }
}

fn checked_issue(
    value: &Value,
    target: &Destination,
    title: &str,
    body: &str,
) -> Result<Issue, &'static str> {
    let number = value["number"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("GitHub issue number invalid")?;
    let id = value["id"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("GitHub issue ID invalid")?;
    let url = format!("https://github.com/{}/issues/{number}", target.repository);
    if value["html_url"] != url
        || value["title"] != title
        || value["body"] != body
        || value["user"]["login"] != target.bot_login
        || value["pull_request"].is_object()
    {
        return Err("GitHub issue provenance or content mismatch");
    }
    Ok(Issue { id, number, url })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn issue_requires_exact_destination_content_and_app_provenance() {
        let target = Destination {
            installation: 1,
            repository_id: 2,
            repository: "owner/repo".into(),
            bot_login: "our-app[bot]".into(),
        };
        let valid = json!({"id":123,"number":4,"html_url":"https://github.com/owner/repo/issues/4","title":"Title","body":"Approved\nmarker","user":{"login":"our-app[bot]"}});
        assert!(checked_issue(&valid, &target, "Title", "Approved\nmarker").is_ok());
        for (field, replacement) in [
            ("html_url", json!("https://evil.example/issues/4")),
            ("body", json!("changed")),
            ("user", json!({"login":"another-user"})),
            ("pull_request", json!({})),
        ] {
            let mut changed = valid.clone();
            changed[field] = replacement;
            assert!(checked_issue(&changed, &target, "Title", "Approved\nmarker").is_err());
        }
    }
}
