# Implementasjonsplan for native Sprøyt

Dato: 24. oktober 2026. Status: gjennomføring starta for avgrensa N00/N02-prøver; seinare pakkar står framleis som plan. Byggjer på [arkitekturplanen](native-client-architecture-plan.md), produktavklaringar med Harald og arkitekturrevisjon med Astra.

Målet er ein rask Android-klient med ein påliteleg felleskjerne som også er prøvd frå Swift og WinUI. Arbeidet blir små PR-ar som kan vurderast og målast kvar for seg. Ingen ny innlogging eller synkroniseringsprotokoll skal aktiverast før nødvendig serverkode, gamle klientar og tilbakeføring er prøvde.

Gjennomføring starta etter uttrykkeleg oppdrag om å bruke tilgjengelege verkty: [N00-verktøybaseline](native-client-tooling-baseline.md) og [ei avgrensa N02-prøve på Windows ARM64](native-client-probe-results.md) er utførte. N00/N02 er berre delvis dekte; N01 og produksjonsfunksjonane er ikkje starta.

## 1. Avklarte produktval

N02-prøva er utvida etter Astra-review med prosessavbrot ved tre punkt rundt
commit, kontrollsum frå før prosessdød og ein dedikert C#-arbeidar med avgrensa
kø, definert avbrotsgrense, utloggingsgjerde og kontrollert lukking. Sjå
[resultat og attståande avgrensingar](native-client-probe-results.md). Dette
dekkjer fleire N02-krav; Kotlin, Swift, faktisk WinUI og identitetsprøva står att.

| Val | Føring for implementasjonen |
| --- | --- |
| Same Authentik og same Sprøyt-konto | Bevar kanonisk issuer/subject og eksisterande medlemskap. Ikkje opprett parallelle brukarar for Android eller iOS. |
| Passnøkkel som føretrekt innlogging | Bruk systemet sitt legitimasjonsgrensesnitt der ein støtta Authentik-flyt kan verifisere det. Direkte native API-integrasjon må bevisast; standard systemnettlesar med passnøkkel er eit avgrensa innloggingssteg, ikkje appgrensesnittet. |
| Direkte opning til dagleg | Vis lokal samtale straks. Forny økta utan innloggingsskjerm når legitimasjonen framleis er gyldig. Eit nettbrot er ikkje grunn til å logge ut. |
| Valfri applås | Lokal biometri/telefonkode kan verne appen. Dette er ikkje ny serverinnlogging og skal ikkje øydeleggje utkast ved avbrot. |
| Gjenoppretting med stadfesta e-post | Ein ny, prøvd Authentik-flyt skal støtte mista telefon og einaste passnøkkel. Dagens MFA-avhengige recovery blir ikkje rekna som ferdig løysing. |
| Varsling og einingsstyring | Meld frå om gjenoppretting og nye innloggingar til eksisterande einingar. Brukaren skal kunne tilbakekalle ein telefon. Manglande pushkvittering skal ikkje låse legitim recovery. |
| Native produktfamilie | Compose, SwiftUI og WinUI med felles kontraktar/kjerne. Linuxval kjem etter ei lita GTK/Qt-vurdering. |

Skisser, minimum OS-versjonar, direkte Credential Manager-støtte og signerings-/byggtilgang er framleis tekniske eller brukarstyrte kontrollpunkt. Dei er ikkje skjulte føresetnader for første leveranse.

## 2. Avgjerdsgrunnlag før kode

- [ADR N01: identitet, appøkter og gjenoppretting](native-client-identity-adr.md).
- [ADR N02: varig synkronisering og lokal lagring](native-client-sync-adr.md).
- Kvar ADR skil vald retning frå ei konkret teknisk prøve. Ingen uavklart detalj skal bli implementert som ein stilltiande tryggleiks- eller kapasitetsgaranti.

**Første gjennomføringsbatch blir N00–N02.** Han skal gi eit konkret svar på innlogging, bindingar og lokal gjenopptaking, ikkje ein halvferdig full chat-app. N03–N05 byggjer servergrunnlaget før dagleg native bruk. UI kan utforskast mot fiktive data medan servergrunnlaget blir prøvd.

## 3. Arbeidspakkar og avhengigheiter

