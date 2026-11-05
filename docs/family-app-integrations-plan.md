# Integrasjon mellom familieappar

Oppdatert 5. november 2026. Sprøyt har grunnmodellen (I1). HTTP-ruter og brukarflata for Open Drinks står att.

Sprøyt skal vere samtale- og samhandlingsflata for tilknytte appar. Integrasjonen
går mellom serverane; brukarhandlingar og eventuelle godkjenningar skjer i Sprøyt.
Open Drinks er første app: ein moderator koplar ein kanal til ein sesjon,
medlemmene svarar om dei deltek, og Open Drinks hentar svara og publiserer innhald.

Den [føreslåtte API-kontrakten](family-app-integration-contract.md) er handover
til agenten som skal implementere Open Drinks. Endepunkta der finst ikkje enno.

## Eksisterande grunnlag

- `src/agent.rs` har tenesteidentitet, roterbare credentials og kanal-/kretsavgrensa
  grants. Dei fire noverande scopes dekkjer ikkje medlemsattributt eller appbindingar.
- `src/integration.rs` og `src/web/integrations.rs` har idempotent innlevering av
  Grafana-alarmar og rapportar. Dette er førebilete, ikkje eit generelt app-API.
- Kanal- og kretsmedlemskap og kretsmoderatorar finst. Integrasjonsrettar må
  kontrollerast eksplisitt; ein eksisterande agent-grant er ikkje tilstrekkeleg.
- `src/web/media_uploads.rs` har avgrensa, delt opplasting og prosesseringskø.
  Endepunkta brukar menneskeautentisering i dag. App-tilgang krev eigen adapter.
- [Prosessarkitekturen](work-items-and-heart-plan.md) og
  [skjemaplanen](process-forms-architecture-plan.md) skil fagdata frå
  Heart-instansar, aktiveringar og brukaroppgåver. Det skiljet skal bestå.

## Brukaroppleving

1. Moderator opnar **Kanalinnstillingar → Tilknytte appar → Legg til**.
   Berre appar aktiverte i systemkonfigurasjonen blir viste.
2. For Open Drinks vel moderator sesjon. Sprøyt-serveren slår opp og validerer
   sesjonen mot Open Drinks-serveren, med autorisasjon for akkurat denne
   tilknytinga. Ein kjend sesjons-ID åleine er ikkje bevis på tilgang.
3. Ei førehandsvising viser sesjonsnamn, kva appen får lese/skrive, og kva
   medlemmene må svare på. Moderator stadfestar, og bindinga blir aktiv.
4. Kanalen får eitt kompakt, varig kort: **Open Drinks · [sesjonsnamn]** med
   **Eg er med** og **Eg er ikkje med**. Kortet finst òg frå kanalinnstillingane,
   slik at nye medlemmer og dei som har rulla forbi kan finne det att.
5. Før innsending ser brukaren at visingsnamn, app-spesifikk identifikator og
   svaret blir delte med Open Drinks. Eit nei er eit aktivt svar som òg blir delt.
   Å ikkje svare deler ingenting. Vis **Endre svar** og **Trekk tilbake deling**.
6. Open Drinks publiserer vanlege meldingar med tydeleg appnamn, Markdown og
   eventuelle vedlegg. Desse skal fungere i web og framtidige native klientar.

Kortet viser eige svar, ikkje automatisk ei offentleg deltakaroversikt. Andre
medlemmer får ikkje sjå personlege appattributt berre fordi dei kan lese kanalen.
Eventuell publisering av deltakarar er ei eiga, synleg funksjon med avklart omfang.

Første versjon har éi aktiv binding per app per kanal. Ny sesjon er ny binding;
gamle svar blir ikkje flytta til den nye sesjonen. Gamle kort viser avslutta status.

## Eigarar og tilgang

| Data/handling | Autoritativ eigar | Kven kan endre |
| --- | --- | --- |
| Tilgjengelege appar, adapter, felt og scopes | Systemkonfigurasjon | Systemdrift gjennom GitOps |
| Kanalbinding og sesjonsval | Sprøyt | Kanaladministrator etter serverpolicy |
| Deltaking og deling | Sprøyt | Den aktuelle brukaren |
| Appattributt, til dømes deltakarreferanse | Ekstern app, lagra i Sprøyt | Appen i eige deklarert namespace |
| Kanal-/kretsrolle | Sprøyt | Eksisterande rolleforvaltning |
| Open Drinks-sesjonen og drikkedata | Open Drinks | Open Drinks |
| Prosessframdrift og ventepunkt | Heart | Heart gjennom autoriserte kommandoar |

