# Kapasitetsprøve og ressursbudsjett – 10. november 2026

Status: gjennomført lokal leveranse B under #353 og #345, med Astra si metode- og arkitekturvurdering. **Førebels budsjett; ingen godkjenning av clusterkapasitet.** Målekoden frå [PR #402](https://github.com/hbjoroy/sproyt/pull/402) er føresetnad og er ikkje utrulla gjennom denne leveransen.

**Avgjerd: behald dagens modulære form.** Behald pooltak og replikaetal. Prøva gir ikkje grunnlag for ny broker, kanalpartisjonering eller arbeidarisolering. Prioriter eit gjennomførbart Heart-utrullingsbudsjett. Dersom faktisk trafikk krev det målte mettingsnivået, mål socket-/protokollventing og databaseutføring før ei avgrensa effektivisering; låg acquire-venting åleine forklarer ikkje ende-til-ende-latensen.

## Prioritering frå produkteigar

10. november 2026: færre enn 30 aktive brukarar. Dei lokale 160/s-prøvene gir tilstrekkeleg grunnlag for dagens prioritering; vidare arbeid med overlast, admission-frist og kontrollert avvising blir utsett. Dette gjer ikkje den lokale prøva til ein produksjonsgaranti. Vurder på nytt ved vedvarande kø/latens, fleire timeoutar, minnepress eller vesentleg trafikk-/ressursendring. Neste prioritet er [Heart sitt utrullingsbudsjett](heart-rollout-budget.md).

## Lokal prøve og gyldigheit

- App: commit `8b35d137631de152b38b33b7fd1bb113647816d3`, Windows ARM64 **release/optimized**, låste avhengnader; binær-SHA256 `63d480eba8381a4fc9025b20a692b41746f175e16e2c6108207eb4b9fae4eb44`. Maskin: Snapdragon X2 Elite, 12 logiske CPU-ar.
- PostgreSQL **18.6**, eiga `wslc`-instans, loopback-port 55453, syntetiske databasar som blir sletta etter kvar prøve. 0,5 CPU og 1536 MiB hardgrense svarar til dagens databasepod; lokal lagring og operativsystem gjer ikkje ytinga direkte samanliknbar med drift. Ingen produksjonslast eller feilinjeksjon.
- Alle API-prosessar deler **éin logisk CPU** via affinitet. Samla 1 GiB RSS/private-byte stopptak er mjuk kontroll, ikkje OS-minnegrense. Poolbudsjettet er fast: **1 × 4 mot 2 × 2**, pluss éin dedikert listener per API (5 mot 6 samband). Kontrollklientar blir rekna separat.
- Fem private kanalar, 10 oppvarmingsmeldingar kvar, 512 teikn per målt melding, 32 sendesocketar totalt. Rolege kanalar held **2 meldingar/s kvar** medan varmkanalen får resten. Tidsstyrte forsøk, høgst 128 samtidige; også usende forsøk og generator-etterslep tel.
- Ingen aktive valfrie arbeidarar i grunnprofilen. Bakgrunnsprofilen aktiverer **éin faktisk emnearbeidar berre på API-1**, same 40-jobbars etterslep i eigen kanal, lokal modellstub med 100 ms svartid. Åtte jobbar blir fullførte per 15-sekundsprøve; 32 står att, med målt køalder i evidensen. Dette prøver ikkje push, agentar, media eller modellfeil.
- Kvar hovudcelle har tre prøver à minst 1200 planlagde sendingar, om lag 15 sekund målevindauge og 100 planlagde, serielle reconnect-forsøk. Reconnect byter replika når to finst, abonnerer på nytt og hentar historikk frå siste sekvens. Vi kontrollerer varige ACK-ar, samanhengande/unike sekvensar og 20 idempotente replay per prøve.
- Mål for **denne prøva**: samla og kvar hot/quiet-profil ≤750 ms send-p99 frå planlagt tidspunkt, generatorlag-p99 ≤100 ms, alle 100 recovery-forsøk gjennomførte utan feil og recovery-p99 ≤5 s. Prøva gjenbruker dei eksisterande 750 ms-/femsekundsmåla i `operations.md`, men legg til planlagd sendetid, generatorlag og krav om 100 gjennomførte recovery-forsøk. Ho utgjer ikkje produksjonsaksept.

