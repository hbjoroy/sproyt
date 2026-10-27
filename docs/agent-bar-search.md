# Mellombels stadssøk for kretsagentar

`find_bars` let ein agent føreslå eit Google Maps-søk etter barar. Den same
kapabiliteten er tilgjengeleg for kretsagentane; han er ikkje knytt til eitt
agentnamn. Modellen lagar ein plan, ikkje eit resultat. Berre tekst frå den
aktuelle menneskeskrivne meldinga kan bli brukt til søk eller namngjeve
utgangspunkt. Ein kan be om eit område, eit uttrykkeleg namngjeve utgangspunkt
eller eit søk nær den posisjonen ein nettopp har delt. Systemet gjettar ikkje
posisjon frå IP-adresse eller samtaleminne.

Agenten legg ut ei nøytral melding med eit stadssøk. Ved opning ser brukaren
den eksakte søkjeteksten og må sjølv starte Google-oppslaget. Dersom ein delt
posisjon blir brukt, blir det forklart før brukaren startar søket. Berre den
som delte posisjonen kan opne eit slikt søk, og delinga må framleis vere gyldig.
Tilgang, agentoppsett, melding og posisjonsdeling blir kontrollerte før og
etter oppslaget. Søket har databasebasert samtidigheits- og mengdegrense.

Google-svaret, kjeldetilvisingar og lenkjer blir vist mellombels, med
`Cache-Control: no-store`, og blir ikkje lagra i meldingshistorikk, jobbar eller
agentminne. Brukaren kan be om gangavstand til eit av dei returnerte stadene
når søket har eit utgangspunkt. Rutene er beta og kan mangle fortau eller
gangvegar. Kortliva krypterte rutetoken er bundne til brukar, kanal, melding,
søk og eventuell posisjonsdeling, og blir ikkje lagra på tenaren.

Google Grounding Lite gir ikkje her eit verifisert utval barar for eit tilfeldig
terningval, eit kartbilete eller verifiserte Untappd-identitetar. Desse er ikkje
implementerte. Google-kjelder kan innehalde andre stader enn barar. Ikkje lat
agenten eller grensesnittet påstå at eit tilfeldig val, kartførehandsvising eller
direktelenkje til ein Untappd-venue finst før ei eigna kjelde er integrert.

For eit seinare terningval trengst eit sjølvstendig, strukturert utval som
faktisk består av barar. Eit mogleg spor er ei separat Places Nearby Search
etter stad-ID-ar, med eigen nøkkel og Places UI Kit til vising av resultatet.
Det må først vurderast opp mot den faktiske bruken og avtalevilkåra; ein må
heller ikkje framstille eit tilfeldig val frå dei returnerte treffa som eit
val mellom alle barane i området. Ei anna mogleg kjelde er OpenStreetMap med
ein påliteleg leverandør og eksplisitt posisjonssamtykke. Testar frå klynga
gav ikkje ein fungerande global Overpass-tenar for København og Malmö.

Sjå [Google sine EØS-vilkår for Maps Grounding Lite, punkt 11](https://cloud.google.com/terms/maps-platform/eea/maps-service-terms)
og [rettleiinga for Places API i EØS](https://developers.google.com/maps/comms/eea/places).

## Konfigurasjon og utrulling

Tenaren nyttar `SPROYT_GOOGLE_MAPS_API_KEY` berre på tenarsida. For gangruter
trengst også `SPROYT_PLACE_ROUTE_SIGNING_KEY`: ein sjølvstendig, tilfeldig
32-byte nøkkel koda med base64url utan padding, lik på alle replikaer. Helm
kan lese dei frå eksisterande Kubernetes Secrets via
`secret.googleMapsExistingSecret`/`googleMapsApiKeyKey` og
`secret.googleMapsRouteExistingSecret`/`googleMapsRouteKeyKey`. Ingen av desse
skal leggjast i Git eller eksponerast til nettlesaren. Utan kartnøkkelen er
verktøyet slått av; utan rutenøkkelen fungerer søket utan gangruteknapp.

**Ikkje aktiver dette på delt Canary-database utan ein koordinert plan.**
Migrasjon 0070 er felles for databasen, og eldre binærar kan avvise ukjende
migrasjonar ved omstart. Produksjon og Canary deler i tillegg agentjobbkø, så
det å setje ein nøkkel berre på Canary avgrensar ikkje effekten til
Canary-brukarar. Prøv funksjonen mot ei isolert database og kø, eller lag og
verifiser ei bakoverkompatibel, koordinert utrulling før migrasjon og aktivering.
Ingen av desse føresetnadene er oppfylte berre ved at ein Canary-image finst.
