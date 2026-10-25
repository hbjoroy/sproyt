# ADR N01: same identitet, native appøkter og recovery

Dato: 24. oktober 2026. Status: vald implementeringsretning, med tekniske kontrollpunkt i N01 og sikkerheitsreview før aktivering. Del av [implementasjonsplanen](native-client-implementation-plan.md). Ei [isolert N01a-prøve](../native/probes/identity/README.md) er implementert med syntetisk OIDC-provider og .NET-klient. Ingen produksjonsruter, appøkter eller Authentik-flytar er aktiverte. N01-aksepten står framleis open.

## Kontekst og produktval

Brukaren vil bruke same Authentik og eksisterande Sprøyt-konto. Til dagleg skal appen opne direkte; applås er valfri. Passnøkkel er føretrekt ved innlogging. Ein brukar som mistar både telefon og passnøkkel, skal kunne få tilbake kontoen med eit nytt bevis til kontoens stadfesta e-postadresse. Eksisterande einingar blir varsla, men må ikkje godkjenne recovery for at han skal kunne fullførast.

Dagens kode avleier intern brukar-ID frå nøyaktig `issuer:subject`. Nettappen bruker serverformidla OIDC og krypterte HttpOnly-cookiear. WS/SSE har periodisk autentiseringskontroll; dette er ikkje åleine ein kontrakt for tilbakekalling av native økter. Dagens dokumenterte recovery krev MFA og oppfyller ikkje mista-einaste-nøkkel-kravet.

## Avgjerd 1: bevar dagens identitetsrom

Backend skal bruke same kanoniske identitetsfunksjon og Authentik-provider som dagens nettinnlogging. Ny plattform, installasjon eller passnøkkel endrar ikkje intern brukar-ID eller medlemskap. Ein Android-/iOS-klient skal aldri innehalde den konfidensielle OIDC-klienthemmelegheita.

Ein ny direkte offentleg OIDC-klient blir berre vurdert dersom same kanoniske identitet kan bevisast, eller ein separat, verifisert koplingsmodell er vedteken. Lik e-post er ikkje kontokopling. Same adresse i ei ny providerøkt er ikkje eit prov på at kontoane skal slåast saman.

## Avgjerd 2: serverformidla innlogging med appbunde kodebyte

Føreslåtte endepunktnamn er kontraktskisser og skal endeleg fastsetjast i N03:

1. Appen lagar eit tilfeldig innloggingsprov og S256-challenge, og lagrar forsøket sikkert før han opnar nettlesaren.
2. `POST /api/v1/native/login` tek challenge, plattform og ein **registrert destinasjons-ID**, ikkje ei vilkårleg returadresse. Valfri invitasjons-/meldingskontekst blir kontrollert og lagra på serveren.
3. Serveren opprettar ein tidsavgrensa transaksjon og gir ei HTTPS-adresse til innlogging. Mobilen bruker systemnettlesar; desktop får ein verifisert returmekanisme tilpassa OS. Android App Links og Apple Universal Links er førsteval på mobil. Windows/Linux-retur blir eit eige testpunkt, ikkje ei ukontrollert URL-opning.
4. Backend eig OIDC-state, nonce, callback og OIDC-PKCE. Dette er ei **anna binding** enn appens eige handoff-prov. Callback stadfestar same kanoniske konto og eventuell invitasjon før han fullfører mobiltransaksjonen.
5. Returen til appen inneheld berre ein kortliva eingongskode og ufarleg rutekontekst. Koden er bunden til transaksjon, eksakt konto, registrert destinasjon, miljø og challenge. Ingen varige token eller invitasjonskapabilitetar blir lagde i retur-URL-en.
6. Appen journalfører ein innløysings-ID og sender kode + prov til `POST /api/v1/native/exchange`. Atomisk innløysing etablerer éi einingsøkt. Same prov og innløysings-ID må kunne hente eit tapt resultat innan eit kort retryvindauge; dette kan ikkje produsere fleire øktfamiliar.
7. Innlogging i appen blir først stadfesta når legitimasjonen er lagra atomisk i rett konto sitt nøkkellager. Avbrot før det skal ha ei forståeleg fortsetje-/prøv-igjen-handling.

Pilotgrenser til vurdering: innloggingstransaksjon ti minutt, handoff-kode eitt minutt, kort og avgrensa retry av same innløysing. Endelege grenser blir validerte i N01, med rategrenser og opprydding. To samtidige innloggingar, feil konto, feil telefon og kode som alt er brukt, skal ikkje gi konto- eller transaksjonsblanding.