| Pakke | Avhengig av | Avgrensa leveranse og utgangskrav |
| --- | --- | --- |
| N00 Førmåling og prosjektkontrakt | Ingen | Registrer aktuell kjelderevisjon, støtta einingar, byggvertar, signering og funksjonskart. Mål dagens API/pool-/minnegrunnlag i isolert test. Lag felles testscenario og testdata utan produksjonsmeldingar. |
| N01 Identitetsprøve | N00 | Prototyp ein OIDC-rundtur med eksisterande kanonisk identitet, provbunden retur og to innloggingsforsøk samtidig. Prøv passnøkkel, tapt upstream-refresh-svar og e-postrecovery i eit isolert Authentik-oppsett på same relevante versjon. Vis nøyaktig kva som er native, kva som bruker systemnettlesaren og kva som ikkje er støtta. Ingen verkelege brukarar blir flytta eller nullstilte. |
| N02 Binding og lagringsprøve | N00 | Eitt lite Rust-bibliotek med utkast, journalført sending og omstart blir kalla frå Kotlin, Swift og C#. Bevis eigarskap, avbrot, pakking og migrering. Ved manglande Apple-byggvert er Swift-prøva uverifisert; full Rust-eigd lagringsarkitektur blir ikkje låst før prøva er køyrd. |
| N03 Appøkter og sikkerheitsgrense | N01 | Additiv backendstøtte for provbunden handoff, einingsøkter, rotasjon, tilbakekalling og autorisasjon gjennom HTTP, WS, SSE, kommandoar og media. Feil legitimasjon og tvitydige cookie/bearer-kombinasjonar blir avviste. Funksjonen blir ståande avslått etter migrering. |
| N04 Passnøkkel, invitasjon og recovery | N03 | Ny og eksisterande brukar kjem tilbake til rett krets etter innlogging. E-postbevis og ny passnøkkel blir knytte til rett eksisterande Authentik-brukar. Test konto-byting, avbrot, utgått lenkje, misbruk, og tilbakekalling av gamle økter. Browser- og appøkter har dokumentert dekning. |
| N05 Varig endringsjournal | N00, ADR N02 | Additiv PostgreSQL-/SQLite-støtte for commit-ordna samtaleendringar, versjonert tilgang/brukartilstand og konsistente snapshot/delta. Alle skrivestiar er kartlagde. Gammal og ny kode kan leve saman med native sync avslått. |
| N06 Native kjerne og lokal journal | N02, N05 | Rust eig lokal database dersom bindingprøva er godkjend. Bygg transaksjonell outbox, endringsmarkørar, eksplisitte feiltilstandar og ressursgrenser. Eit krasj etter server-commit og før lokal kvittering gir replay med same ID, ikkje ny melding. |
| N07 Android samtalepilot | N03, N04, N06 | Compose: krets/kanal, lokalt lesbar historikk, utkast, tekstsending, svar og reaksjonar. Lag både tom, lastande, fråkopla, avvist og utgått økt-tilstand. Rett konto og ingen tastaturstripe på fysisk telefon. |
| N08 Varsling og app-lenkjer | N07 | Native push, einingsregistrering, endra push-token og djup lenkje frå kald/varm start. Ingen meldingsinnhald i push som standard. Manglande eller forseinka push påverkar ikkje endeleg synkronisering. |
| N09 Media og deling | N07 | Native fil-/fotoveljar og delingsmottak, varige filreferansar, avgrensa overføringar, framdrift/avbryt/retry, originalnedlasting og god vising. Native bakgrunnsarbeid bruker skild einingsidentitet og same serverkvotar; ingen varig URI-tilgang blir føresett utan å vere sikra. |
| N10 Rikare innhald og funksjonsdekning | N07 | Markdown/GFM, trådar/omtalar, agentar/minne og invitasjons-/Heart-/arbeidskort i prioritert rekkjefølgje. Kvar type får native renderer eller tydeleg, trygg fallback. Mermaid får ei konkret rendereravgjerd. |
| N11 Fysisk pilot og driftsprøve | N07–N10 etter avtalt MVP-omfang | Deltakarar prøver dei fem hovudforløpa. Mål batteri, lokal respons og isolert serverkostnad med avgrensa lasttrinn. Rett faktiske friksjonspunkt før meir plattformbreidd. |
| N12 Canary-distribusjon | N11, review og aktuelle CI-jobbar | Signert Android-Canary med separat app-ID og kontolager. Dokumenter installasjon, oppdatering, avinstallering og tilbakeføring. Produksjon krev eige mandat og verifisert aksept. |
| N13 iOS og skrivebord | Stabil kjerne og N11-erfaring | Utvid Swift-prøva til full iOS-klient; ta deretter avtalt desktopprioritet. Gjenta dei felles kontraktane og native forløpa, ikkje berre kopier Android-skjermane. |

