# Native HTTP: endepunktregister og første opptak

Dato: 3. november 2026. P02a / PAR-026. Baseline er den pinna Canary-revisjonen `142f71888cdde11dec42a698432530e208f577bb`. Denne endringa er additiv og aktiverer berre lesing og lagring av personlege varslingspreferansar for native. Ho endrar ikkje browserautorisasjon, upload, pushlevering eller clusterkonfigurasjon.

## Første vertikale snitt

`GET` og `PUT /api/v1/native/me/notifications/preferences` bruker den eksisterande `NotificationService`, same lagra preferansar og same validering som nettappen. Native-ruta returnerer berre preferansane; ho eksponerer ikkje browseren sin VAPID-nøkkel, tal pushabonnement eller kanalabonnement. Endepunktet er registrert i `src/web/native.rs` og implementert i den avgrensa childmodulen `src/web/native/notification_preferences.rs`. Det finst ingen generell bearer-fallback i `authenticate_http`.

Begge operasjonar krev nøyaktig éin `Authorization: Bearer <native access token>` og nøyaktig éin `X-Sproyt-Account: <canonical user UUID>`. Kontoheaderen er eit vern mot svar/sending frå feil aktiv konto, ikkje ei kjelde til autoritet. Det er alltid den servervaliderte native-granten som vel eigaren. Ingen admin-/kanalrolle er nødvendig for eigne preferansar; ruta kan ikkje velje ein annan brukar.

Tenesta må vere aktivert gjennom den eksisterande native-konfigurasjonen. `NativeSessions.authenticate` kontrollerer scope, Canary-miljø, service-instance, familien, generasjon, epoch, tokenutløp og den eigne providerøkta. Browsercookie, ugyldig eller duplisert Authorization, refresh-/revoke-/agentcredential og alle queryparameterar blir avviste. Ruta har ikkje `participant`, `user_id` eller annan querykontrakt. Manglande, duplisert eller feil kontoheader gir `409` etter vellukka autentisering. Ugyldig legitimasjon gir `401`; deaktivert native-teneste gir `404`. Når Origin/Sec-Fetch-Site finst, gjeld same-origin-vernet frå den eksisterande transportpolicyen; eit vanleg native HTTP-kall treng ikkje ein Origin-header. PUT materialiserer den autentiserte brukaren gjennom den eksisterande chattenesta etter kontofencen, slik at første native-innlogging kan lagre preferansar før ei chat-/WS-tilkopling har oppretta brukarrada.

Request for PUT og respons for begge metodar:

```json
{
  "mode": "weekly",
  "direct_messages": false,
  "mentions": true,
  "weekly_weekday": 3
}
```

`mode` er `instant`, `weekly` eller `muted`; `weekly_weekday` er 1–7. Feil JSON, ukjend modus, manglande felt eller ekstra felt gir `400`. Ugyldig vekedag gir same `409` som domenetenesta for browser. Requestbody er avgrensa til 8192 byte; større body gir `413`. Ingen konto- eller ressurs-ID kan følgje med i JSON. Vellukka respons har `Cache-Control: private, no-store` og servervald `X-Sproyt-Account`; han set ingen browsercookie. PUT er ei idempotent erstatting av eigne preferansar. Ho innfører ikkje ein ny revisjons-/konfliktprotokoll; ved samtidige gyldige endringar gjeld den eksisterande domenetenesta si lagringsåtferd.

Ein klient må binde operasjonen til den opphavlege kontoen og øktgenerasjonen og ignorere eit seint svar etter kontobytte. Kontoheaderen erstattar ikkje dette klientvernet. `404` kan brukast som uttrykkeleg «denne serverversjonen tilbyr ikkje ruta»; det blir ikkje annonsert nokon generell HTTP-paritetskapabilitet.

## Register: varsling og konto

«Browser» nedanfor betyr eksisterande cookieautorisasjon, med participant berre i eksplisitt utviklingsmodus. Ein native bearer gir ikkje i seg sjølv tilgang til desse browser-rutene. Rolle-/eigarreglane i domenetenesta gjeld framleis.

