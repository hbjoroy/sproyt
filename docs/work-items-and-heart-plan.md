# Plan: applikasjonssaker gjennom Sprøyt og Heart

Dato: 2026-09-28. Status: den avgrensa iterasjonen 0a er implementert og testa mot ekte Heart. Resten av saksfunksjonaliteten er framleis ein plan. Sjå [pilotkontrakten og driftsstatus](process-user-task-pilot.md).

## Mål og første leveranse

Ein brukar kan gjere ei melding om til ei sak for ei applikasjon som kanalen
er konfigurert for. Produktbehandlaren får ei oppgåve, kategoriserer og
prioriterer saka, og vel om ho også skal publiserast i GitHub Issues.
Alle saker vert lagra i eit enkelt register i Sprøyt, også dei som får ei
GitHub-lenkje. Først vert Sprøyt og Utpå sette opp, med Harald som behandlar.
Datamodellen og tilgangsreglane skal støtte fleire behandlarar frå første dag.

Det skal framleis vere ein samtaleapp: brukaroppgåver vert presenterte som
samanfalda, interaktive meldingar i vanlege kanalar som er konfigurerte
for den aktuelle oppgåvetypen og prosessrolla. Innboksen er ikkje ei
samleliste for alle prosessoppgåver.

## Utgangspunkt ved den første planlegginga

- `src/process.rs`: Heart-adapter, varig outbox, retry og prosesslenkjer.
- `src/db/{postgres,sqlite}.rs`: prosessstart krev kanalmedlemskap og
  kretsflagget `heart.event-planning`. Dette er ikkje ei allowlist av
  prosessar per kanal. Klienten sender namespace og definisjonsnamn.
- `src/web/processes.rs` og `src/web/mcp.rs`: fleire inngangar må bruke dei
  same nye tilgangsreglane; det er utilstrekkeleg å skjule ein UI-knapp.
- `helm/sproyt/definitions/event-planning.yaml`: eksisterande pilot nyttar
  automatiske Lua-steg og eit receive-steg for menneskeleg avgjerd.
- Eksisterande `user_tasks` er personlege open/done-oppgåver knytte til
  omtalar i meldingar. Dei har ikkje behandlargruppe, kategori eller sakslivsløp.
- `src/imagegen_prompt.rs`: Santorini vLLM-adapter finst, men har
  biletespesifikke instruksjonar. Gjenbruk transport og konfigurasjon,
  ikkje bileteprompten eller den lange arbeidsflyten.
- Heart-kjeldene finst ikkje i den venta lokale katalogen
  `S:/Source/heart`; gjennomgangen stadfestar Sprøyt-kontrakten, ikkje
  full funksjonalitet i Heart.
- `ProcessGateway::correlate` sender korrelasjonsheader, men ikkje
  dokumentert idempotensnøkkel. Inspeksjon lagrar ei hending utan å
  oppdatere statuskolonnen; endeleg transportfeil kan setje lenkja til
  `failed`. Denne kolonnen kan ikkje nyttast direkte som saksstatus.

Heart sin faktiske kontrakt for menneskeoppgåver og hendingar må avklarast
før implementering. At vegkartet seier «work items», stadfestar ikkje at
rett claim-/completion-API allereie finst i den utrulla versjonen.

## Ansvarsdeling

Sprøyt eig saksregisteret: applikasjon, innmeldar, kjeldemelding,
stadfesta tittel/beskriving, kategori, prioritet, synleg saksstatus,
tilgangsreglar, revisjonshistorikk og GitHub-lenkje.

Heart eig prosessinstansen: definisjonsversjon, aktive steg, ventepunkt,
prosessframdrift og feilhandsaming. Ein node er eit steg i ein
prosessdefinisjon. Nokre steg er automatiske; andre er brukaroppgåvesteg.
Når ei instans kjem til eit brukaroppgåvesteg, opprettar/aktiverer Heart
ei konkret brukaroppgåve og ventar på at ho vert fullført. Automatisk
arbeid og andre nodetypar vert ikkje viste som brukaroppgåver i Sprøyt.

