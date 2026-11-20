# KapasitetsprÃ¸ve og ressursbudsjett â€“ 10. november 2026

Gjeldande driftsbudsjett og utrullingskontroll frå 20. november: [delt kapasitetsbudsjett](shared-capacity-envelope.md).
Status: gjennomfÃ¸rt lokal leveranse B under #353 og #345, med Astra si metode- og arkitekturvurdering. **FÃ¸rebels budsjett; ingen godkjenning av clusterkapasitet.** MÃ¥lekoden frÃ¥ [PR #402](https://github.com/hbjoroy/sproyt/pull/402) er fÃ¸resetnad og er ikkje utrulla gjennom denne leveransen.

**Avgjerd: behald dagens modulÃ¦re form.** Behald pooltak og replikaetal. PrÃ¸va gir ikkje grunnlag for ny broker, kanalpartisjonering eller arbeidarisolering. Prioriter eit gjennomfÃ¸rbart Heart-utrullingsbudsjett. Dersom faktisk trafikk krev det mÃ¥lte mettingsnivÃ¥et, mÃ¥l socket-/protokollventing og databaseutfÃ¸ring fÃ¸r ei avgrensa effektivisering; lÃ¥g acquire-venting Ã¥leine forklarer ikkje ende-til-ende-latensen.

## Prioritering frÃ¥ produkteigar

10. november 2026: fÃ¦rre enn 30 aktive brukarar. Dei lokale 160/s-prÃ¸vene gir tilstrekkeleg grunnlag for dagens prioritering; vidare arbeid med overlast, admission-frist og kontrollert avvising blir utsett. Dette gjer ikkje den lokale prÃ¸va til ein produksjonsgaranti. Vurder pÃ¥ nytt ved vedvarande kÃ¸/latens, fleire timeoutar, minnepress eller vesentleg trafikk-/ressursendring. Neste prioritet er [Heart sitt utrullingsbudsjett](heart-rollout-budget.md).

## Lokal prÃ¸ve og gyldigheit

- App: commit `8b35d137631de152b38b33b7fd1bb113647816d3`, Windows ARM64 **release/optimized**, lÃ¥ste avhengnader; binÃ¦r-SHA256 `63d480eba8381a4fc9025b20a692b41746f175e16e2c6108207eb4b9fae4eb44`. Maskin: Snapdragon X2 Elite, 12 logiske CPU-ar.
- PostgreSQL **18.6**, eiga `wslc`-instans, loopback-port 55453, syntetiske databasar som blir sletta etter kvar prÃ¸ve. 0,5 CPU og 1536 MiB hardgrense svarar til dagens databasepod; lokal lagring og operativsystem gjer ikkje ytinga direkte samanliknbar med drift. Ingen produksjonslast eller feilinjeksjon.
- Alle API-prosessar deler **Ã©in logisk CPU** via affinitet. Samla 1 GiB RSS/private-byte stopptak er mjuk kontroll, ikkje OS-minnegrense. Poolbudsjettet er fast: **1 Ã— 4 mot 2 Ã— 2**, pluss Ã©in dedikert listener per API (5 mot 6 samband). Kontrollklientar blir rekna separat.
- Fem private kanalar, 10 oppvarmingsmeldingar kvar, 512 teikn per mÃ¥lt melding, 32 sendesocketar totalt. Rolege kanalar held **2 meldingar/s kvar** medan varmkanalen fÃ¥r resten. Tidsstyrte forsÃ¸k, hÃ¸gst 128 samtidige; ogsÃ¥ usende forsÃ¸k og generator-etterslep tel.
- Ingen aktive valfrie arbeidarar i grunnprofilen. Bakgrunnsprofilen aktiverer **Ã©in faktisk emnearbeidar berre pÃ¥ API-1**, same 40-jobbars etterslep i eigen kanal, lokal modellstub med 100 ms svartid. Ã…tte jobbar blir fullfÃ¸rte per 15-sekundsprÃ¸ve; 32 stÃ¥r att, med mÃ¥lt kÃ¸alder i evidensen. Dette prÃ¸ver ikkje push, agentar, media eller modellfeil.
- Kvar hovudcelle har tre prÃ¸ver Ã  minst 1200 planlagde sendingar, om lag 15 sekund mÃ¥levindauge og 100 planlagde, serielle reconnect-forsÃ¸k. Reconnect byter replika nÃ¥r to finst, abonnerer pÃ¥ nytt og hentar historikk frÃ¥ siste sekvens. Vi kontrollerer varige ACK-ar, samanhengande/unike sekvensar og 20 idempotente replay per prÃ¸ve.
- MÃ¥l for **denne prÃ¸va**: samla og kvar hot/quiet-profil â‰¤750 ms send-p99 frÃ¥ planlagt tidspunkt, generatorlag-p99 â‰¤100 ms, alle 100 recovery-forsÃ¸k gjennomfÃ¸rte utan feil og recovery-p99 â‰¤5 s. PrÃ¸va gjenbruker dei eksisterande 750 ms-/femsekundsmÃ¥la i `operations.md`, men legg til planlagd sendetid, generatorlag og krav om 100 gjennomfÃ¸rte recovery-forsÃ¸k. Ho utgjer ikkje produksjonsaksept.