## Avgjerd 3: éi tilbakekallbar einingsøkt på alle transportar

Backend får eit einingsregister med kanonisk brukar-ID, øktfamilie, miljø, oppretta-/sist-brukt-tid, utløp, tilbakekalling og sikkerheitsepoke. Telefonnamn er brukarretta metadata, ikkje ein hemmeleg eller autoritativ maskinidentitet.

Tilgangs- og fornyingstoken er ugjennomsiktige, tilfeldige kapabilitetar. Varig serversøk bruker hash. Authentik-refresh-token som serveren faktisk må bruke, må lagrast kryptert og med dokumentert nøkkelrotasjon, ikkje berre hashast. Ei kortliva kryptert resultatkvittering kan også vere nødvendig for krasjtrygg tokenrotasjon.

Eitt felles autentiseringslag gir ein principal med brukar-ID, økt-ID og sikkerheitsepoke før domeneautorisasjonen. Native HTTP/WS/SSE bruker eksplisitt bearer-header; ingen tilgangstoken i querystring. Mediaopplasting, originalnedlasting og native bakgrunnsarbeid bruker same kontogrense. Kombinasjon av cookie og bearer skal ha eintydig, fail-closed handsaming; klienttype eller ein falsk Origin-header er ikkje autorisasjon.

Nettlesaren held fram med cookie, same-origin og CSRF-vernet sitt. Native støtte skal ikkje opne browserruter for vilkårlege opphav eller deaktivere eksisterande kontrollar. WebSocket-handshake, kommandoopptak og utlevering av nye autoriserte sider/batchar må alle ha dokumentert økt- og tilgangskontroll.

Start med indeksert databasekontroll ved autorisert opptak og batchkontroll ved fanout. Slå opp fleire mottakarar samla der det er mogleg. Optimér først etter måling; ein tidsbasert cache utan ein korrekt tilbakekallingsregel er ikkje ei akseptabel innsparing.

Tilbakekalling stoppar seinare autoriserte kommandoar og nye datauttak. Allereie godkjent arbeid eller ein batch i flukt kan ikkje trekkjast tilbake. Endeleg media-publisering og nye opplastingsblokker får sine eigne kontrollpunkt. Eit signal lukkar samband raskt, medan korrektheit må halde også om signalet blir borte. Tilgangstap i ein kanal er ein separat domeneregel og gjeld sjølv om appøkta framleis er gyldig.

## Avgjerd 4: stille fornying, med ein eksplisitt krasjkontrakt

Éin økteigar per konto/eining samordnar UI og bakgrunnsarbeid. Appen fornyar ved oppvakning eller faktisk arbeid når det trengst, ikkje ved ein timer som held ei ubrukt kontoøkt kunstig aktiv. Nettfeil og timeouts bevarer lokal samtale og gir retry. Uttrykkeleg avvist legitimasjon krev ny innlogging før nye serverhandlingar.

For kvar rotasjon:

- Appen lagrar refresh-forsøks-ID før request og bruker same ID ved usikkert utfall.
- Serveren roterer tokenfamilien i éin transaksjon og bevarer det eksakte resultatet kryptert i eit kort, avgrensa retryvindauge. Berre same gamle token, økt og forsøks-ID kan få same resultat tilbake.
- Appen erstattar token atomisk før neste arbeid. Prosessdød på begge sider av lagringa inngår i testen.
- Eit uventa gjenbrukstilfelle utanfor den avtalte retryregelen sperrar tokenfamilien og blir registrert utan å logge token.

Føreslegne pilotverdiar, ikkje permanente produktløfte: tilgang opptil 30 minutt, inaktivitet opptil 30 dagar og absolutt native levetid opptil 90 dagar. Dei må avgrensast av Authentik-policyen og målt brukaroppleving. Ei absolutt grense betyr ei ny passnøkkelinnlogging ved utløp.

Native økter skal ikkje forlenge ei deaktivert eller tilbakekalla Authentik-identitet på eiga hand. N01 må vise kva providerlogout, deaktivert brukar, endra passnøkkel og recovery faktisk gjer med eksisterande grants. N03 må dokumentere korleis appfornying kontrollerer/fornyar providergrant og kva grense ein aktiv access-periode har. Manglande støtta kopling er ein blokk, ikkje ei stilltiande 90-dagars frittståande innlogging.

