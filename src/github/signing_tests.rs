use super::*;
use axum::{Json, Router, extract::Path, http::HeaderMap, routing::post};
use rsa::sha2::Sha256;
use rsa::{
    RsaPrivateKey,
    pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey},
    pkcs1v15::{Signature, VerifyingKey},
    pkcs8::{EncodePrivateKey, LineEnding},
    rand_core::OsRng,
    signature::Verifier,
    traits::PublicKeyParts,
};
use std::sync::OnceLock;

const APP_ID: &str = "123456";

fn fixture_key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut OsRng, 2048).unwrap())
}

fn verify_jwt(jwt: &str, earliest: i64, latest: i64) {
    let (input, signature) = jwt.rsplit_once('.').unwrap();
    let (header, claims) = input.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
    assert_eq!(header, json!({"alg":"RS256","typ":"JWT"}));
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap();
    assert_eq!(claims["iss"], APP_ID);
    let iat = claims["iat"].as_i64().unwrap();
    let exp = claims["exp"].as_i64().unwrap();
    assert!((earliest - 60..=latest - 60).contains(&iat));
    assert_eq!(exp - iat, 360);

    // Use the original library only for independent public-key verification.
    let verifier = VerifyingKey::<Sha256>::new(fixture_key().to_public_key());
    let decoded = URL_SAFE_NO_PAD.decode(signature).unwrap();
    assert_eq!(decoded.len(), 256);
    let signature = Signature::try_from(decoded.as_slice()).unwrap();
    verifier.verify(input.as_bytes(), &signature).unwrap();
    assert!(
        verifier
            .verify(b"modified signing input", &signature)
            .is_err()
    );
}

#[test]
fn github_jwt_accepts_pkcs1_and_pkcs8_and_verifies_as_rs256() {
    let fixture = fixture_key();
    let pkcs1 = fixture.to_pkcs1_pem(LineEnding::LF).unwrap();
    let pkcs8 = fixture.to_pkcs8_pem(LineEnding::CRLF).unwrap();
    for pem in [pkcs1.as_str(), pkcs8.as_str()] {
        let client = GitHub::new(
            APP_ID.into(),
            GitHub::parse_key(pem).unwrap(),
            "http://127.0.0.1".into(),
        )
        .unwrap();
        let earliest = Utc::now().timestamp();
        let jwt = client.jwt().unwrap();
        verify_jwt(&jwt, earliest, Utc::now().timestamp());
    }
}

#[test]
fn github_key_rejects_malformed_public_and_sub_2048_bit_pem() {
    let public = fixture_key()
        .to_public_key()
        .to_pkcs1_pem(LineEnding::LF)
        .unwrap();
    assert!(GitHub::parse_key(&public).is_err());
    assert!(
        GitHub::parse_key(
            "-----BEGIN RSA PRIVATE KEY-----\ninvalid!\n-----END RSA PRIVATE KEY-----\n"
        )
        .is_err()
    );
    assert!(
        GitHub::parse_key("-----BEGIN PRIVATE KEY-----\nYWJj\n-----END PRIVATE KEY-----\n")
            .is_err()
    );
    let short = RsaPrivateKey::new(&mut OsRng, 2047).unwrap();
    assert_eq!(short.n().bits(), 2047);
    let short = short.to_pkcs1_pem(LineEnding::LF).unwrap();
    assert!(GitHub::parse_key(&short).is_err());
}

#[tokio::test]
async fn github_installation_token_authenticates_with_jwt_and_checks_requested_repository_scope() {
    let earliest = Utc::now().timestamp();
    let app = Router::new().route(
        "/app/installations/{installation}/access_tokens",
        post(move |Path(installation): Path<i64>, headers: HeaderMap, Json(body): Json<Value>| async move {
            let jwt = headers["authorization"].to_str().unwrap().strip_prefix("Bearer ").unwrap();
            verify_jwt(jwt, earliest, Utc::now().timestamp());
            assert_eq!(headers["accept"], "application/vnd.github+json");
            assert_eq!(headers["x-github-api-version"], "2026-03-10");
            assert_eq!(body, json!({"repository_ids":[42],"permissions":{"issues":"write","metadata":"read"}}));
            let repository = match installation {
                17 => 42,
                18 => 43,
                _ => panic!("unexpected installation fixture"),
            };
            Json(json!({"token":"fixture-installation-token","repositories":[{"id":repository}],"permissions":{"issues":"write","metadata":"read"}}))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let pem = fixture_key().to_pkcs1_pem(LineEnding::LF).unwrap();
    let client = GitHub::new(APP_ID.into(), GitHub::parse_key(&pem).unwrap(), base).unwrap();
    let mut destination = Destination {
        installation: 17,
        repository_id: 42,
        repository: "fixture/repository".into(),
        bot_login: "fixture[bot]".into(),
    };
    assert_eq!(
        client.token(&destination).await.unwrap(),
        "fixture-installation-token"
    );
    destination.installation = 18;
    assert_eq!(
        client.token(&destination).await.unwrap_err(),
        "GitHub token scope mismatch"
    );
    server.abort();
}
