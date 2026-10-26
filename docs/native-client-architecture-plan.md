# Native Sprøyt: klientarkitektur, brukaroppleving og effektiv drift

Dato: 24. oktober 2026. Status: arkitekturforslag og gjennomføringsplan, revidert med Astra. Ingen app- eller driftsendring er autorisert av dette dokumentet åleine.

Sprøyt skal kjennast rask, sosial og enkel på Android, iOS, Windows, macOS og Linux. Plattformene deler identitet, innhald, leveringsreglar og designmeining, medan grensesnitt og systemintegrasjonar er native. Android blir første brukbare klient; små iOS- og Windows-prøver skal avsløre feil i fellesarkitekturen tidleg.

Den konkrete rekkjefølgja, avhengigheitene og testportane er utdjupa i [implementasjonsplanen](native-client-implementation-plan.md), [identitetsavgjerda](native-client-identity-adr.md) og [synkroniseringsavgjerda](native-client-sync-adr.md).

Ambisjonen frå tidleg WhatsApp blir omsett til låg friksjon, lite bakgrunnsarbeid og målbar kostnad per aktiv brukar. Han er ikkje ein dokumentert kapasitetsgaranti for Sprøyt. Kapasitet må skilje mellom registrerte brukarar, aktive brukarar, samtidige einingar, fanout, meldingar og media.

## 1. Produktløftet

Brukaren skal kome rett inn i samtalen, skrive utan hindringar og forstå kva som er sendt. Kretsar gir tilhøyrsle; kanalar ordnar samtalane. Agentar, prosessar og administrasjon er tilgjengelege i rett kontekst utan å gjere kvar melding til eit kontrollpanel.

- Opna samtalar og utkast blir lesne frå lokal lagring. Nettet oppdaterer dei etterpå.
- Leseposisjon, svarmål, vedlegg og utkast overlever navigasjon og prosessavslutning.
- Lokal kø, uviss levering, serverkvittert sending og avvist sending er ulike tilstandar. «Sendt» krev varig serverkvittering; det tyder ikkje lese av mottakar.
- Vanleg lesing og sending treng ingen meny. Gestar har eit synleg og tilgjengeleg alternativ.
- Varsel og invitasjonar fører til rett konto, krets, kanal og melding, også ved kald oppstart.
- Ingen plattform skal merke ei ukjend innhaldstype som ei tom eller ferdigbehandla melding. Trygg tekstvisning eller ei uttrykkeleg opne-i-nettapp-handling blir brukt til funksjonen finst native.

## 2. Kva vi faktisk kan gjenbruke

Kjeldegrunnlaget er `main` på `42cb3b6` ved planlegging. Dette er kodeobservasjonar, ikkje ei ny produksjonsmåling.

| Grunnlag | Tilstand og konsekvens |
| --- | --- |
| `crates/sproyt-protocol` | Felles Rust-protokoll og kontraktgrunnlag. Bruk skjemaversjonar og golden fixtures til klientbindingar. |
| `crates/sproyt-client-core` | Ei lita deterministisk kjerne for sendetilstand og øktgjenoppretting, også brukt gjennom WASM. Ho er ikkje ein komplett synkroniseringsmotor eller lokal database. |
| `frontend/src/application/runtime.ts` | Nyttig referanse for serialisert hendinghandsaming. TypeScript-/DOM-eigarar kan ikkje importerast direkte i native appar. |
| `src/auth.rs` | Intern brukaridentitet blir avleidd frå issuer og subject. Ny OIDC-provider kan gi ein annan Sprøyt-konto for same person. |
| `src/ws.rs`, `src/web/stream.rs` | WS og SSE har meldingssekvens og lag-reparasjon. Sekvensen for nye meldingar er ikkje ein full markør for alle seinare endringar. |
| `src/db/postgres.rs` | Endringar og reaksjonar blir varsla, men ein gammal redigert eller sletta melding blir ikkje nødvendigvis reparert ved berre å hente nye meldingar etter siste sekvens. |
| `docs/media-upload-queue-architecture.md` | Kø, originalbevaring og avgrensa prosessering er grunnlag for native overføringar. |
| React og `@sproyt/ui` | Produkt- og designreferanse. Native grensesnitt må implementerast og testast på kvar plattform. |

## 3. Arkitekturavgjerder

### 3.1 Felles kontrakt, native presentasjon

