# Legacy session ciphertext fixtures

`aes-gcm-0.10.json` was emitted on 2026-11-07 by the pinned `aes-gcm =0.10.3`
and `base64 =0.22.1` libraries, before upgrading the application dependencies.
The keys, nonces and JSON payloads are synthetic public test inputs. The fixture
proves the historical library wire format; it does not claim that an old
application helper was executed.

Reproduce in an isolated temporary crate: copy `legacy-generator.Cargo.toml`
to `Cargo.toml`, `legacy-generator.Cargo.lock` to `Cargo.lock`, and
`legacy-generator.rs` to `src/main.rs`, then run `cargo run --locked`. On the
Windows development machine use the mandated DevCache wrapper from that crate.
Fixed nonces are for this one-purpose deterministic fixture generator only.

The production codecs keep `nonce[12] || ciphertext || tag[16]` encoded using
URL-safe base64 without padding. Browser cookies use empty AAD. Native secrets
use exactly `sproyt-native-v1|instance|purpose`. Tests read the immutable old
ciphertexts using the current codecs, including a previous rotation key, and
reject a wrong key, mutated nonce/tag, another native purpose/instance and
crossing the browser/native authentication boundary.
