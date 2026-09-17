# Plan: nytt Sprøyt-UI med eksisterande funksjonar

Dato: 2026-09-16. Status: i arbeid. Fase 1–3 er starta; funksjonsmatrisa er ikkje ferdig migrert.

## Mål og avgrensing

Erstatt dagens brukarflate med designsystemet i `sproyt-ui/sproyt-ui`, og før vidare alle eksisterande brukarfunksjonar, tilgangsreglar og leveringsgarantiar. Designpakken bestemmer visuelt uttrykk og gir komponentar; han er ikkje ein fullstendig produktspesifikasjon. Demo-data, forenkla tilstand og manglande skjermbilete skal ikkje redusere Sprøyt.

Den medfølgjande skillen ligg i `.agents/skills/sproyt-ui`, med `SKILL.md`, begge referansane, `agents/openai.yaml` og komponentarkivet. Komponentpakken er lagd som lokal npm-avhengigheit i `frontend/vendor/`; database og produksjon er ikkje endra.

## Grunnlag og prioritet

Kartlegginga byggjer på `assets/index.html`, `frontend/src/app.ts`, dei utskilde klientmodulane, `frontend/tests`, `src/web/{assets,browser}.rs`, `build.rs`, `ARCHITECTURE.md`, `docs/development.md`, og designpakken sine kjelder og referansar. Dette er ei statisk kodekartlegging; nettlesaraksept og full funksjonsverifikasjon er ikkje utført i planfasen.

Ved konflikt gjeld denne rekkjefølgja:

1. Oppdraget: alle Sprøyt-funksjonar skal vere med i det nye designet.
2. Faktisk domene, protokoll, tilgangsstyring og datatryggleik i Sprøyt.
3. Designsystemet sitt visuelle uttrykk og tilgjengelege samspelsmønster.
4. Komponentstandardar og demoeksempel, tilpassa der dei ikkje dekkjer produktet.

Behald nynorsk språk, forståelege namn og synlege handlingar. Tekniske ID-ar høyrer heime i detaljar. Ein funksjon som er skjult bak eit eksisterande funksjonsflagg, skal framleis vere tilgjengeleg når flagget og rettane tillèt det. Backend- og MCP-funksjonar som ikkje har UI i dag, skal bevarast, men treng ikkje nye administrasjonsskjermar som del av denne endringa.

## Tilrådd teknisk retning

Dagens klient bruker TypeScript, direkte DOM-operasjonar, npm og esbuild. Han bruker korkje React eller Vue. `@sproyt/ui` leverer React- og Vue-komponentar, ikkje ein ferdig adapter til dagens klient.

Planen legg opp til React som nytt presentasjonslag med `@sproyt/ui/react`. Dette er eit eksplisitt nytt rammeverksval, ikkje ein eksisterande eigenskap ved repoet. React-sporet kan byggjast med dagens esbuild og TypeScript; vi beheld npm, Rust-serveren, nettadressene og den innbygde leveringa. Ingen Next.js, ny webserver eller kopiering av demoen som applikasjon. Ei rein DOM-omsetjing av biblioteket ville krevje eit eige, varig komponentsett med doble vedlikehaldsplikter og er difor ikkje tilrådd.

Behald `api.ts`, `connection.ts`, `session.ts`, `navigation.ts`, `durable-outbox.ts`, `outbox.ts`, protokolltypane og Rust/WASM-policy som ansvarlege for dei eksisterande kontraktane. Flytt resterande tilstand og kommandohandtering ut av `app.ts` før den tilhøyrande DOM-koden blir erstatta. `client-store.ts` er i dag eit lite status-/mailbox-lag, ikkje ein komplett meldingsstore; planen må omfatte resten av denne utskiljinga.

Føreslått struktur:

