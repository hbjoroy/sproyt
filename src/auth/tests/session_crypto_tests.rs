use super::*;

// Immutable outputs from the old pinned AES/base64 libraries, not this reader.
fn legacy() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tools/fixtures/session-codecs/aes-gcm-0.10.json"
    ))
    .unwrap()
}

#[test]
fn browser_reads_old_aes_ciphertext_after_upgrade_and_key_rotation() {
    let fixture = legacy();
    let old = fixture["key"].as_str().unwrap();
    let ciphertext = fixture["cookie"]["ciphertext"].as_str().unwrap();
    let expected = &fixture["cookie"]["plaintext"];
    let codec = CookieCodec::new(old, &[]).unwrap();
    assert_eq!(
        &codec.open::<serde_json::Value>(ciphertext).unwrap(),
        expected
    );
    let new = URL_SAFE_NO_PAD.encode([2; 32]);
    let rotated = CookieCodec::new(&new, &[old.to_owned()]).unwrap();
    assert_eq!(
        &rotated.open::<serde_json::Value>(ciphertext).unwrap(),
        expected
    );
    assert!(
        CookieCodec::new(&new, &[])
            .unwrap()
            .open::<serde_json::Value>(ciphertext)
            .is_err()
    );
    let bytes = URL_SAFE_NO_PAD.decode(ciphertext).unwrap();
    assert_eq!(&bytes[..12], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    assert_eq!(
        bytes.len(),
        12 + br#"{"refresh_token":"synthetic-browser","generation":7}"#.len() + 16
    );
    for index in [0, bytes.len() - 1] {
        let mut corrupt = bytes.clone();
        corrupt[index] ^= 1;
        assert!(
            codec
                .open::<serde_json::Value>(&URL_SAFE_NO_PAD.encode(corrupt))
                .is_err()
        );
    }
    // A native payload authenticated with AAD cannot become an unbound cookie.
    assert!(
        codec
            .open::<serde_json::Value>(fixture["native"]["ciphertext"].as_str().unwrap())
            .is_err()
    );
}
