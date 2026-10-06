# M1: lagring og eige minne-API

M1 legg til varig lagring og brukarstyring, men startar ikkje innsamling,
modellbygging eller bruk av minne i svar. M2 gir brukarflata; M3 må setje
startgrenser ved faktisk aktivering. Ingen meldingar blir historisk lesne
inn fordi eit minneval blir lagra i M1.

Migrasjon `0058_agent_memory.sql` finst for PostgreSQL og SQLite. Profilen
er unik per krets, agent og brukar. Kanalskop har ein monotont aukande
startsekvens og cursor, generasjon og plass for seinare arbeidarlease.
Notat har strukturert innhald (`jsonb`/JSON-tekst), kategori, opphav,
evidensstatus, revisjon og utløp. Eigne tabellar held kjeldeversjonar,
deltakaravhengnader og gløymsperrer. Indeksar dekkjer eigar, kanal, kjelde,
deltakar og ventande arbeid; ingen innhaldsindeks eller ekstern cache.

Profilens framandnøkkel til kretsmedlemskap slettar eigne minne atomisk ved
utmelding. Kjelde-ID-ar blir bevarte når ei melding blir sletta fysisk:
ei manglande kjelde skal ugyldiggjere notatet, ikkje fjerne avhengnaden og
gjere dei attståande kjeldene til eit tilsynelatande gyldig grunnlag.
M3 legg til aktiv ugyldiggjering ved andre medlemskaps- og kjeldeendringar.

## Tilgang og innsyn

API-et tek eigaren frå den autentiserte menneskelege principalen. Ein
kretsadministrator kan handsame sitt eige minne, men kan ikkje velje ein
annan brukar. Førespurnadene krev medlemskap i kretsen og ein agent som
tilhøyrer den kretsen. Det finst ikkje ein administratorvariant av API-et.

Lesing og eksport bruker same snapshot og same kontrollar. Kvar kjelde
må framleis finnast, vere ei menneskemelding, ha uendra versjon og høyre
til notatkanalen og kretsen. Brukaren må framleis vere kanalmedlem, og
agenten må ha gyldig tilgang; private kanalar krev uttrykkeleg agentval.
Ein kjeldedeltakar som har meldt seg ut, gjer notatet utilgjengeleg.

Skjulte, utgåtte eller ugyldige notat blir berre talde i `unavailable_notes`.
API-et viser ikkje ID, tekst, kanal eller kjelder for desse. Eit kjent
notat kan framleis gløymast av eigaren, og nullstilling fjernar også
utilgjengelege notat. Avslått innsamling skjuler ikkje lesbare eigne notat.

## Rutene

Basisrute:
`/api/v1/me/circles/{circle_id}/chat-agents/{agent_id}/memory`

| Metode | Handling |
| --- | --- |
| `GET` | Eigen profil, lesbare notat og innhaldsfritt tal på utilgjengelege notat. Ingen profil blir oppretta ved lesing. |
| `PATCH` | Lagrar `{"revision":0,"enabled":true}` eller tilsvarande av-val. |
| `POST /actions` | `correct`, `confirm`, `forget` eller `reset`, med venta profilrevisjon. |

Eksempel på retting:

```json
{"revision":1,"action":"correct","note_id":"<UUID>","text":"Eg vil gjerne lære litt gresk."}
```

Stadfesting og gløyming bruker `note_id` utan tekst. Nullstilling bruker
berre `revision` og `action:"reset"`. Ukjende eigarfelt og handlingar blir
avviste. Notattekst er avgrensa til 1 KiB UTF-8, og retting kontrollerer
16 KiB samla tekstbudsjett, også for skjulte notat. Deltakarar og kjelder
kan ikkje veljast gjennom rettings-API-et.

Ein ny profil har revisjon null. Kvar mutasjon aukar profilrevisjonen og
`memory_epoch`; retting/stadfesting aukar også notatrevisjonen og merker
notatet som brukarstyrt og stadfesta. Kjeldeavhengnadene blir bevarte.
Ein konkurrerande eller utdatert mutasjon får `409`; endringar blir
ikkje delvis lagra. SQLite tek skrivarlåsen før autoritetslesing.
PostgreSQL bruker serialiserbare mutasjonar og låser eigarprofilen.

Svar med minnedata og handterte feil får `Cache-Control: no-store`.
Mutasjonar avviser førespurnader frå ein annan nettstad etter same
opphavskontroll som HTTP-kommandoane.

## Gløyming, nullstilling og plassgrenser

Gløyming sperrar notatet sine kjelde-ID-ar og slettar alle eigne notat
som avheng av desse kjeldene. Gløyminga og auken i tryggleiksepoken skjer
i same transaksjon. Chatmeldingane og allereie publiserte agentsvar blir
ikkje sletta.

Når gløymsperrene elles ville passere taket på 1 024, blir dei komprimerte
til nye startgrenser i alle eksisterande kanalskop. All eldre historikk
blir då stengd for ny læring, og `history_compactions` aukar. Urelaterte
notat blir bevarte. Grensene går aldri bakover, og gamle lease blir fjerna.
M3/M4 må respektere desse grensene også for ombygging og bakgrunnskontekst.

Nullstilling slettar alle eigne notat og sperrer og flyttar startgrensene
for alle lagra kanalskop. Minnevalet blir bevart, men innsamling må få eit
nytt startpunkt ved seinare aktivering. Eit av-val bevarer notata og stoppar
seinare innsamling og bruk; eigaren kan framleis inspisere og gløyme dei.

## Agentval og eksport

Det eksisterande agent-API-et får `memory_enabled`, av som standard.
Feltet er valfritt i oppdateringar: utelating bevarer eksisterande val.
Dette agentvalet erstattar ikkje brukarens samtykke eller utrullingsflagga.

Brukaren kan lagre eit påval i M1, men svaret viser alltid
`collection_available:false` og `collection_started_at:null`. Det er eit
lagra val, ikkje ei melding om at læring har starta. Første aktivering i M3
må opprette/oppdatere scope-grensene før innsamling blir tillaten.

`/api/v1/me/export` får eit additivt `agent_memories`-felt. Minnet blir lese
i den eksisterande kontoeksport-transaksjonen, med same eigar- og
kjeldekontroll som innsyns-API-et. Det opnar ikkje private kanalar eller
andre brukarar sine profilar.

## Verifisering og storleik

Same lagringskontrakt blir køyrd mot SQLite og faktisk PostgreSQL i CI.
Han dekkjer eigarisolasjon, agentavvising, revisjonskappløp, retting,
stadfesting, kjeldeendring, privat tilgang, gløyming av skjulte notat,
nullstilling, medlemskapskaskade, eksport, komprimering og tekstbudsjett.
Ein HTTP-prøve bruker dei verkelege rutene og kontoeksporten.

PostgreSQL-prøva lagrar ein syntetisk stor profil med 24 notat, 720
kjeldeavhengnader, 1 024 sperrer og 64 kanalskop. Ho rapporterer tuplebyte
for denne profilen, indeksbyte for heile CI-tabellen og WAL-endring under
fixturebygginga. Sistnemnde omfattar også kanaloppsett og er ei
cluster-måling, ikkje ein presis kostnad per brukarhandling.
CI måler også ein komprimert `pg_dump` av alle syntetiske minneprofilar i
testdatabasen. Backup/restore-jobben kontrollerer det utvida skjemaet og
eit lagra notat med kjeldeavhengnad, med innsamling avslått. Reelt
backupforbruk og driftsretensjon må framleis målast i M6 før aktivering;
tuple-, indeks- og WAL-målingane åleine er ikkje ei slik måling.