Sprøyt-integrasjonen finn nye/endra brukaroppgåver for aktuelle brukarar
og prosessroller og presenterer dei i rett konfigurert kanal. Meldingane
er presentasjon av Heart-oppgåvene, ikkje nye, uavhengige oppgåver eller
prosessnoder. Saka er produktobjektet som kan leve vidare etter steget.
Sak, prosessinstans, brukaroppgåve og meldingspresentasjon har ulike ID-ar
og eksplisitte relasjonar.

Dette er ei føreslått utviding av produktgrensa i `docs/roadmap.md`.
Saksregisteret vert eit avgrensa `WorkItemService`-ansvar med typed
kommandoar og repository-kontrakt, ikkje generell lagring i chat- eller
Heart-metadata. Vegkartet skal presiserast når dette arkitekturvalet
vert vedteke. Saksstatus, stadfesta Heart-status og integrasjonsstatus
er tre separate verdiar med ulike eigarar.

Minimale nye domeneobjekt: `Application`, `ChannelProcessBinding`,
`WorkItem`, `WorkItemRevision`, `ProcessTask`/oppgåveprojeksjon og
`GitHubExport`. Seinare kjem `DevelopmentRequest`. Saks-ID,
prosesslenkje-ID, instans-ID og oppgåve-ID er separate. Oppgåvenøkkelen
omfattar sak, steg og aktivering slik at eit seinare gjenopna steg kan
lage ei ny oppgåve utan å duplisere den førre.

Eksisterande personlege gjeremål held fram separat. «Ferdig» på eit slikt
gjeremål skal aldri fullføre eit Heart-steg. Innboksen kan seinare varsle
og lenkje til oppgåvemeldingane, men sjølve behandlinga skjer i kanalen.

Steg 0 må kontrollere Heart-kontrakten for brukaroppgåver, tildeling,
oppdagelse og fullføring. Dersom han manglar, planlegg nødvendige
utvidingar eksplisitt. Eit generisk receive-steg er ikkje i seg sjølv
ein ferdig kontrakt for brukaroppgåver, og skal ikkje innførast som
erstatning utan ei eiga arkitekturavklaring.

## Kanal, applikasjon og tilgang

Føreslått konfigurasjon:

| Objekt | Ansvar |
| --- | --- |
| Applikasjon | Stabil ID, namn, aktiv/inaktiv, serverstyrt GitHub repo-ID og eksportpolicy |
| Kanal–prosess | Tillatne prosessnøklar og pinnede definisjonsversjonar |
| Kanal–brukaroppgåve | Tillatne oppgåvetypar/steg og prosessroller som skal presenterast her |
| Brukar–prosessrolle | Kven som kan utføre ei konkret oppgåve i prosessens applikasjons-/forretningsområde |
| Kanal–applikasjon | Applikasjonar som kan meldast i kanalen, per prosess |
| Applikasjon–behandlar | Brukarar/grupper som kan lese og behandle saker |
| Saksvisning | Kva innmeldar, kjeldekanal og behandlarar kan sjå |

Alt er avslått utan eksplisitt konfigurasjon. Første oppsett er éin
innmeldingskanal for Sprøyt og éin for Utpå, kvar med si applikasjon.
UI skal be brukaren velje/stadfeste applikasjon også når lista har eitt val.
Medlemskap i Utpå-kanalen gjev ikkje tilgang til Sprøyt-saker.

Serveren kontrollerer rettar per handling: registrering krev tilgang til
kjeldemeldinga, skrivande kanalmedlemskap og tillaten prosess/applikasjon.
Kanalmedlemmer kan førebels lese oppgåvemeldingar i kanalen, men berre
tildelt/kvalifisert brukar med rett prosessrolle kan utføre handlingane.
Read-only-visninga er handheva på serveren, også når nokon kallar API direkte.
Kanalmedlemskap åleine gjev ikkje utføringsrett. Det som vert lagt i
oppgåvemeldinga, er medvite synleg for alle kanalmedlemmer; intern
informasjon som desse ikkje skal sjå må ikkje publiserast der.
Lesing av resten av ei sak, claim og behandling krev saks- og applikasjonsrett;
opning av originalmeldinga/tråden krev framleis kanaltilgang.
GitHub-eksport og utviklingsstart krev eigne rettar i tillegg til
behandlingsrett. Klienten får ikkje velje
vilkårleg Heart-definisjon eller repository; ved registrering er
behandlargruppa serverstyrt. Omfordeling krev eigen rett og serverkontroll
av at føreslått mottakar er kvalifisert. Alle HTTP-, WS/SSE-
kommandoar og MCP-kall går gjennom same applikasjonsteneste.