Små, nye databasar og korte prøver gir ikkje bevis for vedvarande last, produksjonsstorleik eller kapasitet med alle tillegg. Fast rekkjefølgje og variasjon mellom prøver gjer at små skilnader mellom replikaetal ikkje kan tolkast som sikker effekt. Ressursprøvene er punktmålingar med om lag 250 ms API- og 750 ms pluss kontrollkostnad DB-intervall.

## Resultat

Ved **80 meldingar/s** held alle 18 gyldige prøver måla. Alle 21 600 planlagde sendingar får ACK, og alle 1800 reconnect-forsøk blir gjennomførte utan feil. Tabellen viser **spennet i p99 mellom tre prøver**, i millisekund; histogram er ikkje brukte til klientkvantilane.

| API | Profil | Send-p99 | Roleg-p99 | Recovery-p99 |
| ---: | --- | ---: | ---: | ---: |
| 1 | Jamn | 45.4–49.5 | — | 89.0–119.3 |
| 1 | Travel + rolege | 52.0–90.7 | 60.4–91.9 | 91.4–145.6 |
| 1 | Same + emnearbeidar | 53.9–158.7 | 64.9–168.7 | 103.2–217.6 |
| 2 | Jamn | 45.9–58.2 | — | 99.7–117.5 |
| 2 | Travel + rolege | 61.4–149.1 | 62.7–237.9 | 116.4–295.4 |
| 2 | Same + emnearbeidar | 54.6–57.7 | 56.9–66.6 | 94.1–122.4 |

Neste nivå er prøvd tre gonger per oppsett. Ved **160/s** består alle seks prøver. Ved **320/s** består ingen av dei seks prøvene måla. Tabellen viser spennet mellom repetisjonane, ikkje eit estimat for produksjonskapasitet.

| Planlagd rate/s | API | ACK-ar per prøve | Send-p99 ms | Recovery forsøkt |
| ---: | ---: | ---: | ---: | ---: |
| 160 | 1 | 2400–2400 / 2400 | 76.7–80.4 | 100–100 / 100 |
| 160 | 2 | 2400–2400 / 2400 | 58.0–91.1 | 100–100 / 100 |
| 320 | 1 | 3159–3191 / 4800 | 1875.7–1937.2 | 82–83 / 100 |
| 320 | 2 | 4141–4166 / 4800 | 1102.0–1334.3 | 26–28 / 100 |

Ved 320/s kjem alle manglande sendingar frå **generatoren si in-flight-grense**: kommandoane vart aldri sende. Dette er ikkje serveravvising, timeout eller dokumentert server-backpressure. Reconnect-løkka rekk færre enn 100 forsøk; manglande forsøk er ikkje faktiske reconnect-feil. Ingen av dei forsøkte reconnectane i dei seks gyldige 320-prøvene feilar, men observerte svartider kan overstige fem sekund. Haleestimata med færre enn 100 observasjonar er ikkje grunnlag for ein 99-prosentpåstand.

Alle ACK-ar i dei 30 valde prøvene er varige, sekvensane er unike og samanhengande, og replay gir same melding. Ingen ukjende sendeutfall vart funne. Ved 320/s er målt send-acquire lågt (sjå fasehistogram i evidensen), medan aktørkøventing aukar. Klientlatensen er større enn den målte aktørventinga: venting før aktør-admission og databaseutføring er ikkje tilstrekkeleg skilde til å fastslå rotårsaka. Poolstorleik eller CPU-grense er derfor ikkje påvist som flaskehals.

