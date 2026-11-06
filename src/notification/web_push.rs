use std::{any::Any, sync::OnceLock, time::Duration};

use aes_gcm::{
    Aes128Gcm,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ece::{Cryptographer, EcKeyComponents, Error as EceError, LocalKeyPair, RemotePublicKey};
use hkdf::Hkdf;
use p256::{
    PublicKey, SecretKey, ecdh,
    ecdsa::{Signature, SigningKey, signature::Signer},
    elliptic_curve::{Generate, sec1::ToSec1Point},
};
use reqwest::{Url, header};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::{
    DeliveryResult, PushJob, notification_body, notification_path, storage, validate_endpoint,
};
use crate::domain::RepositoryError;

const TTL_SECONDS: i64 = 12 * 60 * 60;
const MAX_PUSH_BYTES: usize = 4096;
const AUTH_SECRET_BYTES: usize = 16;

pub(super) struct PushSender {
    key: SigningKey,
    public_key: String,
    subject: String,
    client: reqwest::Client,
}

impl PushSender {
    pub(super) fn new(
        private_key: &str,
        public_key: &str,
        subject: String,
    ) -> Result<Self, RepositoryError> {
        let private_key = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(private_key)
                .map_err(|_| storage("SPROYT_VAPID_PRIVATE_KEY is not base64url"))?,
        );
        if private_key.len() != 32
            || !(subject.starts_with("mailto:") || subject.starts_with("https://"))
        {
            return Err(storage("invalid VAPID configuration"));
        }
        let key = SigningKey::from_slice(&private_key)
            .map_err(|_| storage("invalid VAPID private key"))?;
        let configured_public = URL_SAFE_NO_PAD
            .decode(public_key)
            .map_err(|_| storage("SPROYT_VAPID_PUBLIC_KEY is not base64url"))?;
        if key.verifying_key().to_sec1_point(false).as_bytes() != configured_public {
            return Err(storage("VAPID public key does not match the private key"));
        }
        initialize_ece().map_err(storage)?;
        Ok(Self {
            key,
            public_key: public_key.to_owned(),
            subject,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(storage)?,
        })
    }

    fn authorization(&self, endpoint: &Url) -> Result<header::HeaderValue, &'static str> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256","typ":"JWT"}"#);
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "aud": endpoint.origin().ascii_serialization(),
            "exp": now + TTL_SECONDS,
            "iat": now,
            "nbf": now,
            "sub": self.subject,
        });
        let input = format!("{header}.{}", URL_SAFE_NO_PAD.encode(claims.to_string()));
        let signature: Signature = self
            .key
            .try_sign(input.as_bytes())
            .map_err(|_| "VAPID signing failed")?;
        let jwt = format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()));
        let mut authorization =
            header::HeaderValue::from_str(&format!("vapid t={jwt}, k={}", self.public_key))
                .map_err(|_| "VAPID authorization could not be encoded")?;
        authorization.set_sensitive(true);
        Ok(authorization)
    }

    fn request(
        &self,
        job: &PushJob,
        endpoint: Url,
    ) -> Result<reqwest::RequestBuilder, &'static str> {
        let peer = URL_SAFE_NO_PAD
            .decode(&job.p256dh)
            .map_err(|_| "invalid push public key")?;
        let auth = URL_SAFE_NO_PAD
            .decode(&job.auth)
            .map_err(|_| "invalid push auth secret")?;
        let title = match job.kind.as_str() {
            "mention" => format!("{} omtalte deg", job.sender),
            "thread_reply" => format!("Svar i tråd frå {}", job.sender),
            "channel_message" => format!("{} i #{}", job.sender, job.channel_name),
            _ => format!("Melding frå {}", job.sender),
        };
        let payload = serde_json::to_vec(&serde_json::json!({
            "web_push": 8030,
            "notification": {
                "title": title,
                "body": notification_body(&job.body),
                "navigate": notification_path(job),
                "tag": format!("message-{}", job.message_id),
            },
        }))
        .map_err(|_| "push payload could not be encoded")?;
        let body = encrypt(&peer, &auth, &payload)?;
        let authorization = self.authorization(&endpoint)?;
        Ok(self
            .client
            .post(endpoint)
            .header("TTL", TTL_SECONDS)
            .header(header::CONTENT_ENCODING, "aes128gcm")
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, body.len())
            .header(header::AUTHORIZATION, authorization)
            .body(body))
    }

    pub(super) async fn deliver(&self, job: &PushJob) -> DeliveryResult {
        let endpoint = match validate_endpoint(&job.endpoint) {
            Ok(endpoint) => endpoint,
            Err(_) => return DeliveryResult::Retry("invalid push endpoint".into()),
        };
        self.send(job, endpoint).await
    }

    #[cfg(test)]
    pub(super) async fn deliver_to_loopback(&self, job: &PushJob) -> DeliveryResult {
        let endpoint = Url::parse(&job.endpoint).expect("fixture URL");
        assert_eq!(endpoint.scheme(), "http");
        assert_eq!(endpoint.host_str(), Some("127.0.0.1"));
        self.send(job, endpoint).await
    }

    async fn send(&self, job: &PushJob, endpoint: Url) -> DeliveryResult {
        let request = match self.request(job, endpoint) {
            Ok(request) => request,
            Err(error) => return DeliveryResult::Retry(error.into()),
        };
        match request.send().await {
            Ok(response) if response.status().is_success() => DeliveryResult::Delivered,
            Ok(response) if matches!(response.status().as_u16(), 404 | 410) => {
                DeliveryResult::Expired
            }
            Ok(response) => {
                DeliveryResult::Retry(format!("push service returned {}", response.status()))
            }
            Err(_) => DeliveryResult::Retry("push transport unavailable".into()),
        }
    }
}