Behandlarrett er ein eigen rett per applikasjon; kanaleigar/moderator får
ikkje automatisk denne retten. Retten til å konfigurere prosessar og
applikasjonar er administrativ og vert loggført.

Registrerte saker pin definisjonsversjon og konfigurasjonsversjon.
Deaktivering stansar nye registreringar; eksisterande saker kan framleis
behandlast av autoriserte behandlarar. Tilbakekalling av ein brukar sin
tilgang gjeld straks, også for lister, lenkjer, vedlegg og sanntidshendingar.

## Same kanal eller separate kanalar

Same prosess kan presentere ulike brukaroppgåver i ulike kanalar,
etter oppgåvetype og prosessrolle. Ein person går inn i kanalen der han
har den aktuelle rolla for å arbeide. Kanalane kan samle fleire
prosessar innan same forretningsområde, men berre eksplisitt tillatne
oppgåvetypar vert publiserte der. Det konkrete kanaloppsettet vert prøvd
ut i pilot; det skal ikkje vere ei universell, blanda oppgåveliste.

Saka får ei uforanderleg kjeldelenkje, og kvar oppgåvepresentasjon har
si kanalbinding. Ho vert ikkje «flytta» ved å flytte eller
kopiere heile samtalen, og prosessen legg ikkje automatisk folk til kanalar.

Føreslått pilot: innmelding i ein brukarkanal og produktbehandlaroppgåva
som melding i ein konfigurert behandlingskanal for same prosess.
Dei kan vere same kanal dersom rollene og synlegheita tilseier det.
Behandlaren kan lese den registrerte saksbeskrivinga gjennom si
applikasjonsrett, men kan berre opne original samtale dersom han òg har
kanaltilgang. Interne notat og offentleg tilbakemelding er ulike felt.

Innmeldaren ser eigne saker og offentleg status. Om resten av kjeldekanalen
skal sjå sakene, er eksplisitt kanalpolicy. Eventuelle statusmeldingar i
kanalen inneheld berre den offentlege projeksjonen. Ein behandlar kan
godkjenne offentleg tilbakemelding utan tilgang til originaltråden;
ein serverstyrt, avgrensa publiseringsveg kontrollerer saka si kanalbinding
og policy før han skriv statusmeldinga. Dette gjev ikkje behandlaren
generell skrivetilgang i kanalen. GitHub-lenkje skal
ikkje visast til aktørar som eksportpolicyen ikkje tillèt.

## Brukar- og behandlarflyt

1. I meldingsmenyen kjem **Lag Issue**, berre der prosessen er tillaten.
2. Brukaren vel applikasjon frå serverens tillatne liste.
3. Santorini føreslår ein kort tittel frå den valde meldinga. Brukaren kan
   redigere tittel og saksbeskriving før registrering. Ingen andre
   kanalar eller heile historikken vert sende til modellen.
4. **Registrer** lagrar saka og køyrer prosessstart gjennom varig kø.
   Brukaren får saksnummer og «Registrert / ventar på behandling».
5. Prosessen kjem til brukaroppgåva «Vurder saka» og ventar.
   Sprøyt-integrasjonen finn oppgåva for Harald si produktbehandlarrolle
   og publiserer éi samanfalda oppgåvemelding i konfigurert kanal.
   Harald opnar meldinga og får handlingane. Andre kanalmedlemmer får
   ei lesbar visning utan utføringsrett. Ved fleire behandlarar må
   tildeling/claim følgje Heart-kontrakten, atomisk og loggført.
