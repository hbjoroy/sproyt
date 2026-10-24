# N02: køyrbar native kjerneprøve

Dato: 24. oktober 2026. Omfang: delvis N00/N02 frå [implementasjonsplanen](native-client-implementation-plan.md). Ingen innlogging, appgrensesnitt, serverendring eller produksjonsutrulling inngår.

## Stadfesta lokalt

Windows ARM64 med Rust 1.96.0 og .NET SDK 10.0.301 lasta det faktiske Rust-biblioteket frå ein C#-konsollklient gjennom P/Invoke. SQLite blir eigd av Rust. Ei felles fiktiv scenariofil blir lesen av både Rust- og C#-prøva.

| Prøve | Observert resultat |
| --- | --- |
| Bygg og ABI | Rust `cdylib` og .NET-konsoll bygd; ABI 1 blir kontrollert ved oppstart. Ingen WinUI- eller Android-app er bygd. |
| Varig lokalt opptak | Utkast blir fjerna i same SQLite-transaksjon som request-ID og wire-payload blir journalførte. Ingen nettverksrequest blir send. |
| Ein eigar | Eit anna C#-prosessforsøk får `store_busy` medan seed-prosessen held databasen open. |
| Prosessavslutning | Runneren tvangsavsluttar seed etter stadfesta commit utan å lukke native-handle. Ein ny prosess opnar og finn sendinga som uviss. |
| Replayidentitet | Foreldreprosessen held SHA-256 av dei faktisk lagra wire-byta frå før prosessdøden. Ny C#-prosess samanliknar den gjenfunne payloaden med denne kontrollsummen før replay. Endra innhald med same ID blir avvist. |
| Transaksjonsmatrise | Barn blir stogga etter outbox-insert, etter utkastfjerning før commit, og etter commit før API-svar. Før retry blir det kontrollert at dei to første bevarer utkast utan outbox, medan den siste bevarer éi fullstendig sending utan utkast. Retry gir same ID/bytar og éi journalrad. |
| Commit-feil | Ein utsett framandnøkkelkontroll gir verkeleg feil frå SQLite ved commit. Først blir det bevist at begge skriveoperasjonane når før-commit-punktet. Rollback blir kontrollert på same samband og etter reopen; ei ny skriving på same samband blir også prøvd. Dette er ikkje ein I/O-feil. |
| Kvittering | Simulert ACK blir lagra og toler dobbel ACK. Endeleg avvist sending kan ikkje gå tilbake til dispatch. Dette prøver ikkje ein ekte server. |
| Innkomande endring | Ei syntetisk redigering blir installert. Eldre entitetsversjon kan ikkje gjenopplive ei sletta melding i den lokale modellen. |
| Arbeidartråd | Alle native-kall, inkludert open/close, skjer på éin dedikert C#-tråd. To ventande oppgåver fyller køen; neste opptak får straks feil. Avbrot før arbeidaren tek oppgåva hindrar native-kall. Etter dette punktet blir det faktiske resultatet levert, også ved avbrot før native-skriving eller etter commit. |
| Utlogging og lukking | Utlogging stengjer opptak straks, avviser ventande gammalt arbeid og invaliderer generasjonen etter påbegynt arbeid. Vanleg lukking tømmer køen og lukkar native-handle éin gong på arbeidartråden, også ved gjenteken dispose. |
| Konto og generasjon | Feil konto kan ikkje opne databasen. Invalidering ryddar logiske data og aukar generasjon; eit seint gammalt kall blir avvist også etter reopen. |
| Feilgrenser | Rust testar ugyldig UTF-8, for liten output, for stor input, stale handle, close-race, ukjend schema, itemgrenser, read-only skrivefeil og feil rundt transaksjonen. Vanleg bibliotek avviser diagnostiske kommandoar. |