- `frontend/src/application/`: eigarskap til klienttilstand, kommandoar og abonnement, med eksplisitte livsløp.
- `frontend/src/ui/`: appskal, visingar og Sprøyt-tilpassingar av komponentane.
- `frontend/src/ui/message-content/`: trygg Markdown, Mermaid, medie- og invitasjonsinnhald.
- `frontend/src/ui/styles.css`: produktutvidingar baserte på `--sp-*`, utan ein konkurrerande palett.
- `frontend/vendor/`: versjonert lokal komponentpakke med reproduserbart opphav.

Ein enkelt applikasjonsinstans eig socket, session, outbox og mailbox. React-mount, omrendering og skjermbyte må ikkje starte ekstra tilkoplingar, dobbel sending eller nye pollingløp. Visingskomponentar abonnerer på tilstand og kallar kommandoar. Eksisterande DOM-renderarar kan mellombels leve i isolerte område der berre éin renderer eig nodane.

## Funksjonskart og ny plassering

| Område og eksisterande funksjonar | Ny brukarflate | Krav som må førast vidare |
|---|---|---|
| Felles, vennekretsar, kanalar og direkte samtalar; samtalesøk, sist vald krets/kanal og ulesttal | `AppShell`, gruppert samtalenavigasjon og tilpassa `ConversationList` | Felles, krets og direkte er ulike område; kanal-ID og gruppenamn må ikkje blandast. Behald lagra navigasjon og eksisterande `?channel=`-inngang. |
| Ny krets med automatisk Prat, oppdage/opprette kanal, open i kretsen/privat kanal, bli med, forlate kanal/krets, slette krets | Eigne krets-/kanalvisingar; fokuserte skjema og stadfestingar i `Dialog` | Eksisterande eigar-/medlemsreglar, konsekvensinformasjon og feilsvar; Prat-oppretting må skje éin gong. |
| Direkte samtale frå personveljar eller medlemsliste; utviding til gruppesamtale gjennom omtale av ny person | `PersonList`, personveljar og stadfestingsdialog | Den opphavlege private samtalen held fram privat. Sjølv kan ikkje veljast som DM-mottakar. |
| Kanalmedlemmer, personsøk, status, leggje til/invitere medlem, Markdown-kanalomtale | Dedikert kanalopplysningsvising med personrader og redigering | Same synlegheit og rettar, lokal feil og ny prøve ved mislukka lasting. |
| Meldingshistorikk, eldre meldingar, redigering, sletting, tidsstempel, avsendarnamn og rå/formatert vising | `Message`, tidslinje og tydelege handlingar | Rettar per melding, historiske namnesnapshot, rekkjefølgje/sekvensar, trygg Markdown og Mermaid. Behald råvising og lenkjer. |
| Utkast, emoji i tekst, @omtalar med tastaturval, sendestatus og varig kø | Utvida `Composer` med verktøyrad og synleg vedlegg-/feilkontekst | Utkast per kanal/tråd, IME, same request-ID ved replay og korrekt avklaring av uviss levering. Ingen tap eller dobbel sending ved reload/samtalebyte. |
| Trådar, svar, svartal, lesemarkering, eigne utkast og medievedlegg | `ThreadPane`; side ved side når plassen tillèt det, eiga detaljvising elles | Svar skal ikkje dupliserast i rot-tidslinja. Behald kanalposisjon, trådtilstand og fokus ved retur. |
| Emoji-reaksjonar med teljing, eigne val og valfri Unicode-emoji | Reaksjonsmerke og tilpassa popup frå designsystemet | Fleire reaksjonar og serveroppdateringar må bevarast; visne/feila handlingar skal ikkje sjå lagra ut. |
| Bilete/video, fleire vedlegg, innliming, opplastingsframdrift og feil, fjerning frå utkast, førehandsvising og fullskjerm | Vedlegg i `Composer.leading`, opplasting i verktøyrada, avgrensa ukutta media og fullstor vising | Sending med berre vedlegg; gjeldande HEIC/HEIF/MOV-handtering; kanal- og trådtilknyting og uendra tilgang til media. |
| Privat biletegenerering frå skrivefelt, referansebilete, jobbstatus, detaljar/kjelder, godta/avvise/skjule og gjenfinne etter reload | Privat resultatvising og skriveverktøy med same visuelle språk | Godtaking legg berre eit upublisert vedlegg i utkast. Generering og godkjenning sender ikkje melding. Behald kanalbinding, gjenoppretting og noverande avgrensingar for referansar. |
| Krets-/kanalinvitasjonar, eksisterande brukar via DM, lenkje/kopiering/deling, registreringsinvitasjon, godta/avvise i melding | Personliste, fokusert invitasjonsskjema og eige invitasjonskort | Authentik eig registrering; uendra token-/returflyt. Kort må vise faktisk svarstatus og utløpt/feila innhald. |
| Ulest, omtalar, markere omtale lesen, lage oppgåve frå omtale, fullføre/opne oppgåve att | Faste inngangar til innboks og oppgåveliste med radbasert design | Eksisterande lesesekvensar og lenkje tilbake til kjeldemeldinga; ulike teljarar må ikkje slåast saman. |
| Profilnamn, offentleg handle, personstatus/emoji og tømming av status | Profil-/innstillingsvising med `TextField` og native felt | Handle er ikkje innloggingsidentitet; namneendring endrar ikkje historiske avsendarsnapshot. |
| Varslingsmodus, DM-/omtaleval, kanalval og nettlesar-push | Varslingsinnstillingar og konteksthandling ved samtalen | Faktiske serverval og nettlesarløyve, avslag/feil og relevante kanalavgrensingar. Demofeltet `muted` er ikkje heile modellen. |
| Mellombels agenttilgang: opprette, kanalavgrensa rettar, kopiere eingongsvist nøkkel og tilbakekalle | Avansert tilgangsvising i det nye designet | Gjeldande funksjonsflagg, utløp og rettar. Hemmelege verdiar skal ikkje flyttast til generell persistent UI-store. |
| Grafana-integrasjon med kanalbunden webhook og eingongsvist token | Integrasjonar under kanalopplysningar | Eigar-/tilgangskontroll; same kanalbinding; tøm løyndomar når visinga blir lukka. |
| Heart: aktivere, starte planlegging, status, inspeksjon og ja/nei-svar | Prosessvising med detaljar og handlingar | Same tilgjenge/feil når Heart er av eller utilgjengeleg. `EventCard` må tilpassast prosessen, ikkje skape ein ny RSVP-modell. |
| Dataeksport, innlogging/utlogging, sessionfornying, eksplisitt ny innlogging, sambandsstatus | Innstillingar og diskret global status med konkrete gjenopprettingshandlingar | Behald HttpOnly-auth, fleirfane-koordinering, WASM/fallback, utkast ved ny innlogging og skilje mellom nettfeil og avvist auth. |
| Installert PWA, offline-side, push-inngang, gjenopptaking etter bakgrunn og mobilt tastatur | Same appskal og design også ved offline-/tom-/feiltilstandar | Service-worker-oppdatering, cache-reglar, deep links, safe-area og `visualViewport`; ikkje cache autentisert HTML eller private data som nye statiske ressursar. |