SmÃ¥, nye databasar og korte prÃ¸ver gir ikkje bevis for vedvarande last, produksjonsstorleik eller kapasitet med alle tillegg. Fast rekkjefÃ¸lgje og variasjon mellom prÃ¸ver gjer at smÃ¥ skilnader mellom replikaetal ikkje kan tolkast som sikker effekt. RessursprÃ¸vene er punktmÃ¥lingar med om lag 250 ms API- og 750 ms pluss kontrollkostnad DB-intervall.

## Resultat

Ved **80 meldingar/s** held alle 18 gyldige prÃ¸ver mÃ¥la. Alle 21 600 planlagde sendingar fÃ¥r ACK, og alle 1800 reconnect-forsÃ¸k blir gjennomfÃ¸rte utan feil. Tabellen viser **spennet i p99 mellom tre prÃ¸ver**, i millisekund; histogram er ikkje brukte til klientkvantilane.

| API | Profil | Send-p99 | Roleg-p99 | Recovery-p99 |
| ---: | --- | ---: | ---: | ---: |
| 1 | Jamn | 45.4â€“49.5 | â€” | 89.0â€“119.3 |
| 1 | Travel + rolege | 52.0â€“90.7 | 60.4â€“91.9 | 91.4â€“145.6 |
| 1 | Same + emnearbeidar | 53.9â€“158.7 | 64.9â€“168.7 | 103.2â€“217.6 |
| 2 | Jamn | 45.9â€“58.2 | â€” | 99.7â€“117.5 |
| 2 | Travel + rolege | 61.4â€“149.1 | 62.7â€“237.9 | 116.4â€“295.4 |
| 2 | Same + emnearbeidar | 54.6â€“57.7 | 56.9â€“66.6 | 94.1â€“122.4 |

Neste nivÃ¥ er prÃ¸vd tre gonger per oppsett. Ved **160/s** bestÃ¥r alle seks prÃ¸ver. Ved **320/s** bestÃ¥r ingen av dei seks prÃ¸vene mÃ¥la. Tabellen viser spennet mellom repetisjonane, ikkje eit estimat for produksjonskapasitet.

| Planlagd rate/s | API | ACK-ar per prÃ¸ve | Send-p99 ms | Recovery forsÃ¸kt |
| ---: | ---: | ---: | ---: | ---: |
| 160 | 1 | 2400â€“2400 / 2400 | 76.7â€“80.4 | 100â€“100 / 100 |
| 160 | 2 | 2400â€“2400 / 2400 | 58.0â€“91.1 | 100â€“100 / 100 |
| 320 | 1 | 3159â€“3191 / 4800 | 1875.7â€“1937.2 | 82â€“83 / 100 |
| 320 | 2 | 4141â€“4166 / 4800 | 1102.0â€“1334.3 | 26â€“28 / 100 |

