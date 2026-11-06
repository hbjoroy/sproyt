use super::*;
use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::IntoResponse,
};
use ring::{aead, agreement, hkdf, rand::SystemRandom, signature};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::{net::TcpListener, sync::mpsc};

// Public synthetic values from RFC 8291, section 5 and appendix A.
const VAPID_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const VAPID_PUBLIC: &str = concat!(
    "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIg",
    "Dll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
);
const RFC_UA_PUBLIC: &str = concat!(
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcx",
    "aOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
);
const AUTH_SECRET: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const SUBJECT: &str = "mailto:web-push-test@example.test";

fn decode(encoded: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(encoded).expect("fixture base64url")
}

struct OutputLength(usize);

impl hkdf::KeyType for OutputLength {
    fn len(&self) -> usize {
        self.0
    }
}

fn expand(prk: &hkdf::Prk, info: &[&[u8]], output: &mut [u8]) {
    prk.expand(info, OutputLength(output.len()))
        .unwrap()
        .fill(output)
        .unwrap();
}

fn decrypt(
    body: &[u8],
    ua_public: &[u8],
    auth_secret: &[u8],
    shared_secret: &[u8],
) -> Result<Vec<u8>, ring::error::Unspecified> {
    let salt = &body[..16];
    let record_size = u32::from_be_bytes(body[16..20].try_into().unwrap()) as usize;
    let key_length = usize::from(body[20]);
    assert_eq!(key_length, 65);
    let server_public = &body[21..21 + key_length];
    assert_eq!(server_public[0], 4);
    let mut record = body[21 + key_length..].to_vec();
    assert!(record.len() < record_size);

    let mut ikm = [0; 32];
    expand(
        &hkdf::Salt::new(hkdf::HKDF_SHA256, auth_secret).extract(shared_secret),
        &[b"WebPush: info\0", ua_public, server_public],
        &mut ikm,
    );
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(&ikm);
    let mut key = [0; 16];
    expand(&prk, &[b"Content-Encoding: aes128gcm\0"], &mut key);
    let mut nonce = [0; 12];
    expand(&prk, &[b"Content-Encoding: nonce\0"], &mut nonce);
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_128_GCM, &key)?);
    let plaintext = key.open_in_place(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::empty(),
        &mut record,
    )?;
    let delimiter = plaintext.iter().rposition(|byte| *byte != 0).unwrap();
    assert_eq!(plaintext[delimiter], 2);
    Ok(plaintext[..delimiter].to_vec())
}

#[test]
fn independent_decryptor_matches_rfc8291_vector() {
    let body = decode(concat!(
        "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml",
        "mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT",
        "pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
    ));
    let shared_secret = decode("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
    let ua_public = decode(RFC_UA_PUBLIC);
    assert_eq!(
        decrypt(&body, &ua_public, &decode(AUTH_SECRET), &shared_secret).unwrap(),
        b"When I grow up, I want to be a watermelon"
    );
    assert!(decrypt(&body, &ua_public, &[0; 16], &shared_secret).is_err());
}

struct CapturedRequest {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
}

async fn push_service(
    status: StatusCode,
) -> (
    String,
    mpsc::UnboundedReceiver<CapturedRequest>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/push/device?token=fixture",
        listener.local_addr().unwrap()
    );
    let (capture, requests) = mpsc::unbounded_channel();
    let router = Router::new().fallback(move |method, uri, headers, body| {
        let capture = capture.clone();
        async move {
            capture
                .send(CapturedRequest {
                    method,
                    uri,
                    headers,
                    body,
                })
                .unwrap();
            if status.is_redirection() {
                (status, [("location", "/unexpected-redirect")]).into_response()
            } else {
                status.into_response()
            }
        }
    });
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (endpoint, requests, server)
}

fn push_job(endpoint: String, ua_public: String) -> PushJob {
    PushJob {
        subscription_id: Uuid::now_v7(),
        message_id: Uuid::now_v7(),
        endpoint,
        p256dh: ua_public,
        auth: AUTH_SECRET.into(),
        sender: "Kari".into(),
        channel: "team".into(),
        channel_name: "Team".into(),
        sequence: 7,
        parent_message_id: None,
        body: "Hei frå Sprøyt [[media:synthetic-token]]".into(),
        kind: "mention".into(),
    }
}

