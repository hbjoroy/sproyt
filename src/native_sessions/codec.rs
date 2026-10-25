use super::{NativeError, Result};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Serialize, de::DeserializeOwned};
pub(super) struct Codec {
    primary: Aes256Gcm,
    readers: Vec<Aes256Gcm>,
    instance: String,
}
impl Codec {
    pub fn new(key: &str, previous: &[String], instance: &str) -> Result<Self> {
        fn cipher(key: &str) -> Result<Aes256Gcm> {
            let bytes = URL_SAFE_NO_PAD
                .decode(key)
                .map_err(|_| NativeError::Configuration)?;
            if bytes.len() != 32 {
                return Err(NativeError::Configuration);
            }
            Aes256Gcm::new_from_slice(&bytes).map_err(|_| NativeError::Configuration)
        }
        Ok(Self {
            primary: cipher(key)?,
            readers: std::iter::once(key)
                .chain(previous.iter().map(String::as_str))
                .map(cipher)
                .collect::<Result<_>>()?,
            instance: instance.into(),
        })
    }
    fn aad(&self, purpose: &str) -> Vec<u8> {
        format!("sproyt-native-v1|{}|{purpose}", self.instance).into_bytes()
    }
    pub fn seal<T: Serialize>(&self, purpose: &str, value: &T) -> Result<String> {
        let raw = serde_json::to_vec(value).map_err(|_| NativeError::Unavailable)?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let aad = self.aad(purpose);
        let encrypted = self
            .primary
            .encrypt(
                &nonce,
                Payload {
                    msg: &raw,
                    aad: &aad,
                },
            )
            .map_err(|_| NativeError::Unavailable)?;
        let mut bytes = nonce.to_vec();
        bytes.extend(encrypted);
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }
    pub fn open<T: DeserializeOwned>(&self, purpose: &str, value: &str) -> Result<T> {
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| NativeError::Unauthorized)?;
        if bytes.len() <= 12 || bytes.len() > 65536 {
            return Err(NativeError::Unauthorized);
        }
        let aad = self.aad(purpose);
        self.readers
            .iter()
            .find_map(|cipher| {
                cipher
                    .decrypt(
                        Nonce::from_slice(&bytes[..12]),
                        Payload {
                            msg: &bytes[12..],
                            aad: &aad,
                        },
                    )
                    .ok()
            })
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .ok_or(NativeError::Unauthorized)
    }
}