N03/N04 og N05 kan byggjast som separate arbeidspakkar etter N01 og N02; dei må samlast før N07. Ingen implementeringsagent skal endre den same autentiserings- eller migreringsstien parallelt utan eit uttrykkeleg eigarskap.

## 4. Planlagd kodeplassering og eigarskap

Dette er ei tilrådd mappeinndeling; ingen tomme prosjekt blir oppretta som del av planarbeidet.

- `crates/sproyt-protocol`: versjonerte DTO-ar, skjemaføring og golden wire-fixtures. Store strukturelle endringar krev eigen kompatibilitetsvurdering.
- `crates/sproyt-client-core`: deterministiske reglar for sync, sendekø og øktutfall. Eksisterande WASM-grense må framleis kunne byggjast utan native IO.
- Eit separat native storage-/bindinglag: SQLite, kontrollert arbeidartråd, migreringar og genererte Kotlin/Swift-bindingar; C-ABI eller dokumentert C#-binding etter N02. Ikkje press SQLite/OS-avhengigheiter inn i `no_std`-WASM-varianten.
- `apps/android`, `apps/apple`, `apps/windows` og seinare Linux: presentasjon, livssyklus og systemadapterar. Verifiser eksisterande repo-struktur før faktisk oppretting.
- Backend `src/auth.rs` og `src/web/*`: felles autentiseringskontekst; eigne mobile endepunkt for øktlivssyklus. `src/db/*` eig transaksjonane og journalen.
- Designkontrakt og fixtures: felles namn på status, token og handlingar. Native komponentar eig geometri, fokus og interaksjon.

Arkitekturen skal ikkje få to konkurrerande lokale databaser, to sendekøar eller to eigarar av øktfornying. Native overføringsarbeidarar skal gå gjennom same konto-/økt- og journalgrenser sjølv om UI-prosessen ikkje lever.

## 5. Testmatrise som skal følgje implementasjonen

| Område | Minste påkravde scenario |
| --- | --- |
| Identitet | Eksisterande brukar får nøyaktig same interne ID; to samtidige forsøk blir ikkje blanda; feil issuer/audience/nonce/prov blir avvist; ingen kode/token i logg eller referer. |
| Onboarding | Ny/eksisterande/allereie-medlem, feil konto, manglande installasjon, utgått invitasjon, avbrot etter kvart steg og retry utan dobbelt medlemskap. |
| Passnøkkel og recovery | Første passnøkkel, ekstra eining, avbroten biometridialog, tapt einaste nøkkel, éin gjenbruk av e-postbevis, misbruk/rategrenser, gamle app- og browserøkter etter recovery. |
| Øktfornying | Tapt refresh-svar etter Sprøyt-commit; dobbelt refresh; prosessdød før/etter lagring; tapt Authentik-svar etter providerrotasjon og definert ny autentisering dersom grant ikkje kan reparerast; utgått Authentik-økt. Nettbrot åleine er ikkje logout. |
| Lokal lagring | Full disk, låst nøkkellager, migrering av tidlegare pilotdatabase, krasj mellom journal/effekt/kvittering, konto-byte og utlogging under fil-/databasearbeid. |
| Synkronisering | Tapt NOTIFY medan klienten framleis er tilkopla; endringar under snapshot; ulike commit-rekkjefølgjer; utgått markør med sletta lokal melding 101 utanfor recent-100; seint history-svar frå før reset; gammal redigering/sletting; fråkopla tap av medlemskap; full utgåande kø. |
| Meldingslevering | Same request-ID og payload blir prøvd att etter uviss levering; definitiv avvising blir ikkje automatisk replaya; privat kanal og tråd held rett avgrensing. |
| Native UX | Kald/varm start, systemtilbake, rotasjon, tastatur, skjermlesar, 200 % tekst, stor vedleggsførevising, delt skjerm og desktopfokus. |
| Varsling/media | Kald/varm app, push nekta eller forseinka, tapt filtilgang, bytt konto, avbrot under opplasting, kvittering ved nedlasting og fil som er større enn arbeidsminnet til appen. |

Påstandar om native åtferd krev fysisk eining. Ei green CI-linje eller ein HTML-prototype er ikkje fysisk Android-/iOS-aksept. Native minimumsversjonar blir fastsette etter einingskartlegginga i N00.