Ved 320/s kjem alle manglande sendingar frÃ¥ **generatoren si in-flight-grense**: kommandoane vart aldri sende. Dette er ikkje serveravvising, timeout eller dokumentert server-backpressure. Reconnect-lÃ¸kka rekk fÃ¦rre enn 100 forsÃ¸k; manglande forsÃ¸k er ikkje faktiske reconnect-feil. Ingen av dei forsÃ¸kte reconnectane i dei seks gyldige 320-prÃ¸vene feilar, men observerte svartider kan overstige fem sekund. Haleestimata med fÃ¦rre enn 100 observasjonar er ikkje grunnlag for ein 99-prosentpÃ¥stand.

Alle ACK-ar i dei 30 valde prÃ¸vene er varige, sekvensane er unike og samanhengande, og replay gir same melding. Ingen ukjende sendeutfall vart funne. Ved 320/s er mÃ¥lt send-acquire lÃ¥gt (sjÃ¥ fasehistogram i evidensen), medan aktÃ¸rkÃ¸venting aukar. Klientlatensen er stÃ¸rre enn den mÃ¥lte aktÃ¸rventinga: venting fÃ¸r aktÃ¸r-admission og databaseutfÃ¸ring er ikkje tilstrekkeleg skilde til Ã¥ fastslÃ¥ rotÃ¥rsaka. Poolstorleik eller CPU-grense er derfor ikkje pÃ¥vist som flaskehals.

Ressursoppsummering for dei 24 bestÃ¥tte prÃ¸vene ved 80/160: API-CPU 5.5â€“9.6 % av Ã©in logisk CPU; samla topp-RSS 24.4â€“45.0 MiB; database-CPU 15.2â€“26.7 % av Ã©in CPU med hardtak 50 %. Detaljerte p50/p95/p99, throughput, generatorlag, histogramsnitt, throttling, minne, lagring og kÃ¸alder stÃ¥r i [prÃ¸vesamandraget](evidence/database-capacity-2026-11-10-probe.json).

**MÃ¥leretting:** eldre teljing av `pg_stat_activity` skilde ikkje alle klientar frÃ¥ andre backend-typar. Merking av kontrollsamband Ã¥leine lÃ¸yste ikkje dette. Den endelege teljinga brukar `backend_type='client backend'` og skil observatÃ¸rar frÃ¥ appen. Opphavet til gamle umerkte rader kan ikkje fastslÃ¥ast; autovacuum er berre ei mogleg forklaring. Feilstoppa prÃ¸ver er bevarte som ekskluderte, ikkje som kapasitetsgrenser. Heile 2-API/jamn-cella og alle avgjerande lastnivÃ¥ er gjentekne; berre prÃ¸ver utan slike stopp er valde. Samandraget viser kjeldekÃ¸yring og alle eksklusjonar. Fullstendige lokale ressursseriar ligg i `.local/353-capacity-*.json` og blir genererte pÃ¥ nytt av verktÃ¸yet.

## Dagens delte ressursar â€“ lesande kartlegging

[Sanitert driftsbevis](evidence/database-capacity-2026-11-10-live.json) skil observerte tal frÃ¥ konfigurasjon. PostgreSQL 18.6 har `max_connections=100`, 3 superbrukarreserverte og 0 andre reserverte: **97 vanlege plassar**. Tre tidsstempla punktprÃ¸ver 08.07â€“08.11 UTC fann **71â€“73 klientar** utan kontrollklienten: SprÃ¸yt API/media 13â€“14, SprÃ¸yt Heart 5, identitet 13 og andre klientar 40â€“41. Dette er ikkje maksimum eller ein historisk tidsserie.

