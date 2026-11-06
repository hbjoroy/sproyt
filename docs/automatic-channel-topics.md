# Automatiske kanal-emne

Dato: 2026-11-06. Første implementasjon; utrulling og modellakseptanse står att.

## Avgjerda

Emne er ein avleidd indeks over meldingar, ikkje endringar i sjølve samtalen.
«Området mitt» får funksjonen på som standard. Andre kanalar og direkte
samtalar får henne av som standard og kan aktivere henne gjennom
kanalinnstillingane. Eit tomt eller lite innhald gir færre emne. Brukarflata
viser høgst seks emne og «Alle», med paginerte treff frå kanalhistorikken.

Kanalmodulen eig innstillingar, tilgang og treff. Innhaldsanalysen er ei felles
kapabilitet i `src/topics`, med ein avgrensa modelladapter og eit varig
databaselager. Ho blir køyrd i bakgrunnen i den eksisterande applikasjonen.
Vi innfører ikkje ein ny meldingsmeklar eller vektordatabase. Dersom målingar
viser at analyse bør skalerast eller isolerast separat, kan arbeidaren seinare
flyttast utan å endre kanal-API-et.

Modellen får avgrensa tekst eller ferdige bilet-førehandsvisingar og returnerer
berre korte emneord. Han har ingen verktøy eller høve til å hente lenkjer.
Opptil 40 nylege emneetikettar frå sjølve kanalen hjelper modellen å gjenbruke
same namn. Etikettar frå lagra kjelder blir haldne ved den lagra meldinga,
slik at dei er omfatta av tilgangskontrollen til originalen. Applikasjonen
normaliserer og indekserer orda og avgjer tilgang.

## Konsistens og tilgang

Migrasjon 73 registrerer analysearbeid transaksjonelt ved meldingar og relevante
medieendringar. Historikk blir teken i avgrensa bolkar. Arbeid med unik lease
kan prøvast om att etter avbrot; endringar ugyldiggjer eldre arbeid. Modellkall
held ikkje ei databasetransaksjon open. Feil i analysen stoppar ikkje sending
eller førehandsvising av bilete.

Alle visingar, teljingar og treff krev gjeldande kanalmedlemskap. Lagra meldingar
i «Området mitt» blir analyserte frå originalen, ikkje teksten «Lagra melding».
Indeksen må framleis kontrollere kjeldemeldinga og eigaren sin tilgang ved
lesing. Sletta kjelder og tap av tilgang må skjule også emne som berre kom frå
desse kjeldene. Lenkjer til andre lagra lenkjer blir ikkje følgde rekursivt.

Innhald og modellresultat skal ikkje skrivast til driftsloggar. Val av
modell-endepunkt avgjer kvar tekst og bilete blir sende; aktivering skal bruke
eit endepunkt godkjent for dei aktuelle kanalane.

## Drift og aktivering

Kanalinnstillinga og operatøren si aktivering av arbeidaren er separate.
Arbeidaren er av som standard, også om den personlege kanalen har emne på.

| Miljøvariabel | Føremål |
| --- | --- |
| `SPROYT_CHANNEL_TOPICS_WORKER_ENABLED` | Start arbeidaren ved eksakt `true` |
| `SPROYT_CHANNEL_TOPICS_MODEL` | Eksplisitt modell-ID |
| `SPROYT_CHANNEL_TOPICS_MODEL_URL` | Valfri dedikert API-base, til dømes med `/v1` |
| `SPROYT_CHANNEL_TOPICS_MODEL_API_KEY` | Nøkkel for dedikert base |
| `SPROYT_VLLM_URL`, `SPROYT_VLLM_API_KEY` | Brukte når dedikert base ikkje er sett |

Helm har tilsvarande `config.channelTopicsWorkerEnabled`, `channelTopicsModel`
og `channelTopicsModelUrl`, og `secret.channelTopicsExistingSecret` med
`channelTopicsApiKeyKey`. API-basen får `/chat/completions` lagt til.
Nettverkspolicyen må tillate det valde endepunktet; eksisterande vLLM-reglar kan
brukast når same modellteneste blir vald.

Arbeidaren har eitt aktivt arbeid per aktivert applikasjonsprosess, to sekund
mellom jobbar og 25 sekund timeout per modellkall. Ein jobb les høgst 4 000
teikn tekst og opptil ti støtta biletvedlegg, med høgst 4 MiB per førehandsvising
(høgst 40 MiB innlesne biletdata per jobb).
Kvart modellkall kan gi opptil fem emneord på 40 teikn i eit svar på høgst 16 KiB.
Kvart bilete får eigne stikkord med medie-ID som opphav. Elleve sekvensielle
modellkall kan ta opptil om lag 275 sekund; lease er 360 sekund. Arbeidaren
kontrollerer tilgang og kjeldeversjon før kvart kall og avbryt ved shutdown.
Fleire replikaer deler
databasen, men aukar samla modelltrafikk. Start med avgrensa aktivering og mål
køalder, feilrate, modellkall og responstid før kapasiteten blir auka. Med
`sproyt::topics::worker=debug` i loggfilteret kan operatøren sjå analysetid og
tal på bilete og emneord per jobb.

Analyseversjon og kjeldeversjon blir lagra med resultata. Endring av ei kjelde
gjer gamle resultat usynlege straks; oppdatering av lagra kopiar skjer i bolkar
på høgst 64. Historikk blir òg lesen i bolkar på 64 meldingar. Første versjon
gjenbruker ikkje biletanalyse mellom meldingar eller lagra kopiar; det kan
optimaliserast med ein cache avgrensa til kjelde og analyseversjon når faktisk
modellbruk er målt. Ingen cache skal dele innhald på tvers av tilgangsgrenser.

Treffoppslag brukar emne-/sekvensindeksen. Valet av dei seks synlege emna er
førebels ei eksakt aggregering av den autoriserte emneindeksen for kanalen,
oppfriska kvart 30. sekund medan samtalen er synleg. Ho kan bli dyrare i store
kanalar. Mål denne spørringa særskilt før vi legg til førehandsrekna teljarar;
slike teljarar må òg handtere tap av tilgang til lagra kjelder korrekt.

Migrasjonen er additiv og støttar at eldre applikasjonar framleis skriv
meldingar gjennom dei same tabellane. Ved tilbakeføring skal den nye
migrasjonshistoria bevarast; køyr ikkje eldre binær sin migrator mot henne.

## Akseptanse før aktivering

- PostgreSQL og SQLite: standardinnstilling, rettar, historikk, redigering,
  sletting, gjenforsøk, stale lease og tilgang til lagra kjelder.
- Modelladapter: avgrensa input/output, ugyldig svar, ingen verktøykall og
  separate tekst-/biletgrunnlag med ein lokal testserver.
- Brukarflate: kanalbyte, paginering, valde filter, smal skjerm, begge tema,
  feiltilstandar og opning av eldre treff i avgrensa samtalekontekst.
- Vald modell: prøv norske meldingar og faktiske bilete i ein isolert pilot.
  Vurder emnekvalitet, synonym, stabilitet og kostnad. Mock-tester dokumenterer
  kontrakten, ikkje kvaliteten på modellen sine emne.

Vurder embeddings eller meir avansert gruppering først dersom piloten viser
at gjenbruk av etikettar ikkje gir gode nok emne. Kontroller databaseplanar og
reelle lese-/skrivekostnader før større endringar i lagring eller prosessar.