| Rute / metode | Faktisk eigar og åtferd | Native i dette snittet | Neste pakke / testport |
| --- | --- | --- | --- |
| `/api/v1/native/me/notifications/preferences` GET/PUT | Native grant → innlogga konto → `NotificationService.settings/save_preferences`. GET returnerer berre preferences. | Implementert med obligatorisk kontoheader og avgrensa opptak. | PAR-026 / P14 kan bygge UI over dette kontraktsnittet etter integrasjon. |
| `/api/v1/me/notifications` GET/PUT | `src/web/account.rs`: browserkonto; GET har òg browser-pushstatus og kanal-ID-ar. PUT lagrar dei same fire preferansefelta. | Uendra, ingen bearer-opptak. | Browser read/write-regresjon og kryssklient read-back blir testa i P02a. |
| `/api/v1/channels/{id}/notifications` PUT/DELETE | `src/web/account.rs`: same autentiserte bruker; `NotificationService.set_channel_subscription` kontrollerer medlemskap og avviser DM. | Ikkje aktivert. | PAR-026: avgrensa native rute, rolle-/medlemskapstap, feil kanal, gamle svar og kontofence. |
| `/api/v1/me/push-subscriptions` POST/DELETE | Browser Web Push-abonnement, endpoint- og nøkkelvalidering. | Ikkje aktivert. | PAR-027/P18 må velje faktisk Windows-mekanisme; ikkje bruk browserregistrering som eit falskt native pushprodukt. |
| `/api/v1/me/push-subscriptions/check` POST | Kontoeigd oppslag etter kjent browserendpoint, avgrensa JSON. | Ikkje aktivert. | Berre relevant etter plattformval; ingen generell endpoint-oppslagstilgang. |
| `/api/v1/me/favorite-circles*`, `/api/v1/me/statuses`, `/api/v1/me/emojis` | `src/web/account.rs`: kontoeigde val og eksisterande kontofence. | Ikkje aktivert. | PAR-001/PAR-024: eigne testbare snitt, inkludert feil konto og seint svar. |

Enkle profil-/statusendringar, lesing, omtalar, oppgåver og medlemskap har allereie versjonerte chatkommandoar. Dei skal bruke eksisterande native WS-/command-opptak og ikkje få parallelle REST-domeneimplementasjonar berre fordi UI-et er native.

## Register: media og opplasting

`src/web/media.rs` skil eksplisitt lesing frå opplasting. Dei autoriserte GET-/HEAD-rutene for media har alt eit selektivt native bearer-opptak med same kanaltilgang som browser. Dette gir ingen rett til å opprette media. `src/web/media_uploads.rs` bruker framleis browserautorisasjon; skrivemetodane krev i tillegg eksisterande same-origin-policy. Ingen av rutene nedanfor er endra i P02a.

| Rute / metode | Faktisk kontrakt på baseline | Native no / port før aktivering |
| --- | --- | --- |
| `/api/v1/media/{id}` og `/download` GET/HEAD | Kanalautorisasjon og avgrensa streaming/Range. | Eksisterande native lesetilgang; bevar mixed-authority-avvising, revoked/expired og feil kanal. |
| `/api/v1/media/{id}/preview` og `/status` GET/HEAD | Same autorisasjon; preview og status frå medielageret. | Eksisterande native lesetilgang. Ingen ny offentlig URL eller cookieoverføring. |
| `/api/v1/channels/{id}/media` POST | Legacy multipart, browserkonto, maks 35 MiB media og 36 MiB outer-body; kontrollert biletebehandling. Avviser legacyflyten når ny upload er på for kanalen. | Ikkje aktivert. P13 må velje den faktiske servercapabilityen, ikkje sende nativefiler gjennom tilfeldig legacybane. |
| `/api/v1/media-uploads/config` GET | `enabled`, `allowed_channel_ids`, `max_file_bytes`, `chunk_bytes`; browserautentisering. | Ikkje aktivert. Første kandidat i eit eige upload-snitt, saman med full opptaks-/eigarpolicy. |
| `/api/v1/channels/{id}/media-uploads` POST | Avgrensa JSON med request-ID, filnamn, type og deklarert storleik; konto-/kanal-/kvotekontroll i `MediaStore`. Same-origin før oppretting. | Ikkje aktivert. Krev native kontofence og negativ test av kanalrettar før store byte blir lesne. |
| `/api/v1/media-uploads/{id}` GET | Oppslag på kontoeigd upload, utløp og framdrift. | Ikkje aktivert. Test feil eigar, utløp, konto A→B→A og tapte lokale filer. |
| `/api/v1/media-uploads/{id}/chunks/{index}` PUT | Same-origin, eigarskap før body, SHA-256-header, 1 MiB chunk, maks 60 sekund lesing og fire transferpermit. Retry må ha same innhald/hash. | Ikkje aktivert. Native adapter må bevare grenser, request-/filidentitet, avbrot og retry. |
| `/api/v1/media-uploads/{id}/complete` POST | Same-origin, kontoeigar, éi samtidig finalisering, deduplisert ferdig media og etterfølgjande domenetilgangskontroll. | Ikkje aktivert. Revoke/tilgangstap rundt finalisering og tapt svar krev dokumentert utfall. |
| `/api/v1/media-uploads/{id}` DELETE | Same-origin og kontoeigar; avbryt staged upload. | Ikkje aktivert. Avbrot skal ikkje slette andre kontoar sine filer eller ferdig publisert media. |

