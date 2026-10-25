# N01e: faktisk plattformpassnøkkel i systemnettlesaren

Dato: 25. oktober 2026. Byggjer på [N01c](README.md) og [N01d](REVOCATION.md).
Prøva bruker det same pinna Authentik-imaget, med eiga database og syntetisk
Alice. Produksjonsprovider, brukarar og innlogging blir ikkje endra.

## Resultat og testgrense

Alice fekk registrert ein plattformpassnøkkel gjennom Authentik sitt vanlege
nettlesargrensesnitt i Edge på Windows ARM64. Ei separat innlogging utan
passord-stage fullførte deretter OIDC og OS-verna app-handoff. Den kanoniske
brukar-ID-en var den same som i baselinen før registreringa. Serversida
stadfesta den konkrete WebAuthn-bruken, og gjenbruk av beviset vart avvist.

Dette er systemnettlesar + operativsystem/nettlesar sin passnøkkeldialog, ikkje
direkte Android Credential Manager eller Apple AuthenticationServices. Observert
autentikatortype var Microsoft Password Manager. Prøva lovar ikkje at nøkkelen
er bunden til TPM, eller at han ikkje kan synkroniserast. Registreringa og
innlogginga kravde brukarverifisering; registrering kravde plattformautentikator
og discoverable credential. Ingen virtuell nøkkel blir brukt som hardwareaksept.

Baseline og passnøkkel har same **localhost-issuer**. Dei har ikkje same interne
ID som den tidlegare 127.0.0.1-labben, sidan issuer inngår i identitetsrommet.
Dette er heller ikkje aksept for ein eksisterande produksjonskonto.

## Kvifor ein separat localhost-variant

WebAuthn bruker RP-ID `localhost` og origin `http://localhost:19090`. Pinna
Authentik-kode avleier desse frå request-host/origin. Browseren gav faktisk
registrering og verifisert innlogging på dette opphavet. Ingen browserflagg,
sertifikatåtvaring eller endring i maskina sitt tillitslager blir brukt.

`start-lab.ps1 -Passkey` lagar ein ny database/secret/provider for denne
varianten. Rust-adapteren tillèt berre det eksakte ekstra localhost-opphavet;
localhost på andre portar og alias blir avviste. Backend-DNS for localhost er
pinna til 127.0.0.1. Broker, app-retur og publisert containerport er framleis
bundne til IPv4-loopback. N01c/d held sitt eige 127.0.0.1-opphav.

## Flyt og beviskjede

1. Fullfør vanleg Authentik-innlogging med syntetisk Alice og lagre ein
   DPAPI-verna handoff-baseline. Eksisterande nettlesarøkt kan brukast i denne
   identitetssamanlikninga; dette er ikkje bevis for ny passordinnskriving.
2. Opne `http://localhost:19090/if/flow/n01e-passkey-setup/`. Brukaren opprettar
   sjølv testpassnøkkelen i systemdialogen. Setup-flow krev autentisert brukar,
   UV `required`, resident key `required` og attachment `platform`.
3. Etter bygging, rett før det separate innloggingsforsøket, lag ein ny
   prøve-ID og ti-minutts snapshot. Eit servereigd, eingongs prøvebevis blir
   lagra på den syntetiske brukaren. Alle Alice-browserøkter blir sletta via
   admin-API, og providerens authentication-flow blir sett til eiga
   WebAuthn-validering → user-login. Ho har ingen password-stage eller annan
   konfigurator/fallback, `not_configured_action=deny`, terskel null og UV
   `required`.
4. Fullfør den ferske innlogginga gjennom systemnettlesaren. Native klient
   samanliknar issuer/subject og kanonisk ID med baselinen, og les resultatet
   tilbake frå den varige DPAPI-journalen.
