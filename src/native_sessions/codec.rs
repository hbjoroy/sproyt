use super::{NativeError, Result};
use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit, Payload},
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
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|_| NativeError::Unavailable)?;
        let aad = self.aad(purpose);
        let encrypted = self
            .primary
            .encrypt(
                &nonce.into(),
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
        let nonce: [u8; 12] = bytes[..12]
            .try_into()
            .map_err(|_| NativeError::Unauthorized)?;
        self.readers
            .iter()
            .find_map(|cipher| {
                cipher
                    .decrypt(
                        &nonce.into(),
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

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn native_reads_old_aes_ciphertext_with_exact_instance_purpose_and_rotation() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/fixtures/session-codecs/aes-gcm-0.10.json"
        ))
        .unwrap();
        let old = fixture["key"].as_str().unwrap();
        let instance = fixture["native"]["instance"].as_str().unwrap();
        let purpose = fixture["native"]["purpose"].as_str().unwrap();
        let value = fixture["native"]["ciphertext"].as_str().unwrap();
        let expected = &fixture["native"]["plaintext"];
        let codec = Codec::new(old, &[], instance).unwrap();
        assert_eq!(
            &codec.open::<serde_json::Value>(purpose, value).unwrap(),
            expected
        );
        let new = URL_SAFE_NO_PAD.encode([2; 32]);
        let rotated = Codec::new(&new, &[old.to_owned()], instance).unwrap();
        assert_eq!(
            &rotated.open::<serde_json::Value>(purpose, value).unwrap(),
            expected
        );
        assert!(
            Codec::new(&new, &[], instance)
                .unwrap()
                .open::<serde_json::Value>(purpose, value)
                .is_err()
        );
        assert!(
            Codec::new(old, &[], "another-instance")
                .unwrap()
                .open::<serde_json::Value>(purpose, value)
                .is_err()
        );
        assert!(
            codec
                .open::<serde_json::Value>("credential:another", value)
                .is_err()
        );
        assert!(
            codec
                .open::<serde_json::Value>("receipt:fixture", value)
                .is_err()
        );
        let bytes = URL_SAFE_NO_PAD.decode(value).unwrap();
        assert_eq!(
            &bytes[..12],
            &[12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23]
        );
        assert_eq!(
            bytes.len(),
            12 + br#"{"refresh_token":"synthetic-native","generation":7}"#.len() + 16
        );
        for index in [0, bytes.len() - 1] {
            let mut corrupt = bytes.clone();
            corrupt[index] ^= 1;
            assert!(
                codec
                    .open::<serde_json::Value>(purpose, &URL_SAFE_NO_PAD.encode(corrupt))
                    .is_err()
            );
        }
        assert!(
            codec
                .open::<serde_json::Value>(
                    purpose,
                    fixture["cookie"]["ciphertext"].as_str().unwrap()
                )
                .is_err()
        );
    }
}
