# Security exceptions

Dependency advisories fail CI unless an entry below explicitly accepts the
bounded risk. An exception is not evidence that a dependency is generally
safe; it records why the vulnerable operation is not reachable in Sproyt and
when the exception must be removed.

## RUSTSEC-2023-0071: RSA timing side channel

- Severity: medium (CVSS 5.9).
- Dependency path: `sproyt -> openidconnect -> rsa 0.9.10`.
- Upstream status: no fixed `rsa` release is available as of 2026-07-13.
- Production use: OIDC ID-token signatures are verified with provider public
  keys. Sproyt does not perform RSA private-key decryption or expose a private
  RSA operation to attacker-controlled ciphertext.
- Test-only use: the direct development dependency signs synthetic ID tokens
  with an ephemeral test key; no production secret is involved.
- Acceptance: the Marvin key-recovery attack requires timing observations of
  a private RSA operation, which is absent from the production path.
- Compensating controls: CI ignores only this advisory ID; all other RustSec
  findings still fail. OIDC issuer, algorithms, audience, nonce, and token
  lifetime remain validated.
- Owner: Sproyt release owner.
- Removal condition: upgrade and remove the ignore as soon as `openidconnect`
  can use a fixed RSA implementation, or immediately if Sproyt adds any RSA
  private-key operation.

Review this exception at every production release and retain the `cargo audit`
output with the release evidence.
