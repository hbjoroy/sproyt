use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit, Payload}};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

fn seal(nonce: [u8; 12], plaintext: &[u8], aad: &[u8]) -> String {
    let cipher = Aes256Gcm::new_from_slice(&[1;32]).unwrap();
    let encrypted = cipher.encrypt(Nonce::from_slice(&nonce),Payload {msg:plaintext,aad}).unwrap();
    let mut wire=nonce.to_vec(); wire.extend(encrypted); URL_SAFE_NO_PAD.encode(wire)
}
fn main() {
    let cookie=seal([0,1,2,3,4,5,6,7,8,9,10,11],br#"{"refresh_token":"synthetic-browser","generation":7}"#,b"");
    let native=seal([12,13,14,15,16,17,18,19,20,21,22,23],br#"{"refresh_token":"synthetic-native","generation":7}"#,b"sproyt-native-v1|fixture-instance|credential:fixture");
    println!(r#"{{"generator":{{"aes-gcm":"0.10.3","base64":"0.22.1","wire":"URL_SAFE_NO_PAD(nonce12 || AES-256-GCM ciphertext || tag16)","provenance":"Generated using the pinned old libraries, not the application helper. Fixed synthetic test inputs only."}},"key":"{}","cookie":{{"nonce_hex":"000102030405060708090a0b","aad":"","plaintext":{{"refresh_token":"synthetic-browser","generation":7}},"ciphertext":"{}"}},"native":{{"nonce_hex":"0c0d0e0f1011121314151617","instance":"fixture-instance","purpose":"credential:fixture","aad":"sproyt-native-v1|fixture-instance|credential:fixture","plaintext":{{"refresh_token":"synthetic-native","generation":7}},"ciphertext":"{}"}}}}"#,URL_SAFE_NO_PAD.encode([1;32]),cookie,native);
}