P13 må levere opplasting som éin vertikal kontrakt: config → opprett → chunks/status → fullfør/avbryt → vedlegg i opphavleg kanal-/trådutkast → eksplisitt sending. Eit isolert beareropptak på berre POST er ikkje opplastingsparitet. Servergrenser, kvotar, staging-utløp og filbudsjett skal haldast avgrensa. Full offlinesync er ikkje nødvendig for å opne dette online-snittet, men varig upload-/utkastidentitet og trygg konto-/restart-handtering er nødvendig.

## Seinare ruter: ingen implisitt aktivering

| Familie og kjelde | Native-status etter P02a | Før feature-UI kan bruke ruta |
| --- | --- | --- |
| Imagegen list/create/preview/review, `src/web/imagegen.rs` | Browserautorisasjon. | PAR-014/P17: eigar, kanal/trådmål, privat review, referansemedia og konto-/øktfence. |
| Krets-/kanalagentar, memory/location/place/handoff, `src/web/chatbot.rs`, `agent_memory.rs`, `agent_location.rs`, `agent_place_lookup.rs` | Browserautorisasjon. | PAR-028–032/P15: skil read/admin/consent, bevar roller, revisjonar og avgrensa capabilities. |
| Heart/prosesspilot/work-items, `src/web/processes.rs`, `process_pilot.rs`, `work_items.rs` | Browserautorisasjon. | PAR-033/PAR-034/P16: medlemskap, prosessrolle, oppgåveeigar, kjeldemelding og stabil operasjons-ID. |
| Enrollment, `src/web/enrollment.rs` | Browserautorisasjon og servervald Authentik-operasjon. | PAR-022/P14: eksplisitt globalt/kretsscope, relevant rolle og faktisk identitetsaksept. |
| Agent-/Grafana-integrasjonar, `src/web/agents.rs`, `integrations.rs` | Browser-/serviceautoritet etter eksisterande rute. | PAR-032: skil menneskeleg administrasjon frå webhook-/agentcredential. Ingen native bearer-fallback på serviceinngangar. |

## Verifikasjon

Den nye HTTP-kontrakten ligg i `src/auth/tests/native_notification_preferences_tests.rs`, registrert med to testlinjer i `src/auth.rs` sitt eksisterande testmodul. Han gjenbruker den syntetiske OIDC-provideren og går gjennom ekte dedikert native login, callback, account-confirm, exchange og validator. Ingen runtime-endring er gjort i autentiseringsmodulen.

Testane bruker eit faktisk migrert SQLite-varslingslager utan førehandsoppretta native-brukar og kontrollerer første PUT før chat-/WS-opning, native → browser og browser → native read-back, uendra andre brukar, manglande/feil/duplisert kontoheader, cookie/bearer-blanding, duplisert og feil tokenautoritet, query/participant, ugyldig body/vekedag og bodygrense, cross-site-headerar, utløp, revoke og deaktivert teneste. Browsergranten held fram etter native revoke. Scope-negative testar bruker refresh/revoke/agentcredential, som ikkje kan opptre som native access. Samtidigheit/rotasjon i sessiontenesta har separate eksisterande testar.

Målretta kommando:

```powershell
$env:CARGO_TARGET_DIR = 'S:\Build\native-parity-http\target'
cargo test --locked --bin sproyt native_notification_preferences
```

SQLite-fixturane inneheld berre syntetiske testdata og ligg under targetmappa sin `native-http-fixtures/`. Dette snittet gjer ingen live-/Canary-skriving eller utrulling.

Faktisk køyrt 3. november 2026 i den reine P02a-worktreeen på Windows ARM64, Rust 1.96.0, mot baseline pluss denne ucommitta endringa:

| Kontroll | Resultat |
| --- | --- |
| `cargo test --locked --bin sproyt native_notification_preferences -- --test-threads=1` | 2 grøne HTTP/OIDC/SQLite-testar. Etter review vart førehandsoppretta konto fjerna frå fixturen og førstegongs-PUT retta; dei endelege gruppene nedanfor køyrde med den eksplisitte kontrollen for manglande brukarrad. |
| `cargo test --locked --bin sproyt native_ -- --test-threads=1` | 14 testfunksjonar rapporterte grønt, inklusive den nye HTTP-kontrakten, eksisterande grant-/refresh-/media-testar og native-session-testar. Den PostgreSQL-avhengige testen er miljøguardert og gir ikkje PostgreSQL-aksept her. |
| `cargo test --locked --bin sproyt notification -- --test-threads=1` | 16 testfunksjonar rapporterte grønt, inklusive nye native-testar og eksisterande varslings-/push-/kontotestar. Den PostgreSQL-avhengige testen er miljøguardert og gir ikkje PostgreSQL-aksept her. |
| `cargo fmt --all -- --check`, `git diff --check` | Grøne. |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Grøn. |

Gruppene overlappar; tala skal ikkje summerast til eit tal uavhengige testar. Full integrert release-/PostgreSQL-/Canary-aksept kjem etter integrasjon og er ikkje erstatta av desse lokale prøvene.
