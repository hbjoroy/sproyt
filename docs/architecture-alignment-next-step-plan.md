# Neste steg i arkitekturomlegginga – #345

Dato: 10. november 2026. Status: **framlegg til gjennomføringsplan**, ikkje implementert arkitektur eller godkjent driftsbudsjett. Utarbeidd med Astra etter dei avtalte arkitekturprinsippa frå 5. november.

**Tilråding:** Start med avgrensa måling av database- og chatventing under #353. Avklar datalivsløp under #356 samstundes. Prøv deretter kontodataeksport som første vertikal under #355. Bruk målingane og leasekontrollen til å avgjere om push skal bli første nye sjølvstendige arbeidar under #354. Behald modulær monolitt, eksisterande database og eksisterande leveringskontraktar.

## 1. Kjeldegrunnlag og kva som faktisk står att

Kodegrunnlaget er `main` ved `ccc31db55306dc380eb825a23436a51f1b62d50d`, kontrollert mot remote 10. november. Alle kodetilvisingar nedanfor gjeld dette treet. Den opphavlege arbeidskopien på `codex/live-reaction-snapshots` er eldre og har andre pågåande endringar; han er ikkje brukt som dagens kodegrunnlag. Ingen produksjonstilstand eller ytelse er målt i denne planlegginga.

