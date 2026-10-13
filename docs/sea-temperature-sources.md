# Kjeldekontrakt for sjøtemperatur

M1 kontrollert 12. oktober 2026 kl. 20:40:58–59 UTC med lesande HTTPS-kall.
Dette er regionale modellestimat. Stadnamna identifiserer offentlege
kystområde; dei representerer ikkje målestasjonar eller bestemte badestrender.

| ID / visingsnamn | Etterspurd (lat, lon) | Rapportert (lat, lon) | Avstand, om lag | Lokal avstandsgrense |
|---|---|---|---|---|
| `parikia` / Parikia, Paros | 37.085, 25.148 | 37.125, 25.125015 | 4,9 km | 6 km |
| `aliki` / Aliki, Paros | 36.997, 25.137 | 36.958336, 25.125015 | 4,4 km | 6 km |
| `bjoroy` / Bjorøy | 60.33, 5.17 | 60.3359, 5.1571 | 1,0 km | 2 km |
| `nordnes` / Nordnes, Bergen | 60.399, 5.304 | 60.3986, 5.2971 | 0,4 km | 1,5 km |

Alle fire svara var HTTP 200 og inneheldt temperaturseriar med rette einingar.
Det rapporterte punktet vart også visuelt kontrollert i OpenStreetMap-kystkart:
[Parikia](https://www.openstreetmap.org/?mlat=37.125&mlon=25.125015#map=12/37.125/25.125015),
[Aliki](https://www.openstreetmap.org/?mlat=36.958336&mlon=25.125015#map=12/36.958336/25.125015),
[Bjorøy](https://www.openstreetmap.org/?mlat=60.3359&mlon=5.1571#map=14/60.3359/5.1571),
[Nordnes](https://www.openstreetmap.org/?mlat=60.3986&mlon=5.2971#map=14/60.3986/5.2971).
Dei rapporterte punkta ligg i sjøen: nordvest for Parikia-bukta, sør for Aliki,
vest/nordvest for Bjorøy og vest for Nordnes i Byfjorden. Kartgrunnlag:
© OpenStreetMap contributors. Denne kontrollen stadfestar sjø/kyst-kontekst,
ikkje nøyaktig lokal badetemperatur. Vi strammar inn den generelle 10 km-grensa
per stad som vist over; brot gir utilgjengeleg resultat utan geografisk fallback.

## Open-Meteo Marine: Paros

Fast endepunkt: `https://marine-api.open-meteo.com/v1/marine`.
Eksakte parameter er katalogens `latitude`/`longitude`,
`hourly=sea_surface_temperature`, `timezone=GMT`, `timeformat=unixtime`,
`forecast_days=2`, `cell_selection=sea`.
[Marine-dokumentasjonen](https://open-meteo.com/en/docs/marine-weather-api)
beskriv SST som overflatetemperatur. Underlaget er Meteo-France / Copernicus
Marine med om lag 8 km, seks timars grunnoppløysing og dagleg oppdatering.
Timevis API-serie er ikkje timevise observasjonar.

Prøvene gav 1050/1053 byte. `hourly_units.time=unixtime`,
`hourly_units.sea_surface_temperature=°C`, `timezone=GMT` og
`utc_offset_seconds=0`. Begge svara hadde berre `Date` av dei kontrollerte
cache-/alderheaderane: ingen `Expires`, `Cache-Control`, `Age` eller
`Retry-After`. Lokal gjenbruk og neste ordinære henting er difor éin time.

Dokumentasjonen kallar toppnivåkoordinatane senteret av rutenettcella brukt til
å lage prognosen. Dei blir derfor også lagra som `model_grid_coordinates`,
med `distance_basis=documented_model_grid`. Dette seier ikkje at ein felles
API-posisjon beviser nøyaktig temperaturmodellpunkt eller strandoppløysing.
`generationtime_ms` er reknetid, og blir aldri kjelde- eller modellutgjevingstid.
Modellalder og konkret kjeldeoppdateringstid er ukjende i denne kontrakten.

[Prising og vilkår](https://open-meteo.com/en/pricing) vart kontrollerte for
gratis ikkje-kommersiell bruk. Aktiveringa byggjer på opplysninga om at Sprøyt
er ei privat, ikkje-kommersiell sosial hobbyteneste. Ingen abonnement eller
kjøp er gjort. Endra kommersiell bruk krev ny vurdering før vidare gratisbruk.
Den avgrensa katalogen og éin times cache gir høgst 48 ordinære Paros-kall per
prosess og døgn; fleire replikaer har kvar sin cache. Kreditering i svaret:
Open-Meteo, Meteo-France / Copernicus Marine og DWD, CC BY 4.0.

## MET Oceanforecast: Vestland

Fast endepunkt: `https://api.met.no/weatherapi/oceanforecast/2.0/complete`.
Parameter: `lat` og `lon` frå katalogen, med høgst fire desimalar.
[Oceanforecast-kontrakten](https://api.met.no/weatherapi/oceanforecast/2.0/documentation)
seier at kystpunkt kan flyttast til næraste havpunkt.
[Datamodellen](https://docs.api.met.no/doc/oceanforecast/datamodel.html)
definerer `sea_water_temperature` som overflatetemperatur, i `celsius`.
Dokumentert bølgjeoppløysing blir ikkje brukt som temperaturmodelloppløysing.

Prøvene gav 42703/42794 byte og GeoJSON `Feature` / `Point`.
`geometry.coordinates` har rekkjefølgja lon, lat. Rapportert posisjon blir
lagra som `reported_coordinates`, `distance_basis=reported_position`;
`model_grid_coordinates` er null fordi eit bestemt temperaturmodellpunkt
ikkje er dokumentert. Alle prøvepunkt var innan dei lokale avstandsgrensene.

Prøveheaderar: `Age: 0`; `Expires` 21:12:25 UTC (Bjorøy) og 21:12:53 UTC
(Nordnes), omtrent 31 minutt etter henting. `Last-Modified` var hentetida,
så denne headeren er ikkje modellalder. `properties.meta.updated_at` var
18:07:53/18:07:57 UTC og blir lagra som kjeldeoppdatering.
[JSON-kontrakten](https://docs.api.met.no/doc/ForecastJSON) definerer dette
som siste oppdatering av prognosedata; det er ikkje modellutgjevingstid.
Modellalder blir framleis opplyst som ukjend. Prøveseriane brukte UTC-tider.

[MET-vilkåra](https://docs.api.met.no/doc/TermsOfService) og
[cache-rettleiinga](https://docs.api.met.no/doc/GettingStarted) ligg til grunn
for User-Agent `Sproyt/sea-temperature (https://sproyt.no)`, kreditering
Meteorologisk institutt (MET Norway, CC BY 4.0) og respekt for `Expires`.

## Grenser som adapteren handhevar

Faste HTTPS-endepunkt utan redirect, åtte sekund HTTP-frist, 256 KiB respons
og høgst to samtidige kjeldekall per prosess. Éin mutex per katalogstad
samordnar identiske oppslag, også ved feil. Cachen har fire faste plassar,
inneheld råserie og opphavleg hentetid, og vel tidsprøva på nytt ved kvart svar.
Berre autoriserte katalog-ID-ar blir godtekne; ingen brukarkoordinatar,
modellvalde URL-ar, nye tenester eller distribuert cache.

Aktuell UTC-tid må liggje innan tidsseriedekninga, og næraste UTC-prøve må
liggje innan 90 minutt. Null er manglande data, medan
gyldig 0 °C blir godteken. Endeleg temperatur må vere mellom −3 og 45 °C.
Kjeldeoppdatering eldre enn 36 timar, eller meir enn fem minutt i framtida,
blir avvist når feltet finst. Manglande eller feil einingar og posisjon blir
avviste. Lokal tid blir konvertert frå UTC med Europe/Athens eller Europe/Oslo,
og dermed med rett sommar-/vintertid.

Lokal cacheutløp er minimum av éin time og leverandørens ferskheitsgrense.
Neste henting respekterer `Expires`, `Cache-Control: max-age` (minus `Age`)
og `Retry-After` (sekund eller HTTP-dato). Lengre leverandørfrist kan stanse
ny henting etter lokal utløp; svaret er då utilgjengeleg. `no-cache`/`no-store`
gir ingen brukbar lokal cache i MVP. Transport-/formatfeil gir minst 60 sekund
pause, og eventuell lengre leverandørfrist blir bevart. Ingen stale-on-error
og ingen automatisk retry i same samtale.

Publiseringsfristen er minimum av fem minutt etter normalisering,
cacheutløp, slutt på kjeldedekninga, prøvetid pluss 90 minutt og kjent
kjeldeoppdatering pluss 36 timar.
Ved delvis dekning blir kvar stad vist sjølvstendig. Tenarrendereren brukar
faste namn/lenkjer og validerte tal/tider; ekstra modellprosa og påstandar om
strandmåling, badevasskvalitet eller «varmast» blir ikkje kopierte inn.
Loggar har berre faste kjelde-/feilkategoriar, utan koordinatar eller samtaletekst.

Fixtureprøver i adaptermodulen dekkjer null/0, næraste tid og ny prøve ved
cachegjenbruk, einingar, feil posisjon, forelda tidsprøve/kjelde,
cache-/Retry-After-grenser, samtidige cacheoppslag med opphavleg hentetid,
feil etter utløp, UTC/DST, delvis svar og injisert modellprosa.
Rotoppgåva køyrer samla Rust-kontrollar og publiseringsprøver.