fn configured_sender() -> PushSender {
    PushSender::new(VAPID_PRIVATE, VAPID_PUBLIC, SUBJECT.into()).unwrap()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn verify_vapid(request: &CapturedRequest, endpoint: &str, before: u64) {
    let authorization = request.headers["authorization"].to_str().unwrap();
    let fields = authorization.strip_prefix("vapid ").unwrap();
    let (jwt, public_key) = fields.split_once(',').unwrap();
    let jwt = jwt.trim().strip_prefix("t=").unwrap();
    assert_eq!(public_key.trim().strip_prefix("k=").unwrap(), VAPID_PUBLIC);
    let (signed, sig) = jwt.rsplit_once('.').unwrap();
    let (header, claims) = signed.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&decode(header)).unwrap();
    let claims: Value = serde_json::from_slice(&decode(claims)).unwrap();
    assert_eq!(header["alg"], "ES256");
    assert_eq!(claims["sub"], SUBJECT);
    let origin = reqwest::Url::parse(endpoint)
        .unwrap()
        .origin()
        .ascii_serialization();
    assert_eq!(claims["aud"], origin);
    let issued = claims["iat"].as_u64().unwrap();
    assert!(issued >= before && issued <= unix_seconds());
    assert_eq!(claims["nbf"], issued);
    let expiry = claims["exp"].as_u64().unwrap();
    assert_eq!(expiry, issued + 43_200);
    let verifier = signature::UnparsedPublicKey::new(
        &signature::ECDSA_P256_SHA256_FIXED,
        decode(VAPID_PUBLIC),
    );
    verifier.verify(signed.as_bytes(), &decode(sig)).unwrap();
    assert!(
        verifier
            .verify(b"modified signing input", &decode(sig))
            .is_err()
    );
}

#[tokio::test]
async fn existing_vapid_key_delivers_an_independently_verified_and_decrypted_push() {
    let (endpoint, mut requests, server) = push_service(StatusCode::CREATED).await;
    let ua_private =
        agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &SystemRandom::new())
            .unwrap();
    let ua_public = ua_private.compute_public_key().unwrap();
    let job = push_job(endpoint.clone(), URL_SAFE_NO_PAD.encode(ua_public.as_ref()));
    let before = unix_seconds();
    assert!(matches!(
        configured_sender().deliver_to_loopback(&job).await,
        DeliveryResult::Delivered
    ));
    let request = requests.try_recv().unwrap();
    assert_eq!(request.method, Method::POST);
    assert_eq!(request.uri, "/push/device?token=fixture");
    assert_eq!(request.headers["ttl"], "43200");
    assert_eq!(request.headers["content-encoding"], "aes128gcm");
    assert_eq!(request.headers["content-type"], "application/octet-stream");
    assert_eq!(
        request.headers["content-length"],
        request.body.len().to_string()
    );
    assert!(request.body.len() <= 4096);
    verify_vapid(&request, &endpoint, before);

    let ephemeral_public = &request.body[21..86];
    assert_ne!(ephemeral_public, decode(VAPID_PUBLIC));
    let shared_secret = agreement::agree_ephemeral(
        ua_private,
        &agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, ephemeral_public),
        |secret| secret.to_vec(),
    )
    .unwrap();
    let plaintext = decrypt(
        &request.body,
        ua_public.as_ref(),
        &decode(AUTH_SECRET),
        &shared_secret,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&plaintext).unwrap(),
        json!({
            "web_push": 8030,
            "notification": {
                "title": "Kari omtalte deg",
                "body": "Hei frå Sprøyt",
                "navigate": format!("/?channel=team&message={}&sequence=7", job.message_id),
                "tag": format!("message-{}", job.message_id),
            },
        })
    );
    let mut tampered = request.body.to_vec();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(
        decrypt(
            &tampered,
            ua_public.as_ref(),
            &decode(AUTH_SECRET),
            &shared_secret
        )
        .is_err()
    );
    server.abort();
}

