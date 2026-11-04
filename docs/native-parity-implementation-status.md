# Framdrift for native funksjonsparitet

Oppdatert 4. november 2026. Fundamentpulja ligg i PR #330 på
`codex/native-parity-foundation` (`96425c1`), frå Canary-baseline
`142f71888cdde11dec42a698432530e208f577bb`. Andre pulje er integrert i
`S:/Source/sproyt-native-daily`, på `codex/native-history-navigation` oppå
fundamentet. Tre arbeidarar leverte retry-, historikk- og navigasjonsdelane
medan integratoren kopla dei saman og køyrde Windows-prøvene.

Trådar, reaksjonar, opplasting,
innboks og dei andre uferdige brukarflatene står framleis att i
[funksjonsmatrisa](native-canary-parity-matrix.md).
Sjå [Astra-planen](native-canary-parity-plan.md) for vidare rekkjefølgje og
[baseline-statusen](native-client-status-2026-11-03.md) for utgangspunktet.

## Første implementerte snitt

| Pakke | Implementert | Vidare arbeid |
| --- | --- | --- |
| P00 | Eiga `LiveContracts`-grense for UI og applikasjon, testbar leveringskoordinator, integrert sendestatus og CI-steg for dei nye kontraktane. | Uttrekk av navigasjon, samtale og innhald i eigne funksjonsmodular når desse blir implementerte. |
| P01a | Korrelerte serverfeilkodar, endeleg avslag skilt frå uvisst nett-/serverutfall, nytt kontogjerde før resultatbruk og additive kanal-/meldingsfelt for rolle, lesing, ranking, tråd og redigering/sletting. | Dei typa kommandoane og projeksjonane som kvar ny brukarflate treng. Felt i modellen er ikkje levert UI. |
| P02a | Selektiv GET/PUT for eigne varslingspreferansar med native grant, obligatorisk kontoheader og same domeneteneste som web. Førstegongs-PUT fungerer før chatopning. | Preferanse-UI, kanalval, upload og andre HTTP-familiar. Sjå [endepunktregisteret](native-http-endpoint-register.md). |
| P03a | Lokal schema 2, monoton utkastrevisjon, atomisk revisjonsbunden enqueue og replay utan ny ryddeeffekt. Eksplisitt opt-in for den oppdaterte klienten. | Produktgrenser utover åtte element/1024 UTF-8-byte, rikt utkast, vedleggsjournal og eventuelt fullt historikklager. |

Definitive avslag blir lagra som `rejected` og blir ikkje sende automatisk
på nytt. UI viser «Ikkje sendt — avvist». Brukaren kan fjerne ei slik melding
etter ei stadfesting; dette frigjer plassen utan å sende eller fjerne andre
meldingar. Lagringstilstanden blir avstemd med UI dersom eit lokalt svar går
tapt etter commit. Gjenteken avstemming av uendra tilstand startar ikkje ei
ny pumpeløkke.

Uvisst nettutfall bevarer request-ID og payload. Eit nytt utkast med identisk
tekst har ei anna varig revisjon, og eit gammalt retry-forsøk kan ikkje rydde
det. Oppdatert innlogging med berre avviste gamle køelement krev ikkje
samtykke for ei ny melding. Gamle ikkje-avviste sendingar held fram med å
krevje eksplisitt samtykke etter ny innlogging.

## Lokal verifikasjon

| Kontroll | Bevis og avgrensing |
| --- | --- |
| Rust-lager med fault injection | 25 grøne testar i integrert kjelde, inklusive seks faktiske child-process-drap rundt insert/commit, migrering/rollback, eksakt replay og eksplisitt rejected-discard. |
| Transportkontrakt | Ekte `NativeChatClient` mot lokal Kestrel WebSocket: feilkodar, korrelasjon, tapte ACK, same wire/ID, delte waiters og seint resultat etter kontobyte. Økteigaren i denne prøva er syntetisk. |
| Leveringskoordinator | 10 grøne kontraktar, inklusive feil før/etter lokal commit, varig avslag, retry utan ny sending, kansellering og kontoendring under lagring. |
| Windows ARM64 | Publish og faktiske Rust-DLL-/Workspace-prøver, UI/omstart, syntetisk session-UI og medie-/live-view-fixture er grøne. Nye prøver dekkjer avvist kø, avbroten slettestadfesting og avstemming etter tapt lokalt resultat. |
| Native HTTP | To integrerte HTTP/OIDC/SQLite-testar grøne. Dei går gjennom dedikert native login/confirm/exchange, eigarskap, feil autoritet, utløp/revoke og native av. Browser/native les tilbake same preferansar. |
| Backend-regresjon | Native-gruppa rapporterte 14 grøne og varslingsgruppa 16; gruppene overlappar, og PostgreSQL-prøver er miljøguarderte. Dette er ikkje 30 uavhengige eller PostgreSQL-godkjende scenario. |
| Kvalitetskontroll | Backend og prøvelager har grøne format-/Clippy-kontrollar. Native-workflowen har actionlint-kontroll. Testartefaktar blir lagra separat frå kjelda. |