6. Behandlar vel kategori og prioritet, kan be om meir informasjon,
   og vel **Lagre i register** eller **Lagre og send til GitHub**.
7. GitHub-eksport viser eigen status: ikkje vald, i kø, sendt eller feil.
   Saka er lagra sjølv om GitHub er utilgjengeleg. Retry må ikkje lage
   dobbelt issue. Ei offentleg tilbakemelding er eit eksplisitt val.

Føreslåtte kategoriar: feil, endringsønske, spørsmål/anna. Prioritet:
uavklart, låg, normal, høg, kritisk. Dette er startforslag, ikkje
hardkoding som tvingar alle framtidige prosessar inn i same skjema.

Føreslått saksstatus: ny, til behandling, treng informasjon, planlagt,
under utvikling, løyst, avvist, duplikat. Kategori og prioritet er uavhengige
av status. Duplikat peikar på ei anna sak utan å avsløre henne til
brukarar som ikkje har tilgang.

**Start utvikling** er seinare ein autorisert handling med ei separat
utviklingsbestilling. Ei slik bestilling kan visast som status, men
ein tilfeldig statusverdi eller GitHub-label er ikkje startløyve.

## Varig registrering og synkronisering

- Éin Sprøyt-transaksjon lagrar sak, kjeldesnapshot, revisjon,
  registreringskvittering og outbox for Heart-start. LLM-kallet er før
  denne transaksjonen. Heart-nedetid må ikkje miste aksepterte saker.
- Same request-ID og same innhald returnerer same sak. Same nøkkel med
  endra kanal/applikasjon/innhald vert avvist som konflikt. Retry må
  framleis kontrollere gjeldande tilgang og returnere lagra identitet.
- Registreringsskjemaet held på utkast ved feil. Melding som vert sletta
  eller endra under førebuing vert revalidert; ingen stille overskriving.
  Original snapshot, seinare revisjonar og sletting/retensjon får ein
  eksplisitt policy før pilot.
- Heart-start og oppgåveprojeksjon må kunne reparerast ved avstemming
  etter restart. Eit aktivt steg får høgst éi aktiv behandlaroppgåve.
- Heart er føreslått autoritet for brukaroppgåvene og deira tildeling/
  fullføring. Sprøyt lagrar presentasjon, kanalruting og leveringsstatus.
  Eventuelle manglar i Heart må avklarast i steg 0, utan å lage ein
  konkurrerande oppgåvemotor som skjult fallback. Avstemming bevarer nyare tildelingar.
  Tilbakekalla behandlarrett frigjev oppgåva til kvalifisert kø;
  inga kvalifisert mottakar vert vist som blokkert og varsla til administrator.
- Behandlingskommando har venta saksrevisjon og stegaktiverings-ID. Forelda
  skjema eller dobbelt completion vert ikkje brukt på eit nytt steg.
- Periodisk, avgrensa avstemming er påkravd sjølv om Heart seinare
  tilbyr hendingar. Meldingsmottak er ikkje bevis på at rett steg er
  fullført. Steg 0 må stadfeste mottaksdeduplisering og oppslag av
  usikkert resultat, eller planleggje ei nødvendig Heart-utviding.
- Skil lokal «avgjerd lagra, prosessoppdatering ventar» frå stadfesta
  Heart-framdrift. Avstemming og kontrollert retry fullfører koplinga.
  Same aktivering kan ikkje godta ei motstridande avgjerd medan den
  første ventar på levering; endring krev ein definert ny overgang.
- GitHub er først einvegseksport. GitHub-kommentarar, lukking og labels
  endrar ikkje automatisk Sprøyt-status. Tovegssynk er ei eiga seinare sak.
- Eksport har stabil lokal eksport-ID, markør i issue og resultatlenkje.
  Ved timeout etter mogleg oppretting vert utfallet avstemt før nytt
  forsøk; ved uavklart resultat stoppar blind retry og varslar behandlar.
  Godkjend eksporttekst vert lagra. Private vedlegg og interne notat
  vert ikkje automatisk sende til GitHub.