pub(super) fn valid_public_key(raw: &[u8]) -> bool {
    parse_public_key(raw).is_ok()
}

fn parse_public_key(raw: &[u8]) -> ece::Result<PublicKey> {
    if raw.len() != 65 || raw[0] != 4 {
        return Err(EceError::InvalidKeyLength);
    }
    PublicKey::from_sec1_bytes(raw).map_err(|_| EceError::CryptoError)
}

fn initialize_ece() -> Result<(), &'static str> {
    static BACKEND: P256Cryptographer = P256Cryptographer;
    static INIT: OnceLock<Result<(), &'static str>> = OnceLock::new();
    *INIT.get_or_init(|| {
        ece::crypto::set_cryptographer(&BACKEND)
            .map_err(|_| "push crypto backend already initialized")
    })
}

fn encrypt(peer: &[u8], auth: &[u8], payload: &[u8]) -> Result<Vec<u8>, &'static str> {
    if auth.len() != AUTH_SECRET_BYTES {
        return Err("invalid push auth secret");
    }
    if payload.len() > MAX_PUSH_BYTES {
        return Err("push payload is too large");
    }
    initialize_ece()?;
    let body = ece::encrypt(peer, auth, payload).map_err(|_| "push payload encryption failed")?;
    // RFC 8291 uses one record. This limit also excludes Mozilla ece's
    // multiple-record output with its default 4096-byte record size.
    if body.len() > MAX_PUSH_BYTES {
        return Err("encrypted push payload is too large");
    }
    Ok(body)
}

// Mozilla ece owns the Web Push encryption protocol and record framing. This
// adapter delegates its cryptographic primitives to their library APIs.
struct P256Cryptographer;
struct Remote(PublicKey);
struct Local(SecretKey);

impl RemotePublicKey for Remote {
    fn as_raw(&self) -> ece::Result<Vec<u8>> {
        Ok(self.0.to_sec1_point(false).as_bytes().to_vec())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl LocalKeyPair for Local {
    fn pub_as_raw(&self) -> ece::Result<Vec<u8>> {
        Ok(self.0.public_key().to_sec1_point(false).as_bytes().to_vec())
    }

    fn raw_components(&self) -> ece::Result<EcKeyComponents> {
        Ok(EcKeyComponents::new(
            self.0.to_bytes().to_vec(),
            self.pub_as_raw()?,
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Cryptographer for P256Cryptographer {
    fn generate_ephemeral_keypair(&self) -> ece::Result<Box<dyn LocalKeyPair>> {
        let key = SecretKey::try_generate().map_err(|_| EceError::CryptoError)?;
        Ok(Box::new(Local(key)))
    }

    fn import_key_pair(&self, components: &EcKeyComponents) -> ece::Result<Box<dyn LocalKeyPair>> {
        let secret = components.private_key();
        if secret.len() != 32 {
            return Err(EceError::InvalidKeyLength);
        }
        let key = SecretKey::from_slice(secret).map_err(|_| EceError::CryptoError)?;
        let local = Local(key);
        if local.pub_as_raw()? != components.public_key() {
            return Err(EceError::CryptoError);
        }
        Ok(Box::new(local))
    }

    fn import_public_key(&self, raw: &[u8]) -> ece::Result<Box<dyn RemotePublicKey>> {
        Ok(Box::new(Remote(parse_public_key(raw)?)))
    }

    fn compute_ecdh_secret(
        &self,
        remote: &dyn RemotePublicKey,
        local: &dyn LocalKeyPair,
    ) -> ece::Result<Vec<u8>> {
        let remote = remote
            .as_any()
            .downcast_ref::<Remote>()
            .ok_or(EceError::CryptoError)?;
        let local = local
            .as_any()
            .downcast_ref::<Local>()
            .ok_or(EceError::CryptoError)?;
        let secret = ecdh::diffie_hellman(local.0.to_nonzero_scalar(), remote.0.as_affine());
        Ok(secret.raw_secret_bytes().to_vec())
    }

    fn hkdf_sha256(
        &self,
        salt: &[u8],
        secret: &[u8],
        info: &[u8],
        len: usize,
    ) -> ece::Result<Vec<u8>> {
        let mut output = vec![0; len];
        Hkdf::<Sha256>::new(Some(salt), secret)
            .expand(info, &mut output)
            .map_err(|_| EceError::CryptoError)?;
        Ok(output)
    }

    fn aes_gcm_128_encrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> ece::Result<Vec<u8>> {
        let cipher = Aes128Gcm::new_from_slice(key).map_err(|_| EceError::InvalidKeyLength)?;
        let nonce: [u8; 12] = iv.try_into().map_err(|_| EceError::CryptoError)?;
        cipher
            .encrypt(&nonce.into(), data)
            .map_err(|_| EceError::CryptoError)
    }

    fn aes_gcm_128_decrypt(&self, key: &[u8], iv: &[u8], data: &[u8]) -> ece::Result<Vec<u8>> {
        let cipher = Aes128Gcm::new_from_slice(key).map_err(|_| EceError::InvalidKeyLength)?;
        let nonce: [u8; 12] = iv.try_into().map_err(|_| EceError::CryptoError)?;
        cipher
            .decrypt(&nonce.into(), data)
            .map_err(|_| EceError::CryptoError)
    }

    fn random_bytes(&self, dest: &mut [u8]) -> ece::Result<()> {
        getrandom::fill(dest).map_err(|_| EceError::CryptoError)
    }
}