Sprøyt og Authentik deler **ikkje éin transaksjon**. Ein lokal resultatkvittering kan ikkje åleine reparere at Authentik har rotert sitt refresh-token, men svaret går tapt før Sprøyt lagrar det. N01/N03 skal injisere dette utfallet og velje anten ei dokumentert, støtta provider-retry-/overlappsordning eller ny autentisering når grant ikkje kan gjenopprettast. Vi lovar ikkje stille gjenoppretting i dette tilfellet før installert versjon er prøvd. Lokal samtale og usende utkast blir bevarte medan brukaren loggar inn att. Tilsvarande tapt resultat ved første OIDC-kodebyte gir trygg omstart av innlogging, ikkje ein ny eller gjetta identitet.

## Avgjerd 5: passnøklar og recovery blir forvalta av Authentik

Målet er invitasjon → stadfesting av e-post → registrering av passnøkkel → same konto og rett krets. For eksisterande brukarar blir nøkkelregistrering lagt til den eksisterande kontoen etter gyldig autentisering. Eksisterande nettinnlogging blir ikkje fjerna som eit sidearbeid.

Standard systemnettlesar med Authentik/WebAuthn kan gi telefonen si passnøkkeldialog. **Direkte** Android Credential Manager og Apple AuthenticationServices krev ei eiga bevist integrering av støtta serverutfordring, RP-domene og app-til-domene-tilknyting. Ingen privat Authentik-flow-API eller generell WebView blir ei uspesifisert autentiseringsløysing. Dersom direkte native innlogging ikkje kan leverast støtta, skal N01 presentere den konkrete forskjellen før MVP-omfanget blir låst.

Ein separat Authentik-recovery-policy må:

1. Sende eit nytt kortliva eingongsbevis til den **allereie stadfesta** adressa på den eksakte eksisterande kontoen. Eit invitasjonstoken, ei fritt innskriven adresse eller ein gammal innloggingscookie er ikkje dette beviset.
2. Vere bunden til pending user og den stadfesta adressa, ha rategrenser, utløp, kontrollerte feiltekstar og vern mot enkel konto-opplisting.
3. Tillate den avtalte e-postbaserte vegen når den einaste passnøkkelen er mista, utan å krevje den same tapte MFA-faktoren. Ikkje reaktivere ein deaktivert konto automatisk.
4. Gjere det mogleg å erstatte den tapte autentikatoren og registrere ny passnøkkel utan å endre kanonisk identitet.
5. Fullføre med ei ny bevist innlogging og ein tydeleg, varig recovery-kvittering til Sprøyt.

N01 skal definere og prove ei **støtta, autentisert serverkopling** mellom fullført Authentik-recovery og Sprøyt. Ingen klientflagga `recovered=true`, sjølvrapportert e-post eller vilkårleg redigerbar profilattribute kan etablere denne tilliten. Mekanismen kan vere ei avgrensa serverintegrering, men blir ikkje rekna som eksisterande API før han er kontrollert mot installert versjon.

Sprøyt treng idempotent recovery-samordning med ein recovery-ID, eksakt kanonisk brukar og sikkerheitsepoke. Fullføring aukar epoken og sperrar gamle native øktfamiliar, pending handoffs og relevante browserøkter. Valet av ny, autorisert økt må vere tydeleg. Retrying same recovery skal ikkje sperre den nye økta om att. Delvis svikt mellom systema skal stenge nye sensitive opptak og kunne fullførast ved retry.

Browsercookiear må få kompatibel epokestøtte og felles kontroll før recovery blir aktivert. Gamle cookiear utan felt kan representere epoke null så lenge kontrollen er trygg og alle lesarar er oppgraderte. Etter ein epoch-auke skal gamle browsercookiear ikkje gi ny appøkt. Tidlegare Authentik-SSO eller ferdige callbacks kan heller ikkje omgå recovery; test støtta providerrevokasjon og krav om dokumentert fersk autentisering.

Varsling til gamle einingar er informativ og kan bli forseinka eller utebli. Ho er ikkje eit vilkår for e-postrecovery. Brukaren skal kunne sjå og tilbakekalle einingar etter ny innlogging. Usende meldingar høyrer framleis til opphavleg konto og request-ID; dei blir ikkje flytta til ein annan konto eller automatisk sende etter ei ny identitetsstadfesting.

## Avgjerd 6: lokal applås og lagring har tydelege grenser

