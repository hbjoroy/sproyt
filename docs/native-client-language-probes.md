# N02: Kotlin- og Swift-bindingar

Desse prøvene utvidar [Rust/C#-grunnlaget](native-client-probe-results.md) med Kotlin/JVM på Linux og Swift på macOS. Omfanget er språkgrense og lokal livssyklus. Android/ART, iOS-pakking, fysisk eining, innlogging og produksjonssynkronisering er eigne akseptportar i [implementasjonsplanen](native-client-implementation-plan.md).

## Verifisert 24. oktober 2026

[CI-køyring 40125478249](https://github.com/hbjoroy/sproyt/actions/runs/40125478249) er grøn for kode `9c971432f468336edfe9acb5a0d3e8f8e0dca71a` i [PR 284](https://github.com/hbjoroy/sproyt/pull/284). Dei fire nedlasta kvitteringane har rein kjelde; den testa merge-revisjonen har same kjeldetre som den vurderte kode-revisjonen.

| Klient | Verifisert vert og verkty | Resultat |
| --- | --- | --- |
| Kotlin/JNI | Linux x64, Kotlin 2.3.21, Temurin 21.0.12.1, Maven 3.10.0, `-Xcheck:jni` | Alle seks prosessfasar og tolv utfall. |
| Swift/C | macOS ARM64, Apple Swift 6.1.2 | Alle seks prosessfasar og tolv utfall, med kontroll av faktisk lasta biblioteksti. |
| C#/C | Hosted Windows x64 og Linux x64 | Dei eksisterande elleve utfalla og tre transaksjonskrasjpunkt på begge vertar. |

Alle fire har same SHA-256 for faktisk wire-payload frå før prosessdød: `f705a69c26a18d1a84f4644655d5c1299337c3c0b5414c9e412be8287bac570e`. Dermed er same fiktive sending bevart over både språkgrense, plattform og omstart. Dette er ikkje eit server-dedupliseringsbevis.

Astra reviderte arkitekturen og kjelda. Ein kansellerbar Kotlin-utloggingsbarriere vart retta og fekk ein deterministisk regresjonstest. Første Swift-køyring bygde, men den strenge bibliotekkontrollen oppdaga Cargo sin interne `debug/deps`-sti. Runneren les no Mach-O-installnamnet, krev at det ligg i denne byggmappa og har identisk hash med byggeartefakten, og bruker den faktiske stien vidare. Astra godkjende rettinga; neste hosted køyring lasta rett bibliotek og fullførte alle fasane. Allokeringsfeil og svikt i responslevering er kjeldereviderte, men ikkje feilinjisert gjennom JVM/Swift.

## Eitt lagringsansvar

Rust eig framleis SQLite, utkast, outbox, handle-register og generasjonskontroll. Kotlin kallar den eksisterande C-ABI-en gjennom ein liten valfri JNI-adapter. Swift importerer den autoritative C-headeren med ein Clang-modul. Ingen av klientane får eigen SQL-tilgang eller eigne leveringsreglar.

Kotlin-adapteren kopierer Java-input før native-kallet. Han held ikkje kritiske eller pinna Java-arrayar under blokkerande databasearbeid. All responsplass blir reservert før kommandoen køyrer. Feil i konvertering/levering etter at kallet er starta kan vere uvist utfall; ingen automatisk gjentaking får lage ny request-ID. Rust-panikk blir fanga både ved JNI-grensa og i C-kallet, innanfor dei dokumenterte grensene for abort/allokeringsfeil.

Swift sine peikarar lever berre under det synkrone C-kallet. Swift verifiserer med `dladdr` at funksjonen faktisk kjem frå bibliotekstien runneren har oppgitt. JVM lastar den eksplisitte absolutte bibliotekstien med `System.load`. Begge held biblioteket lasta medan handles eller kall lever.

## Same scenario, eigne språkprøver

Begge les `native/probes/scenarios/persistence-v1.json` og skriv berre fiktive data i ei ny, test-eigd mappe:

1. Avvis diagnostiske kommandoar, ugyldig UTF-8, tom/for stor input og for liten output utan mutasjon.
2. Lagre utkast, journalfør sending og marker dispatch. Forelderen held kontrollsum av faktisk lagra wire-payload før han tvangsavsluttar barnet med handle open.
3. Avvis samtidig databaseeigar. Opne i ny prosess, kontroller uviss levering og kontrollsummen før retry. Same ID/bytar gir éi sending; endra payload med same ID blir avvist.
4. Kontroller dobbel ACK, innkomande redigering, stale handle, feil konto og varig utloggingsgjerde etter reopen.
5. Køyr språkspesifikk arbeidar med to ventande oppgåver. Kontroller metting, avbrot før arbeidaren tek oppgåva, avbrot etter dette punktet før native-skriving og etter commit, utlogging med ventande arbeid, og lukking medan arbeid pågår.

Arbeidaren tek oppgåva under same lås som utlogging stengjer opptaket. Etter dette punktet kan avbrot ikkje skjule det faktiske resultatet. Utlogging avviser ventande gammalt arbeid, lèt påbegynt arbeid bli ferdig og invaliderer deretter native-generasjonen. Vanleg lukking tømmer opptatt arbeid og lukkar handle éin gong på eigartråden. Det finst ingen callback eller asynkron kansellering gjennom språkgrensa.

## Bygg og evidens

[Workflowen](../.github/workflows/native-probe.yml) køyrer dei eksisterande C#-prøvene på Windows/Linux, Kotlin-prøva på Linux med JDK 21 og Swift-prøva på `macos-15`. Kotlin-kompilator og JSON-pakke er festa i POM; Swift-verktøyversjonen blir registrert frå byggverten. Rust er festa i repoet sin toolchain. Ingen Android SDK eller Java-/Swift-installasjon blir utført på den lokale Windows-maskina.

[Runneren](../native/probes/run-language-probe.ps1) skriv språk-kvittering berre etter alle fasar er grøne. Kvitteringa har eige schema 1, scope `desktop_language_binding_feasibility`, kjelderevisjon, dirty-status, bibliotekhash, payload-hash frå før prosessdød, verktøyversjonar og tolv utfall. CI-artefaktane gir resultat for den eksakte køyringa; ein skrivebordsprøve blir ikkje tolka som ein mobiltest. Ein feil blir ikkje skjult ved å merke porten som fullført.

Den opphavlege transaksjonsmatrisa og verkeleg commit-feil er testa i Rust/C# og blir framleis køyrde. Desse blir ikkje dupliserte gjennom alle språk. Det handskrivne JNI-/C-laget avgjer heller ikkje kva genererte bindingar den endelege SDK-en skal bruke.

Teknisk grunnlag: [Kotlin sitt Maven-oppsett](https://kotlinlang.org/docs/maven-configure-project.html), [Swift sine modul-kart](https://docs.swift.org/main/documentation/packagemanagerdocs/modulemaps/) og [JNI sine bufferreglar](https://docs.oracle.com/en/java/javase/25/docs/specs/jni/functions.html).