Ressursoppsummering for dei 24 beståtte prøvene ved 80/160: API-CPU 5.5–9.6 % av éin logisk CPU; samla topp-RSS 24.4–45.0 MiB; database-CPU 15.2–26.7 % av éin CPU med hardtak 50 %. Detaljerte p50/p95/p99, throughput, generatorlag, histogramsnitt, throttling, minne, lagring og køalder står i [prøvesamandraget](evidence/database-capacity-2026-11-10-probe.json).

**Måleretting:** eldre teljing av `pg_stat_activity` skilde ikkje alle klientar frå andre backend-typar. Merking av kontrollsamband åleine løyste ikkje dette. Den endelege teljinga brukar `backend_type='client backend'` og skil observatørar frå appen. Opphavet til gamle umerkte rader kan ikkje fastslåast; autovacuum er berre ei mogleg forklaring. Feilstoppa prøver er bevarte som ekskluderte, ikkje som kapasitetsgrenser. Heile 2-API/jamn-cella og alle avgjerande lastnivå er gjentekne; berre prøver utan slike stopp er valde. Samandraget viser kjeldekøyring og alle eksklusjonar. Fullstendige lokale ressursseriar ligg i `.local/353-capacity-*.json` og blir genererte på nytt av verktøyet.

## Dagens delte ressursar – lesande kartlegging

[Sanitert driftsbevis](evidence/database-capacity-2026-11-10-live.json) skil observerte tal frå konfigurasjon. PostgreSQL 18.6 har `max_connections=100`, 3 superbrukarreserverte og 0 andre reserverte: **97 vanlege plassar**. Tre tidsstempla punktprøver 08.07–08.11 UTC fann **71–73 klientar** utan kontrollklienten: Sprøyt API/media 13–14, Sprøyt Heart 5, identitet 13 og andre klientar 40–41. Dette er ikkje maksimum eller ein historisk tidsserie.

| Forbrukar | Normalt konfigurert sambandstak | Samtidig prod/canary-rullering, før ekstra terminerande poddar |
| --- | ---: | ---: |
| Sprøyt API: prod 2 × (4 pool + 1 listener), canary 1 × (2 + 1) | 13 | 21 |
| Mediaarbeidar, Recreate, eigen pool 2 | 2 | 2 |
| Sprøyt Heart: prod 2 + canary 1, pool 2 kvar | 6 | 10 |
| **Kjent appsum** | **21** | **33** |
| Grafana, Recreate | 10 | 10 |
| Authentik og andre appklientar | Ukjent | Ukjent |
| Migrering, Heart-bootstrap, backup, drift og reserve | Ikkje ferdig budsjettert | Ikkje ferdig budsjettert |

**Konkret avvik:** dei tre Heart-poddane er lesande mappa til same databaserolle, som har **rolletak 8**. Normal poolsum 6 passar; konfigurert samtidig surge gir 10 og passar ikkje. Dette er eit budsjettavvik, ikkje bevis for ti faktisk opne samband. Migrering, to samtidige bootstrap-containerar per definisjonsjobb og terminerande poddar kan krevje meir. Avklar ei prøvd sekvensering/surge-grense før slik overlapp; ikkje auk rolletaket automatisk.

Kjende CPU-/minnetak: kvar API 500m/512 MiB, Heart 500m/256 MiB, mediaarbeidar 500m/512 MiB med 2 GiB ephemeral storage; PostgreSQL 500m/1536 MiB; Grafana 750m/768 MiB. Authentik og fleire andre Heart-instansar manglar ressursgrenser i den observerte konfigurasjonen. Tre noder har kvar 4 allokerbare CPU-ar og om lag 7,87 GiB minne; dette summerer ikkje til ei godkjenning av samtidige poddtak, plassering eller reserve.

Databasen sin Longhorn-PVC er 8 GiB; filsystemprøva viste om lag 3,83 GiB brukt og 3,93 GiB ledig. Sum ikkje-template databasar var om lag 3,62 GiB under punktprøvene; Sprøyt stod for om lag 2,59–2,60 GiB. Filsystemet omfattar meir enn tabellstorleik, mellom anna WAL. Backup er planlagd 03.00 med `Forbid`, seriell `pg_dump` og `psql`, utan konfigurerte CPU-/minnegrenser; tilkoplingstak, IO-topp og backup-/WAL-reserve er uverifiserte. Sprøyt-migrering har kodefast pooltak 1 utan listener. Heart-migrering/bootstrap og samtidige hook-/backupscenario har enno ikkje verifiserte totalar; chart-standardar er ikkje driftsbevis.

