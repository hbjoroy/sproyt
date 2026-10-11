# Store originalar og køyr mediebehandling frå kø

Status: plan etter Astra-vurdering, 2026-10-11. Ikkje implementert.

## Mål og produksjonsstatus

Brukaren skal kunne laste opp store originalbilete og videoar utan å redusere
dei sjølv. Originalfilene skal bevarast byte for byte. Opplasting, vising og
nedlasting skal ikkje krevje at webserveren held heile fila i minnet.
Førehandsvisingar og agentens biletebehandling skal ha eigne ressursgrenser.

Prod og canary køyrer framleis `a35a5a7`, med minnegrensa auka til 512 MiB
gjennom rocket-applications PR #242. Sprøyt PR #258 er merga, men ikkje
publisert eller utrulla. Kravet der om manuell reduksjon over 24 MP er
avvist. Denne grensa må fjernast frå den komande løysinga før neste release;
ikkje publiser noverande main som ei ferdig løysing på medieproblemet.

## Kvifor ei kø ikkje er nok åleine

Opplastingsruta samlar multipart-fila i ein `Vec`, repository lagrar ein heil
`bytea`, og nedlastingsruta les heile fila tilbake til ein `Vec`. Manglande
preview kan dessutan føre til at originalen blir lesen. Å leggje berre
skalering i ei kø fjernar derfor ikkje minnetoppane ved store filer.

Første løysing brukar PostgreSQL, som vi allereie driftar, til avgrensa
filblokker og ei varig jobbkø. Objektlager kan kome seinare utan å endre
meldingane sine attachment-ID-ar. Unngå Redis eller ei ny køteneste no.

## Etappar

### Q0 — sikring og kapasitet

- Behald 512 MiB som mellombels headroom; dette er ikkje løysinga i seg sjølv.
- Erstatt den avviste opplastingsgrensa i #258 før kodeutrulling. Behald
  avgrensa samtid for operasjonar som framleis dekodar i webprosessen.
- Fastset maksimal filstorleik, brukarkvote og samla lagringsbudsjett som
  driftsgrenser, ikkje som krav om å redusere vanlege mobilbilete sjølv.
- PostgreSQL-disken har om lag 4,6 GiB ledig av 8 GiB per 2026-10-11.
  Avklar disk- og backupkapasitet før store videoar blir allment aktiverte.
  Ingen automatisk PVC-utviding utan kontroll av Longhorn-kapasitet.

### Q1 — varig lagring og avgrensa nedlasting

- Additiv migrasjon for upload-sesjonar og ordna filblokker, om lag 1 MiB
  per blokk; tilsvarande repository-støtte for PostgreSQL og SQLite.
- Behald `media_objects`, attachment-ID-ar og eksisterande `media_blobs`.
  Ny lagring får ein eigen storage-key/type. Les både gamle og nye filer.
- Strøm ferdige filer blokkvis med backpressure. Støtt HEAD, Content-Length
  og eitt byte-range med korrekte 206/416-svar for videospoling.
- Kontroller tilgang før lesing, og avbryt lesing når klienten forsvinn.
- Avgrens samtidige overføringar og blokkbuffer per prosess; strømming
  åleine er ikkje ei grense for samla minnebruk.
- Test ekte gamle og nye vedlegg, range-grenser og stabilt minne ved ei
  nedlasting som er større enn minnegrensa til appen.

### Q2 — opplastingsprotokoll og klient

- Opprett eigar- og kanalavgrensa upload-sesjon før innhaldet blir sendt.
  Kontroller kanaltilgang, kvote, venta storleik og utløp.
- Reserver kvote atomisk ved oppretting og tel òg uferdige opplastingar.
  Frigjer reservasjonen ved ferdigstilling eller trygg opprydding.
- Kontroller eigarskap og gjeldande kanaltilgang på kvar blokkoperasjon
  og ved endeleg publisering, ikkje berre ved sesjonsoppretting.
- Klienten sender `File.slice` i små, sekvensielle blokker med framdrift,
  avbrot og retry. Blokkindeks og hash gjer retry idempotent; avvis
  motstridande innhald for ein allereie lagra blokk.