## 6. Yting og maskinvarebudsjett

Baselinen skal rapportere API, database, køarbeidarar, lagring og eventuell ingress/push-kostnad kvar for seg. Delte maskiner må få målt den samla lasten. Ei rask API-måling utan samtidig media/agentarbeid er ikkje ei total kapasitetserklæring.

Første mål er det eksisterande driftsmålet på 99 % sendingar kvitterte innan 750 ms for ei oppgitt arbeidslast, og reconnect innan fem sekund for eit avgrensa etterslep. Native lokal respons blir målt separat. Vi legg ikkje til Kafka/Redis eller fleire tenester før konkrete målingar viser kva som ikkje held.

Test 100, så 1 000 og eventuelt 5 000 syntetiske tilkopla einingar i eit isolert miljø med fast ressursbudsjett. Oppgi aktiv del, einingar per brukar, meldingsstorleik/rate, kanalstorleik, fanout og sync-volum. Køgrenser, sidegrenser, retry og rategrenser skal vere konfigurerte før lastprøva. Stopp ved varig SLO-brot eller mindre enn 30 % nødvendig ressursreserve. Etter kvart trinn vel vi målretta tiltak eller eit lågare akseptert kapasitetstak.

## 7. CI, release og tilbakeføring

- Bruk den etablerte PR → review → CI → immutabelt image → GitOps-rutina for backend. Ingen lokal utrulling som snarveg.
- Legg til målretta Rust-, database- og transportkontraktar når faktisk kode blir endra. UI- og native bygg skal veljast etter endra område, ikkje køyrast blindt fem gonger for kvar liten endring.
- Native kjernen må byggjast på relevante arkitekturar. Swift-aksept krev macOS-byggvert; WinUI/C# må faktisk lasta Rust-biblioteket på Windows. Eit Rust-kryssbygg er ikkje ein full klientprøve.
- Kvar native prøve og release får sporbar kilde-, kjerne-, protokoll- og lokal skjemaversjon. API-kapasitetar lèt gamle og nye appar fungere samtidig.
- Alle backendutvidingar startar additive og inaktive. Ved journalutrulling må alle relevante skrivarar vere oppgraderte før native synkkapasitet blir annonsert; sjå ADR N02. Eldre produksjonsappar skal framleis virke.
- Etter at nye native klientar er sleppte til, kan ikkje serveren rullast tilbake til ein versjon som manglar lova økt-/synkkontrakt. Vel kapasitetsstyrt pause, minimum kompatibel versjon eller roll-forward. Eksisterande meldingar blir aldri sletta for å få ein pilot til å fungere.
- Kjerne- og lokale skjemamigreringar må ha ein dokumentert kompatibilitetsveg; ein eldre APK kan ikkje føresetjast å kunne opne ein nyare database.

## 8. Review og stoppunkt

Sol styrer avgrensa arbeidspakkar og gjer mesteparten av implementasjonen. Astra blir brukt ved N01-identitet/recovery, ADR N02 og endringsjournalen, vurdering av N02-lagringsprøva, og før ein faktisk release. Klare mekaniske oppgåver kan delegerast; eigarskap til transaksjonar og tryggleikskontraktar skal vere eintydig.

Stopp og avklar dersom native passnøkkel krev ustøtta Authentik-API, same brukar får ny ID, recovery krev ein nøkkel brukaren nettopp har mista, SQLite får to eigarar, bindingane krev mykje plattformspesifikk kjernekode, eller ein syncmarkør kan hoppe over ei commit. Registrer den konkrete blokka og det enklaste alternativet. Ikkje bruk token på gjentekne fulle bygg eller utrullingar for å omgå eit uavklart arkitekturval.

Gjennomføringsbatch N00–N02 er starta etter oppstartsinstruks. Resultat og avgrensingar blir førte i dei lenkja prøvedokumenta; ein pakke blir ikkje rekna som fullført fordi ein delprøve er grøn.

### Astra sin sluttrevisjon

Astra reviderte ADR-ane og rekkjefølgja. To konkrete hol er tekne inn: reset etter utgått markør må ugyldiggjere eldre cache utanfor ny grunnkopi, og refresh må teste tap mellom Authentik-rotasjon og Sprøyt-lagring. Begge har no eigne akseptscenario. Serverautentisert recovery-kvittering, providerkopling og binding-/lagringseigarskap står framleis som tekniske prøveportar; dei er ikkje rapporterte som implementerte funksjonar.