Standard er direkte opning, OS-verna legitimasjon, appprivat database og ingen legitimasjon i logg eller ordinær backup. OS-nøkkellager skal prøvast på fysisk eining, inkludert etter omstart og avbroten opplåsing.

Valfri applås vernar opning av grensesnittet, appførevising ved bakgrunn og synleg varselinnhald. Han er ikkje i seg sjølv ei lovnad om ein separat kryptert SQLite-database eller vern mot ein kompromittert telefon. Krav om ekstra kryptering av database, søkjeindeks og vedlegg må veljast som eit samanhengande tillegg, med eigne migreringar og nøkkel-/backupreglar.

Vi skal ikkje både krevje biometrisk godkjenning av kvar nøkkelbruk og love uavhengig sensitiv bakgrunnsprosessering. N01/N02 må bevise vald OS-tilgjengeklasse. Ved låst nøkkellager ventar sending og sensitive overføringar; generiske pushsignal kan framleis mottakast. Ingen skjult kopi av låst legitimasjon skal innførast for å få ei bakgrunnsjobb til å gå.

## Aktiveringsport og referansar

[N01c-observasjonen på Authentik 2026.8.2](../native/probes/authentik/README.md)
stadfestar normal refresh-rotasjon og avvist gammalt refresh-token etter eit
medvite kasta upstream-svar med den valde labpolicyen. N03 må handtere dette
som ny autentisering med bevart lokal tilstand når grantet ikkje kan reparerast.
Dette er ikkje ei lovnad om stille recovery eller ein produksjonsaksept.

[N01d](../native/probes/authentik/REVOCATION.md) stadfestar dessutan at sletting
av éi Authentik-browserøkt kan stoppe gammal access/cookie medan refresh framleis
utferdar nytt aktivt access-token. Ei slik øktsletting er ikkje i seg sjølv
Sprøyt si framtidige globale native tilbakekalling. N03 må ha eksplisitt
einingssperring og ei verifisert providerkopling; RP-logout og recovery er enno
ikkje aksepterte av denne prøva.

[N01e-passnøkkelprøva](../native/probes/authentik/PASSKEY.md) har verifisert
systemnettlesar med plattformpassnøkkel på Windows ARM64. Ho held same
localhost-identitet før/etter registrering og krev eit ferskt, eingongs
grant-/økt-/devicebevis ved app-handoff. Ho er ikkje bevis for direkte native
Credential Manager eller produksjonsidentiteten. Den observerte løkka ved
ekstra `prompt=login` er dokumentert; UV-kravet vart ikkje redusert.

[N01f1](../native/probes/authentik/RECOVERY.md) beviser e-post-/lenkjeåtferd,
men underkjenner den prøvde policybaserte recovery-sperra: ytre motorfeil
kan hoppe over Deny-stage. Databaseunikskap åleine reparerer ikkje dette.
Ein støtta, feillukka overgang og ei varig, autentisert fullføringsbinding
står att; ingen `recovered`-attribute eller generell webhook blir godkjend
som erstatning for den manglande kontrakten.

[N01f2](native-recovery-integration-adr.md) vel ein Sprøyt-eigd koordinator
med Authentik-eigd WebAuthn som retning, men måler at den offentlege
recovery-lenkja slepper to samtidige restaureringar til registrering.
API-et åleine er derfor ikkje godkjent som atomisk opptak. Ei støtta
fullføringsbinding er heller ikkje etablert; ingen vedlikehalden providerfork
er innført utan ei eiga avgjerd.

N01 beviser same konto, støtta passnøkkelinnlogging og recovery utan den tapte nøkkelen. N03 beviser rotasjon, providerkopling og autorisasjon på alle transportar. N04 beviser heile invitasjons-/recoverykjeda med testkontoar. Funksjonane er inaktive i prod til desse portane er godkjende og eit eige utrullingsmandat finst.

- [Gjeldande Sprøyt/Authen-kontrakt](authentik.md).
- [OAuth for native appar](https://www.rfc-editor.org/rfc/rfc8252).
- [OAuth Security BCP og refresh-token-rotasjon](https://www.rfc-editor.org/rfc/rfc9700.html).
- [Authentik WebAuthn og passnøklar](https://docs.goauthentik.io/add-secure-apps/flows-stages/stages/authenticator_webauthn/).
- [Authentik eksempel på recovery-flytar](https://docs.goauthentik.io/add-secure-apps/flows-stages/flow/flows/).
- [Android Credential Manager](https://developer.android.com/identity/credential-manager).