5. Broker gir SHA-256-fingeravtrykk av **den eksakte transaksjonen sitt**
   access-token. Berre den private kandidatkvitteringa bruker dette. Fixturen
   finn det eksakte provider/Alice-grantet, følgjer grantets autentiserte økt
   til login-eventen, krev ny event etter prepare og identisk `auth_time`, og
   kontrollerer WebAuthn-metode og referanse til riktig device som er brukt
   etter snapshoten. Ei anna fane sin globale device-timestamp er ikkje nok.
6. Under radlås blir servermarkøren kontrollert og brukt opp. Ein ny
   verifisering av same kandidat og snapshot må bli avvist. Først då får den
   kombinerte kvitteringa `passkeyVerified=true`. Fingeravtrykket blir fjerna
   frå den offentlege kvitteringa; token, cookie, credential-ID og private
   nøklar kjem aldri dit.

Servermarkøren er ein lokal testfixture, **ikkje** den framtidige recovery-
integreringa eller ei tillitsfull produksjonsattribute. Native handoff åleine
har alltid `passkeyVerified=false`.

## Observert ekstra autentiseringsrunde

`prompt=login` i denne Authentik-versjonen gav ein ekstra runde etter første
anonyme innlogging. Første WebAuthn-innlogging vart godteken; seinare svar i
ekstrarunden vart avviste fordi UV-flagget mangla. Dette vart ei løkke i prøva.
Vi har ikkje stadfesta om årsaka ligg i nettlesar, passnøkkelprovider eller
Authentik, og har ikkje retta noko i desse produkta.

Den isolerte prøva bruker derfor ikkje forced prompt. Fersk øktsletting og den
konkrete grant-/event-/devicebindinga ovanfor beviser ny autentisering. UV-
kravet er halde uendra. Dette er ei prøveavgjerd, ikkje den endelege native
reauth-policyen i produksjon.

## Køyring og CI

```powershell
./native/probes/authentik/start-lab.ps1 -ArtifactsDirectory S:/Build/sproyt-authentik-passkey -Passkey
./native/probes/authentik/run-passkey.ps1 -RunDirectory <labmappe> -Phase Contract
./native/probes/authentik/run-passkey.ps1 -RunDirectory <labmappe> -Phase PasswordBaseline
# Brukaren registrerer testpassnøkkelen i setup-fana.
./native/probes/authentik/run-passkey.ps1 -RunDirectory <labmappe> -Phase PasskeyLogin
./native/probes/authentik/stop-lab.ps1 -RunDirectory <labmappe>
```

`PasskeyLogin` krev eit nytt prepare for kvar køyring; gamle snapshots blir
ikkje brukte til ny innlogging. `Finalize -HandoffDirectory <eksisterande
checkmappe>` kan fullføre ein allereie lagra kandidat etter ein avbroten
etterkontroll, innan same korte prøvevindauge. Det krev ubrukt servermarkør og
heile den same beviskjeda. Det startar ikkje ei ny innlogging.

Lokal negativ kontraktprøve stadfestar WebAuthn-only-policy, avvist ukjend
assertion og at autentisert cookie åleine ikkje hoppar over valideringa.
CI køyrer denne kontrakten på ein eigen kald localhost-lab og kompilerer
Rust/.NET-endringane gjennom dei eksisterande native-jobbane. CI-kvitteringa
har `hardwarePasskeyVerified=false` og `nativeWindowsHandoff=false`; ho
erstattar ikkje den faktiske Windows-prøva med brukargodkjenning. Artefaktlista
inneheld berre ikkje-hemmelege `evidence/receipt.json`.

Recovery, RP-logout, passnøkkelfjerning og native produksjonsøkter står framleis
att. N01e gir ikkje åleine produksjonsaksept for N03.

Pinna kjeldegrunnlag:

- [Origin og RP-ID](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/authenticator_webauthn/utils.py)
- [WebAuthn-verifisering](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/authenticator_validate/challenge.py)
- [Login-event knytt til økt](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/events/signals.py)
