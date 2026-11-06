# Security exceptions

An exception accepts a specific dependency advisory for a documented use.
Release evidence includes the audit output and the advisory database revision.

## RUSTSEC-2023-0071

**Scope.** This exception applies to `rsa 0.9.10` for OIDC public-key
verification and synthetic test keys. OpenID Connect verifies provider ID-token
signatures with public JWKs. The normal dependency graph also reaches `rsa`
through `web-push-native → jwt-simple → superboring`. Sproyt uses ES256 for VAPID.
The optional SQLx MySQL path appears in the lockfile and is absent from the
normal graph. The direct `rsa` dependency is test-only. The advisory reports
possible private-key recovery from timing
observations and lists no patched versions. RustCrypto merged implicit
rejection for PKCS#1 v1.5 decryption on 1 November 2026. The later published
`0.10.0-rc.21` contains that change. The locked `0.9.10` predates it.
[RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071.html),
[upstream fix](https://github.com/RustCrypto/RSA/pull/749),
[release ancestry](https://github.com/RustCrypto/RSA/compare/0eb430ca6d27ec52a02e8bc23be878d41fcfef41...v0.10.0-rc.21),
[published prerelease](https://crates.io/crates/rsa/0.10.0-rc.21).

**GitHub App signing.** The previous assessment omitted a private operation.
With the App ID and PEM path configured, the GitHub adapter loads the
server-held key and signs installation-token JWTs. An authenticated
export request first records an authorized work-item decision. The background
worker scans up to five eligible receipts per iteration, claims a lease and
requests the installation token. The JWT signs a fixed RS256 header,
the configured App ID and server-generated issuance and expiry times.
The requester supplies the issue title and body and may influence when
a receipt is queued. Those inputs and the installation and repository IDs
are outside the signed JWT. Only the fixed GitHub API receives the JWT.
The requester can observe asynchronous task status and
coarse completion timing, which includes worker scheduling, database work
and outbound network calls. The HTTP admission response does not time the
signature operation directly. The review found no chosen-ciphertext endpoint
or demonstrated key recovery for this path. Indirect observability still
warranted moving the private operation out of the affected crate.
[GitHub adapter](../src/github.rs),
[receipt processing](../src/work_items/github_export.rs),
[GitHub App JWT contract](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app).

The adapter now uses the existing `ring 0.17.14` dependency for RS256 signing.
It uses constant-time private exponentiation primitives, with the limitations
described by ring. The application accepts conventional unencrypted two-prime
RSA keys in PKCS#1 or PKCS#8 PEM with a 2048–4096-bit modulus, subject to ring's
key validation. Invalid or unsupported configured keys fail at startup. The
JWT claims, GitHub destination and scoped installation-token exchange retain
their existing contract. Verification uses synthetic keys and a local GitHub
API mock. A live credential was outside this review.
[ring implementation](https://github.com/briansmith/ring/blob/2723abbca9e83347d82b056d5b239c6604f786df/src/rsa/keypair.rs),
[ring side-channel notes](https://github.com/briansmith/ring/blob/2723abbca9e83347d82b056d5b239c6604f786df/SIDE-CHANNELS.md),
[signing tests](../src/github/signing_tests.rs).

**Owner and review.** Security/release owns the exception with the GitHub
capability owner. CI ignores this one advisory ID. Review the exception at
each production release and if a reachable
`rsa` private operation, key format, signer or dependency path changes.
Remove the ignore when the remaining dependency paths use an advisory-listed
fixed implementation or the affected version leaves the audited graph.
Reassess promptly if upstream status or evidence of exploitation changes.

## Advisory database and release gate

On 6 November 2026 the release review retired the August advisory database pin,
which was 89 days old. CI now pins `cargo-audit 0.22.2`, verifies the restored
binary version, fetches the current RustSec database and logs its commit and
timestamp. Failure to fetch or parse the database fails the audit.
Security/release owns the audit tool and any future temporary database pin.
A new pin requires a documented reason and a condition for removal.
Review the tool version at each release and whenever upstream advisory data
needs a newer parser or a tool security/correctness fix is published.
[CI audit](../.github/workflows/ci.yml).

The unfiltered audit at RustSec revision `26fd2d43` found 14 advisories with
the previous lockfile. After compatible dependency updates, it reports
RUSTSEC-2023-0071 and RUSTSEC-2026-0387, with no informational warnings.
The second finding affects `crypto-bigint 0.5.5` through `elliptic-curve`
and `p256`. Web Push uses that path for private VAPID signing and ephemeral
P-256 key agreement. The advisory lists a fix in `crypto-bigint 0.7.6` or
later, outside the current dependency range. The review did not establish a
practical attack on this deployment. RUSTSEC-2026-0387 has no exception and
blocks Sprøyt application publication while it remains in the audited graph.
Security/release and the Web Push owner track remediation in #372.
[RustSec advisory for crypto-bigint](https://rustsec.org/advisories/RUSTSEC-2026-0387.html),
[remediation issue](https://github.com/hbjoroy/sproyt/issues/372).
