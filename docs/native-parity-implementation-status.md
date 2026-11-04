# Framdrift for native funksjonsparitet

Oppdatert 4. november 2026. Fundamentpulja ligg i PR #330 på
`codex/native-parity-foundation` (`96425c1`), frå Canary-baseline
`142f71888cdde11dec42a698432530e208f577bb`. Andre pulje er integrert i
`S:/Source/sproyt-native-daily`, på `codex/native-history-navigation` oppå
fundamentet. Tre arbeidarar leverte retry-, historikk- og navigasjonsdelane
medan integratoren kopla dei saman og køyrde Windows-prøvene.

Tredje pulje ligg på `codex/native-thread-actions` i
`S:/Source/sproyt-native-conversations`. Ho byggjer på `9c0c456` i den
framleis opne [PR #331](https://github.com/hbjoroy/sproyt/pull/331).
Pulja omfattar trådar, reaksjonar og handlingar på eigne meldingar.
Fjerde pulje byggjer vidare på denne greina med riktekst og opplasting,
og femte pulje legg til biletvisar og filslipp. Sjette pulje tek eit
avgrensa snitt av personar, innboks og profil; desse radene har framleis
delvis dekning i [funksjonsmatrisa](native-canary-parity-matrix.md).
Sjå [Astra-planen](native-canary-parity-plan.md) for vidare rekkjefølgje og
[baseline-statusen](native-client-status-2026-11-03.md) for utgangspunktet.

## Synk med siste leverte kode

Den 4. november kl. 13:58 UTC rapporterte både Canary og prod
`e3ac63e9c751289c7f6d1b5f0c17ba13b5cfee5d`, lik `origin/main`.
Alle fire PR-greinene er synka i rekkjefølgje frå fundamentet til riktekst og
opplasting, med vanlege merge-commits og utan konfliktar eller omskriven historikk.
Dette tek inn #329, #333 og #335. Native-kode, serverdomene og wire-kontraktar
er uendra av synken; funksjonsomfanget er det same.

Den samla frontendkontrollen passerte 211 testar, og 23 lokale Chromium-scenario
passerte for mottatte dokument, eksisterande biletflyt og den oppdaterte zoomen.
CI-scope og browser-sharding passerte høvesvis 24 og 3 testar.
Tidlegare testkvitteringar er bevarte som historiske bevis; nye kjeldeidentitetar
og vidare CI-status er registrerte i [synkkvitteringa](evidence/native-main-sync-2026-11-04.json)
og dei fire PR-ane. Ingen deploy eller produksjonsendring vart utført i denne synken.

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

## Tredje pulje med trådar og meldingshandlingar

Astra-planen delte arbeidet mellom tre funksjonseigarar og éin integrator.
Transportarbeidaren eig `native/session-client/NativeChatClient.cs` og
WebSocket-prøvene i `native/probes/conversation-contracts`. Trådarbeidaren
eig oversikta, detaljvisinga, ankeret og lesepolicyen i `Features/Threads`.
Meldingsarbeidaren eig reaksjonsveljaren, redigering, slettestadfesting og
feiltilstandane i `Features/MessageActions`. Integratoren koplar desse til
`LiveApplication`, `LiveChatComponent`, den målte rulleflata og det varige lageret.

| Brukaroppgåve | Akseptkrav i tredje pulje | Vidare arbeid |
| --- | --- | --- |
| PAR-005/006 · Finne, lese og svare i ein tråd | Oversikta kan opne ei rot utanfor den lasta kanaltidslinja. Rota og svara har eiga vising og eige anker. Kanalutkast og utkast i to ulike trådar overlever navigasjon og omstart. Berre målte, synlege svar i det aktive besøket kan gi eit lesekall. Serverkvitteringa avgjer vist lesetilstand. | Oversikta er avgrensa til 2000 trådar. Ein tråd har plass til 2000 svar, og transporten tek imot svarrammer på inntil 20 MiB. Større trådar får kapasitetsstatus. Eksterne lenkjer ved kaldstart, full offlinesynkronisering og lesing på tvers av to ekte klientar står att. |
| PAR-008 · Redigere og slette eigne røter og svar | Redigering bevarer vedleggsmarkørane og held på teksten ved avslag. Sletting krev stadfesting. Ei sletta melding viser slettemarkør også etter eit forseinka lastesvar. | Moderatorsletting av andre sine meldingar og endeleg utprøving mot Canary står att. |
| PAR-009 · Reagere i kanalen og tråden | Brukaren kan velje fleire emoji og sjå tal, eigne val og kjende deltakarnamn. Etter tapt kvittering kan brukaren kontrollere servertilstanden før eit eventuelt nytt forsøk. | Lagra personlege emoji og full namnedekning for deltakarar utanfor den lasta samtalen står att. |

Konto, full øktidentitet og konkret kanal-/trådbesøk avgrensar forseinka svar.
Eit trådsvar bruker den eksisterande leveringskoordinatoren. Ved uvisst
sendingsutfall bevarer køen request-ID, tekst og foreldre-ID.
Eit endeleg avslag stoppar leveringa. Etter eit uvisst redigerings-, slette-
eller reaksjonskall les klienten tilbake meldinga eller reaksjonane på
brukaren si oppmoding.

Alle sju lokale kontraktgrupper passerte på den integrerte kjelda. Dei
dekkjer klienttransport, historikk, samtaletransport, trådvising,
meldingshandlingar, levering og navigasjon. Rust-lageret passerte 30 testar.
Ein separat Windows-prøve mot den bygde Rust-DLL-en opna arbeidsområdet på
nytt og fann rett forelder på eit uvisst trådsvar. Den integrerte Windows ARM64-
køyringa passerte lager, omstart, session-UI og dei faktiske WinUI-kontrollane
for trådar, utkast, synleg-lesing, rediger/slett og reaksjon/readback.
CI på GitHub blir følgt i PR-en; fysisk input og live Canary-aksept står att.
Kjeldehashar og resultat
blir samla i [kvitteringa for tredje pulje](evidence/native-thread-actions-2026-11-04.json).

## Lager og tilbakeføring

Første pulje migrerte skjema 1 til skjema 2 og innførte revisjonsbundne
utkast. Tredje pulje migrerte vidare til skjema 3; fjerde pulje migrerer
til skjema 4 i éin transaksjon og legg til vedleggsmetadata og kvitteringar.
Eit utkast er no nøkla på kanal og foreldre-ID. Eksisterande kanalutkast
beheld revisjonen sin, og lagra payload for gamle rotsendingar er
byteidentisk ved replay. Grensene på åtte varige utkast, åtte oppføringar
i utboksen og 1024 UTF-8-byte per melding gjeld også når fleire trådar er opne.

Windows-klienten krev eksplisitt `thread_targets`, `draft_revisions` og
`upload_drafts` ved opning. Ein eldre DLL avviser skjema 4. Utan `thread_targets` avviser den
nye DLL-en opning dersom eit trådutkast eller ei trådsending ligg i lageret.
Utan `upload_drafts` blir ein profil med opplastingskvitteringar eller
vedleggsmetadata avvist. Gamle kø-payload-byte blir ikkje skrivne om.
Tilbakeføring krev eit testa app/DLL-par og ein kompatibel profilbackup.
Migreringsprøvene dekkjer også rollback ved feil under tabellkopieringa.

## Vidare arbeid etter sjette pulje

1. Prøve fysisk input og lesing/meldingshandlingar med to brukarar mot den
   aktuelle Canary-kandidaten; halde hosted CI skilt frå denne live-aksepten.
2. Leggje til utviding av DM til gruppesamtale, oppgåveoppretting og
   lagra statusar/emoji.
   Dei grunnleggjande person-, profil- og innboksflatene under er ikkje
   ei erklæring om full dekning av desse brukaroppgåvene.
3. Kople favorittar, eksterne meldings-/trådlenkjer og kaldstart til
   navigasjonen; varslingspreferansar og OS-varsling har eigne attståande flowar.
4. Utvide trygg rik tekst og delte visings- og handlingskontraktar for media,
   invitasjonar, agentkort og arbeidskort.

Integrator eig framleis sentrale appfiler. Arbeidarane får separate
funksjonsmapper og kontraktar; alle fire skal ikkje redigere
`LiveChatComponent` eller `LiveApplication` samtidig.


## Fjerde pulje med riktekst og opplasting

Den 4. november bygde fire arbeidsspor vidare på `7866b8d` frå [PR #332](https://github.com/hbjoroy/sproyt/pull/332).
Ein arbeidar eigde riktekstrenderaren, ein eigde HTTP-opptaket på serveren og ein eigde klienttransporten og opplastingskøen. Integratoren kopla dei til Windows-skallet, kontolageret, kanal- og trådkomponisten. PAR-007, PAR-012 og PAR-013 har **delvis dekning**.

| Pakke | Kva brukaren kan gjere | Grenser for denne pulja |
| --- | --- | --- |
| P12 riktekst | Lese Markdown/GFM med overskrifter, lister, sitat, tabellar, lenkjer og kode, og byte til den opphavlege meldingsteksten. Markørar i kode held seg bokstavlege. HTML blir vist utan køyring. Eksterne bilete får ein plasshaldar, og opning av ei godkjend lenkje krev eit brukarval. | Mermaid blir vist som kjeldekode. Grafisk Mermaid-vising står att. |
| P13 opplasting | Velje fleire filer eller lime inn filer og bilete i ein kanal eller tråd. Utklippstavlebilete blir koda som PNG etter kontroll av ei grense på 16 × 1024 × 1024 pikslar. Køen viser framdrift, avbrot og nytt forsøk. | Inntil åtte jobbar per konto og 512 MiB totalt til private, uforanderlege filkopiar. Dra-og-slepp, OS-delingsmottak og Office-dokument står att. |
| P13 dokument | Laste ned PDF, ZIP, UTF-8-tekst og CSV via ein eksplisitt lagringsdialog. Serveren leverer desse som vedlegg med trygg filending, `Content-Disposition: attachment` og `nosniff`. | Fysisk aksept for filveljar, utklippstavle, touch og skjermlesar, og ei sending mellom to ekte kontoar, står att. |

Dei dedikerte native-endepunkta tek imot tilgangstoken og kontohovud. Serveren kontrollerer eigarskap og kanalmedlemskap, medan klienten bind jobben til konto, innloggingsfamilie, kanal og eventuell trådrot. Klienten brukar 1 MiB per blokk og les høgst 16 KiB per kontrollsvar. Filkopien blir hashkontrollert før overføring. Den krypterte Windows-journalen held på jobben fram til utkastet har teke imot vedlegget. Sjå [endepunktregisteret](native-http-endpoint-register.md).

React- og legacy-mottakarar på web viser dei nye dokumentmarkørane som filnamn og ei eksplisitt nedlastingslenkje. Dokument blir ikkje lasta som bilete eller automatisk førehandsviste. Serveren avgjer filnamn og filending. Typekontroll, bygg, 211 frontendkontrollar og tre isolerte Chromium-scenario passerte; full live-aksept mellom native og web står framleis att.

Skjema 4 skriv kvittering, utkastrevisjon, redigerbar tekst og ei ordna liste med vedleggsmarkørar i same transaksjon. Brukaren kan til dømes skrive ein uavslutta kodeblokk, lukke appen og framleis finne teksten og vedlegga kvar for seg etter omstart. Ved tapt lokalt svar kan klienten lese kvitteringa ved lukking eller fjerning av ein jobb frå ei tidlegare innloggingsfamilie. Oppslaget startar inga ny opplasting. Eit gjenteke svar etter at meldinga er send, kan heller ikkje setje markøren inn i neste utkast. Overføringa til utkastet sender inga chatmelding. Brukaren må velje **Send**.

Den samla meldingsteksten har framleis ei grense på 1024 UTF-8-byte, og utboksen har plass til åtte meldingar. Dersom eit ferdig vedlegg ikkje får plass i utkastet, står jobben som `Ready`. Brukaren kan frigjere plass og prøve overføringa til utkastet på nytt.

Fire backend-regresjonar for HTTP, 34 Rust-lagertestar og alle ti integrerte
vindaugslause klientgrupper er grøne lokalt. Den samla Windows ARM64-prøva
passerte publisering, faktisk Rust-DLL/SQLite/DPAPI, omstart og session-UI,
og alle WinUI-scenarioa for riktekst, råtekst, redigeringsfokus, dokumentstraum
og vedlegg i kanal og tråd. CI på GitHub blir følgd i PR-en.
Resultat og kjeldeidentitet for pulja er førte i
[kvitteringa for riktekst og opplasting](evidence/native-richtext-uploads-2026-11-04.json).

## Femte pulje: biletvisar og filslipp

Denne pulja byggjer på klienttippen `dbe556cf4701a44a136ea86bd7e4436da718f81a`
og inkluderer den avgrensa backend-releasen frå main
`bb103337ff505bcd19f99ab2c319db4d94e9c694`. Canary køyrer denne backenden;
produksjon er ikkje endra av pulja. Planlegging og gjennomgang vart gjorde
med Astra. Fire samtidige arbeidsspor delte geometri, WinUI-visar, filinntak
og integrasjon/aksept. Alle nye arbeidstre og bygg ligg på C-disken.

| Rad | Ny brukarflow | Attståande omfang |
| --- | --- | --- |
| PAR-012 | Slepp kopierbare filer på kanal- eller trådkomponisten. Målet blir fanga før Windows opnar filene; eit halde montert felt bevarer målet ved navigasjon. Mapper, lenkjer som vedlegg, ulesbare filer og meir enn åtte filer blir avviste. Tekst blir ikkje endra og inga melding blir send. | Eit avmontert komponistfelt avbryt uavklart filslipp. Produksjonsnavigasjon kan avmontere tråden medan neste tråd blir lasta. Fysisk Explorer-drag/drop og faktisk WinRT-deferral står att. |
| PAR-013 | Opne originalbilete med breiddetilpassing, heile-biletet-tilpassing, 100 %, prosentvising, zoom inn/ut og panorering. Normalstorleiken kjem frå orienterte originalmål og held seg ved endra visingsstorleik. Originalen kan lagrast eksplisitt; bilete, video og dokument deler trygg lagring med filtype frå serveren. | Fysisk mus, touch/kniping, tastatur/IME, skjermlesar og faktisk lagringsdialog står att. Video blir framleis lasta heilt ned før avspeling. |

Biletvisaren dekodar høgst 16 × 1024 × 1024 fysiske pikslar og høgst 8192
pikslar per kant. Originalmåla styrer geometrien sjølv ved redusert dekoding;
brukaren får då ei varig melding om avgrensa detaljnivå og kan lagre originalen.
Dette er ikkje ei påståing om uavgrensa full detaljgrad for store bilete.

Mellombelse originalar blir pensjonerte per konto og innloggingsfamilie.
Tre reproduserte livsløpsfeil vart retta: forelda opptak kunne blokkere ein ny
familie, opprydding kunne slette den nye familien si fil, og siste kontosjekk
før lagring var ikkje atomisk med filbytet. Sluttsteget held no kontofencen
rundt det avgrensa filbytet, medan dialog og filkopiering skjer utanfor låsen.
Vanleg tokenfornying bevarer gyldige filer og lagring.

Prøvene skil reine geometri-/livsløps-/filinntakskontraktar, faktiske WinUI-
kontrollar med syntetisk teneste og filval, og manuell live-aksept. Kvitteringa
for pulja blir ført i `docs/evidence/native-media-interaction-2026-11-04.json`.
Ingen backendendring, ny utrulling eller melding til ekte brukarar inngår i
pulja. PAR-012 og PAR-013 har framleis delvis dekning.

Den samla Windows ARM64-køyringa passerte med kjelde `9cf30f6`: faktisk
Rust-DLL/SQLite/DPAPI, varige utkast, omstart/sesjonar og alle WinUI-prøvene.
Dei nye kontraktgruppene har åtte geometriprøver, åtte livsløpsprøver og atten
filslipp-prøver; i tillegg passerer den utvida prøva av faktisk applikasjon
og kø ved stadfesta, delvis og uvisst opptak. Eit filslipp
blir berre kvittert til Windows når minst éi fil faktisk vart stadfesta i køen.
Delvis opptak blir oppgjeve som N av M, utan automatisk rollback eller retry.
Kjeldebyte og program-DLL-hash er med i [kvitteringa](evidence/native-media-interaction-2026-11-04.json).
README var einaste ucommitta native-fil under sluttkøyringa; ingen kompilert
kode hadde lokale endringar. Full CI blir følgd i PR-en.

## Sjette pulje: personar, innboks og profil

Denne pulja byggjer vidare på biletvisar- og filslipp-pulja i
[PR #337](https://github.com/hbjoroy/sproyt/pull/337). Astra planlegg og
går gjennom kontraktane. Fire arbeidsspor deler typa personlege kommandoar,
innboks, person-/profilflater og integrasjon med faktiske WinUI-kontrollar.
Pulja bruker eksisterande chatkommandoar; nye REST-endepunkt, backendutrulling
og handlingar mot ekte brukardata inngår ikkje i dette arbeidet.

| Rad | Implementert brukarflow | Avgrensing |
| --- | --- | --- |
| PAR-018 | Søkje personar på visningsnamn og brukarnamn, med eigen konto filtrert bort, og opne eller opprette ei ein-til-ein-samtale. Kanallista blir oppdatert før navigasjon. | Utviding til privat gruppesamtale står att. Personsøk bruker den henta personlista, ikkje eit nytt serversøk. |
| PAR-023 uleste/omtalar | Sjå uleste samtalar frå stadfesta kanaldata, opne kjelda til ei omtale og velje Marker lesen eksplisitt. Omtalar med trådforelder får presist trådmål. | Omtalelista er dei opptil 200 nyaste tilgjengelege omtalane; protokollen har inga paginering for eldre omtalar. Å opne innboksen merker ikkje samtalane lesne. |
| PAR-023 oppgåver | Sjå eigne tildelte oppgåver og velje Ferdig eller Opne att, med eigen status for kvar oppgåve og korrelert kvittering. | Oppgåveoppretting er ikkje del av snittet. Oppgåvelista har ei klientgrense og blir ikkje presentert som komplett dersom ho ikkje kan lastast innanfor henne. |
| PAR-002 kjelde og retur | Opne eit kjent kanal-/meldings-/trådmål frå innboksen og gå tilbake gjennom verten si navigasjon, med samtaleutkast og leseposisjon eigde av dei eksisterande samtalekomponentane. | Ei oppgåve utan uavhengig kjent kjeldemelding får berre Opne samtalen. Ein utilgjengeleg kanal sperrar kjeldehandlinga utan å fjerne den tildelte oppgåva. Eksterne lenkjer og kaldstart står att. |
| PAR-024 | Endre eige visningsnamn, statustekst og statusemoji, og tømme status med eit eksplisitt brukarval. Utkast i felta og stadfesta profil er skilde. | Lagra statusval, lagra emoji og dei andre kontofunksjonane frå web står att. Varslingspreferansar og OS-varsling er ikkje leverte av denne pulja. |

Dei personlege flatene deler den eksisterande transport- og kontoeigaren.
Førespurnads-ID, venta hendingstype, ressurs-ID, full øktidentitet og konkret
flatebesøk må stemme før eit svar kan endre UI. Ein sein listeoperasjon kan
ikkje rulle tilbake ei nyare stadfesta lese- eller oppgåvekvittering.
Avslag bevarer den stadfesta tilstanden. Uvisst utfall får ei eiga handling
for å lese servertilstanden tilbake; kontrollen gjentek ikkje skrivehandlinga
automatisk. Det gjeld òg DM-opning og profil/status.

Tilbakekalla kanaltilgang fjernar både vanlege og haldne uavklarte omtalar,
inkludert avsendar, kanalnamn og meldingstekst. Seine listesvar eller
lesekvitteringar kan ikkje hente dei tilbake. Tildelte oppgåver har ei anna
serverkontrakt: dei kan framleis visast og fullførast, medan kjelda blir
utilgjengeleg. Oppgåvekontrakten har ikkje foreldre-ID eller sekvens for
kjeldemeldinga; klienten finn difor ikkje på eit presist meldingsmål.

Dette er delvis dekning av PAR-018, PAR-023 og PAR-024, med eit avgrensa
tillegg til PAR-002. Utviding av DM til gruppesamtale, oppgåveoppretting,
lagra statusar/emoji, favorittar, varslingspreferansar og OS-varsling er
eksplisitt vidare arbeid. Dei eksisterande grensene for samtaleutkast og
varig sendekø er ikkje auka av denne pulja.
Kjeldeidentitet, resultat frå kontraktprøver, faktisk WinUI-integrasjon og
hosted CI skal førast i sluttkvitteringa for denne pulja. Denne oppdateringa
registrerer kodeomfanget; ho godkjenner ikkje live Canary-aksept, fysisk
input, skjermlesar eller lesesynkronisering mellom to ekte klientar.