- Android: Kotlin og Jetpack Compose.
- iOS og macOS: Swift og SwiftUI, med UIKit/AppKit der plattforma krev det.
- Windows: C# og WinUI 3.
- Linux: GTK/libadwaita og Qt skal vurderast i ei lita integrasjonsprøve før valet blir låst. Ikkje innfør ein ekstra runtime berre for å få same pikselplassering.
- Nettappen blir vidareført. Ho deler kontraktar og utvalde reine reglar, ikkje nødvendigvis lokal lagringsimplementasjon.

Designkjelda skal innehalde semantiske token, typografiske roller, ikonmeining, leveringsstatusar og dokumenterte handlingar. Native tema blir genererte eller kontrollerte mot denne kjelda. Mobil får éi hovudflate om gongen; store vindauge får samtaleliste, samtale og valfri tråd/detaljflate. Native tilbakeknapp, meny, filveljar, tekstval, snarvegar og skjermlesar blir følgde.

Oppsett og administrasjon er utdjupa i
[planen for innstillingar](settings-and-administration-plan.md). Han definerer
felles eigarskap, destinasjonar, agentstatus og lagringsutfall, med native
presentasjon på kvar plattform. Planen er ikkje ei ny aktivering eller utrulling.

### 3.2 Ei lita felles Rust-kjerne, med eit reelt stoppunkt

Utvid eksisterande klientkjerne gradvis med deterministisk synkronisering, leveringsreglar, kø og normalisering. Unngå eigne implementasjonar av desse reglane i fem språk.

Før vi vel full lagringsarkitektur, må same prøve køyre i Kotlin, Swift og C#: opprette utkast, journalføre sending, simulere tapt kvittering, starte om og motta ei redigering. Kotlin/Swift kan bruke UniFFI. C#-støtta er ikkje like offisiell; ein liten stabil C-ABI er eit alternativ som må prøvast og kostnadsvurderast.

**Føreslege eigarskap for native klientar:** Rust eig ein appprivat SQLite-database og migreringane, med éin serialisert skrivar per konto. Native grensesnitt får paginerte modellar og endringssignal gjennom bindinga. Room/Core Data skal ikkje samstundes eige eller migrere dei same tabellane. Dersom dette blir for komplisert i plattformprøvene, vel vi native lagring med felles kontrakttestar før vidare bygging.

OS-adapterane eig nettverk, tid, bakgrunnsplanlegging, varsling, trygg nøkkellagring og filtilgang. Kjernen tek imot resultat og gir avgrensa effektar/kommandoar tilbake. Ingen nettverks- eller databaseoperasjon på UI-tråden. Bindinga må ha tydelege reglar for levetid, avbrot, feil, trådar og maksimale batchstorleikar. Media blir overførte som filreferansar eller straumar, ikkje store bytekopiar over FFI.

### 3.3 Same konto og trygg mobilinnlogging

Første retning er serverformidla mobilinnlogging som bevarer dagens Authentik-provider og kanoniske Sprøyt-identitet. Android/iOS bruker systemnettlesaren for OIDC. Appen opprettar eit innloggingsforsøk med kryptografisk prov; serveren bind forsøket til gyldig OIDC-resultat og leverer ein kortliva eingongskode gjennom ei verifisert applenkje. Berre appen med rett prov kan løyse inn koden til ei eiga, tilbakekallbar appøkt.

Ingen klienthemmelegheit i appen, ingen kopiering av nettlesarcookie til appen og ingen varige token i URL-ar. Appøkter får avgrensa levetid og kontrollert fornying. Backend må ha éin autentiseringsgrense for appøkter på HTTP, WS, SSE, kommandoar og media, før dei same eksisterande tilgangskontrollane blir brukte. Utlogging og tilbakekalling må også avslutte sanntid og stoppe nye overføringar.

Avklart produktretning er passnøkkel ved innlogging, direkte opning til dagleg og valfri lokal applås. Vi skal ikkje føresetje at Google-kontoen på telefonen er Sprøyt-kontoen. Dagens OIDC-flyt kan bruke passnøklar dersom Authentik-oppsettet er gjort klart for det. Direkte Credential Manager-integrasjon krev framleis dokumentert serverstøtte, rett RP-domene og Digital Asset Links. Kontoar blir aldri automatisk slått saman berre fordi e-postadressene er like.

