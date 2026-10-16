# Prioritert leveranseplan, 9. oktober 2026

Gjennomgangen omfattar alle 17 opne GitHub-saker. #240 og #253 har high etter
Harald si presisering. Astra har vurdert gruppering og dei kritiske grensene i
agentverktøya. Prioritet tyder rekkjefølgje; leverte rettingar med attståande
fysisk akseptanse skal ikkje implementerast på nytt eller lukkast utan evidens.

## Runde 1: high

| Pakke | Saker | Konkret arbeid og akseptanse |
|---|---|---|
| Felles agentverktøy | #240, #253 | Modellvalde leseverktøy for stadnamn, dagens heile Parikia-tabell/fartøysfilter og autoriserte AIS-observasjonar. Gjenbruk eksisterande Weather-Service og Ferry-Schedule; same tilgang for Maria, Fogd og andre konfigurerte agentar. Test native Qwen-utval, faktisk kjeldeoppslag og ferdig svar. |
| Eigen status | #245 | Namn, first-50-merke og status får separate treffområde. Eigen status opnar statusredigering; merket opnar merkeinformasjon. Test mobil/tastatur og bevar andre brukarar si vising. |
| Bilete og retur | #198, #201 | Zoom-/gesture- og iOS-delingsrettingar er allereie leverte (#215/#228). Vern panoreringsflata mot native biletemeny; bevar eksplisitte nedlastingskontrollar. Fysisk prøve må identifisere eventuell attståande popup og dokumentere retur frå iPhone sitt OS-delingsark med same utkast. |
| Innmelding og UI-tilstand | #169 | Kretsnamn, innmelding og realtime-audit er leverte (#217/#234/#235). Kontroller eksisterande nettlesarkontraktar for utkast, fokus, opne detaljar og leseposisjon under andre event. Faktisk nybrukarreise krev fersk invitasjon og verifisert e-post; ikkje føreset at gamle invitasjonar har ny kontekst. |

Ingen av verktøya gir modellen frie URL-ar, vilkårlege nettverkskall eller
skrivetilgang. GTP er plan; AIS er ei avgrensa observasjonskjelde og dokumenterer
ikkje automatisk kaiankomst, forseinking eller kansellering. Namneoppslag for
vêr rapporterer den faktisk resolverte staden og landet. Feil oppslag kan ikkje
stille bruke standardstaden i staden. Begge vêrkjelder blir bevarte om modellen
kan samanlikne dei; kortaste kjeldefrist gjeld ved publisering.

## Runde 2: normal, etter high

