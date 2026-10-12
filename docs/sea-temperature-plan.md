# Plan for sjøtemperatur hos kretsagentar

Dato: 12. oktober 2026. Oppfølging av #243.

Målet er at Maria og andre kretsagentar kan svare om sjøtemperatur ved Paros
og utvalde kyststader i Vestland, med stad, tidspunkt og forståeleg kjelde.
Første leveranse brukar Open-Meteo Marine for Paros og MET Oceanforecast for
Vestland. Dette er modellert sjøtemperatur. Lokale bademålingar, Yr-nøkkel,
Copernicus og POSEIDON kjem i seinare leveransar.

## Kjeldegrunnlag

| Kjelde | Bruk i første leveranse | Avgrensing |
|---|---|---|
| [Open-Meteo Marine](https://open-meteo.com/en/docs/marine-weather-api) | `sea_surface_temperature`, JSON, Paros | Temperaturgrunnlag rundt 8 km, seks-timars grunnoppløysing, dagleg oppdatering. Timevis API-utdata er ikkje timevise målingar. |
| [MET Oceanforecast](https://api.met.no/weatherapi/oceanforecast/2.0/documentation) | `sea_water_temperature`, JSON, Vestland | Dekning og faktisk returnert havpunkt må kontrollerast per stad. Oppløysinga til bølgjemodellen må ikkje brukast som temperaturmodellens oppløysing. |

Lesande API-prøver gav HTTP200 og temperaturdata for eit kystpunkt ved Parikia
og Bjorøy. Open-Meteo rapporterte ei posisjon fleire kilometer frå Bjorøy-
punktet; MET rapporterte ei nærare posisjon. Dette beviser transport og feltdekning,
ikkje nøyaktig badetemperatur ved ei bestemt strand.

[MET-vilkåra](https://docs.api.met.no/doc/TermsOfService) krev identifiserande
User-Agent, kreditering og respekt for cache-/Expires-headerar.
[Open-Meteo-vilkåra](https://open-meteo.com/en/pricing) skil mellom gratis
ikkje-kommersiell bruk og kommersielt abonnement. Avklar produksjonsbruken før
aktivering; offentleg evalueringsendepunkt er nok for kjeldeprøver.

## Arkitektur og avgrensing

Bruk eksisterande Rust-worker, HTTP-klientmønster og native leseverktøy i
`src/chatbot/tools.rs`. Lag ein avgrensa `sea_temperature`-modul med to
leverandøradapterar; ikkje bygg eit nytt generelt verktøyrammeverk eller endre
Weather-Service for å få første versjon på plass. Sjøtemperatur skal kunne
aktiverast uavhengig av luftvêr og vere tilgjengeleg for alle kretsagentar.

Eit lite servereigd stadregister har stabile ID-ar, namn/alias, region,
koordinatar, tidssone, kjelde og kontroll av rapportert posisjon. Start med Parikia
og Bjorøy; ta med Aliki og Nordnes berre etter konkrete kjeldeprøver. Kvar
agent har eitt standardstad-ID og ei avgrensa liste over tillatne stad-ID-ar.
Eigaren/moderatoren vel desse i eksisterande agentoppsett. Ukjend eller tvitydig
stad krev presisering; vi bruker ikkje fri namnesøking eller modellgjetta GPS.

`lookup_sea_temperature({locations?: [location_id, ...]})` tek høgst to
autoriserte stad-ID-ar. Utelate liste betyr agentens standardstad. To stader
gjer ei samanlikning mogleg i eitt verktøykall. Tenaren vel kjelde frå
stadregisteret; modellen vel aldri URL, leverandør, koordinatar eller
autentisering. Ingen automatisk overgang til ein annan geografisk stad ved feil.

Dette verktøyet følgjer eksisterande grense: høgst tre verktøykall totalt,
eitt per verktøy, i éin validert batch. Ein ny definisjon aukar ikkje grensene
eller den samla 140-sekundsfristen/globalt model-permit. Definisjon/prompt må
forklare både grensene og dei tilgjengelege stad-ID-ane.

Verktøydefinisjonen følgjer autorisert sjøtemperaturkonfig, uavhengig av
luftvêrkonfig og av om ein leverandør er oppe. Hent marinefakta først når
verktøyet blir kalla; vanleg prat får inga marinehenting eller marinefrist.
Kjeldeutfall skal ikkje skjule capabilityen eller fjerne agenten. Når begge
ferdigheiter er på, må feil i atmosfærisk standardvêr ikkje stanse eit gyldig
sjøtemperaturoppslag før modellen får bruke verktøyet.

MVP bruker berre offentlege, konfigurerte stader. Eksisterande mellombels
GPS-deling gir ikkje automatisk sjøtemperatur for den delte posisjonen. Svar
namngir sjøtemperaturstaden, også når luftvêr gjeld ei delt brukarplassering.
Private GPS-koordinatar skal ikkje inn i marinecache eller agentminne.

## Datakontrakt og kvalitet

Normalisert resultat inneheld status (`available`, `partial`, `unavailable`),
etterspurd stad-ID/namn, kjelde/lenkje/kreditering,
type `model_forecast`, temperatur i °C, gyldig tidspunkt, hentetid og tidssone.
Ta med modellutgjevingstid/oppløysing berre når dei faktisk er oppgitt eller
dokumenterte. Open-Meteo sin `generationtime_ms` er ikkje modellutgjevingstid.
Tilgang til data no er ikkje bevis på ei fersk observasjon.

Skil `requested_coordinates` og `reported_coordinates` frå valfri
`model_grid_coordinates`. Returnerte API-koordinatar er ikkje automatisk
dokumentasjon på det faktiske modellpunktet. Avstand får eit eksplisitt
grunnlag (`reported_position` eller `documented_model_grid`); ukjend avstand
er `null`, aldri 0 km. Begge kjelder blir kalla regionale modellestimat i MVP,
også når koordinatane ligg nær stranda. Avstand åleine seier ikkje kor presist
modellen skildrar vatnet ved stranda.

Vel ein tidsprøve nær aktuell UTC-tid innanfor kjeldas dekning, med innleiande
grense 90 minutt. Ikkje bruk første time ved midnatt som «no». Alle tal må
vere endelege og einingar/tidssoner gyldige. Manglande temperatur blir
utilgjengeleg, aldri 0 °C; ei faktisk gyldig 0 °C-prøve skal framleis godtakast.
Innleiande kvalitetsgrenser er 90 minutt frå gyldig UTC-prøve, høgst 10 km
frå etterspurd til rapportert posisjon, og høgst 36 timar frå dokumentert
kjeldeoppdatering når ho finst. Dette er produktgrenser, ikkje påstandar om
modellnøyaktigheit. M1 skal verifisere metadatabetydninga og godkjenne eller
stramme inn posisjonsgrensa per stad etter kontroll av kyst/bukt/øy. Avvik krev
ein dokumentert planendring før staden blir valbar. Ukjend modellalder blir
opplyst som ukjend; fersk henting og gyldig prognosetid erstattar ikkje henne.

Ved samanlikning blir begge kjelder, stader og tidspunkt viste. Ingen
gjennomsnittsberekning mellom leverandørar eller påstand om kva strand som er
varmast når oppløysinga ikkje støttar det. Modellresultat kan ikkje brukast som
lokale målingar eller som stadfesting av badevasskvalitet.

Modellen vel om oppslag trengst og kva tillatne stader som er relevante.
Når sjøtemperaturverktøyet er brukt, lagar ein liten tenarrenderer sjøsvaret
frå dei autoriserte snapshot-felta: stad, temperatur, tidspunkt, modellmerknad
og kjeldelenkje. Bruk eventuelt ei kort agenttilpassa innleiing frå godkjende
formuleringar; ikkje kopier agentens konfigurerte svarord som kjeldefakta.
To stader blir to sjølvstendige resultat; feil blir tydeleg utilgjengeleg.
Ingen ny modellrunde er nødvendig for å skrive om desse tala.

Fri modellprosa skal ikkje leggjast på marinefakta som om ho var validert.
Vanleg samtale/vêr/ferge utan marineoppslag bevarer dagens modellflyt.
MVP sitt marineverktøy gir eit avgrensa sjøtemperatursvar; dersom eit samansett
spørsmål også ber om andre fakta, må denne avgrensinga vere synleg, og vi skal
ikkje late som sjøsvaret dekkjer resten. Samanfletting med fri prosa kjem seinare.
Test at modellen ikkje kan få feil tal, «målt på stranda» eller ein udokumentert
«varmast»-påstand inn i det publiserte marineoppslaget. Dette gjer faktabindinga
konkret utan eit generelt faktavalideringsrammeverk.

## Ressursbruk og feilhandtering

Faste HTTPS-endepunkt, ingen redirect, avgrensa respons på 256 KiB og timeout
på høgst åtte sekund per HTTP-kall. HTTP/batch må passe innan eksisterande
worker-lease og modellfrist. Høgt to samtidige kjeldekall per prosess.

Bruk ein liten, avgrensa prosesscache for offentlege stad-ID-ar og kjelder,
med samordning av samtidige identiske oppslag. Lagre den avgrensa tidsserien
og opphavleg hentetid; vel aktuell tidsprøve på nytt ved kvart cacheoppslag.
Éin time er øvre lokal gjenbrukstid i MVP. Cacheheaderar/Retry-After kan krevje
at neste HTTP-kall ventar lenger; dersom lokale data då er utgåtte, svarer vi
utilgjengeleg fram til eit nytt oppslag er lov. Ingen stale-on-error. M1 må
stadfeste at faktiske headerar gir ei tenleg ordning før kjelda blir aktivert.
Nokre duplikatkall mellom dei tre webreplikaene
er akseptable i MVP; ikkje legg til Redis eller ein distribuert cachejobb.
429 får avgrensa backoff/Retry-After, ikkje mange retry per samtale.

Cachealder, gyldig datatid og publiseringsfrist er ulike ting. Jobben lagrar
autoriserte offentlege kjeldesnapshot. Publiseringsfristen er minimum av fem
minutt etter svarproduksjon og alle brukte snapshotfristar, inkludert cache-
utløp og tidspunktet der tidsprøva fell utanfor 90-minuttsgrensa. Jobbar utan
marineoppslag får ingen marinefrist. Ny hentetid må ikkje gjere gamle datatider
ferske. Publisering kontrollerer fristen, agentrevisjon og kanal-/medlemstilgang
inne i SQLite-/PostgreSQL-transaksjonen, etter eventuell låseventing.
Retry av eit lagra svar bruker same snapshot og absolutt frist; han blir ikkje
fornya ved gjenbruk. Nye fakta krev at eit utgått svar blir forkasta og ein
ny, autorisert generering blir gjennomført innan vanlege retry-grenser.
Tillegg til nullable agentkonfig og jobb-snapshot/fristar treng ein additiv
SQLite/PostgreSQL-migrasjon; neste frie nummer blir valt ved implementering.
Eksisterande agentar har sjøtemperatur av, og eksisterande jobbar får ikkje
ferdigheita retroaktivt.

Kjeldefeil gir eit strukturert utilgjengeleg resultat, slik at agenten kan
forklare feilen og framleis svare på vanleg prat. Når éin av to stader feilar,
kan den andre visast med tydeleg delvis dekning. Brot på tilgang eller
publiseringsfrist stoppar publisering. Brukaren får ikkje eit oppdikta standardtal.
Målingar/loggar har faste kjelde-/feilkategoriar, ingen samtaletekst, private
posisjonar, API-nøklar eller stad-ID-ar som uavgrensa metric-labels.

## Aktiveringsportar

Port A er godkjend kjeldekontrakt, kvalitetsgrenser og bruksvilkår i M1.
Port B er kompatibelt schema og kode på alle arbeidarar som deler køen,
stadfesta med køyrande revisjonar. Port C er éin avgrensa pilotagent etter
denne barrieren. Port D er pilotaksept før ordinære agentar får ferdigheita.
Den globale sjøtemperaturgaten startar av. Ho gjeld både konfigurasjon,
marineoppslag og publisering av marinebaserte svar, også lagra svar på retry.

Ved rollback skal nye marinejobbar stoggast, agentrevisjonen aukast og alle
ventande/leasa marinejobbar avsluttast kontrollert før eldre arbeidarar kan
ta køen. Flagget åleine gjer ikkje ein gammal arbeidar trygg. Bevar snapshot-
kolonnar og meldingar; ikkje slett forretningsdata som del av tilbakeføringa.

## Gjennomføring og ferdigkriterium

1. **M1 kjeldekontrakt og stadregister.** Dokumenter eksakte parameter,
   tid/metadata, kreditering, cacheheaderar og rapportert posisjon for stadene.
   Test føreslåtte Aliki/Nordnes før dei blir valbare. Fastset eksplisitte
   grenser for posisjonsavstand, datatid og kjelde-/modellalder. Ferdig når kvar aktiv
   stad har ein dokumentert prøve og alle viktige kvalitetsgrenser er bestemte.
2. **M2 adapter og cache.** Rust-modul, normalisering, avgrensa HTTP/cache,
   429-/timeout-/delvis-feilresultat. Fixtureprøver for null, 0 °C, ugyldige
   tal, einingar, tidssone/DST, feil geografisk punkt og manglande/forelda data.
   Ferdig når prøvene viser korrekt modelltype, koordinatar og tidspunkt utan
   andre tenester eller bakgrunnsarbeidarar.
3. **M3 agentkontrakt og publisering.** Nullable konfig/snapshot/fristar,
   eksisterande eigar-/moderator-CRUD, revisjonar, native tool og tenarrenderer.
   Bevar PATCH absent/null-semantikk og alle tilgangsgjerde. Test ulike agentnamn,
   eit/to stadoppslag, uautoriserte ID-ar, ugyldig batch, retry, endra konfig,
   sletta kjeldemelding og utløp under låseventing i begge databaseadapterar.
   Test sjøtemperatur på med luftvêr av, begge av og feil i kvar leverandør;
   dessutan tom/duplisert liste, over to stader, ingen marinehenting i vanleg
   prat og ingen retroaktiv capability på gamle jobbar. Rendererprøver
   injiserer oppdikta modellfakta og verifiserer at dei ikkje blir publiserte.
   Astra tek ei kort kritisk arkitektur-/publiseringskontroll etter denne etappen.
4. **M4 agentoppsett og svaroppleving.** Kompakt val i eksisterande
   agentdialog, utan fleire knappar i chat eller tittellinje. Følg Sprøyt-
   designsystemet og bevar gamle agentoppsett. Prompt/followup støttar sjø- og
   badetemperaturspørsmål, kjelde-/stadpresisering og naturleg feilforklaring.
   Dokumenter reelle Qwen-prøver for standardstad, annan tillaten stad,
   Paros–Vestland-samanlikning og urelatert samtale. Prøver utan kanalmelding
   først; ingen modellprøve skal sjølv endre oppsett eller publisere.
5. **M5 CI og kompatibel utrulling.** Relevante Rust, PostgreSQL og
   Chromium/WebKit-kontrollar, ARM64/security/publisering gjennom vanleg CI/CD.
   Canary får kompatibel kode/schema med ferdigheita av; deretter prod.
   Aktivering krev at alle prod-/canary-arbeidarar har same nye kontrakt.
   Gjenbruk relevant backup/restore-evidens og rett migrasjonsgate; ikkje
   køyr fleire fulle bygg for uendra revisjon. Ingen manuell cluster-utrulling.
6. **M6 pilot og aktivering.** Etter kompatibilitetsbarrieren aktiverer vi
   ferdigheita for ein testagent med avgrensa kanaltilgang og to offentlege
   stader. Prod og canary deler DB/jobbar: canary er ikkje isolert, og begge
   arbeidarsetta må kunne behandle dei same aktiverte jobbane. Kontroller
   eitt faktisk svar/oppfølging, feiltilfelle, kreditering og logg/målingar.
   Kort Astra-review før ordinær aktivering av Maria og ein annan agent.
   Slå av pilotagenten/auke revisjonen ved feil og drener marinejobbar før
   rollback til gammal kode. Nye kolonnar og gamle meldingar blir bevarte.

## Astra-revisjon

Astra reviderte planen 12. oktober. Endringane er tekne inn: tenarrendra
marinefakta, separate koordinatgrunnlag, konkret tid/cachepolitikk,
sjølvstendig henting, transaksjonelle publiseringsfristar og fire utrullingsportar.
Etter ny kontroll fann Astra ingen attståande blokkerande arkitekturval.
M1-kjeldekontroll, avklaring av produksjonsvilkår og kompatibilitetsbarrieren
er framleis nødvendige ferdigkriterium før aktivering. Faktagarantien gjeld
marineoppslaga, ikkje all fri modelltekst. Brukaren godkjende implementasjon og utrulling til både prod og canary.
M1-kjeldeprøvene er dokumenterte i `sea-temperature-sources.md`; kode,
agentinnstillingar og migrasjon 66 er implementerte. Aktivering skjer først
etter CI og kontroll av alle arbeidarane mot den nye kontrakten.

## Seinare utvidingar

Yr sine [registrerte badetemperaturar](https://hjelp.yr.no/hc/no/articles/5949243432850-API-for-badetemperaturer)
krev nøkkel og konkret dekningstest. Nordnes publiserer separat sjø- og
bassengtemperatur på [si sanntidsside](https://nordnessjobad.no/sanntid/);
API/vidarebruk må avklarast før integrasjon. Eigne termometermålingar kan seinare
leggast til med måletid, stad, måledjupn og tydeleg observatør/kjelde.

[Copernicus Mediterranean SST](https://data.marine.copernicus.eu/product/SST_MED_SST_L4_NRT_OBSERVATIONS_010_004/description)
kan gi dagleg satellittanalyse ned til rundt 1 km. Han representerer nattleg
grunnleggjande temperatur, ikkje badetemperatur akkurat no.
[POSEIDON API](https://api.poseidon.hcmr.gr/swagger/) krev avtalt tilgang og
verifisering av relevante måleplattformer nær Paros. Desse stega blir ikkje
lagde inn i første implementasjon.