Onboarding: invitasjon → krets og inviterande person → synleg aktiv konto / vel annan konto → eksisterande konto eller registrering → stadfesta medlemskap → rett samtale. Avbrot, utgått invitasjon, allereie medlem, feil konto og app som enno ikkje er installert må vere uttrykkelege testscenario.

Gjenoppretting via kontoens stadfesta e-post er også vald, med varsling til eksisterande einingar og moglegheit til å tilbakekalle ein telefon. Det krev ei ny bevist Authentik-policy: dagens dokumenterte recovery krev MFA og dekkjer ikkje mista telefon og einaste passnøkkel. Gamle provider-, browser- og native økter må få dokumentert handsaming; sjå ADR N01.

### 3.4 Synkronisering må dekke endringar, ikkje berre nye meldingar

Før lokal historikk kan vere påliteleg, treng vi ein dokumentert reparasjonskontrakt for nye meldingar, redigeringar, slettingar, reaksjonar, lesemarkørar og medlemskap. Tilgang blir alltid kontrollert på serveren. Ein lokal kopi er aldri bevis på gjeldande rettar.

Tilrådd retning er ein varig, transaksjonell endringsjournal for delte samtaleendringar, med paginerte delta, ugjennomsiktige markørar og avgrensa lagringstid. Brukarspesifikk tilstand og tilgangsendringar må få eigne versjonerte reparasjonsreglar; ikkje press alt inn i éin global markør. Ein stigande database-ID åleine beviser ikkje at alle tidlegare transaksjonar har fullført: markør og sidegrenser må vere trygge ved ulik commit-rekkjefølgje. Bruk delte journalar og små einingsmarkørar framfor ei permanent hendingkopi per apparat. Det konkrete omfanget blir eit eige ADR før implementering. For gammal markør skal gi ei konsistent ny avgrensa grunnkopi, ikkje stille datatap eller heil konto-nedlasting.

Database-commit er kjelda til sanning. PostgreSQL NOTIFY er eit vekkesignal; tapt NOTIFY eller ein full klientkø blir reparert frå varig tilstand. Snapshot, deltastraum og lokale migreringar må tole krasj mellom kvart steg. Når tilgang fell bort, blir den aktuelle lokale tilgangen sperra, abonnement avslutta og cache rydda ved neste kontakt. Tilbakekalling kan ikkje slette det ein fråkopla brukar allereie har sett.

Synkroniserings-ADR-en må også definere eit konsistent overgangspunkt mellom snapshot og delta ved samtidige endringar, og korleis ein framleis tilkopla klient oppdagar eit tapt signal. Ein journal reparerer ikkje seg sjølv utan ei innhentingsutløysing. Vi skal undersøkje delt framdriftskontroll på serversida og samanslegne kontrollar ved normal aktivitet før eventuell polling per apparat.

## 4. Brukaroppleving som leveransekrav

Før detaljbygging skal fem forløp prøvast med eit lite sett faktiske brukarar: invitert inn i rett krets; finne og svare i ein samtale; dele bilete; forstå sending utan nett; opne eit varsel i rett samanheng.

Føreslegne mål som skal målast i pilot, ikkje påståast oppnådde no:

- Lokal visuell respons innan 100 ms på ein representativ mellomklassetelefon.
- Varm opning av ei lagra samtale innan 500 ms; kald oppstart til brukbar lokal samtale innan 1 sekund, målt separat frå nettinnhenting.
- Minst 90 % av pilotbrukarane fullfører invitasjon og første svar utan hjelp; rapporter også faktisk deltakarantal og observerte feil.
- Ingen tastaturstripe, skjult composer eller tap av utkast ved kanalbyte, rotasjon, dvale og prosessdød.
- Minst 48 dp treffmål på Android og 44 pt på Apple, synleg fokus, ingen berre-hover-handlingar og fungerande hovudflyt ved 200 % tekstskalering.
- Ingen falsk leveringskvittering eller duplikat i dei spesifiserte feilprøvene. Status skal kunne lesast av skjermlesar.

Skissene prøver to mobile prioriteringar: samtalen først og kretsen først. Begge skal gi roleg meldingsflate, eitt konsist skrivefelt, kontekstuelle meldinghandlingar og synleg vedleggs-/svarmål. Skrivebord får meir oversikt og tastaturnavigasjon, ikkje ein forstørra telefon.