Føreslått kanaladministratorpolicy: kanal-eigar eller krets-eigar/moderator,
med faktisk tilgang til kanalen. Ein kretsmoderator utan tilgang til ein privat
kanal får ikkje konfigurere integrasjonen. Kanalar utan krets krev kanaleigar.
Alle sjekkar skjer på serveren ved kvar operasjon, også etter rolleendringar.

Appen får ikkje skrive brukarens ja/nei, gi samtykke, endre roller, hente
usvara medlemmer eller lese meldingshistorikk som sideeffekt av tilknytinga.
Den eksterne appen må heller ikkje bruke ein vanleg brukars innloggingssession.

## Datamodell

Bruk relasjonelle tabellar med avgrensa, skjemavalidert JSONB for attributt i
PostgreSQL og tilsvarande validert JSON i SQLite. Ikkje bygg generell EAV,
eigen cacheteneste eller ein ny prosessmotor. Nye tabellar er additive.

- `app_bindings`: ID, app-ID, kanal-ID eller krets-ID, konfigurert ressurs-ID,
  pinna manifestversjon, status, revisjon, opprettar og tidspunkt. Nøyaktig eitt
  scope må vere sett; eksplisitte foreign keys, ikkje berre eit fritt scope-felt.
- `app_member_responses`: binding, intern brukar-ID, medlemskapsperiode,
  svar, delingsstatus, versjon av informasjonen brukaren godtok, revisjon og
  tidspunkt. Unik per binding og brukar; servervaliderte overgangar.
- `app_member_attributes`: binding, brukar, felt, typa verdi, revisjon og
  skrivaridentitet. Berre manifestdeklarerte felt. Svaret blir ikkje duplisert her.
- `app_subjects`: stabil, opak identifikator per app og brukar, mappa internt.
  Eksponerer ikkje OIDC-subject, e-post eller global Sprøyt-ID. Same app kan
  kjenne att ein brukar på tvers av bindingar, men kvar binding krev eiga deling.
- `app_binding_events` og `app_command_receipts`: varig endringsstraum og
  idempotenskvitteringar for trygg polling, retry og eventuell Heart-levering.

Kanalattributt og kretsattributt er separate fakta, utan implisitt arv eller
overstyring. Ein kanalbinding gjev aldri tilgang til kretsattributt. Grunnmodellen
skal støtte begge nivå frå første migrasjon; første brukarflate og Open Drinks
brukar kanalnivå. Kretsbindingar treng seinare eiga konfigurasjons-/delingsflate.

Medlemskap må vere gyldig både ved svar, app-lesing og app-skriving. Utmelding
eller utestenging trekkjer tilbake deling og lagar ei tombstone-hending.
Gjeninnmelding reaktiverer ikkje tidlegare deling; brukaren svarar på nytt.
Trekking av deling er ulikt eit nei. Appen får ei minimal tilbakekallingshending
slik at han kan fjerne eller avgrense sin kopi; Sprøyt kan ikkje garantere sletting
av data som alt er overførte. Lagringstid og sletting må vere del av appavtalen
før aktivering, med avgrensa kvitterings-/tombstone-retensjon og ingen svarpayload
i vanlege logger. Brukaren skal kunne sjå dei lagra eigenskapane i bindinga.

## Systemkonfigurasjon og serverintegrasjon

Manifestet inneheld stabil app-ID, namn, adaptertype, tillatne scope-nivå,
konfigurasjonsskjema, brukarfelt, appskrivbare felt, nødvendige scopes,
delingsinformasjon og versjon. Ikkje last ekstern JavaScript eller vilkårleg
HTML. Første skjema er eit lite, typa deltakarskjema; generell skjemabyggjar ventar.

Tenestecredential ligg berre i secret på serversida. Bygg vidare på eksisterande
credential-rotasjon og tilbakekalling, men innfør bindingavgrensa autorisasjon
for app-API-et. Hemmelegheiter skal ikkje liggje i manifestet, meldinga eller
nettlesaren. Katalogoppføringa åleine gjev ingen datatilgang.

Open Drinks må tilby serverautorisert sesjonsoppslag. MVP kan bruke ei driftsstyrt
liste over sesjonar som Sprøyt-integrasjonen faktisk har lov å knyte til, validert
av Open Drinks. Seinare kan appen krevje ei brukarstyrt godkjenningsreise.
Den må binde godkjenninga til moderator, binding, sesjon og utgåande eingongsstate;
moderatorklikk i Sprøyt må aldri gi tilgang til ein vilkårleg Open Drinks-konto.

