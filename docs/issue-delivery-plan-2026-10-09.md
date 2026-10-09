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
