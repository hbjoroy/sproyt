use crate::{
    App, ProbeError,
    store::{self, Result},
};
use axum::{
    Form, Json,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::Response,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::SigningKey,
    rand_core::OsRng,
    signature::{SignatureEncoding, Signer},
    traits::PublicKeyParts,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const CLIENT: &str = "n01-broker";
pub const SECRET: &str = "synthetic-provider-secret-only";

#[derive(Clone)]
struct Grant {
    nonce: String,
    subject: String,
    challenge: String,
    fault: String,
    expires: i64,
}

pub struct Provider {
    key: RsaPrivateKey,
    wrong_key: RsaPrivateKey,
    grants: Mutex<HashMap<String, Grant>>,
}

impl Provider {
    pub fn new() -> Result<Self> {
        Ok(Self {
            key: RsaPrivateKey::new(&mut OsRng, 2048).map_err(|_| ())?,
            wrong_key: RsaPrivateKey::new(&mut OsRng, 2048).map_err(|_| ())?,
            grants: Mutex::new(HashMap::new()),
        })
    }
}

pub async fn metadata(State(app): State<Arc<App>>) -> Json<Value> {
    Json(
        json!({"issuer": app.issuer, "authorization_endpoint":format!("{}/authorize",app.issuer),
        "token_endpoint":format!("{}/token",app.issuer), "jwks_uri":format!("{}/jwks",app.issuer),
        "response_types_supported":["code"], "subject_types_supported":["public"],
        "id_token_signing_alg_values_supported":["RS256"], "token_endpoint_auth_methods_supported":["client_secret_basic"],
        "code_challenge_methods_supported":["S256"]}),
    )
}

pub async fn jwks(State(app): State<Arc<App>>) -> Json<Value> {
    let public = RsaPublicKey::from(&app.provider.key);
    Json(
        json!({"keys":[{"kty":"RSA","use":"sig","alg":"RS256","kid":"fixture-key",
        "n":URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())}]}),
    )
}

#[derive(Deserialize)]
pub struct Authorization {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    state: String,
    nonce: String,
    code_challenge: String,
    code_challenge_method: String,
}

pub async fn authorize(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    query: std::result::Result<Query<Authorization>, axum::extract::rejection::QueryRejection>,
) -> std::result::Result<Response, ProbeError> {
    let Query(input) = query.map_err(|_| ProbeError)?;
    if input.response_type != "code"
        || input.client_id != CLIENT
        || input.redirect_uri != app.oidc_callback
        || input.code_challenge_method != "S256"
        || !store::valid_challenge(&input.code_challenge)
        || !store::valid_challenge(&input.state)
        || !store::valid_challenge(&input.nonce)
    {
        return Err(ProbeError);
    }
    let account = headers
        .get("x-probe-account")
        .and_then(|v| v.to_str().ok())
        .ok_or(ProbeError)?;
    if !matches!(account, "alice" | "bob") {
        return Err(ProbeError);
    }
    let fault = headers
        .get("x-probe-fault")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !matches!(fault, "" | "nonce" | "audience" | "issuer" | "signature") {
        return Err(ProbeError);
    }
    let code = store::random().map_err(|_| ProbeError)?;
    {
        let mut grants = app.provider.grants.lock().map_err(|_| ProbeError)?;
        grants.retain(|_, v| v.expires > store::now());
        if grants.len() >= 32 {
            return Err(ProbeError);
        }
        grants.insert(
            store::digest(&code),
            Grant {
                nonce: input.nonce,
                subject: account.into(),
                challenge: input.code_challenge,
                fault: fault.into(),
                expires: store::now() + 60,
            },
        );
    }
    let mut target = reqwest::Url::parse(&app.oidc_callback).map_err(|_| ProbeError)?;
    target
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &input.state);
    crate::redirect(target.as_str())
}

#[derive(Deserialize)]
pub struct TokenRequest {
    grant_type: String,
    code: String,
    code_verifier: String,
    redirect_uri: String,
}

pub async fn token(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    form: std::result::Result<Form<TokenRequest>, axum::extract::rejection::FormRejection>,
) -> std::result::Result<Json<Value>, ProbeError> {
    let expected = format!("Basic {}", STANDARD.encode(format!("{CLIENT}:{SECRET}")));
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        != Some(expected.as_str())
    {
        return Err(ProbeError);
    }
    let Form(input) = form.map_err(|_| ProbeError)?;
    if input.grant_type != "authorization_code"
        || input.redirect_uri != app.oidc_callback
        || !store::valid_challenge(&input.code)
        || !store::valid_verifier(&input.code_verifier)
    {
        return Err(ProbeError);
    }
    let grant = {
        let mut grants = app.provider.grants.lock().map_err(|_| ProbeError)?;
        let key = store::digest(&input.code);
        let grant = grants.get(&key).ok_or(ProbeError)?;
        if grant.expires <= store::now() || grant.challenge != store::digest(&input.code_verifier) {
            return Err(ProbeError);
        }
        grants.remove(&key).ok_or(ProbeError)?
    };
    let issuer = if grant.fault == "issuer" {
        "https://wrong.invalid/"
    } else {
        &app.issuer
    };
    let audience = if grant.fault == "audience" {
        "wrong-audience"
    } else {
        CLIENT
    };
    let nonce = if grant.fault == "nonce" {
        "wrong-nonce"
    } else {
        &grant.nonce
    };
    let claims = json!({"iss":issuer,"aud":audience,"sub":grant.subject,"iat":store::now(),"exp":store::now()+60,
        "nonce":nonce,"email":"same-address@example.invalid","email_verified":true,"name":"Synthetic person"});
    let jwt_header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT","kid":"fixture-key"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).map_err(|_| ProbeError)?);
    let signing_input = format!("{jwt_header}.{payload}");
    let key = if grant.fault == "signature" {
        &app.provider.wrong_key
    } else {
        &app.provider.key
    };
    let signature = SigningKey::<Sha256>::new(key.clone()).sign(signing_input.as_bytes());
    let jwt = format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    );
    Ok(Json(
        json!({"access_token":"synthetic-provider-only","token_type":"Bearer","expires_in":60,"id_token":jwt}),
    ))
}