Denne matrisa skal bli ei sporbar sjekkliste under implementering: kjelde/handling, ny vising, relevante roller/flagg, test og status. Ho skal utvidast dersom gjennomgangen av eventhandtering eller testar avdekkjer fleire variantar. Ingen funksjon er ferdig migrert berre fordi hovudskjermen finst.

## Designvedtak og nødvendige tilpassingar

1. **Visuelt grunnlag:** varm lys bakgrunn, mørkt koltema, citron som standardaksent, eksisterande periwinkle-alternativ, semantiske token, tydeleg typografi, strekar og lite kortbruk i chat. Lys/mørk/system og tettleik får éin eigar og same uttrykk i dialogar og popup-ar.
2. **Navigasjon:** éi gruppert hovudnavigasjon erstattar dagens overlappande skuffer, sidefelt og botnveljarar. Alle funksjonane får ein synleg inngang. Ved appbreidd på høgst 800 px viser vi liste eller detalj; tråd står ved sida av kanalen berre når sjølve innhaldsområdet er minst 780 px. Breidda er containerbreidd. Retur skal ta vare på både utkast og leseposisjon.
3. **Motstridande føringar:** designreferansen nemner både 650 og 800 px, og både 12 og 8 px mellom meldingar. Planen bruker dei seinare responsive føringane og faktisk CSS: 800 px og 8 px, med den korte streken over neste avsendar. CSS har også 28 px meldingshandlingar der teksten seier 32 px; normaliser til minst 32 px for fin peikar og 44 px for berøring. Rett dokumentasjonen når bibliotektilpassinga blir gjort.
4. **Composer er ei sperre før chat kan migrerast:** standardkomponenten tillèt berre ikkje-tom tekst og eksponerer ikkje alle textarea-koplingane Sprøyt treng. Utvid med eksplisitt sendbar tilstand for vedlegg, felt-ref, cursor/selection, paste, tastaturhandtering med prioritet for omtalar, fokus, ARIA og readOnly uavhengig av busy. Behald éi linje i kvile, vekst til 3½ linjer og valfrie verktøy; vedlegg og feil skal vere synlege også når verktøya er lukka.
5. **Sendekontrakt:** dagens Enter-sending på skrivebord og linjeskift med Shift+Enter blir vidareført; mobil/IME og omtalemeny blir handterte før send. Hjelpeteksten må svare til faktisk oppførsel. Tømming skjer etter eksisterande aksept for sending eller varig kølagring; serverkvittering, feil og uviss levering blir framleis handterte av outbox/pending-modellen. Ikkje erstatt dette med demoen sitt `setText('')`.
6. **Reaksjonar:** popup-en sitt `selected`-felt er eitt emoji-val, og katalogen har berre 24 emoji. Utvid eller bygg ein designtru adapter for fleire eigne reaksjonar, teljing, søk og innliming av Unicode. Synleg tastaturknapp skal fungere saman med høgreklikk og langt trykk utan å øydeleggje scrolling.
7. **Kort representerer ekte data:** medlemsinvitasjonar og Heart-prosessar er ulike ting. Bruk eige invitasjonskort for godta/avvise og ekte status. Ikkje legg til `maybe` eller simulerte deltakarar fordi `EventCard` støttar dette.
8. **Manglande komponentar:** lag Sprøyt-komposisjonar for select, checkbox, textarea, tabs/visingsval, bekrefting, framdrift, medievising, innboks, oppgåver og avanserte skjema med same token og native semantikk. Eksisterande `TextField` er ikkje ei erstatning for alle felttypar.
9. **Sikker rendering:** bevar den trygge Markdown-/lenkjehandteringa. Ikkje erstatte henne med ukontrollert HTML i React. Mermaid, medieinnhald og invitasjonstoken får tydeleg eigarskap og opprydding.