| Arbeid | Observert status i GitHub 10. november | Konsekvens |
| --- | --- | --- |
| [#346](https://github.com/hbjoroy/sproyt/issues/346), #347–350 | Lukka som fullførte | Gjenbruk CI-policy og målebevis. Dei gamle sjekkboksane i #345 er ikkje arbeidsrekkjefølgja no. |
| [#351](https://github.com/hbjoroy/sproyt/issues/351), [#352](https://github.com/hbjoroy/sproyt/issues/352) | Lukka som fullførte | Eksportgrenser og RSA-arbeid skal ikkje leverast på nytt. |
| [#353](https://github.com/hbjoroy/sproyt/issues/353)–[#356](https://github.com/hbjoroy/sproyt/issues/356) | Opne | Desse fire er neste arkitekturetappe. |
| [#170](https://github.com/hbjoroy/sproyt/issues/170) | Lukka etter produkteigar si driftsaksept 7. november | Gjenbruk aksepterte øvingar; ikkje opne saka igjen. #353/#356 eig vidare arbeid. |
| [#231](https://github.com/hbjoroy/sproyt/issues/231) | Implementert; open for utsett produktaksept | Gjenbruk agentkapabilitetane. Ny agent-/bileteplattform er ikkje nødvendig. |

[Hovudsaka #345](https://github.com/hbjoroy/sproyt/issues/345) og produkteigaravklaringa frå 7. november tek både kapasitet og personvern med i omlegginga. Eldre formuleringar i #353 om å levere sluttaksept til #170 må lesast i lys av denne avklaringa.

Observerte forhold i aktuell kode:

- [Poolbygging](../src/db/mod.rs) deler PostgreSQL-pool, med fem sekund acquire-timeout. [Konfigurasjonen](../src/config.rs) har standardtak fire; dette er ikkje stadfesting av driftsverdiane. Det er ikkje funne måling som dekkjer poolventing og alle timeoutar.
- [ChatEngine](../src/chat.rs) har postkasse med 1 024 plassar. Fleire produsentar ventar i `send().await`; aktøren utfører fleire databaseavhengige kommandoar sekvensielt. Ei avgrensa postkasse avgrensar ikkje åleine talet på ventande produsentoppgåver.
- [HTTP-målingane](../src/operations/measurements.rs) og [Heart-/transportmålingane](../src/operations/dependencies.rs) finst allereie. Dei måler ikkje separat venting før og inne i chatpostkassa.
- [Oppstarten](../src/server.rs) startar push, bilegenerering, emneanalyse, samtaleagentar/minne, prosesspilot, arbeidsoppgåver og mediaopprydding saman med webprosessen. Fleire initialiseringar kan returnere feil før API-et er klart. [Mediaarbeidaren](../src/media_worker.rs) er allereie ein eigen køyrerolle med eige image i chartet.
- [Kontodataeksport](../src/domain/account_export.rs) og [SQL-eksporten](../src/db/account_export.rs) er allereie avgrensa og strøymande. Admission og orkestrering ligg framleis i `ChatEngine`; eksportmetoden er del av [ChatRepository](../src/domain/repository.rs).
- Nye arrangement og emne finst i [events](../src/events.rs) og [topics](../src/topics.rs). Eksporten omfattar allereie arrangementsdata. Datalivsløpskartet må også omfatte avleidde emne, kjeldekopiar og ventande analysejobbar.

[Septemberkartlegginga](database-capacity-assessment-2026-09-24.md) er historisk og uttrykkeleg ikkje godkjent kapasitetsbudsjett. [ARCHITECTURE.md](../ARCHITECTURE.md) har også eldre klient-/CI-tekst; bruk aktuell kode og [CI-policyen](ci-iteration-strategy.md) ved gjennomføring. Ingen av desse kjeldene beviser dagens utrulling.

## 2. Arkitekturval og ansvar

Minste tilstrekkelege løysing er sterkare grenser og måling i eksisterande prosessar. Separate tenester, broker, generell køplattform, kanalpartisjonering og høgare pooltak blir ikkje innførte av denne planen. Dei krev eit konkret behov som måling eller feilscenario dokumenterer.

Ein **kapabilitet** er ei evne eigd utanfor vertikalane, brukt gjennom ein kontrakt. Tabellen foreslår eigarroller; personar og faktisk bemanning er ikkje avklarte. Eit initiativ kan skrive implementasjonen utan å overta kontrakteigarskapen.

| Område | Føreslegen eigarrolle | Forbrukarar og kontrakt |
| --- | --- | --- |
| Vertikal: samtalar | Chat-kjerne | Meldingar, kanalrekkjefølgje og tilgangsreglar; HTTP/WS/MCP brukar same tenester og persistenskontrakt. |
| Vertikal: kontodata | Konto/personvern | Autentisert eigeneksport; seinare vedtekne livsløpsoperasjonar. Leser andre område gjennom eksplisitte eksportbidrag innan same snapshot. |
| Vertikalar: arrangement, emne, agentdialog, prosessar | Kvar funksjonseigar | Eigne reglar og dataansvar; delte kanal-/identitetsmekanismar blir brukte, ikkje kopierte. |
| Kapabilitet: identitet og økter | Identitet/tryggleik | Browser, native, MCP og vertikalar: identitet, validering og tilbakekalling. Felles implementasjon/Authentik; ingen lokal tokenvalidering per vertikal. |
| Kapabilitet: media | Media/plattform | Chat, arrangement, bilegenerering og agentar: tilgang, kvotar, original/variant, publisering og sletting. Gjenbruk MediaStore og eksisterande arbeidar. |
| Kapabilitet: varsling | Varsling/plattform | Chat og andre publiserande vertikalar: varig outbox, preferansar og avgrensa ekstern levering. Felles tilstand og implementasjon. |
| Kapabilitet: modelltilgang | Modell/plattform | Agentdialog, minne, emne og bilegenerering: autorisert endepunkt, budsjett, timeout og admission per faktisk leverandør. Gjenbruk #231 og minnearbeidet. |

Modelltilgang er førebels eit **ansvarskart**, ikkje påstand om at alle forbrukarar nyttar éin implementasjon. `topics/worker.rs` har eigen modellklient; agentdialog/minne har delt admission. Før kapasitet blir endra, må ein kartleggje om dei brukar same avgrensa modellressurs. LLM og ComfyUI skal ikkje få éin generell lås berre fordi begge er modellar.

Ingen ny lokal kapabilitetsimplementasjon er nødvendig i første etappe. Dersom rein formattering seinare blir implementert i fleire klientar, skal ein versjonert spesifikasjon og samsvarstestar styre åtferda. Autentisering, publiseringskontroll og tilstandsberande kvotar skal framleis ha autoritativ felles implementasjon.

## 3. Leveranse A – mål venting utan å endre kapasitet (#353)

Status 10. november: implementert og prøvd lokalt. [Målekontrakt og lokale bevis](operations.md#database-and-chat-capacity-observations)
dokumenterer faktisk dekning og avgrensingar. Kapasitetsprøva i B og driftsbudsjettet står framleis att under #353.

Dette er det første konkrete implementeringsoppdraget, som éin avgrensa PR. Eigarroller: drift og chat-kjerne. Ingen datamigrering, nye driftsroller, høgare grenseverdiar eller SQL-omskriving på tvers av heile repoet.

**Føreslege endringssett:** ein liten målemodul under `src/operations/`, kopling til `/metrics`, tidsstempel rundt chatkommandoane og eksplisitt målt PostgreSQL-acquire på det avgrensa utvalet nedanfor. Bruk eksisterande stil med faste enumar og histogram; ikkje innfør eit nytt observabilitetsrammeverk.

| Måling | Presis definisjon | Første dekning |
| --- | --- | --- |
| Poolstorleik, idle og konfigurert maksimum | Punktverdiar frå delt pool; ikkje tolka som ventetid | Webprosessen sin delte PostgreSQL-pool |
| Pool-acquire | Frå faktisk acquire blir polla til samband, timeout, stengd pool eller kansellering | Persistens for vanleg meldingssending, historikkinnhenting brukt ved reconnect, og push claim/complete i prøvevegen |
| Venting på postkasseplass | Frå produsent søkjer plass til kommandoen er teken inn; kansellering/stengd kø tel separat | Alle produksjonsinngangar til `ChatActor`, også eksterne meldingar og vedlikehald |
| Venting i postkassa | Frå kommandoen er teken inn til aktøren byrjar han | Same inngangar, med internt kommandotidsstempel |
| Utføring | Frå aktøren byrjar til kommandoen er ferdig | Faste kommandoklassar; ikkje bland med køventing |
| Ventande produsentar og købelegg | Noverande tal og observert topp | Prosesslokalt; gjer manglande produsentgrense synleg |

Mål sjølve `pool.acquire()` før ein transaksjon blir starta; tid rundt `pool.begin()` omfattar meir enn acquire. Flytt berre dei utvalde databaseoperasjonane til eksplisitt samband og bevar transaksjonsgrenser. Bruk drop-basert sluttregistrering for kansellering slik at eit avbrote forsøk ikkje forsvinn eller blir telt to gonger. SQLx-callbackar åleine er ikkje bevis for venting før tildeling, timeout eller kansellering.

Dekninga skal stå ved metrikknamna og i dokumentasjonen: **utvalde acquire-forsøk, ikkje alle databasekall**. Registrer nøyaktige metodar og eventuelle ekstra acquire i desse vegane ved implementering. Globale size/idle-tal gjev støtte, ikkje total ventestatistikk. Utvid berre når prøva viser eit konkret udekt behov; ikkje bygg ein generell SQLx-proxy.

Tillat berre faste labelverdiar for operasjonsklasse, resultat og poolrolle. Ingen kanal, brukar, agent, request-id, rå SQL eller feilmelding i labels. Behald den eksisterande feilkontrakten og postkassa si FIFO-/kanselleringsåtferd. Rein måling skal ikkje innføre timeout eller drop av tidlegare akseptert arbeid.

**Akseptanse for A:**

1. Ein PostgreSQL-prøve held einaste samband og viser målt venting, suksess etter frigjeving, timeout og kansellering kvar for seg. Eksisterande delingsprøver stadfestar at det ikkje er laga nye poolar.
2. Ei kontrollert aktørprøve skil full postkasse, tid bak ein blokkert kommando og sjølve utføringa. Feil/kansellering nullstiller in-flight-tal, og kvar kommando får høgst eitt utfall.
3. Metrikkprøva dokumenterer øvre tal på seriar og at identifikatorar/innhald ikkje kjem ut. Full dekning av dei erklærte inngangane blir kontrollert.
4. Eksisterande sending, idempotens og reconnect-prøver består. Eit lite samanlikningsgrunnlag med måling av/på kontrollerer overhead; påstand om kapasitet blir utsett til B.
5. CI-utvalet følgjer aktuell policy for endra backend-/runtimefiler. Ingen ny restore-øving berre fordi metrikken er ny.

Tilbakeføring: reverser målekoden. Ingen data eller konfigurasjonsgrenser er endra; dokumenter eventuell endra metrikknamngjeving før dashbord tek ho i bruk.

## 4. Leveranse B – avgrensa kapasitet og avgjerd (#353 → #354)

**Oppfølging 10. november:** [lokal kapasitetsprøve og oppdatert ressurskartlegging](database-capacity-assessment-2026-11-10.md) er gjennomførte. 30 valde prøver samanliknar 1/2 API ved fast poolbudsjett, med ekte emnearbeid og reconnect. Avgjerda er å behalde forma. Heart sitt samtidige utrullingsbudsjett må avklarast først; samla clusterbudsjett og eigaraksept står att under #353. Målekoden i [PR #402](https://github.com/hbjoroy/sproyt/pull/402) er føresetnad; ingen av desse leveransane er utrulla som del av prøva. Resten av avsnittet er gjennomføringskriteria.

Avhengnad: A og eit **faktisk isolert** prøvemiljø. Canary deler database/arbeid med produksjon og er ikkje automatisk isolert. Bruk syntetiske data, eigne køar og avgrensa/stubba push-/modellendepunkt. Kontroller `wslc --help` før lokal containerbruk; bruk den etablerte DevCache-rutinen ved Cargo-bygg.

Drift samlar lesande oppdatert konfigurasjon og tidsseriar før budsjettet blir godkjent. Skil målt bruk, konfigurerte tak og ukjent for API, arbeidarar, LISTEN-samband, migrering, Heart, identitet, backup og andre databaseklientar. Chart-standardar er ikkje driftsfakta.

Tilkoplingsrekneskapen må halde for normal drift **og** utrullingsscenario:

`sum(samtidige prosessar × pooltak + dedikerte samband) + migrering/backup + avtalt reserve ≤ faktisk tilgjengelege PostgreSQL-plassar`.

Ta med ekstra og terminerande poddar, overlapp prod/canary og andre appar på same database. Uavhengig workerpool kan auke totalen sjølv om API-poolen er uendra. Infrastruktur-/identitetseigaren må godkjenne reserve; septembertalet 97 er ikkje dagens grense.

**Minste prøvematrise:** først éin og to API-prosessar med fast worker-tal; samanlikn vanleg trafikk, éin travel + fleire rolege kanalar, og same trafikk med push/representativt bakgrunnsarbeid. Legg til reconnect under last. Auke til fire API-prosessar eller fleire arbeidarar krev at første resultat gir eit relevant spørsmål og at prøvebudsjettet toler det.

Registrer inputlast, datasett, varigheit, revisjon og konfigurasjon saman med p50/p95/p99 for send/reconnect, throughput, poolventing, alle tre chatfasar, køalder, feil, CPU/RSS, databasebruk og lagring. Rolege og travle kanalar kan skiljast i prøveklienten utan kanal-ID i produksjonsmetrikkane. Gjenbruk [eksisterande recovery-gates og MCP-verktøy](operations.md); dei beviser ikkje åleine påverknad mellom fleire kanalar.

Foreslått start er minst 1 000 sendeforsøk per lastprofil og tre like repetisjonar etter oppvarming; rapporter faktisk tal og usikkerheit, også når målet ikkje er nådd. Auke last kontrollert til første metting eller avtalt CPU/minne/DB-grense; stopp der. Eksterne feil og krasj skal berre injiserast i isolert miljø.

Avgjerda frå B skal vere éi av: **behald** eksisterande form, **effektiviser** dokumentert flaskehals, eller **isoler** namngjeven arbeidar. Ved global chatkøkopling vurderer ein først dyre kommandoar/SQL; eventuell kanalpartisjonering krev eiga bestilling og bevart rekkjefølgje på tvers av replikaer. Ingen automatisk auke av pooltak.

## 5. Leveranse C – kontodata som første vertikal (#355)

Dette er eit konkret forslag til modularitetsprøve, ikkje nytt eksportprodukt. Behovet er vidare datalivsløpsarbeid i #356 og nye dataområde. #351 har allereie gitt eit eigna lite skilje med eksporttypar og SQL-hjelparar. Alternativa er å halde alt i chat-kjernen, eller starte i agentdialog/minne; det første bevarer koplinga, det andre har større publiserings- og personvernrisiko.

Avhengnad: ansvarskartet i punkt 2 og ein kort gjennomgang av eksisterande eksportkontrakt. C kan gjerast uavhengig av endeleg workerval. Eigarroller: kontodata/personvern og vedlikehaldarane av chat/persistens.

- Innfør ei smal `AccountDataService` og ein `AccountExportRepository` (arbeidsnamn). Flytt eksportadmission og `start_user_export` frå `ChatEngine`, og eksportmetoden frå `ChatRepository`, utan å endre JSON, HTTP-feil, tidsgrenser, snapshot eller låsing.
- La dei same PostgreSQL-/SQLite-adapterane implementere eksportkontrakten og bruke eksisterande SQL-eksport. Ikkje del transaksjonen i tenestekall, nye poolar eller per-vertikal snapshot; konsistent eksport krev framleis éi transaksjonsgrense.
- Gjer eksisterande `upsert_user` før eksport til ein eksplisitt identitets-/profilavhengnad. Bevar feltvern og tidsfrist; ikkje gøym ein full `ChatRepository` bak det nye eksportgrensesnittet.
- Komponer tenesta éin gong i `server.rs`/`AppState` og la eksisterande konto-adapter bruke henne. Alle klonar må dele same admission; eitt nytt semaforobjekt per førespurnad ville oppheve #351-grensa.
- Flytt relevante testhjelparar og dokumenter eksportbidrag frå andre område. Dataeigaren bestemmer kva felt som høyrer med; kontodataeigaren eig snapshot-/formatkontrakten. Ingen dynamisk pluginregistrering er nødvendig.

**Akseptanse:** eksportvegen avheng ikkje lenger av `ChatEngine` eller eksportmetode på `ChatRepository`; wireformat og tilgang er identiske; SQLite/PostgreSQL-kontraktane for snapshot, timeout, disconnect og admission består. Gjenbruk #351 sine 4k/40k-målingar medan føresetnadene held. Ny lastprøve krev ei reell endring i ressursåtferd, ikkje berre flytta kode.

Gjer modulgrensa etterprøvbar med ei lita import-/avhengnadssjekk. Oppdater CI-testkart for nye stiar; delte adapterfiler kan framleis krevje brei backendkontroll. Ingen lovnad om smal CI berre fordi ein fil er flytta. Frontend, innstillingar og kontosletting er uendra i denne leveransen. Tilbakeføring er kode-revert utan datamigrering.

## 6. Leveranse D – første sjølvstendige arbeidar (#354)

**Førebels kandidat: push.** Han har eiga teneste, varig outbox og ekstern sideeffekt som kan feile uavhengig av chat. Endeleg val krev B og kontrollane nedanfor. Mediaarbeidaren er allereie isolert. Agentdialog/minne har meir komplisert versjons-/publiseringsavhengnad; emneanalyse er ny og må vere med i målingane før valet blir låst.

| Rolle | Oppstart og avhengnader | Helse, avslutting og budsjett |
| --- | --- | --- |
| Eksisterande web/combined | HTTP/WS/MCP, chat, repository og dagens tillegg | API-readiness gjeld chatkrava. Uendra standardåtferd ved innføring. |
| Web med pusharbeidar av | Same API og pushinnstillingar/outbox-admission; ingen pushdrenering | Skal ikkje trenge leveringsklient eller privat VAPID-nøkkel berre for å tilby innstillingane. Andre innebygde arbeidarar er framleis der; dette er ikkje ei rein API-rolle. |
| Føreslegen `push-worker` | Berre konfigurasjon, avgrensa eigen pool, NotificationService sin leveringsdel og operasjonstilstand | Éin jobb om gongen i første steg. Eigen liveness og framdrift/køalder; feil hos leverandør er ikkje API-unready eller automatisk restartstorm. |
| Eksisterande `media-worker` | MediaStore, eigen pool og avgrensa dekodingsprosess/scratch | Gjenbruk eksisterande ressurs-/shutdownreglar; ta med i samla budsjett. |
| Andre tillegg | Bilete, emne, agent/minne, Heart/prosesspilot, arbeidsoppgåver, opprydding | Kartlegg flagg, pool/modellbruk, lease og oppstartsfeil. Flytting er utsett til konkret behov. |

Same applikasjonsimage med ny subkommando og eksplisitt flagg for pushdrenering er minste føreslegne mekanisme. Arbeidaren skal ikkje initialisere full `AppState`, OIDC, `ChatEngine`, realtime-lyttarar eller andre arbeidarar. Nye verdiar skal vere validerte og kompatible med dagens standard; ikkje kall eksisterande webrolle «rein API» før det er sant.

**Observert føresetnad som må løysast før pushflytting:** [claim/complete](../src/notification.rs) brukar 30-sekunds lease utan jobbeigartoken i fullføringa. Delivered oppdaterer outbox og abonnementsstatus, Retry frigjev lease, og Expired slettar abonnement. [HTTP-timeout på 15 sekund](../src/notification/web_push.rs) beviser ikkje tryggleik etter prosesspause og leaseovertaking.

Prøv fyrst gammal arbeidar som fullfører etter ny eigar. Dersom prøva stadfestar utrygg åtferd, lever ei separat, avgrensa føresetnadsretting med lease-generasjon/token og atomisk eigarkontroll for **alle** sluttutfall og tilhøyrande abonnementsendringar. Å verne berre `delivered_at` er utilstrekkeleg. Gammal kode handhevar ikkje nye token automatisk.

**Overgang og tilbakeføring:**

1. Dokumenter alle prod-/canary-konsumentar av same outbox, skjema og image. Dersom fencing blir endra, stopp/drener alle gamle pushkonsumentar før ny garanti blir teken i bruk; set av eit avgrensa leveringsopphald medan chat framleis lagrar outbox.
2. Utfør eventuell additiv migrering og oppgrader alle relevante konsumentar til kompatibel kode. Nye jobbtypar eller schemaforventningar blir ikkje aktiverte før heile forbrukarsettet toler dei.
3. Slå av innebygd pushdrenering og start den separate arbeidaren med budsjett godkjent i B. Dokumenter faktisk overlapp og køalder. API-tal og pusharbeidartal skal kunne endrast kvar for seg.
4. Ved SIGTERM: slutt å claime, avgrens draining innan termineringsfristen og la uavklart arbeid bli teke opp att etter lease. Ukjent ekstern leveringsutfall kan gi duplikat; ikkje lov «exactly once».
5. Tilbakefør ved å stoppe ny arbeidar og aktivere innebygd drenering i **kompatibel** kode. Behald additive felt. Ikkje rull tilbake til ein binær som omgår nye eigar-/publiseringskontrollar.

**Akseptanse:** worker-restart/OOM, feil konfigurasjon og treg/utilgjengeleg pushleverandør tek ikkje ned vanleg chat; 1→2 API-replikaer endrar ikkje pusharbeidartalet; ingen utilsikta listenerar/andre arbeidarar startar; stale fullføring kan ikkje endre ny eigar sitt arbeid eller slette abonnement. Køalder og ressursbruk held det avtalte prøvebudsjettet. Eksterne duplikat og retry er dokumenterte.

Dersom målingane ikkje forsvarer flyttinga, er eit grunngjeve avvik med eigarrolle og ny vurdering ved aukande køalder, oppstartsfeil eller trafikk tilstrekkeleg. Leasefunnet blir likevel vurdert som konkret korrektheitsarbeid.

## 7. Leveranse E – datalivsløp og operatørveg (#356)

Start parallelt med A; dette krev ikkje workervalet. Eigarroller: produkt/personvern, tryggleik og drift. Planen tek ikkje stilling til lovgrunnlag eller juridisk etterleving. Før prosedyren blir vedteken, må ansvarleg verifisere faktisk bruk, jurisdiksjon og gjeldande krav mot autoritative kjelder.

Lag eitt kort register med datatype, føremål, eigar, tilgang, faktisk leverandør, retensjon, eksport/sletting og backupetterslep. Ta med identitet/økter, chat og media, agentminne/kjeldebevis, modell-/ComfyUI-jobbar, pushabonnement/outbox, arbeidsoppgåver/Heart, arrangement/deltaking/bilete, emne/kjeldekopiar/jobbar, revisjonsloggar og eksterne kopiar. Skil kjende grenser frå avgjerder som manglar; ikkje finn på retensjonstal.

Skriv ein gjennomførbar kontoslettings-/tilbakekallingsprosedyre: verifiser mandat og identitet, stans ny tilgang/arbeid, tilbakekall økter og agentrettar, handter ventande jobbar, utfør vedteken sletting eller anonymisering, følg opp eksterne kopiar og dokumenter sluttkontroll. Bevar andre brukarar sine data og grunngjev eventuelle unntak. Kartlegg kva som faktisk kan gjerast med dagens verktøy; manglande kontrollar blir små konkrete oppgåver.

Gjenbruk [agentminne sin restore-reset](agent-memory-worker-operations.md). Han dekkjer ikkje automatisk sletting i alle andre område. Avklar korleis sletting/tilbakekalling etter backupdato blir handheva etter restore; nødvendig register eller annan mekanisme må ha eige vedtak. Ingen gjenoppretting skal opne gamle samtykke eller starte att tilbakekalla jobbar ukritisk. Ny full restore-prøve blir nødvendig berre dersom gjenopprettingsføresetnadene blir endra; elles gjenbruk gyldig bevis.

Lag enkel operatørveg for rapport, avgrensa undersøking, mellombels avgrensing/suspensjon, avgjerd og retting av feilaktige inngrep. Definer kven som får sjå kva, lagring av saksbevis og eskalering. Dette er ikkje mandat til generell innhaldsovervaking.

**Akseptanse:** bordøving med (a) kontosletting medan ein jobb er i arbeid, (b) restore frå før tilbakekalling og (c) feilaktig suspensjon som blir retta. Dokumenter ansvar, manglande verktøy og observerbart sluttresultat. Uavklarte hol får eigarrolle og frist/utløysar før breiare bruk; akseptert #170-arbeid blir ikkje gjenteke.

## 8. Kort styringsoversikt og ferdigdefinisjon

| Mål eller grense | Utgangspunkt / avtale | Kven avklarer og når |
| --- | --- | --- |
| Send/reconnect | Eksisterande mål: 99 % kvitterte sendingar innan 750 ms; 99 % reconnect innan 5 s | Chat/drift; B dokumenterer last og om måla held. Ikkje påstå dagens måloppnåing. |
| Varig levering | Ingen kvitterte meldingar tapte; retry gir same logiske melding; rekkjefølgje per kanal | Chat-kjerne; eksisterande kontraktprøver + relevante feilscenario ved A/D. |
| Database | Standardpool fire, acquire-timeout 5 s er kodefakta; samla driftsgrense/reserve ukjend | Drift/infrastruktur/identitet; før B blir godkjent og før D blir aktivert. |
| CPU/minne/lagring | Chartgrenser er utgangspunkt, ikkje målt budsjett. Eksportgrenser og mediaarbeidarens scratchgrenser skal bevarast | Drift og funksjonseigar; samla normal-/utrullingsrekneskap i B. |
| Kø/modelleverandør | Chatpostkasse 1 024; ventande produsentar og samla modellpress må målast | Chat-/modellrolle; B avgjer behov for ny admission, ikkje A. |
| Personvern og retensjon | Agentminne har konkrete reglar; samla register og prosedyrar manglar avklaring | Produkt/personvern/drift; E før breiare bruk. |
| Iterasjon | Risikobasert CI er levert; bruk gyldig bevis og oppdater sti-/testkart ved faktiske grenser | Vedlikehaldar for kvar PR; brei regresjon etter gjeldande policy. |

Avvik som er medvitne no: delvis poolmåledekning (utvid ved udekt flaskehals), innebygde tillegg (vurder etter B), ukjent samla ressursbudsjett (må lukkast før skalering), og uavklart personvernlivsløp (må lukkast eller avgrense bruk før utviding). Eigarrollene i tabellen skal få namngjevne ansvarlege før oppgåvene startar.

Etappen er ferdig når #353 har eit etterprøvbart budsjett/avgjerd, #355 har éi åtferdsbevarande utskiljing, #354 har eit prøvd avgrensa rollesteg eller uttrykkeleg akseptert avvik, og #356 har eit vedteke register og prøvde prosedyrar. Før då skal ingen av dei merkast fullførte berre fordi planen finst.

Avslutt med ei kort oppfølging i #345 som viser resultat, gyldig bevis, eigarar og gjenståande avvik. Oppdater `ARCHITECTURE.md` først når val er blitt kode. Neste ordinære revisjon er seinast 5. februar 2027, eitt kvartal etter første revisjon 5. november, eller tidlegare ved vesentleg trafikkauke, delt kapasitetskrise, endra databruk eller utrullingshending. Ingen kalenderautomatisering eller GitHub-endring er oppretta som del av denne planleveransen.
