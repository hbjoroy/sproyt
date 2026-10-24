# N02: første køyrbare native kjerneprøve

Dato: 24. oktober 2026. Omfang: delvis N00/N02 frå [implementasjonsplanen](native-client-implementation-plan.md). Ingen innlogging, appgrensesnitt, serverendring eller produksjonsutrulling inngår.

## Stadfesta lokalt

Windows ARM64 med Rust 1.96.0 og .NET SDK 10.0.301 lasta det faktiske Rust-biblioteket frå ein C#-konsollklient gjennom P/Invoke. SQLite blir eigd av Rust. Ei felles fiktiv scenariofil blir lesen av både Rust- og C#-prøva.

| Prøve | Observert resultat |
| --- | --- |
| Bygg og ABI | Rust `cdylib` og .NET-konsoll bygd; ABI 1 blir kontrollert ved oppstart. Ingen WinUI- eller Android-app er bygd. |
| Varig lokal opptak | Utkast blir fjerna i same SQLite-transaksjon som request-ID og wire-payload blir journalførte. Ingen nettverksrequest blir send. |
| Ein eigar | Eit anna C#-prosessforsøk får `store_busy` medan seed-prosessen held databasen open. |
| Prosessavslutning | Runneren tvangsavsluttar seed etter stadfesta commit utan å lukke native-handle. Ein ny prosess opnar og finn sendinga som uviss. Dette er ikkje straumbrot eller krasj midt i transaksjon. |
| Replayidentitet | Rust samanliknar eksakt payload før og etter close/reopen. C# samanliknar den gjenfunne payloaden med lokal replay og avviser endra innhald med same ID. |
| Kvittering | Simulert ACK blir lagra og toler dobbel ACK. Endeleg avvist sending kan ikkje gå tilbake til dispatch. Dette prøver ikkje ein ekte server. |
| Innkomande endring | Ei syntetisk redigering blir installert. Eldre entitetsversjon kan ikkje gjenopplive ei sletta melding i den lokale modellen. |
| Konto og utlogging | Feil konto kan ikkje opne databasen. Invalidering ryddar logiske data og aukar generasjon; eit seint gammalt kall blir avvist også etter reopen. |
| Feilgrenser | Ni Rust-testar dekkjer mellom anna ugyldig UTF-8, for liten output, for stor input, stale handle, close-race, ukjend schema, itemgrenser og verkeleg read-only skrivefeil. Full disk og sikker sletting er ikkje prøvde. |

`cargo fmt --check`, Clippy med `-D warnings`, ni Rust-testar og den samla managed prosessprøva er grøne lokalt. Bygg og fixtures ligg på C:, ikkje i det plassavgrensa S:-treet. Sjå [runner og ABI-avgrensingar](../native/probes/README.md) for reprodusering. JSON-kvitteringa blir skriven i den private artefaktmappa og inneheld plattform, utfall og lokale filstiar, ikkje legitimasjon.

## Review og CI

Astra vurderte omfanget før implementering og gjekk gjennom ABI, eigarskap, generasjonsfencing, journalopptak og testgrenser. Ingen blokkerande feil vart funnen i den vurderte prøva. Presiseringane om read-only feil, prosessavslutning etter commit, manglande schema-migrering og skiljet mellom lokale og hosted resultat er tekne inn.

Ein eigen CI-jobb byggjer og køyrer same prøve på hosted Windows og Linux. Eit grønt resultat der er separat bevis for dei runnerane; lokalkøyringa er Windows ARM64. Hosted resultat skal ikkje førehandsmerkast grøne.

## Kva som står att

N00 er ikkje fullført: einingsmatrise, minimum OS-versjonar, signering og isolerte API-/database-/kapasitetsmålingar står att. Denne prøva gir ikkje eit nytt brukartal eller ytingsløfte.

N02 er heller ikkje fullført: Kotlin-/Swift-bindingar, faktisk WinUI, OS-nøkkellager, UI-trådar/avbrot, migrering frå eldre native schema, full disk/commit-feil, replayretensjon og produksjonsdugeleg fleirprosessmodell står att. Shared Rust-lagring er derfor framleis ei retning med stoppunkt, ikkje ei endeleg låst produktarkitektur.

N01-identitet/recovery og N05-varig serversynkronisering er ikkje starta. Android SDK/Java og ein macOS-byggvert er ikkje tilgjengelege i det undersøkte lokale miljøet. Vi har ikkje installert dei, kontakta Authentik eller endra kontoar for å kome rundt desse avgrensingane.