#[tokio::test]
async fn http_delivery_expires_only_gone_subscriptions_and_never_follows_redirects() {
    let sender = configured_sender();
    for status in [
        StatusCode::NOT_FOUND,
        StatusCode::GONE,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::FOUND,
    ] {
        let (endpoint, mut requests, server) = push_service(status).await;
        let result = sender
            .deliver_to_loopback(&push_job(endpoint, RFC_UA_PUBLIC.into()))
            .await;
        if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
            assert!(matches!(result, DeliveryResult::Expired), "{status}");
        } else {
            assert!(matches!(result, DeliveryResult::Retry(_)), "{status}");
        }
        assert_eq!(requests.try_recv().unwrap().uri.path(), "/push/device");
        assert!(matches!(
            requests.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        server.abort();
    }
}

#[tokio::test]
async fn corrupt_stored_subscription_keys_are_rejected_before_transport() {
    let (endpoint, mut requests, server) = push_service(StatusCode::CREATED).await;
    let sender = configured_sender();
    let mut job = push_job(endpoint, RFC_UA_PUBLIC.into());
    for auth in [
        "%".into(),
        URL_SAFE_NO_PAD.encode([0; 15]),
        URL_SAFE_NO_PAD.encode([0; 17]),
    ] {
        job.auth = auth;
        assert!(matches!(
            sender.deliver_to_loopback(&job).await,
            DeliveryResult::Retry(_)
        ));
    }
    job.auth = AUTH_SECRET.into();
    let mut off_curve = [0; 65];
    off_curve[0] = 4;
    let ua_public = decode(RFC_UA_PUBLIC);
    let mut compressed = [0; 33];
    compressed[0] = 2 | (ua_public[64] & 1);
    compressed[1..].copy_from_slice(&ua_public[1..33]);
    for p256dh in [
        "%".into(),
        URL_SAFE_NO_PAD.encode(compressed),
        URL_SAFE_NO_PAD.encode(off_curve),
    ] {
        job.p256dh = p256dh;
        assert!(matches!(
            sender.deliver_to_loopback(&job).await,
            DeliveryResult::Retry(_)
        ));
    }
    assert!(matches!(
        requests.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    server.abort();
}

#[test]
fn invalid_or_mismatched_vapid_configuration_fails_at_startup() {
    let zero = URL_SAFE_NO_PAD.encode([0; 32]);
    let short = URL_SAFE_NO_PAD.encode([1; 31]);
    for private in ["%", &zero, &short] {
        assert!(PushSender::new(private, VAPID_PUBLIC, SUBJECT.into()).is_err());
    }
    for public in ["%", RFC_UA_PUBLIC] {
        assert!(PushSender::new(VAPID_PRIVATE, public, SUBJECT.into()).is_err());
    }
    assert!(PushSender::new(VAPID_PRIVATE, VAPID_PUBLIC, "http://example.test".into()).is_err());
}

#[tokio::test]
async fn oversized_payload_is_rejected_before_transport() {
    let (endpoint, mut requests, server) = push_service(StatusCode::CREATED).await;
    let mut job = push_job(endpoint, RFC_UA_PUBLIC.into());
    // 3800 sender bytes fit the plaintext limit but leave too little room for
    // the RFC 8291 header, tag and padding.
    job.sender = "a".repeat(3800);
    assert!(matches!(
        configured_sender().deliver_to_loopback(&job).await,
        DeliveryResult::Retry(error) if error == "encrypted push payload is too large"
    ));
    job.sender = "a".repeat(4096);
    assert!(matches!(
        configured_sender().deliver_to_loopback(&job).await,
        DeliveryResult::Retry(_)
    ));
    assert!(matches!(
        requests.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    server.abort();
}

#[tokio::test]
async fn persisted_endpoint_is_validated_again_before_transport() {
    let (endpoint, mut requests, server) = push_service(StatusCode::CREATED).await;
    let result = configured_sender()
        .deliver(&push_job(endpoint, RFC_UA_PUBLIC.into()))
        .await;
    assert!(matches!(result, DeliveryResult::Retry(_)));
    assert!(matches!(
        requests.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    server.abort();
}
