# N01c: ekte Authentik i isolert prøve

Dato: 25. oktober 2026. Dette byggjer vidare på [N01b](../identity/DESKTOP.md).
Berre syntetiske kontoar og ein eigen database blir brukte. Ingen data, nøklar,
providerar eller flytar blir kopierte frå produksjon.

[N01d-matrisa](REVOCATION.md) prøver no separat tilbakekalling av refresh-token,
administrativ browserøktsletting og deaktivert brukar. Ho viser kvifor desse
ikkje kan behandlast som same hending i den komande native øktmodellen.

[N01e](PASSKEY.md) legg til ein separat localhost-lab med faktisk
plattformpassnøkkel, fersk passordfri innlogging og ein kontrollert beviskjede
frå app-handoff til providerøkt og WebAuthn-device.

[N01f1](RECOVERY.md) måler e-postrecovery med lokal SMTP-fangar, lenkjeforkar
og feilinjeksjon. Funna held recovery-porten **underkjend**: ein ytre
policyfeil kan hoppe over ei Deny-sperre. Dette er eit isolert eksperiment,
ikkje ei recovery-løysing klar for brukarar.

[N01f2](../../../docs/native-recovery-integration-adr.md) prøver det offentlege
recovery-API-et og vel vidare eigarskap. Kontoavgrensa mint fungerer, men to
samtidige restaureringar av same lenkje når begge nøkkelregistrering.
Native recovery er framleis avslått.

## Pinna miljø og køyring

Produksjonsversjonen vart kontrollert lesande med Kubernetes: server og worker
brukte `2026.8.2@sha256:ff8489a5af4f4fe415ffd180a8e3c10b120bc2592d13d79dac050d977f7b9ecd`.
Prøva bruker dette same manifestet, og PostgreSQL 16 Alpine med manifestet
`sha256:f4610795587492cd4fb887c2dc3de301b813278bb6f616992061141b5f15ac36`.
Begge er prøvde på lokal ARM64. Oppgraderingar krev ei ny, uttrykkeleg prøve.

På Windows blir WSLC brukt, med hjelp kontrollert før bruk. Linux CI bruker
Docker på sin eigen byggvert. Ingen Docker-installasjon blir gjort på Windows.
Artefaktar og private prøvefiler blir lagde i vald mappe, normalt på S:.
WSLC administrerer sjølv container-/volumlageret sitt.

```powershell
./native/probes/authentik/start-lab.ps1 -ArtifactsDirectory S:/Build/sproyt-authentik
./native/probes/authentik/run-lab.ps1 -RunDirectory <mappa-som-vart-laga>
./native/probes/authentik/run-lab.ps1 -RunDirectory <same-mappe> -SystemBrowser
./native/probes/authentik/stop-lab.ps1 -RunDirectory <same-mappe>
```

Portane 19090 (Authentik), 19191 (broker) og 19192 (appretur) må vere ledige.
HTTP er tillate berre på eksakt IPv4-loopback i dette laboratoriet. Vanlege
brukarar, ekte brukarnamn/passord og produksjonsissuer skal ikkje brukast her.

PostgreSQL og worker ligg på eit internt nettverk. Serveren har i tillegg eit
eige edge-nettverk for svar på lokal portforwarding; berre `127.0.0.1:19090`
blir publisert. Docker-socket og SMTP er ikkje kopla inn. Analytics, update check
og ekstern feilrapportering er avslått. Kvar container har minne-/CPU-grenser.
Opprydding verifiserer namn og eigarlabel før berre dei tre prøvecontainerane
blir stoppa. Volum og evidens blir bevarte, utan rekursiv filsletting.

Private filer har eksplisitt brukar-/SYSTEM-ACL på Windows og modus 0700 på
Linux. Hemmelegheiter blir ikkje sende på kommandolinja eller lasta opp frå CI.
N01c upstream-token ligg berre i broker-minnet; dette er ikkje den endelege
serverlagringa for N03.

## Kontraktar og testgrenser

`--authentik-config` er eit separat, lokalt brokeroppsett. Før JWKS blir henta,
må discovery gi eksakt issuer og alle brukte endepunkt må ha same kontrollerte
loopback-opphav. Proxyar, redirect-following og vilkårlege nettmål er avslått.
Syntetiske provider-/expire-ruter finst ikkje i denne modusen. OIDC bruker same
`openidconnect`-verifisering av signatur, issuer, audience, nonce og provider-PKCE;
app-prov og eingongsinnløysing er framleis separate.

Labprovider bruker `user_uuid`, ikkje e-post, som subject. Dei to testkontoane
gir ulike identitetar, medan ny innlogging med Alice gir same issuer/subject og
kanoniske interne ID. Lokal issuer er med vilje ulik produksjon: dette er ikkje
ei påstand om at vi har autentisert ein eksisterande produksjonskonto.

Automatiske providerprøver bruker eksplisitt førehandsautentiserte Django-
prøveøkter, med Authentik sin signerte cookieinnpakking. Dette testar OIDC og
refresh etter ei prøveøkt, ikkje brukarnamn/passord eller passnøkkel.
Ein separat faktisk Edge-rundtur på ARM64 har verifisert brukarnamn/passord,
OIDC-retur og OS-verna appfullføring. Appen bruker systemnettlesaren; ingen
privat Authentik flow-API eller password grant er lagd inn i klienten.

Windows sin vanlege modus bruker N01b sin DPAPI-journal. Linux CI krev det
eksplisitte valet `-HttpOnly`: der er app-prov og retur berre i RAM for
serverkontraktprøva. Kvitteringa markerer `nativeWindowsHandoff=false` og
`localStatePreserved=null`; dette er inga klartekstfallback for native klientar.

## Målt refresh-åtferd og konsekvens

Den valde labpolicyen gir access-token i fem minutt, refresh-token i éin time
og `refresh_token_threshold=seconds=0`. Normal fornying vart godteken og roterte
refresh-tokenet. Eit andre svar vart medvite kasta før broker-minnet vart
oppdatert, etter at Authentik hadde utført fornyinga. Retry med det gamle
refresh-tokenet vart avvist som `invalid_grant`.

**Tapt upstream-svar kan derfor krevje ny innlogging.** Ein lokal
Sprøyt-resultatkvittering kan ikkje reparere denne separate providerrotasjonen.
N03 må bevare lokal tilstand og skilje eksplisitt avvist grant frå nettfeil.
Prøva stadfestar at den verna app-prøveposten er uendra; integrert oppførsel for
ekte samtalecache og utkast står framleis att.

Upstream-prøver er bundne til `(transaksjon, brukar)`, slik at ny innlogging
ikkje erstattar grantet under test. `retry-old` rapporterer berre om gammalt
token blir godteke/avvist; han installerer ikkje eit nytt svar og blir ikkje
rapportert som reparert grant. Utfallet ligg i kvitteringa for kvar køyring.

CI lastar berre opp `evidence/receipt.json`, med revisjon, image/policy og
scenarioresultat. Cookiear, passord, klienthemmelegheit, databasar og token blir
haldne utanfor artefaktlista. Passnøkkel, recovery, RP-logout og
produksjonskopling står framleis att. Administrativ tilbakekalling blir prøvd
separat i N01d. N01c frigir ikkje N03-porten åleine.