- Kvar blokk blir lagra i ein kort transaksjon. Ikkje samle heile fila i
  klienten med `arrayBuffer`, i webserveren eller i ein lang DB-transaksjon.
- Forsegl sesjonen før sluttkontroll. Kontroller lengd og SHA-256 blokkvis,
  og publiser mediaobjektet atomisk og idempotent etter godkjend kontroll.
  Ingen samtidige blokkskrivingar under sluttkontroll.
- Rydd utgåtte/avbrotne sesjonar og ubrukte originalar med eksplisitt
  retensjon. Rydding må aldri ta filer som ei melding refererer til.
- Etter reload kan brukaren måtte velje den same lokale fila på nytt;
  gjenbruk berre blokker som faktisk samsvarar med fila.
- Fjern gamle 35 MiB-klientgrenser når ny protokoll er aktiv. Hald
  request-grensene små for blokkrutene; ikkje auk alle HTTP-grenser blindt.

### Q3 — varig preview-kø og isolert worker

- Opprett preview-jobben saman med ferdigstilling av mediaobjektet.
  Jobbar har lease, frist, forsøksteljar, backoff og endeleg feilstatus.
- Køyr éin worker i ein eigen Deployment til å starte med, med eige minne,
  CPU og avgrensa diskbasert scratch. Webappen skal ikkje dekode originalen.
- Prøv filbasert `vipsthumbnail`/libvips med shrink-on-load. Roter og
  normaliser berre derivatet; originalfil og originalhash er uendra.
- Worker-OOM eller timeout skal ende med avgrensa retry/feila preview,
  medan originalen framleis kan lastast ned. Lease må kunne hentast att
  etter worker-krasj. Ikkje bruk det eksisterande 16 MiB `/tmp` som
  arbeidsdisk for store filer.
- Agent-vision og genereringsreferansar brukar avgrensa derivat eller ei
  eiga køoppgåve; dei skal ikkje lese vilkårleg store originalar i webappen.

### Q4 — brukaroppleving, video og akseptanse

- Vis pending/feila preview i tidslinja; aldri automatisk last ein stor
  original fordi preview manglar. Gje eksplisitt opning/nedlasting.
- Behald video utan transkoding først. Spel av når nettlesaren støttar
  kodeken; tilby nedlasting elles. Poster-uttrekk er ei seinare køoppgåve.
- For store filer på iOS: direkte nedlasting når OS-deling ville krevje
  ein heil browser-`File` i minnet. Behald tydeleg tilbakemelding.
- Test bilete over 35 MiB og over 24 MP, video større enn appminnet,
  eksakt SHA-256 etter nedlasting, samtidige trege overføringar,
  avbrot/retry, tilgangstap, full disk og worker-krasj.
- Kontroller backup/restore med både gamle blobs, nye blokker og køjobbar.
  Køstatus skal ikkje gjere originalen utilgjengeleg etter restore.

## Utrulling og review

Q1–Q3 kan leverast som små PR-ar med feature-flagg og additivt skjema.
Gamle lesarar og skrivarar blir verande til den nye flyten er testa i canary.
Bruk Astra til review av tilgang/finalisering og worker-isolasjon før
aktivering. Følg CI/CD og GitOps for app, worker og migrasjonar.

Aktiver ny skrivar i canary først. Verifiser minnebruk og akseptanse før
prod. Pending/feila-preview og fjerning av automatisk original-fallback frå
Q4 er absolutte krav før store filer blir aktiverte, også i canary.
Tilbakerulling må bevare lesetilgangen til filer som alt er lagra med
ny protokoll; ikkje rull tilbake til ein app utan den nye lesaren.
Konvertering av gamle blobs er ei seinare, avgrensa bakgrunnsoppgåve.

## Kjelder til teknologival

- [libvips: vipsthumbnail](https://www.libvips.org/API/current/using-vipsthumbnail.html)
  dokumenterer filbasert skalering og shrink-on-load. Bibliotekvalet skal
  validerast med representative filer og workerens faktiske minnegrense.
- [PostgreSQL: large objects](https://www.postgresql.org/docs/17/largeobjects.html)
  tilbyr òg strømming. Vanlege blokk-tabellar er tilrådde her for å bruke
  eksisterande repository-, tilgangs- og migrasjonsmønster.