Bibliotekendringar skal skje i kjeldepakken under `sproyt-ui/sproyt-ui`, med testa bygg og eit nytt, eintydig versjonert arkiv. Ikkje patch `node_modules` eller endre innhaldet i eit arkiv under uendra versjon. Synkroniser skillreferansar og pakkeversjon etterpå; ha éin vedlikehalden kjelde for dei.

## Gjennomføringsrekkjefølgje

### 1. Etabler funksjonsfasit og regresjonsgrunnlag

- Gå gjennom matrisa mot alle UI-handlingar, serverevent og eksisterande testar, inkludert administrator-/flaggstyrte flater.
- Registrer dagens hovudflytar og skjermbilete i isolert utviklingsmiljø: desktop, smal container og mobil med tastatur.
- Køyr eksisterande frontend-/browser-testar før endringar og dokumenter eventuelle eksisterande feil.
- Prioriter manglande åtferdstestar for vedlegg utan tekst, gruppesamtale, trådutkast, reaksjonar, invitasjonar, oppgåver, rettar og session/outbox. Test kontraktar, ikkje kopiar av implementasjonen.

**Ferdig når:** alle kartlagde handlingar har målvising og ein konkret verifikasjonsmåte.

### 2. Skil applikasjonslogikk frå DOM

- Flytt tilstand, pending-request-korrelasjon og eventhandtering frå `app.ts` til avgrensa modular, éin funksjon om gongen.
- Eksponer tilstand og kommandoar for begge renderarar under overgangen. Behald persistensnøklar, request-ID-ar og eksisterande protokoll.
- Skil nettverk/polling i `imagegen.ts` frå den direkte DOM-renderinga.
- Gjer oppstart/opprydding og abonnement eksplisitte utan å utvide WASM-migreringa til eit nytt prosjekt.