Revisjon, kjeldehashar og lokale artefaktstiar er samla i
[fundamentkvitteringa](evidence/native-parity-foundation-2026-11-03.json).
Prøvene bruker syntetiske kontoar og testprofilar. Første pulje er ikkje
rulla ut, og det er ikkje posta meldingar til Canary eller endra brukaren
sin aktive native-profil.

Fundamentet har grøn hosted x64-/Linux-CI og PostgreSQL-regresjon
([native](https://github.com/hbjoroy/sproyt/actions/runs/41716737542),
[hovud-CI](https://github.com/hbjoroy/sproyt/actions/runs/41716737849)).
Dette kvitterer ikkje andre pulje. Fysisk IME/touch/skjermlesar og ekte
sending med to brukarar mot den aktuelle Canary-kandidaten har eigne
attståande portar. Ei WinUI-fixture blir ikkje brukt som bevis for desse.

## Andre implementerte snitt: dagleg samtalebruk

| Del | Implementert | Avgrensing |
| --- | --- | --- |
| PAR-011 retry/backoff | Monoton, avgrensa venting på 2–60 sekund per request-ID; separat lagringsretry; éin oppvekkjartimer. Manuell retry har minst eitt sekund mellom forsøk. Aktivering av vindauget er ikkje manuell retry. Mottatt ACK/avslag blir ferdiglagra utan ny nettverkssending. | Den eksisterande køgrensa på åtte meldingar gjeld framleis. Dette er ikkje full offlinesynkronisering. |
| P10a / PAR-001–002 | Eiga navigasjonseigar og native-vising med søk, krets-/DM-faner, serverbasert DM-rangering, ulestindikator og tilbake/fram. Separate kanalutkast og meldings-/pikselanker blir bevarte. Besøk, konto og øktfamilie gjerder forseinka arbeid. | Favorittkretsar, OS-lenkjer/kaldstart og trådmål står att. Historikk er avgrensa i minnet; berre fanepreferansen blir lagra per konto. |
| P11a / PAR-004–005 | Eldre rothistorikk i sider på 50 rå rader, også gjennom sider med berre trådsvar. UI bevarer faktisk synleg melding og pikselposisjon ved prepend og høgdeendringar. Lesing blir sendt for målte synlege røter og vist som stadfesta først etter korrekt serverkvittering. | Maks 2000 lasta rotmeldingar med synleg kapasitetsstatus. Ingen full lokal historikklagring, trådlesing eller fabrikkerte ulesttal. |

Transporten sperrar gamle historikksvar etter disconnect, kontobyte og nye
visingar. Bakgrunnsreparasjon kan ikkje byte tilbake til ein tidlegare kanal.
Lesing er stoppa under gjenoppretting av anker, skjult samtaleflate og inaktivt
vindauge. Gamle tilgangsavslag kan ikkje rydde ei nyare vising.

Dei integrerte headless-prøvene dekkjer backoff/timer med styrt tid,
historikk-/lesekommandoar over ekte lokal WebSocket og navigasjon med
kontobyte, tilgangstap og forseinka operasjonar. Windows ARM64-prøva køyrer
det faktiske native kontrolltreet: søk/faner, tilbake/fram, separate utkast,
virtualisert prepend, synleg-lesing, inaktivt vindauge og endra radhøgd.
Ankerprøva måler faktisk meldings-ID, Y-posisjon og rulleposisjon innan 2 DIP.
Ho injiserer ikkje fysisk input og er ikkje live Canary-aksept.

Sjå [kvitteringa for andre pulje](evidence/native-history-navigation-2026-11-04.json)
for kjeldehashar og artefaktar. P10 og P11 er framleis delvis leverte;
funksjonsmatrisa si baseline er bevart.

## Lager og tilbakeføring

Schema 1 blir migrert transaksjonelt til schema 2 med utkast, request-ID-ar
og payload intakte. Ein eldre DLL avviser schema 2; den nye klienten avviser
ein eldre DLL som ikkje forstår den eksplisitte revisjonskontrakten. Det er
ikkje innført automatisk nedgradering av lokale databasar. Oppdatering og
tilbakeføring må bruke eit testa app/DLL-par og ta omsyn til profilbackup og
lokal schemaversjon før ein ekte pilotprofil blir oppgradert.

## Neste UI-pulje

1. Samtale: trådar, reaksjonar og rediger/slett med same leverings-, besøks-
   og kontogjerde.
2. Navigasjon: favorittar, meldings-/trådlenkjer frå eksterne flater og
   innboksintegrasjon med bevart utkast og leseposisjon.
3. Innhald: trygg rik tekst og delte renderer-/handlingskontraktar for media,
   invitasjonar, agentkort og arbeidskort.

Integrator eig framleis sentrale appfiler. Arbeidarane får separate
funksjonsmapper og kontraktar; alle fire skal ikkje redigere
`LiveChatComponent` eller `LiveApplication` samtidig.