| Forbrukar | Normalt konfigurert sambandstak | Samtidig prod/canary-rullering, fÃ¸r ekstra terminerande poddar |
| --- | ---: | ---: |
| SprÃ¸yt API: prod 2 Ã— (4 pool + 1 listener), canary 1 Ã— (2 + 1) | 13 | 21 |
| Mediaarbeidar, Recreate, eigen pool 2 | 2 | 2 |
| SprÃ¸yt Heart: prod 2 + canary 1, pool 2 kvar | 6 | 10 |
| **Kjent appsum** | **21** | **33** |
| Grafana, Recreate | 10 | 10 |
| Authentik og andre appklientar | Ukjent | Ukjent |
| Migrering, Heart-bootstrap, backup, drift og reserve | Ikkje ferdig budsjettert | Ikkje ferdig budsjettert |

**Konkret avvik:** dei tre Heart-poddane er lesande mappa til same databaserolle, som har **rolletak 8**. Normal poolsum 6 passar; konfigurert samtidig surge gir 10 og passar ikkje. Dette er eit budsjettavvik, ikkje bevis for ti faktisk opne samband. Migrering, to samtidige bootstrap-containerar per definisjonsjobb og terminerande poddar kan krevje meir. Avklar ei prÃ¸vd sekvensering/surge-grense fÃ¸r slik overlapp; ikkje auk rolletaket automatisk.

Kjende CPU-/minnetak: kvar API 500m/512 MiB, Heart 500m/256 MiB, mediaarbeidar 500m/512 MiB med 2 GiB ephemeral storage; PostgreSQL 500m/1536 MiB; Grafana 750m/768 MiB. Authentik og fleire andre Heart-instansar manglar ressursgrenser i den observerte konfigurasjonen. Tre noder har kvar 4 allokerbare CPU-ar og om lag 7,87 GiB minne; dette summerer ikkje til ei godkjenning av samtidige poddtak, plassering eller reserve.

Databasen sin Longhorn-PVC er 8 GiB; filsystemprÃ¸va viste om lag 3,83 GiB brukt og 3,93 GiB ledig. Sum ikkje-template databasar var om lag 3,62 GiB under punktprÃ¸vene; SprÃ¸yt stod for om lag 2,59â€“2,60 GiB. Filsystemet omfattar meir enn tabellstorleik, mellom anna WAL. Backup er planlagd 03.00 med `Forbid`, seriell `pg_dump` og `psql`, utan konfigurerte CPU-/minnegrenser; tilkoplingstak, IO-topp og backup-/WAL-reserve er uverifiserte. SprÃ¸yt-migrering har kodefast pooltak 1 utan listener. Heart-migrering/bootstrap og samtidige hook-/backupscenario har enno ikkje verifiserte totalar; chart-standardar er ikkje driftsbevis.

## Neste avgrensa steg og opne eigarval

1. **Drift + Heart-eigar:** prÃ¸v eitt utrullingsscenario mot rollegrense 8 som inkluderer produksjon, canary, terminerande poddar, migrering og begge bootstrap-containerane. Dokumenter sekvensering og grenser fÃ¸r endring i drift.
2. **Infrastruktur/identitet + andre DB-eigarar:** fastset dokumenterte maksimum og drifts-/identitetsreserve under 97-plassgrensa, samt CPU/minne, lagring/WAL, backup-IO og overlapp. Namngjev ansvarlege; desse er eigarroller, ikkje tildelte personar.
3. **Produkt + chat-kjerne:** vedta trafikk-/varigheitsmÃ¥l og representativ datastorleik. Dersom 320/s eller kÃ¸kopling er relevant, mÃ¥l socket-til-aktÃ¸r-venting og dei dyre databaseoperasjonane fÃ¸r kodeendring. EmneprÃ¸va Ã¥leine vel ikkje push eller annan arbeidar for #354.

#353 blir stÃ¥ande open for det samla budsjettet og eigaraksepten. #170 si tidlegare driftsaksept blir ikkje opna att; denne rapporten er supplerande bevis. #355 kan gÃ¥ vidare med den avgrensa kontodatautskiljinga. Ingen delte konfigurasjonsgrenser, databaseinnhald eller utrulling i drift er endra av arbeidet.

