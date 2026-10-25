# N01a: isolert innloggingsprøve

Denne prøva skal bevise serverformidla OIDC, separat app-prov og eingongs
innløysing med retry. Berre syntetiske identitetar og prøvelegitimasjon blir
brukte. Ho kan ikkje kontakte Authentik eller gi tilgang til Sprøyt-API.

## Avgrensa gjennomføring

1. Rust-serveren og ein syntetisk RS256-OIDC-provider bind berre IPv4-loopback.
2. Ein .NET-klient eig returlyttaren før serveren startar. Destinasjonen blir
   konfigurert ved oppstart, ikkje teken frå eit vilkårleg retur-URL i request.
3. Eit delt CookieContainer simulerer same eksterne nettlesar for to samtidige
   forsøk. Kvar transaksjon har eigen browsercookie, OIDC-state, nonce og
   provider-PKCE. App-prov og innløysings-ID er separate.
4. Verifiserte claims blir omsette med same identitetsfunksjon som nettappen.
   Lik e-post er aldri kopling. Ein forventa konto er berre ei innsnevring.
5. SQLite serialiserer eingongsinnløysing og nøyaktig retry-resultat atomisk.
   Returen har berre kortliva kode og transaksjon, aldri prøvelegitimasjon.
6. CI køyrer same HTTP-prøve på Windows og Linux. Dette er ikkje ei fysisk
   Android-/iOS-prøve, verkeleg systemnettlesarinnlogging eller passnøkkelaksept.

Ingen nye produksjonsruter, appøkter, Authentik-policyar eller migreringar blir
aktiverte. Første snitt stoppar etter review av desse bevisa. N01 står framleis
ope for isolert Authentik, passnøkkel, providerrotasjon og recovery.

## Intern HTTP-kontrakt for prøva

Rust startast med `--database <ny-sti> --callback-uri <eksakt-loopback-retur>`.
Han skriv éi `N01_READY {"address":"http://127.0.0.1:PORT","issuer":".../provider"}`
til stdout etter at OIDC-discovery er klar. Ikkje logg callback-URL-ar,
request-/responsebodyar, kode, prov eller prøvelegitimasjon.

- `POST /probe/start`: JSON `{challenge, platform:"windows", environment:"n01-lab", destination_id:"windows-loopback", expected_user_id?:UUID}`.
  Svar `{transaction_id, authorization_url}`. Challenge er S256, base64url utan padding, 43 teikn.
- `GET /probe/browser/{transaction_id}` byrjar OIDC og set ein transaksjonsunik cookie.
- `/provider/authorize` er ein syntetisk fixture. Nettlesarsimulatoren set
  `X-Probe-Account: alice|bob` berre her. Begge kontoane har same e-post.
  Valfri `X-Probe-Fault: nonce|audience|issuer|signature` lagar ugyldig assertion.
- `GET /probe/callback?code=...&state=...`: verifiserer cookie, state og OIDC,
  og returnerer 302 til den registrerte destinasjonen med `transaction_id` og `code`.
- `POST /probe/exchange`: JSON `{transaction_id, code, verifier, redemption_id, environment:"n01-lab", destination_id:"windows-loopback"}`.
  Svar `{kind:"non_api_fixture", user_id, session_family, credential}`.
  Same kode/prov/innløysings-ID gir nøyaktig same resultat innan retryvindauget;
  ny ID, feil binding eller replay blir avvist utan ny familie.
  `X-Probe-Drop-Ack: 1` simulerer broten respons etter commit i denne fixture-serveren.
- `POST /fixture/expire`: JSON `{transaction_id, scope:"login"|"handoff"|"receipt"}`.
  Testkontroll for utløp utan lange ventetider. Finst berre i dette laboratoriet.
- `GET /probe/status`: berre teljingar `{attempts, session_families}`.

JSON-/formbodyar er avgrensa til 8 KiB, forsøkslageret til 32 transaksjonar. Feil gir
4xx med fast, hemmelegheitsfri JSON. Konfigurasjon og retur-URL må vere eksakt
loopback; ingen proxy, redirect-following eller vilkårleg upstream blir brukt.

SQLite inneheld berre fiktive prov/token. Produksjon treng kryptert legitimasjon,
retry-kvittering og OS-nøkkellager etter ADR N01. Denne databasen er ikkje ei
produksjonsmigrering og blir aldri opna av den vanlege Windows-prøva.

## Køyring

```powershell
./native/probes/run-identity-probe.ps1 -ArtifactsDirectory S:/Build/sproyt-native-identity
```

Krev Rust-toolchain frå repoet og .NET SDK 10.0.301 med ASP.NET Core-runtime.
Runneren byggjer i vald artefaktmappe, gir kvar prøve ein ny database og skriv
`runs/<id>/receipt.json`. Han slettar ingen tidlegare prøver. CI bruker sin
eigen midlertidige artefaktmappe på Windows og Linux.

Den komplette HTTP-prøva er køyrd på lokal Windows ARM64. Ho verifiserer også
manglande/feil browsercookie, kryssa callback før originalsuksess, broten
ACK-respons etter SQLite-commit og to samtidige innløysingar med same ID.
`UserId::from_oidc` er felles med nettinnlogginga og bevarer nøyaktig det gamle
UUIDv5-grunnlaget `issuer:subject`; ingen normalisering eller e-postkopling.

Providerassertion blir verifisert av `openidconnect`, same bibliotek som
nettappen. Provideren i prøva genererer syntetiske signaturar med `rsa`.
App-prov og provider-PKCE er to separate verdiar. Returlyttaren er eigd før
serveren får den registrerte destinasjonen, og nettlesarsimulatoren har eit
anna HttpClient-/cookie-omfang enn appen. Vanleg systemnettlesar, OS-retur og
OS-nøkkellager er ikkje prøvde her.

Callback-reservasjonen blir frigjord ved feil eller kansellering. Berre éin
callback kan vere under verifisering per forsøk; ein kryssa kode skal avvisast
utan at originale A/B-forsøk blir øydelagde. Ein brokerprosess som døyr midt
under providerutveksling, er ikkje gjenopptakingsbevist i dette snittet.

Referansar: [native OAuth](https://www.rfc-editor.org/rfc/rfc8252),
[S256/PKCE](https://www.rfc-editor.org/rfc/rfc7636),
[identitets-ADR](../../../docs/native-client-identity-adr.md).
