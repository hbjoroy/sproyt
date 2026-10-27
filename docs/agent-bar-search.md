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

For terningval nyttar ein ei sjølvstendig kjelde: Geoapify sitt strukturerte
OpenStreetMap-utval av barar og pubar. Den opphavlege brukaren må godkjenne
det separate Geoapify-oppslaget, inkludert deling av posisjon der det er
aktuelt. For namngjevne stader vel brukaren rett startpunkt frå geokodinga.
Serveren gir agenten kandidatnamn utan startkoordinatar; agenten ber om eit
ekte `roll_die`-kast med like mange sider som kandidatar. Serveren bind kastet
til ein bestemt kandidat og lagrar utfallet før idempotent publisering. Etter
at utfallet er lagra, gjev retry ikkje eit nytt kast. Svaret opplyser radius,
tal treff og kjelde og har
lenkje til den valde staden i Google Maps. Utvalet er avgrensa til inntil 100
returnerte treff innan 1,5 km og kan mangle barar eller vere utdatert. Det er
ikkje eit tilfeldig val blant alle barar i området.

Namngjevne geokodingskandidatar, vald stad og arbeidsstatus blir lagra
mellombels i arbeidsflyten med utløp; kandidatlista og delt startposisjon blir
ikkje lagra der. Endeleg resultat blir ei ordinær melding i kanalen. Kjelda
kan ikkje garantere data om til dømes craft-øl, mat, sport eller opningstider.
Google-resultata er ikkje grunnlag for trekkinga. Kartførehandsvising og
verifiserte Untappd-venue-lenkjer er ikkje implementerte.

Sjå [Google sine EØS-vilkår for Maps Grounding Lite, punkt 11](https://cloud.google.com/terms/maps-platform/eea/maps-service-terms)
og [rettleiinga for Places API i EØS](https://developers.google.com/maps/comms/eea/places).

## Konfigurasjon og utrulling

Tenaren nyttar `SPROYT_GOOGLE_MAPS_API_KEY` berre på tenarsida. For gangruter
trengst også `SPROYT_PLACE_ROUTE_SIGNING_KEY`: ein sjølvstendig, tilfeldig
32-byte nøkkel koda med base64url utan padding, lik på alle replikaer. Helm
kan lese dei frå eksisterande Kubernetes Secrets via
`secret.googleMapsExistingSecret`/`googleMapsApiKeyKey` og
`secret.googleMapsRouteExistingSecret`/`googleMapsRouteKeyKey`. For det
separate terningoppslaget krevst `SPROYT_GEOAPIFY_API_KEY`, til dømes frå
`secret.geoapifyExistingSecret`/`geoapifyApiKeyKey`. Ingen av desse nøklane
skal leggjast i Git eller eksponerast til nettlesaren. Utan kartnøkkelen er
Google-verktøyet slått av; utan rutenøkkelen fungerer søket utan gangruteknapp.
Utan Geoapify-nøkkelen blir terninghandlingen ikkje vist. Geoapify er førebels
berre testa med simulerte svar; ein reell konto/nøkkel og ende-til-ende-prøve
trengst før han kan aktiverast.

**Ikkje aktiver dette på delt Canary-database utan ein koordinert plan.**
Migrasjon 0070 er felles for databasen, og migreringsjobbar med eldre binærar
kan avvise ukjende migrasjonar ved seinare synkronisering eller rollback.
Produksjon og Canary deler i tillegg agentjobbkø, så
det å setje ein nøkkel berre på Canary avgrensar ikkje effekten til
Canary-brukarar. Prøv funksjonen mot ei isolert database og kø, eller lag og
verifiser ei bakoverkompatibel, koordinert utrulling før migrasjon og aktivering.
Ingen av desse føresetnadene er oppfylte berre ved at ein Canary-image finst.
