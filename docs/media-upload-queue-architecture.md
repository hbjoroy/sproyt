# Mediearkitektur: originalar, blokker og isolert behandling

Dato: 2026-10-11. Design konkretisert og kontrollert av Astra.
Dette dokumentet utdjupar `media-upload-queue-plan.md` (PR #259).

## Kontraktar og leveringsrekkjefølgje

1. **Lesar:** `ChatRepository::media_metadata` og `read_media_slice`, med
   forwarding gjennom ChatEngine og implementasjon i PostgreSQL, SQLite og
   testlageret. Original- og download-rutene brukar HTTP-strømming og ranges.
2. **Lager:** additive tabellar, kvotereservasjon og opplastingslivssyklus.
   Utvid lesaren til å støtte både gamle blobs og nye blokker.
3. **Skrivar:** små HTTP-blokker, retry og same flyt frå begge composerar og
   share-target. Ingen dekoding eller omskriving av originalen ved mottak.
4. **Worker:** separat image/Deployment og varig preview-kø.
5. **Aktivering:** pending-preview, bounded agent-derivat og kvoter for alle
   skrivarar; canary-akseptanse før prod.

Etappe 1 er implementert i `codex/media-streaming-reads`, men ikkje utrulla.
Lager og HTTP-protokoll er implementerte i `codex/media-chunk-storage`,
med nye skrivarar avslått. Den separate workeren, image og Helm-oppsett er
implementerte i `codex/media-preview-worker`, men er framleis deaktiverte.
Klientintegrasjonen er implementert i `codex/media-upload-client`: same
blokkprotokoll for kanal, tråd og share-target; retry med same identitet
inkludert brotne 2xx-responsar; ingen fullfil-buffer i nettlesaren.
Aktiveringsgatane står att. CI kontrollerer stale leases,
publisering som ventar på lås til etter utløp, og at timeout/shutdown
drep og reap-ar kodekprosessen før opprydding. Eit separat kodeksmoke-test
lagar førehandsvising av eit 100-megapikselbilete innanfor 512 MiB.
PR #258 si avviste opplastingsgrense må erstattast før
kodeutrulling. Det gjeld også ein release som inneheld berre den nye lesaren.

## Lesing og kompatibilitet

Metadata blir henta utan blob. Kvar SQL-lesing returnerer høgst 1 MiB og
kontrollerer gjeldande kanalmedlemskap. Ingen SQL-kopling eller transaksjon
blir halden medan klienten tek imot blokka. Response-body held ein av åtte
download-permits per webprosess fram til ferdigstilling eller kansellering.

GET støttar eitt byte-range, inkludert suffix og open slutt, med 206/416.
HEAD les berre metadata og ignorerer Range. SHA-256 er sterk ETag;
If-Range med annan validator gir full respons. Fleire ranges og ukjend
range-eining blir ignorerte. Uventa EOF eller tilgangstap avsluttar straumen.
Attachment-filnamn, autentisering og `nosniff` blir bevarte.

Legacy bruker SQL `substring`/`substr` av eksisterande blob. Dette avgrensar
webserverens filbuffer; PostgreSQL kan framleis måtte dekomprimere ein
TOAST-verdi. Vi aukar derfor ikkje legacy-multipartgrensa som del av lesaren.
Gamle preview-fallbacks bruker òg denne strøymde ruta. For nye, store filer
skal manglande preview gi ein status, aldri automatisk original-fallback.

## Nye tabellar

- `media_uploads`: server-ID, owner, channel, client request-ID, filnamn,
  deklarert type, venta storleik, neste blokkindeks, mottatte byte,
  `open`/`sealed`/`complete`, utløp og endeleg media-ID. Unik owner/request-ID.
- `media_upload_chunks`: upload-ID, indeks, lengd, SHA-256 og innhald.
  Primærnøkkel upload/indeks, hard sjekk på høgst 1 MiB per blokk.
- Singleton budget-lock for atomisk kvoteadmission.
- `media_preview_jobs`: media-ID, status, forsøk, available-at, lease-token,
  lease-utløp, siste feil og derivative-version.

Ferdige sesjonar blir bevarte fordi dei eig filblokkene. Endeleg media-ID
refererer `media_objects` med delete-cascade; blokkene refererer sesjonen
med delete-cascade. `storage_key=chunks:v1:<upload-id>` skil nye filer frå
gamle `db:<media-id>`. Publiser aldri eit uferdig mediaobjekt.

Kvote blir rekna under budget-lock: ferdige originalstorleikar pluss venta
storleik for alle uferdige sesjonar, globalt og per eigar. Kvar fil/sesjon
blir belasta minst 1 MiB for å dekkje rad-/indeks-/preview-overhead.
Høgst åtte aktive sesjonar per eigar og 64 totalt blir godtekne atomisk.
Utgåtte sesjonar
tel til blokkene faktisk er sletta. Dette unngår skjøre kvoteteljarar i
første versjon. Genererte bilete og andre legacy-skrivarar må delta i same
budsjett før store filer blir aktivert.

## Opplastingslivssyklus

Repository-operasjonar: create, status, put-chunk, seal, read-sealed-slice,
complete og abort. Alle kontrollerer eigarskap og gjeldande kanaltilgang.
PostgreSQL bruker row locks; SQLite bruker reell write-transaction før
lesing og validering, slik prosjektets eksisterande atomiske flytar gjer.

Put-chunk låser sesjonen og kontrollerer open status, utløp, indeks, hash
og korrekt lengd (full blokk, bortsett frå siste). Berre neste indeks eller
ein byte-identisk retry av tidlegare indeks blir godteken. Innsetting og
framdrift blir commit-a i ein kort transaksjon.

Complete forseglar sesjonen etter kontroll av lengd og blokktal. Deretter
blir immutable blokker lesne avgrensa for SHA-256 og signaturkontroll,
utan pikseldekoding. Ein kort slutt-transaksjon kontrollerer tilgang og
sealed status på nytt, publiserer mediaobjekt, fullfører sesjonen og
enqueue-ar preview-jobben. Gjentatt complete returnerer same mediaobjekt.
Ein avbroten klient kan etterlate sealed sesjon; retry kan starte hashing
på nytt. Avgrens samtidige finaliseringar; ikkje lagre SHA internals i DB.

## HTTP og klient

```text
POST   /api/v1/channels/{id}/media-uploads
GET    /api/v1/media-uploads/{id}
PUT    /api/v1/media-uploads/{id}/chunks/{index}
POST   /api/v1/media-uploads/{id}/complete
DELETE /api/v1/media-uploads/{id}
```

Autoriser og ta transfer-permit før body blir konsumert. Ein `Bytes`-
extractor som allereie har bufra innhaldet er for seint. JSON og chunk-ruter
har små body-grenser. Klienten sender `File.slice` sekvensielt med backoff
ved mellombels kapasitetssvar. Ikkje krev full browser-hash: WebCrypto digest
er ikkje inkrementell. Per-blokk hash og serverens fullhash er tilstrekkeleg.

Multipart-kompatibilitet kan bruke `field.chunk()` til same blokk-sink,
med gammal liten request-grense. Oppdatert klient bruker blokkprotokollen
for store filer. Etter reload må eventuell revald lokal fil verifiserast
mot lagra blokker før resume.

Nye skrivarar krev `SPROYT_MEDIA_CHUNK_UPLOADS_ENABLED=true` og eksplisitte
bytegrenser i `SPROYT_MEDIA_MAX_FILE_BYTES`, `SPROYT_MEDIA_OWNER_BUDGET_BYTES`
og `SPROYT_MEDIA_TOTAL_BUDGET_BYTES`. Ingen av desse er aktiverte i prod
eller canary. Dette er operatørkonfigurasjon for seinare aktivering, ikkje
ei tilråding om å slå på funksjonen før dei andre gatane er ferdige.

## Separat worker og feilisolering

`sproyt media-worker` skal veljast før server/AppConfig/OIDC-start. Han
treng berre DB, logging, scratch, ressursgrenser og shutdown. Legg til
Tokio fs/process/io-util. Web-image blir verande utan libvips; eit eige
worker-image har libvips-runtime og same Rust-binær. Éin worker først.

Worker claim-ar jobben med token og utløp, skriv originalen blokkvis til
unik diskfil og køyrer `vipsthumbnail` utan shell, med faste argument og
servervalde stiar. Subprosessen har timeout og skal drepast og reap-ast
ved timeout/shutdown. Cleanup fjernar scratch; startup ryddar gamle
arbeidsfiler. Diskbasert scratch må dimensjonerast; dagens 16 MiB `/tmp`
er ikkje tilstrekkeleg.

Les berre avgrensa derivative-output. Publish sjekkar gjeldande lease-token
og at lease ikkje er utgått; ein gammal worker får aldri overskrive eit
nyare resultat. Preview-status er separat frå `analysis_status`. OOM,
timeout og ugyldig fil gir avgrensa retry/feila preview, men originalen er
framleis tilgjengeleg. Ingen EXIF-rotasjon eller re-encoding av originalen.

Agent-vision og genereringsreferansar må bruke avgrensa derivat eller eiga
køoppgåve før aktivering. Video blir lagra utan transkoding; nettlesaren
spelar av støtta kodekar. Poster-uttrekk er ei seinare worker-oppgåve.

## Akseptanse og drift

Test eksakt hash for store JPEG, motion-photo, PNG, HEIC og video; samtidige
kvotereservasjonar, conflicting retry, dobbel complete, tilgangstap,
slow clients, kansellering, ranges og EOF. Test stale lease, worker-kill,
timeout og full disk. Backup/restore skal bevare originalar og både gamle
og nye lesarar. Aktiver aldri store filer før pending-preview og avgrensa
agentlesing er ferdige.

PostgreSQL hadde om lag 4,6 GiB ledig på 8 GiB PVC ved siste kontroll.
Set eit eksplisitt budsjett for originalar, WAL, backups og scratch før
stor-video-aktivering. Objektlager er neste lagringsalternativ ved vekst,
ikkje eit krav for første protokoll. Canary og prod følgjer CI/CD og GitOps.
Rollback må minst vere til ein release med dual reader for nye blokker.