1. **Kompakt navigasjon og indikatorar (#238/#239/#244):** bruk logoen som
   returhandling, avklar stabil sortering av varsla kanalar, og gjer aktiv bjelle
   like diskret som dempaindikatoren. Kontroller kanalval, uleste og tastatur i
   begge tema. Felles komponentendringar høyrer til designsystemet.
2. **Statusval (#246):** lagra tidlegare emoji/tekst og eventuelt mest brukte.
   Dette er ei utviding etter at sjølve statusredigeringa fungerer.
3. **Agentdata (#243/#241):** verifiser kjelde for havtemperatur; posisjonsdeling
   må vere eksplisitt og ha avgrensa levetid/tilgang. Ikkje lat modellen gjette GPS.
4. **Produktakseptanse (#231) og drift (#170):** dokumenter attståande
   samtale-/bileteprøver, OIDC-øvingar og avgrensa belastnings-/driftsakseptanse.

## Navigasjonspakken (#238/#239/#244)

Kompakt samtalevising brukar logoen som den eine returknappen, med 44 px
trefflate og tilgjengeleg namn «Samtalar». Retur endrar berre liste/detail-vising;
kanal-/trådutkast og lesetilstand høyrer framleis til verten.

Varsla kanalar kjem først innan kvar eksisterande krets eller Felles-gruppe.
Sorteringa er stabil mellom like preferansar og endrar ikkje kjeldelista,
kretsrekkjefølgja, direkte samtalar, utval eller ulesttilstand. Flytting skjer
etter vellukka lagring; feil gir lokal retry. Fokus blir gjenoppretta på
varselknappen etter lagring dersom det gjekk tapt, men blir ikkje teke frå
eit anna kontrollfelt brukaren har gått til.

Den delte `symbol`-varianten i designsystem 0.1.2 brukar same diskrete kvilestil
for begge bjellene. Forma og `aria-pressed` skil tilstanden; fokus, hover og
pending/disabled blir bevarte. Andre toggleknappar beheld eksisterande stil.
React og Vue har same variant; pakkesmoken finn arkivet ut frå kjeldeversjonen.

## Seinare

#242 krev dokumentert trafikkjelde før sanntidskøar kan lovast. #211 er eit
avgrensa øl-/ratingeksperiment som krev avklaring av kjelde og tilgang. #186 har
levert share-target-MVP, men fysisk Android-PWA-akseptanse står att.

## Designsystem og levering

Den eksisterande pakken er installert frå `frontend/vendor`, medan kjelda har
lege i den utracka designkatalogen. Nye delte kontraktar skal ha kanonisk kjelde
i `packages/sproyt-ui`, React/Vue-paritet, testar og versjonert vendora pakke.
Bevar originalkatalogen og uttrykket: papir/kol, symbolske handlingar, lite
metadata, regel over byline og mest mogleg rom til meldingar/komponering.
Statusrettinga er første konkrete kontraktforbetring, ikkje ein full visuell
omskriving. Dei neste indikator-/navigasjonsendringane skal byggje vidare på dette.

Samla endringar går gjennom Astra-review, relevant lokal kontroll og CI med
ekte PostgreSQL før merge. Utrulling skjer med immutable image og GitOps.
Gjenbruk verifisert revisjon som release-baseline for å unngå dobbel full
nettlesartest av identisk tre. Desse endringane krev ikkje databasemigrasjon.
Fysisk mobil- og innmeldingsakseptanse er eiga evidens, aldri ei påstått følgje
av emulering eller grøne einingstestar.

Lokal evidens for første pakke: 107 frontend-, 17 delte komponent- og 8
React-renderingtestar; 12 målretta nettlesartestar for status/bilete/nedlasting;
samla #169-tilstandstest i Chromium og WebKit iPhone. Sistnemnde bevarer fokus,
utkast, markering, leseposisjon og open oppgåvedetalj ved meldingar og reaksjonar
i aktiv/inaktiv kanal. Native live-verktøyprøve med begge tenester og Qwen har
passert. Ekte PostgreSQL-kontroll og full nettlesarregresjon går i CI før merge.


## Oppdatert leveranseplan – 16. oktober

Mandat: alle opne saker med normal eller høgare prioritet, til canary via
CI/CD. Produksjonsimage og delte brukar-/agentkonfigurasjonar blir bevarte.
Astra reviderte denne avgrensinga før implementasjon og tek ny kontroll før release.

Autoritativ prioritet frå Sprøyt-registeret: #270, #271, #272 og #274 normal;
#273 låg og utanfor leveransen. GitHub-eksportmarkøren er request_id i
work_item_github_exports, ikkje work_item.id. Andre opne saker med priority/later
(#186/#211/#242) er utanfor.

1. UI: #270 bevar native tekstmarkering/kopiering og eksplisitt reaksjonsknapp;
   #272 vis meny berre ved faktiske handlingar, inkludert eksisterande arbeidssaker
   når prosessen er deaktivert; #274 gjenopprett avvist prompt i opphavleg identitet,
   kanal/tråd utan å overskrive nyare utkast, også etter reload/literal-modus.
2. #170: avgrensa WS/SSE send-/kø-/lag-målingar og Heart utfall/latens med faste
   etikettar. Ingen migrasjon eller driftsinfrastrukturendring; avbrotsøvingar
   skal ikkje gjerast mot den delte Heart-/chat-databasen.
3. #271: generell, eigarkonfigurert videresending i eitt steg mellom agentar i
   same krets og kanal. Ei eiga opptakskø er isolert frå eldre prod-arbeidarar;
   mottakaren brukar ordinær kø og sine eigne tilgangar, verktøy og minne.
   Ingen hardkoda mottakar, rekursiv delegering eller lån av GPS/minne.
   Migrasjon 0067 lagrar berre ID-ar/revisjonar; originalmeldinga blir lesen på
   nytt med tilgangskontroll. Canary får eige flagg, avslått som standard.
4. #169/#198/#201/#231 har leverte kodeendringar og manglar konkret ekstern
   akseptanse (ny verifisert brukar, opphavleg fysisk popup/iPhone-share-retur,
   naturleg agentsamtale og separat godkjend bileteidentitet). Dei skal ikkje
   lukkast eller implementerast om utan ny feil/evidens.

Kontroll: relevante frontend-grensetestar og Chromium/WebKit, Rust/Clippy,
PostgreSQL og eksisterande arbeidsflytkontraktar, så Astra-review av endeleg diff.
Publiser eitt uforanderleg ARM64-image frå kontrollert main; oppdater berre canary
web-image/chart og migrasjonsimage via GitOps etter fersk backup/restore-gate for
0067. Bevar worker-/Heart-image, prod-digest og produksjonskonfigurasjon.
Verifiser /versionz, readiness, faktisk canary-metrikk og uendra produksjon.