Kjelda til den interaktive [designskissa](design/native-client-concepts.html) ligg ved som ein lokal HTML-fragmentprototype. Ho viser to mobile inngangar og skrivebord, med simulerte meldingar og kvitteringar. Ho sender ingen data og er ikkje ein native app eller ei nettmåling. Native tastatur, bakgrunnsdrift og tilgjenge skal framleis prøvast på plattformene. Fragmentet blir vist i samtalen; for separat førevising treng det ein enkel vert med ikon og variantveljar.

## 5. Mange brukarar på lite maskinvare

Vi vidarefører Rust/Tokio og PostgreSQL og skil lett meldingsarbeid frå dyr medie- og modellprosessering. Ingen ny broker, cacheklynge eller tenesteoppdeling utan ein målt flaskehals.

- Éi multipleksa sanntidstilkopling per aktiv appinstans, ikkje per kanal. Ingen databaseøkt per telefon; bruk den delte poolen og eit eksplisitt tilkoplingsbudsjett.
- Abonner på nødvendige aktive samtalar og små oversiktsdelta. Ikkje last all historikk eller hent uendra fullstendige lister ved kvar hending.
- Bakgrunn på mobil bruker plattformvarsling og avgrensa innhenting. Ikkje hald WS/SSE eller minuttlege oppgraderingsprobar i gang medan appen søv. Varsel er signal; autentisert innhenting avgjer kva brukaren får sjå.
- Samle unødvendige gjentakingar av lesemarkørar og presence. Serverkvittering av ei melding blir ikkje forseinka for å samle kosmetisk tilstand.
- Køar skal vere avgrensa i både byte og element, også før serialisering. Treige mottakarar må kunne kople til att og hente etterslep utan å halde serverminne uavgrensa.
- Del same ferdigkoda hending mellom mottakarar der det er rett, utan å blande tilgangsvariantar. Mål kostnaden ved store kanalar og fanout til fleire einingar.
- Originalar, previewar og opplasting er separate arbeid. Straum filer og bruk filreferansar; avgrens dekoding, modellkø og agentkontekst uavhengig av API-trafikk.
- Cache skal ha bytebudsjett, utløp, kontoskilje og sletting ved utlogging. Serverprotokollen må tole at ein klient har kasta historikken sin.
- Lågkortinalitetsmålingar skal vise køalder, poolventing, etterslep, CPU, RAM og byte per operasjon utan å logge meldingsinnhald eller identitetstoken. Tel også databaseførespurnader og skrivingar per apparattime og per mottakarlevering; periodisk presence og tilgangskontroll ved fanout kan koste meir enn sjølve meldingsserialiseringa. Eventuell tilgangscache må bevare tilbakekalling.

### Kapasitetsprøve før skalalovnader

Køyr i isolert miljø med syntetiske data og fast CPU/RAM-/DB-budsjett, ikkje i prod. Mål 100 → 1 000 → 5 000 tilkopla einingar som trinn, ikkje som allereie støtta kapasitet. Stopp ved SLO-brot eller mindre enn 30 % ressursreserve. Største trinn er berre relevant dersom mindre trinn gir forsvarleg framskriving.

Prøv først inaktive samband, så ein dokumentert aktiv del, så ein stor kanal, reconnect-bølgje og samtidig medie-/agentarbeid. Oppgi faktisk meldingsrate, meldingsstorleik, mottakarar per melding og tal einingar per brukar. Mål p50/p95/p99, inkrementelt RAM per samband, databasearbeid per godteken sending og byte per synkronisering. Behald dagens føreslegne driftsmål: 99 % av sendingane kvitterte innan 750 ms og 99 % av reconnect-innhenting innan fem sekund for det uttrykkeleg definerte etterslepet. Uavgrensa gammalt etterslep kan ikkje lovast innhenta på fem sekund.

## 6. Klientane skal kunne utviklast i takt