**Ferdig når:** gammalt UI fungerer gjennom dei utskilde kontraktane og regresjonstestane er grøne.

### 3. Integrer bibliotek, bygg og nødvendige komponentutvidingar

- Legg til React og den lokale pakken med npm/lockfile og konfigurer TSX i dagens bygg. Vel og lås eksakte versjonar ved implementering innanfor pakken sine peer-krav.
- Implementer Composer-/reaksjonsutvidingane før dei koplast til reell chat. Prøv lys/mørk, tastatur, tom tekst med vedlegg og feilgjenoppretting.
- Importer bibliotek-CSS éin gong. Utvid esbuild-output, `build.rs` både normalt og med `SPROYT_FRONTEND_PREBUILT`, `src/web/assets.rs`, `src/web/browser.rs` og rutene i `src/server.rs` for innbygd, fingeravtrykt CSS og eventuelle lokale ressursar.
- Kontroller CI- og containerbygg sine kopieringssteg og tilgang til `frontend/vendor`; ingen demo-server blir produksjonsavhengig.
- Behald CSP, nonce, WASM-URL, autentisering og funksjonsflagg. Behald eksisterande asset-kompatibilitet for opne klientar under overgangen.

**Ferdig når:** eit minimalt nytt appskal blir servert av den ordinære Rust-serveren, også frå prebuilt-bygg, med fungerande ressursar og utan CSP-feil.

### 4. Migrer navigasjon, tidslinje, skriving og trådar

- Bygg hovudskalet og samtaleveljaren, deretter meldingar og trygg innhaldsrendering.
- Kople på utvida Composer, outbox, omtalar, reaksjonar, vedlegg og trådar.
- Før vidare lesemarkering, eldre historikk, stabil scroll etter medielasting, fokus og kanal-/trådutkast.
- Bruk ein mellombels utviklingsstyrt UI-veljar for samanlikning. Berre eitt UI og éin klientruntime blir starta per side.

**Ferdig når:** reell chat og medieflytar er funksjonelt likeverdige, og responsive visingar følgjer designet.

### 5. Migrer resten av funksjonsmatrisa

- Først kretsar, medlemskap, DM/gruppesamtalar og alle invitasjonsvariantar.
- Deretter ulest/omtalar/oppgåver, profil, varsel, dataeksport og sessionvisingar.
- Så biletegenerering, Heart, agenttilgang og Grafana, inkludert avslåtte funksjonar og feiltilstandar.
- Oppdater offline-side og PWA-relaterte flater til same design.

**Ferdig når:** kvar rad i matrisa er verifisert med relevante roller, flagg og feiltilstandar; ingenting krev retur til gammalt UI.

### 6. Fullfør designkontroll og fjern det gamle UI-et

- Visuell kontroll av alle flater i begge tema, systemtema, lange namn, mykje innhald, tomme lister, lasting, feil og sakte nett.
- Kontroller containergrensene 800/780 px på begge sider, ein smal innebygd container på brei skjerm, mobilportrett/landskap og skjermtastatur. Kontroller også zoom, fokus, Escape, retur frå tråd/dialog og reduserte rørsler.
- Fjern gammal markup, global CSS, ubrukte eventlyttarar og overgangsveljar når funksjons- og designkontrollen er fullført.
- Oppdater `ARCHITECTURE.md` og utviklingsdokumentasjonen. Behald kompatibilitetsruter til det er trygt å fase dei ut.