Første synk er polling frå Open Drinks, utan krav om webhooklevering. Bruk
avgrensa sider, endringscursor, backoff og idempotens. Ein transaksjon lagrar
svaret, revisjonen og endringshendinga saman. Ordninga må fungere med fleire
Sprøyt-replikaer; cursorrekkjefølgje må ikkje miste seinare committa transaksjonar.
Bruk serialisert sekvens per binding med radlås i same transaksjon.

## Heart

Eit enkelt, redigerbart ja/nei-svar går direkte til Sprøyt. Heart trengst når
tilknytinga krev fleire steg, godkjenning, fristar eller samhandling mellom appar.
Heart får referansar til binding, svarrevisjon og den aktuelle brukaroppgåva;
Sprøyt eig delingsavgjerda og appen eig sine fagdata.

Ved ein slik flyt blir svar og varig kommando lagra atomisk i Sprøyt, og ein
arbeidar leverer til Heart med stabil idempotensnøkkel. Brukarflata viser
ventande til prosessmotoren har kvittert. Gjenbruk mønsteret frå prosessarbeidet,
men ikkje knyt generiske appbindingar til work-item-tabellane. Ingen nettverkskall
til Heart eller Open Drinks skal haldast opne inne i ein databasetransaksjon.

## Meldingar og medium

Bruk den eksisterande meldingsmotoren og Markdown-renderaren. Appen står som
avsendar; han kan ikkje etterlikne ein brukar. HTML og privilegerte prosessmakroar
blir ikkje tillatne gjennom appmeldingar. Kort blir serveroppretta, bundne til
binding og faktisk meldings-ID, ikkje aktivert ved å lime inn ein makro.

Vedlegg blir lasta opp gjennom ein appautorisert adapter til eksisterande
mediekø. Valider bytegrenser, innhaldstype, eigarskap og binding ved ferdigstilling
og melding. Referer til varige media-ID-ar, ikkje signerte URL-ar i Markdown.
Ikkje hent vilkårlege eksterne URL-ar på serveren. Lesetilgang til publiserte
medium følgjer meldinga/kanalen, ikkje credentialet som lasta opp.

## Etappar og akseptanse

| Etappe | Leveranse | Må vere bevist før neste steg |
| --- | --- | --- |
| I0 Kontrakt | Avklar sesjonsautorisasjon med Open Drinks, felt, retensjon, scopes; maskinlesbar OpenAPI og kontraktfixture | Begge appar er samde om ID-ar, nei/tilbakekalling og API-format |
| I1 Grunnmodell | Manifest, bindingar, medlemssvar/-attributt på begge nivå, policy og migrasjonar | SQLite/PostgreSQL-paritet; ingen lekkasje mellom kanal, krets eller app; gjeninnmelding krev nytt svar |
| I2 Første vertikale flyt | Moderator knyter sesjon; medlem svarar; Open Drinks hentar respondentar | Ja og nei, ikkje-svara, to brukarar, endra svar, utmelding og tapt HTTP-svar |
| I3 Appskriving | Deklarerte appattributt, Markdown-meldingar og medieopplasting | Retry dupliserer ikkje; appen kan ikkje endre brukarsvar; mobil og desktop viser media |
| I4 Drift og handover | Cursor/recovery, rotasjon, disable, observability, eksempelklient og ferdig integrasjonsrettleiing | Avbrot/oppstart, tilbakekalling, cursor utgått og to replikaer; ende-til-ende i canary |
| I5 Seinare behov | Kretsbrukarflate, fleire ressursar per app/kanal, Heart-godkjenningsflytar | Eit konkret nytt brukstilfelle; ikkje føresetnad for Open Drinks |

I2 er første brukbare iterasjon, men heile det etterspurde omfanget krev I3/I4.
Automatiske testar skal vektlegge tilgangsgrenser, samtidige endringar, retry og
medieeige. Bruk eksisterande CI/CD og målretta UI-testar. Aktiver først i éin
canary-kanal; delte databasar krev serverstyrt binding-/miljøaktivering slik at
produksjon ikkje byrjar å eksportere eller skrive før utrullinga er godkjend.
Migrering 0071 bruker ein fast verdi for eksisterande medlemskap og ein ny verdi ved kvar innmelding. PostgreSQL endrar metadata utan å skrive om dei delte tabellane og avbryt ei låseventing etter to sekund.

Tilbakeføring deaktiverer nye kall, bevarer meldingar og køyrer ingen destruktiv
databaserollback. Ingen produksjonsendring er del av denne planleggingsrunden.