- Hemmelegheiter ligg i eksisterande secret-handtering. Bruk ei
  repository-avgrensa GitHub-integrasjon; endeleg autentiseringsval
  og repo-ID-ar må stadfestast før eksportsteget.
- vLLM får eit kort, avgrensa tekstoppdrag utan verktøy, nettforsking
  eller rett til å starte prosessar. Timeout/kapasitet gjev manuelt
  tittelval. Avgrens input/output, samtidige kall og frekvens; ingen
  meldingstekst i driftsloggar. Santorini deler ressursar med bilete/video.

## UI og mobil

Bruk `@sproyt/ui`, eksisterande dialogar, semantiske tema og meldingsmeny.
Registrering er eit lite skjema med applikasjon, tittel, beskriving og
Registrer. Behandling skjer ved å opne oppgåvemeldinga i kanalen.
Som med dei eksisterande `[[]]`-makroane får meldinga ei strukturert
visning, men ei oppgåve skal vere knytt til ein faktisk Heart-oppgåve-ID.

Samanfalda visning viser kort tittel, oppgåvetype, mottakar/rolle og
status. «Samanfalda» er berre visningstilstand, ikkje status «fullført».
Trykk opnar detaljar og, for rett brukar, kategori/prioritet og handlingar.
Andre medlemmer kan opne den same meldinga read-only. Fullført eller
kansellert oppgåve vert verande som historikk med oppdatert status;
opning, lesing eller ein vanleg chatreply fullfører henne ikkje.

Integrasjonen bruker varig oppdagelse/avstemming av nye og endra
Heart-oppgåver, med hendingar dersom tilgjengeleg og regelmessig sjekk.
Stabil oppgåve-/aktiverings-ID og kanalbinding hindrar doble meldingar
ved retry, restart og fleire Sprøyt-replikaer. Ruting skal vere eintydig
for den konkrete oppgåva, utan automatisk kopiering til alle kanalar der
mottakaren er medlem. Endra tildeling, tilbakekalla rettar og manglande
kanalbinding må handterast synleg, utan at oppgåver forsvinn i det stille.

Vis saksnummer/status og valfri lenkje diskret i meldingskonteksten.
Test lange namn/titlar, tastatur og touch, begge tema og eksisterande
mobil input-/viewport-åtferd. Vanlege kanalvarslar og ulestmarkeringar
peikar til oppgåvemeldinga. Ulest melding og ufullført brukaroppgåve
er separate tilstandar. Innboksen skal ikkje blande alle oppgåvetypar.

## Leveransesteg med stoppunkt

### Føresetnad før ekte saksbehandling: Heart runtime v2 MVP

Etter den sekvensielle minipiloten skal Heart få eit avgrensa MVP for
blanda brukaroppgåver/Lua/val og parallelle greiner med ein parvis AND-join.
Internt vert framdrift representert med varige token og aktiveringar;
YAML er framleis prosessformatet. Første versjon er ein endeleg graf utan
løkker eller nøsta forgreining. Avansert OR-join, queue/receive i v2,
kandidatgrupper og automatisk konvertering av instansar kjem seinare.