## Neste avgrensa steg og opne eigarval

1. **Drift + Heart-eigar:** prøv eitt utrullingsscenario mot rollegrense 8 som inkluderer produksjon, canary, terminerande poddar, migrering og begge bootstrap-containerane. Dokumenter sekvensering og grenser før endring i drift.
2. **Infrastruktur/identitet + andre DB-eigarar:** fastset dokumenterte maksimum og drifts-/identitetsreserve under 97-plassgrensa, samt CPU/minne, lagring/WAL, backup-IO og overlapp. Namngjev ansvarlege; desse er eigarroller, ikkje tildelte personar.
3. **Produkt + chat-kjerne:** vedta trafikk-/varigheitsmål og representativ datastorleik. Dersom 320/s eller køkopling er relevant, mål socket-til-aktør-venting og dei dyre databaseoperasjonane før kodeendring. Emneprøva åleine vel ikkje push eller annan arbeidar for #354.

#353 blir ståande open for det samla budsjettet og eigaraksepten. #170 si tidlegare driftsaksept blir ikkje opna att; denne rapporten er supplerande bevis. #355 kan gå vidare med den avgrensa kontodatautskiljinga. Ingen delte konfigurasjonsgrenser, databaseinnhald eller utrulling i drift er endra av arbeidet.

## Gjenta prøva

Køyr frå eigen arbeidskopi i PowerShell 7. Bruk `DevCache.ps1` for release-bygg etter diskplasskontrollen. Les `wslc --help` og hjelpa for `run`, `exec`, `inspect`, `stop` og `remove` før bruk.

```powershell
$env:SPROYT_FRONTEND_PREBUILT='1' # krev eksisterande gyldige frontend-artefaktar
$env:CARGO_PROFILE_RELEASE_DEBUG='0'
& 'S:\Build\DevCache\DevCache.ps1' Run -CargoArgs @('build','--release','--locked')
# Bruk Target frå DevCache.ps1 Status for den aktuelle arbeidskopien.
wslc run --name sproyt-353-capacity-pg --label sproyt.capacity-fixture=true --detach --publish 127.0.0.1:55453:5432 --env POSTGRES_USER=sproyt --env POSTGRES_PASSWORD=capacity-fixture --env POSTGRES_DB=postgres --cpus 0.5 --memory 1536M postgres:18.6-alpine -c max_connections=32 -c shared_buffers=128MB
node tools/capacity-probe.mjs --confirm-isolated --binary '<Target>\release\sproyt.exe' --ramp
# Gjenta siste beståtte nivå med tre prøver dersom rampen berre prøvde det éin gong:
node tools/capacity-probe.mjs --confirm-isolated --binary '<Target>\release\sproyt.exe' --replicas 1,2 --profiles hot --rate 160 --repetitions 3 --output .local/353-capacity-160-final.json
node --test tools/test-capacity-probe.mjs
```

Passordet ovanfor er berre for den syntetiske eingongsinstansen. Runneren krev rett label, loopback-binding, ingen host-bind mounts, ressursgrenser og genererte databasenamn før skriving. Han startar og stoppar eigne API-prosessar og slettar eigne prøvedatabasar. Etterpå: kontroller eigarskap/ID, stopp berre denne fixture-instansen og fjern henne med det anonyme volumet. Evidens skal takast vare på før opprydding.

Validering: 10 Node-kontraktar, 45 CI-scope-prøver og 5 browser-shard-prøver består; eksisterande MCP-lastkontrakt består; PowerShell-parser og `git diff --check` er reine. App-kode og frontend er uendra i denne leveransen; gyldig Rust/PostgreSQL/browser-CI frå #402 blir gjenbrukt.