`cargo fmt --check`, Clippy med `-D warnings`, 14 Rust-testar med standardfunksjonar, 13 med `fault-injection`, .NET-bygg og den samla prosess-/arbeidarprøva er grøne lokalt. Skilnaden i testtal kjem av at standardprofilen har ein ekstra test som avviser diagnostiske kommandoar. Bygg og fixtures ligg på C:, ikkje i det plassavgrensa S:-treet.

Sjå [runner, livssyklus og ABI-avgrensingar](../native/probes/README.md) for reprodusering. JSON-kvitteringa i den private artefaktmappa har schema 2 og inneheld kjelderevisjon, dirty-status, plattform, bibliotekhashar, payload-hash frå før prosessdød og dei tre krasjpunkta. Ingen legitimasjon eller produksjonsdata inngår. Diagnostikk er avslått som standard og blir bygd/lasta frå ein separat biblioteksti.

## Review og CI

Astra vurderte først ABI, eigarskap, generasjonsfencing og journalopptak. Ei uavhengig revisjon tilrådde transaksjonsmatrisa, foreldrehalden kontrollsum og den avgrensa arbeidarprøva over. Sluttreview fann ingen blokkerande feil; to ekstra testar av ny skriving etter commit-feil og avbrot etter at arbeidaren har teke oppgåva vart tekne inn. Etter-commit-feil blir uttrykkeleg handsama som uviss kvittering, ikkje rollback.

Grunnlagsprøva i [PR 282](https://github.com/hbjoroy/sproyt/pull/282), kjelde `550376dc1c7bb78b90d076b0a34e50137126c65d`, er grøn på hosted Windows x64 og Linux x64 i [CI-køyring 40118539359](https://github.com/hbjoroy/sproyt/actions/runs/40118539359). Denne køyringa gjaldt før transaksjons- og arbeidarutvidinga. Ho er separat frå lokal Windows ARM64-evidens.

Den utvida prøva i [PR 283](https://github.com/hbjoroy/sproyt/pull/283), kjelde `67dcfcf05a40f979edc5631862afd0fc13ca1fb0`, er også grøn på hosted Windows x64 og Linux x64 i [CI-køyring 40120408892](https://github.com/hbjoroy/sproyt/actions/runs/40120408892). Begge nedlasta schema-2-kvitteringane har rein kjelde, elleve utfall og tre gjenoppretta krasjpunkt. Den testa merge-revisjonen har nøyaktig same kjeldetre som den vurderte head-revisjonen.

CI-scope skil denne sjølvstendige prøvemappa frå appkode. Prøvene får framleis native Windows/Linux-køyring og dei obligatoriske appkontrollane, men ei endring berre her treng ikkje nettlesar- eller databasegjenopprettingsøving. Ukjende native-stiar, endra delt kjerne, blanda appendringar, manglande baseline og eksplisitt full regresjon held full gate.

## Kva som står att

N00 er ikkje fullført: einingsmatrise, minimum OS-versjonar, signering og isolerte API-/database-/kapasitetsmålingar står att. Denne prøva gir ikkje eit nytt brukartal eller ytingsløfte.

N02 er heller ikkje fullført: Android/ART- og iOS-pakking av bindingane, faktisk WinUI og UI-dispatch, OS-nøkkellager, migrering frå eldre native schema, full disk/commit-time I/O-feil, replayretensjon og produksjonsdugeleg fleirprosessmodell står att. [Kotlin/JVM og Swift/macOS](native-client-language-probes.md) er no verifiserte mot den same Rust-kjerna. Tvungen prosessdød er ikkje straumbrot. Livssyklusprøvene er ikkje ein produksjons-SDK. Shared Rust-lagring er framleis ei retning med stoppunkt før dei mobile plattformprøvene er køyrde.

N01-identitet/recovery og N05-varig serversynkronisering er ikkje starta. Android SDK/Java og ein macOS-byggvert er ikkje tilgjengelege i det undersøkte lokale miljøet. Dei nye språkprøvene bruker hosted Linux/macOS i CI. Vi har ikkje installert SDK-ar lokalt, kontakta Authentik eller endra kontoar for å kome rundt desse avgrensingane.