## Gjenta prÃ¸va

KÃ¸yr frÃ¥ eigen arbeidskopi i PowerShell 7. Bruk `DevCache.ps1` for release-bygg etter diskplasskontrollen. Les `wslc --help` og hjelpa for `run`, `exec`, `inspect`, `stop` og `remove` fÃ¸r bruk.

```powershell
$env:SPROYT_FRONTEND_PREBUILT='1' # krev eksisterande gyldige frontend-artefaktar
$env:CARGO_PROFILE_RELEASE_DEBUG='0'
& 'S:\Build\DevCache\DevCache.ps1' Run -CargoArgs @('build','--release','--locked')
# Bruk Target frÃ¥ DevCache.ps1 Status for den aktuelle arbeidskopien.
wslc run --name sproyt-353-capacity-pg --label sproyt.capacity-fixture=true --detach --publish 127.0.0.1:55453:5432 --env POSTGRES_USER=sproyt --env POSTGRES_PASSWORD=capacity-fixture --env POSTGRES_DB=postgres --cpus 0.5 --memory 1536M postgres:18.6-alpine -c max_connections=32 -c shared_buffers=128MB
node tools/capacity-probe.mjs --confirm-isolated --binary '<Target>\release\sproyt.exe' --ramp
# Gjenta siste bestÃ¥tte nivÃ¥ med tre prÃ¸ver dersom rampen berre prÃ¸vde det Ã©in gong:
node tools/capacity-probe.mjs --confirm-isolated --binary '<Target>\release\sproyt.exe' --replicas 1,2 --profiles hot --rate 160 --repetitions 3 --output .local/353-capacity-160-final.json
node --test tools/test-capacity-probe.mjs
```

Passordet ovanfor er berre for den syntetiske eingongsinstansen. Runneren krev rett label, loopback-binding, ingen host-bind mounts, ressursgrenser og genererte databasenamn fÃ¸r skriving. Han startar og stoppar eigne API-prosessar og slettar eigne prÃ¸vedatabasar. EtterpÃ¥: kontroller eigarskap/ID, stopp berre denne fixture-instansen og fjern henne med det anonyme volumet. Evidens skal takast vare pÃ¥ fÃ¸r opprydding.

Validering: 10 Node-kontraktar, 45 CI-scope-prÃ¸ver og 5 browser-shard-prÃ¸ver bestÃ¥r; eksisterande MCP-lastkontrakt bestÃ¥r; PowerShell-parser og `git diff --check` er reine. App-kode og frontend er uendra i denne leveransen; gyldig Rust/PostgreSQL/browser-CI frÃ¥ #402 blir gjenbrukt.

## Ny driftsprÃ¸ve 19. november 2026

**Avgjerd frÃ¥ kapasitetsvurderinga:** Behald chataktÃ¸ren og den delte poolen. Det mest presserande avviket er no plass til ordinÃ¦re databaseinnloggingar. Ved to av tre punktprÃ¸ver var alle ordinÃ¦re plassar opptekne. Ei utrulling som treng nye samband mÃ¥ rekne inn migrering, nye og terminerande poddar og andre klientar fÃ¸r ho startar. Pooltak eller fleire API-replikaer pÃ¥ dei same 100 plassane kan auke presset.

| UTC | Tilkopla klientar utan observatÃ¸r | Plassar til nye ordinÃ¦re klientar |
| --- | ---: | ---: |
| 21.08.13 | 97 | 0 |
| 21.08.26 | 97 | 0 |
| 21.08.48 | 96 | 1 |

PostgreSQL hadde framleis 100 plassar, tre reserverte for superbrukar og ingen andre reserverte. Alle etablerte klientar tel mot innslagsgrensa pÃ¥ 97, ogsÃ¥ dei tre eksisterande superbrukarklientane i dei to fÃ¸rste prÃ¸vene. Klokka 21.08.48 var fordelinga API/media 15, Heart 6, identitet 9, Grafana 4 og andre 62. Berre to klientar var aktive ved siste prÃ¸ve. Eit release-backupoppdrag kÃ¸yrde under kontrollen. Tala seier ikkje kor lenge presset varte eller om ein brukar fekk feil. [Lesande SQL for ny mÃ¥ling](../tools/capacity-postgres-budget.sql) ekskluderer sin eigen klient og viser berre faste grupper.