**Ferdig når:** det nye designsystemet er den einaste ordinære brukarflata og heile matrisa er godkjend gjennom verifikasjon.

## Verifikasjon og leveringskrav

Bruk repoet sine eksisterande kommandoar ved implementering:

```text
npm --prefix frontend run check
npm --prefix frontend run test:e2e
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo test -p sproyt-client-core
cargo check -p sproyt-client-core --target wasm32-unknown-unknown
```

`test:e2e` inkluderer dagens frontend-test og bygg. Køyr i tillegg biblioteket sine komponenttestar/bygg ved bibliotekendringar, og den eksisterande PostgreSQL-/releaseporten når endringa skal leverast. Tilpass gamle testar som låser skuffe-layout eller kjeldestrengar til den nye strukturen; bevar åtferdspåstandane. Ikkje fjern testar fordi gamle DOM-ID-ar forsvinn.

Særleg viktige akseptprøver:

- Mist sambandet under sending, last sida på nytt, og verifiser éi melding med same request-ID og korrekt køstatus.
- Byt kanal/tråd medan sending, opplasting eller biletegenerering pågår; resultat og utkast skal hamne i rett kontekst.
- Test utløpt session, nettfeil ved sessionprobe, fleirfane-fornying og bakgrunn/gjenopptaking utan å miste utkast.
- Send berre vedlegg; send med omtale; bruk IME og tastaturval av omtale utan utilsikta sending.
- Godta eit generert bilete utan at det blir publisert; reload skal bevare den private jobbflyten.
- Prøv eigar, vanleg medlem og manglande tilgang, og slå av Heart/agent/avansert/registreringsstøtte der dei har eigne vilkår.
- Kontroller oppgradering av service worker og eldre opne faner, samt normal/prebuilt/CI-bygg med korrekt CSS-cache og uendra privatlivsgrenser.

Bruk isolert database og testkontoar; ikkje send ekte invitasjonar eller endre produksjonsdata som del av UI-verifikasjonen. Dersom containerar trengst på Windows, les `wslc --help` og bruk `wslc` i samsvar med arbeidsavtalen.

Lever i avgrensa endringar etter fasane over. Produksjonsovergang kjem etter komplett funksjonskontroll og den eksisterande releaseprosessen. Behald førre applikasjonsartefakt for tilbakeføring; UI-migreringa skal ikkje krevje endra databaseskjema eller sletting av lokale utkast/kødata. Mål pakkestorleik, oppstart og lange tidslinjer mot grunnlaget frå fase 1 før levering.

## Viktigaste risikoar

- **Funksjonstap:** demoen dekkjer ikkje heile Sprøyt. Mottiltak: matrise og kontroll av kvar handling, også dei som er skjulte bak flagg.
- **Doble tilstandseigarar:** React rundt direkte DOM-/socket-kode kan skape duplikat eller tap. Mottiltak: éin runtime, eksplisitte abonnement og avgrensa renderer-eigarskap.
- **For enkel Composer/reaksjonsmodell:** biblioteket kan stoppe eksisterande flytar. Mottiltak: nødvendige utvidingar før migrering av chat.
- **Ressurslevering og PWA:** nytt JS/CSS kan kome i utakt med Rust-binæren eller gamle faner. Mottiltak: fingeravtrykk, prebuilt-verifikasjon og oppgraderingsprøve.
- **Falsk ferdigstatus:** eit nytt appskal kan skjule mykje umigrert funksjonalitet. Mottiltak: gammalt UI blir ikkje fjerna før alle radene i matrisa er verifiserte.

Neste steg, når implementering blir bestilt, er fase 1. Arbeidet stoppar her i denne oppgåva.