- Versjoner API-kapasitetar og klientkjerne separat frå skjermbilete. Ein server må støtte ei dokumentert rekkje eldre appversjonar; nettapp og butikkappar kan ikkje krevjast oppdaterte samtidig.
- Del golden fixtures for identitet, ordre, duplikat, redigering/sletting, tilgangstap, kontobyte og avbrot. Native testadapterar køyrer dei same forløpa mot kjernen.
- Kvar funksjon får ei plattformmatrise: implementert, fysisk akseptert, avgrensa eller ikkje støtta. Hald kode og ekte telefonaksept frå kvarandre.
- Ta med agentar/minne, Markdown/GFM, kode, Mermaid, omtalar, reaksjonar, trådar, invitasjonar og Heart-/arbeidskort. Førstegongsvisning, handlingar og trygg fallback blir spesifiserte per innhaldstype.
- Native Mermaid- og rikinnhaldsvisning krev eige val av renderer. Servergenerert SVG kan gi ekstra serverkostnad; ein privilegert WebView er ikkje eit uspesifisert universalalternativ.
- Felles token og kontraktar har automatiske kontrollar. Kvar plattform har i tillegg test for tastatur, fokus, skjermlesar, systemtilbake, varsling og filtilgang.
- CI byggjer signerte interne Android-leveransar og kontrollerer Rust/bindingar. iOS/macOS krev ein macOS-byggvert; Windows/WinUI må prøvast på dei arkitekturane vi faktisk vil støtte. Signeringsnøklar og butikktilgang blir ikkje føresette tilgjengelege.

## 7. Granulert gjennomføring med stoppunkt

| Steg | Konkret leveranse | Må vere bevist før neste steg |
| --- | --- | --- |
| P0a Produkt og dekningskart | Fem forløp, innhaldsmatrise, mobile skisser og desktop-retning | Brukarar finn samtale, svar, vedlegg og rett konto; identitetsvalet er presisert. |
| P0b Arkitekturavgjerder | ADR for mobilinnlogging, synkronisering og lagring/FFI | Astra-review av kontoidentitet, tilgangstap, endringsreparasjon og bindingseigarskap. |
| P1a Kryssplattformprøve | Same lille kjernesekvens i Kotlin, Swift og C# | Ingen ekstra konto, ingen dobbel DB-eigar; avbrot og feil kan presenterast native. |
| P1b Invitasjon og økt | Native Android-prøve mot Canary, med ny og eksisterande konto | Full invitasjon, avbrot, feil konto, utlogging og tilbakekalling på fysisk telefon. |
| P2a Varig synkronisering | Backend-reparasjonskontrakt og klientjournal med feilinjeksjon | Gamle redigeringar/slettingar/reaksjonar og tilgangstap blir reparerte etter fråkopling. |
| P2b Meldingspilot | Compose-samtaleliste, historikk, utkast, kø og sending | Faktiske brukarar kan bruke appen dagleg; inga stille misting av meldingar eller utkast i feilmatrisa. |
| P3a Varsling og lenkjer | Push, app-lenkjer og trygg oppstart i rett konto | Kald/varm oppstart, nekta varsel og fleire einingar gir rett åtferd utan doble varsel. |
| P3b Media og deling | Native mottak, foto/video, kø, nedlasting og vising | Store originalar og bakgrunnsavbrot fungerer på fysisk telefon; ressursbudsjett halde. |
| P4 Funksjonsdekning | Agentar og prioriterte rike kort/handlingar | Kvar rad i dekningskartet har ein avklart status og brukarforståeleg fallback. |
| P5 Effektivitet og release | Isolerte lastprøver, batterimåling, intern distribusjon og driftsplan | Målte kostnader, dokumenterte grenser, Canary-aksept og Astra-review før eventuell prod. |

Android er hovudleveransen. iOS- og Windows-prøvene i P1 er små risikoprøver, ikkje tre parallelle fullstendige produkt. Full iOS kjem etter at felleskjerne og brukarforløp er stabile; desktop nyttar same kontraktar når plattformprioriteringa er vald. Ingen masseimplementering eller produksjonsutrulling som del av denne planrunden.

## 8. Referansar

- [Eksisterande protokoll](protocol.md), [driftsmål](operations.md), [skaleringsvurdering](architecture-scaling-assessment-plan.md), [Authen-oppsett](authentik.md) og [UI-paritet](sproyt-ui-parity-audit.md).
- [UniFFI: Kotlin og Swift er offisielle språk; andre bindingar må vurderast særskilt](https://mozilla.github.io/uniffi-rs/latest/).
- [Android Credential Manager](https://developer.android.com/identity/credential-manager) og [Digital Asset Links](https://developer.android.com/identity/credential-manager/prerequisites).
- [OAuth for native appar, RFC 8252](https://www.rfc-editor.org/rfc/rfc8252).
- [PostgreSQL NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html): signal er ikkje ein full varig endringsjournal.
- [Android treffmål](https://support.google.com/accessibility/android/answer/7101858).