| Eigarskap | Konfigurert ramme og overlapp |
| --- | --- |
| Chat-kjerne og drift | API-pool 4 i prod og 2 i canary, Ã©in lyttar per replika. Tre normale API-poddar brukar hÃ¸gst 13, samtidig konfigurert surge 21 fÃ¸r eventuelle terminerande poddar. Eiga SprÃ¸yt-migrering treng opptil Ã©in per jobb. |
| Media og Heart | Media har Ã©in arbeidar med pool 2. Heart kÃ¸yrer no med Recreate i begge miljÃ¸ og pool 2 pÃ¥ tre poddar. Den [isolert prÃ¸vde ramma](heart-rollout-budget.md) er 6 normalt, 8 med begge vedlikehaldsjobbane. Summen av kjende SprÃ¸yt-runtimegrenser er 21 normalt. Summen med begge API-surge, media og Heart-vedlikehald er 31 fÃ¸r API-migrering, terminerande API-poddar og backup. |
| Identitet, andre tenesteeigarar og infrastruktur | 15 av 16 innloggingsroller utan superbrukarrett hadde uavgrensa rolletak. Authentik-server og -arbeidar hadde Ã©in replika kvar, 25 % surge og ingen CPU-/minnetak. Grafana brukar Recreate. Eit komponentspecifikt pooltak kan avgrense ein klient sjÃ¸lv utan rolletak. Samla Ã¸vre grense for desse eigarane er uavklart. |
| Infrastruktur og lagring | Tre noder hadde kvar fire allokerbare CPU og 8255904 KiB minne. Bruk ved ei prÃ¸ve var 390â€“2374m CPU og 2757â€“4599 MiB per node. PostgreSQL brukte 155m / 567 MiB mot tak 500m / 1536 MiB. DB-volumet pÃ¥ 8 GiB hadde 4005876 KiB ledig. Ledig filplass er punktbruk. NÃ¸dvendig WAL-/backupreserve og I/O ved topp er enno uavklart. |
| Backup og drift | Den daglege jobben kl. 03 har Forbid og ingen CPU-/minnetak. Forbid avgrensar berre overlapp innan den same CronJob-en. Dei deklarerte release-backupvoluma var 32 GiB i kvart miljÃ¸ og eit ekstra 8 GiB-volum i canary. Storleik Ã¥leine viser ikkje ledig plass. |

For Ã¥ kunne halde av **Ã¥tte** ordinÃ¦re plassar til samtidig API-surge frÃ¥ normal drift (5 prod + 3 canary), mÃ¥ talet pÃ¥ andre og eksisterande klientar vere hÃ¸gst **89** fÃ¸r surge, og lÃ¥gare nÃ¥r migrering, terminering, backup, identitetsreserve eller andre nye klientar fell saman. Ã…tte er kostnaden ved nett denne operasjonen. Dei tre superbrukarplassane er ein separat reserve. Ved den siste prÃ¸va ville den skisserte auken mangle sju plassar fÃ¸r anna vedlikehald. Ingen slik utrulling eller lastprÃ¸ve vart starta som del av mÃ¥linga.

**Ope for #353:** Infrastruktur og dei delte tenesteeigarane mÃ¥ fastsetje eigne maksimum og ordinÃ¦r driftsreserve som kan haldast ogsÃ¥ ved utrulling. Godkjenning av dette samla budsjettet, inkludert lagring og backup, stÃ¥r att. Dei isolerte lastprÃ¸vene ovanfor dekkjer avgjerda om chatarkitekturen. [Driftsaksept #170](https://github.com/hbjoroy/sproyt/issues/170) stÃ¥r ved lag.