Spesifikasjonen ligg i Heart sitt
[runtime-v2-mvp.md](https://github.com/hbjoroy/heart/blob/codex/runtime-v2-contract/docs/runtime-v2-mvp.md), med eige
[migreringsgrunnlag](https://github.com/hbjoroy/heart/blob/codex/runtime-v2-contract/docs/runtime-v2-migration.md) for andre
Heart-brukarar. Første leveranse er implementert i
[Heart PR 7](https://github.com/hbjoroy/heart/pull/7): eksplisitt runtime,
avgrensa grafvalidering, v2-tilstandsformat og additivt schema 006 med eigne
v2-tabellar. V1-oppgåver og fullføringskvitteringar vert bevarte, prova med
ein faktisk PostgreSQL-oppgraderingsprøve. V2-start er framleis sperra.
105 lokale workspace-testar, strict clippy og chart-rendering passerte;
Astra har kontrollert arkitekturen og integrasjonsgrensa. Heart v2 er ikkje
utrulla; vidareføring/kansellering og fork/join-motor er neste leveransar.

Sprøyt treng ikkje migrering av gamle forretningsprosessar: den tidlegare
integrasjonen har ikkje vore teken i praktisk bruk. Eventuelle aksepterte
Prosesstest-instansar skal likevel inventerast og fullførast/bevarast.
Nye instansar får ein ny, pinna v2-definisjon. Heart held fram med v1
for andre eksisterande brukarar og instansar; inga stille omtolking.

Gjennomføring: (1) kontrakt/schema og v1-kompatibilitet, (2) sekvensiell
blanda v2-motor med varig vidareføring/kansellering, (3) fork/join med
konkurranse-/restart-testar, (4) generell Sprøyt-projeksjon og canaryprøve.
Adapteren må slutte å føresetje to sekvensielle oppgåver og bruke faktisk
aktiverings-ID ved projeksjon. Heart avgjer når join er ferdig.

Canaryprøva i Rocket-admins → Prosesstest får to parallelle oppgåver for
Harald, etterfølgde av ei siste stadfesting når begge er fullførte.
Test begge rekkjefølgjer, samtidige fullføringar, restart, dobbelt trykk,
kansellering, feil og lesetilgang for andre medlemmer. Ingen GitHub- eller
utviklingsautomatikk vert aktivert. Astra-review, grøn CI og manuell
canaryaksept er krav før produksjon. Dette vert neste føresetnad før
steg 1–3 vert tekne i ekte bruk; den eksisterande minipiloten står som
regresjonstest. Fleire små PR-ar, ikkje ei samla motoromskriving.

### Tidleg minipilot: to brukaroppgåver med overlevering

Før saksregister, tittelforslag og GitHub-integrasjon byggjer vi ei svært
lita ende-til-ende-iterasjon i **Rocket-admins → Prosesstest**.
Kontroller faktisk krets-/kanalidentitet før oppsett; kanalen vert berre
oppretta dersom han ikkje allereie finst. Berre denne testprosessen og
dei to oppgåvetypane vert aktiverte der. Ingen applikasjonseksport eller
utviklingsautomatikk er med.

Prosessen har to sekvensielle brukaroppgåvesteg, begge tildelte same
testperson (Harald i første prøve):

1. **Steg 1 – Første stadfesting.** Ein eksplisitt start frå kanalen
   opprettar instansen. Heart aktiverer første brukaroppgåve og ventar.
   Sprøyt finn henne og legg ut ei samanfalda oppgåvemelding i Prosesstest.
   Harald opnar meldinga og vel «Fullfør steg 1».
2. **Steg 2 – Stadfest overlevering.** Etter stadfesta fullføring av
   første oppgåve aktiverer Heart ei ny brukaroppgåve for same person.
   Sprøyt finn denne og legg ut ei ny samanfalda melding i same kanal.
   Harald opnar henne og vel «Fullfør steg 2». Prosessen avsluttar.

Oppgåvene har kvar sin oppgåve-/aktiverings-ID og kvar si melding.
Den første meldinga står att som fullført når den andre vert aktiv.
Steg 2 skal ikkje publiserast eller vere mogleg å fullføre før Heart
har gått vidare frå steg 1. Andre kanalmedlemmer kan opne meldingane
read-only; serveren avviser deira fullføringsforsøk.

Godkjenningskrav for denne iterasjonen:

- Éin start gjev éi instans; steg 1 → steg 2 → avslutta kan observerast
  både i Heart og i kanalens oppgåvemeldingar.
- Sprøyt oppdagar steg 2 frå den faktiske Heart-oppgåva. Klienten lagar
  ikkje steg 2 lokalt fordi nokon trykte på knappen i steg 1.
- Dobbelt trykk, refresh og repetert sjekk av Heart gjev ikkje doble
  fullføringar eller oppgåvemeldingar. Eit gamalt skjema for steg 1
  kan ikkje fullføre steg 2.
- Restart av Sprøyt mellom dei to stega mistar ikkje overleveringa;
  avstemming finn att aktiv oppgåve og eventuell manglande melding.
- Mellombels Heart-feil ved fullføring viser «ventar på stadfesting»
  og vert avstemd utan å vise ein usann framdrift.
- Harald kan opne/fullføre begge oppgåvene på mobil. Ein annan
  kanalmedlem får berre lesetilgang, også via direkte API-kall.

Start med eksisterande brukaroppgåvekontrakt dersom Heart har han.
Manglande kontrakt skal avklarast og eventuelt utvidast i steg 0;
piloten skal ikkje simulere overleveringa med to lokale gjeremål.
Lever som ein liten eigen PR og test i canary med eksplisitt kanalbinding.
Dette steget skal prove prosess/oppgåve/presentasjon, ikkje føregripe
den endelege sakshandsamingsmodellen.

| Steg | Leveranse | Krav før neste steg |
| --- | --- | --- |
| 0 | Kort kontraktavklaring med Heart: autoritet for oppgåver/tildeling/completion, receive, mottaksdeduplisering, hendingar/avstemming, tilgang og eigarskap | Vald integrasjon dokumentert og minimal kontrakttest spesifisert, inkludert timeout etter godteken avgjerd; app-/repo-identitet og Harald-ID stadfesta |
| 0a | Minipilot i Rocket-admins → Prosesstest med to sekvensielle brukaroppgåver for same person | Faktisk Heart-overlevering gjev éi ny oppgåvemelding; fullføring, read-only, retry og restart verifiserte før større funksjonar |
| 1 | Applikasjonsregister, kanalpolicy og serverhandheving | Negative tilgangstestar for HTTP/MCP og begge databasar; eksisterande Heart-pilot framleis fungerer med eksplisitt binding |
| 2 | Lag Issue, vLLM-tittel, varig register og Heart-start | Dobbel registrering, LLM/Heart-nedetid, restart og sletta kjeldemelding handtert; inga GitHub-utsending |
| 3 | Oppdagelse av Heart-brukaroppgåver, kanalruting, samanfalda oppgåvemelding, tildeling, kategori/prioritet og avgjerd | Harald gjennomfører flyten i rett kanal; andre ser read-only; to behandlarar kan ikkje fullføre same aktivering; restart gjev ikkje doble meldingar |
| 4 | Valfri GitHub-eksport | Testrepo først; timeout etter oppretting gjev ikkje dobbel issue; tilgang/eksportinnhald kontrollerte |
| 5 | Eige produktbehandlarforløp for statusendring | Autoriserte overgangar, revisjonar og offentleg/intern visning; Start utvikling framleis utan automatisk utføring |
| 6 | Avgrensa utviklingspilot | Først dry-run, deretter ei manuelt vald sak og éin aktiv jobb; eigen godkjenning av aktivering |

Steg 3 omfattar minimale overgangar ny → til behandling → treng
informasjon/ferdig vurdert/avvist. Å be om informasjon opprettar eit
definert ventepunkt og ei synleg melding; svar held fram rett aktivering.
Steg 5 utvidar til produktlivsløpet. Status «under utvikling» startar
ingen jobb før steg 6 er eksplisitt aktivert.

Steg 1–3 utgjer første nyttige vertikale leveranse. Steg 4 fullfører
førsteflyten med GitHub. Statusprosess og utviklingspilot kjem etterpå.

Kvar leveranse får eigen PR, relevante Rust-/frontend-/repository- og
Heart-kontrakttestar gjennom eksisterande CI. Nye migrasjonar er additive
og vert prøvde i SQLite og PostgreSQL; ikkje endre utrulla migrasjonar.
Publiser uforanderleg image og pinn definisjonar gjennom eksisterande
GitOps-rutine. Først canary med eksplisitt testkanal og testrepo;
produksjon vert berre promotert etter aksept. Canary deler produksjonsdata,
så global aktivering og ekte GitHub-eksport er ikkje ein trygg «UI-test».
Rollback deaktiverer nye starter/eksport, men tek vare på register,
oppgåver og allereie akseptert arbeid for avstemming.

## Seinare: kontrollert automatisk utvikling

OpenAI dokumenterer programmatisk styring gjennom
[Codex SDK](https://learn.chatgpt.com/docs/codex-sdk) og
[ikkje-interaktiv køyring](https://learn.chatgpt.com/docs/non-interactive-mode).
Dette dokumenterer byggjeklossar, ikkje at ein ChatGPT-samtale automatisk
overvaker ein vilkårleg GitHub-status. Utførar og autentisering må veljast
og prøvast i steg 6; ingen slik kopling er aktivert av denne planen.

Sprøyt lagrar ei utviklingsbestilling med eksplisitt autorisert behandlar,
godkjend spesifikasjon, fast repo/base, request-ID og kostnadsramme.
Heart styrer eit separat forløp. Ein adapter kan levere bestillinga til
vald Codex/ChatGPT-utførar. Første prøve er berre plan/dry-run. Neste prøve
kan lage ein isolert branch og draft PR for ei enkelt vald sak.

Godkjenning bind bestillinga til spesifikasjonsrevisjon og repository/
base-revisjon; vesentlege endringar krev ny bestilling eller godkjenning.
Utføraren kontrollerer gjeldande rettar og aktivering før start.

Automatikken er av som standard og krev applikasjonsallowlist,
naudstopp, maksimal samtidighet 1, tids-/kostnadsgrense og synleg logg
over kvar bestilling. Gjentekne hendingar, statusveksling og restart
startar ikkje same bestilling på nytt. Kansellering og uvisst startutfall
skal kunne avstemmast. Definer både stopp av nye starter og kansellering
av køyrande jobb, med synleg stadfesting/utfall. Ingen automatisk merge eller produksjonsutrulling
i pilot. Tekst i ei innmeldt sak eller label frå GitHub er aldri
autorisasjon for fleire jobbar, andre repo eller utvida rettar.

## Seinare: applikasjonsidear

Ein eigen, eksplisitt konfigurert prosess kan seinare handsame nye
applikasjonsidear. Han får eige skjema, behandlarrettar og godkjenningssteg
før oppretting av repo, miljø, identitet eller andre ressursar. Gjenbruk
register-/oppgåvepresentasjon og integrasjonsadapterar; ikkje gje
«Lag Issue» sideeffekten å opprette ein ny applikasjon.

## Avklaringar før implementering

1. Heart-kontrakten og kva som må endrast der, eventuelt ingen endring.
2. Kanal-ID-ar, Utpå sitt faktiske GitHub-repo og kva saker innmeldar/
   kanalmedlemmer skal sjå. Oppsettet er føreslått, ikkje oppretta.
3. Kven som kan registrere sak frå andre sine meldingar. Føreslått:
   skrivande kanalmedlemmer, med både innmeldar og originalforfattar lagra.
4. Kva kategoriar/prioritetar/statusar som er nyttige i første pilot,
   og policy for innhaldssnapshot, vedlegg og sletting.
5. Val av seinare utviklingsutførar, tilgang og reell kostnads-/køkontroll.
6. Presis oppgåvekontrakt og rolle-/kanalruting: direkte tildeling eller
   kandidatgruppe, kva innhald alle kanalmedlemmer skal sjå, og kva
   skjer dersom brukaren manglar medlemskap i den konfigurerte kanalen.

Desse avklaringane skal ikkje låse same/separate kanalar for framtida.

## Planreview

Astra har vurdert arkitekturen og gått gjennom utkastet i to separate
avsjekkar. Funna om tilgang per handling, éin tildelingsautoritet,
stegaktivering, separate statusar og usikker Heart-levering er innarbeidde.
Den sekvensielle Heart-kontrakten er no avklart og implementert for iterasjon 0a. Vidare saksregister, rolle-/kanalruting og andre grafkombinasjonar må avklarast i dei seinare stega.
Etter brukaravklaring er innbokskøa erstatta med Heart-brukaroppgåver
presenterte som samanfalda meldingar i rolle-/oppgåvekonfigurerte kanalar.
Dette er eit pilotoppsett som skal prøvast og justerast, ikkje ein ferdig
generell task-motor.
