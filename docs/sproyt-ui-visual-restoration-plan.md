# Plan: gjenopprett uttrykket frå Sprøyt-designsystemet

## Oppdrag og avgrensing

Planlagd 17. september 2026 etter samanlikning av originalen i `sproyt-ui/sproyt-ui`, den installerte pakken, applikasjonskoden og brukaren sine fire skjermbilete. Denne oppgåva endrar berre planen. Ingen applikasjonskode eller produksjon skal endrast i planfasen.

Målet er å likne originaldesignet, med alle eksisterande Sprøyt-funksjonar bevarte. Det er ikkje eit nytt designprosjekt. Brukaren si presisering om ikon, ro og mobilbruk går føre formuleringar i skillen som kan mistolkast som krav om synleg knappetekst overalt.

## Kva samanlikninga viser

| Område | Originalen | Dagens integrasjon og konsekvens |
| --- | --- | --- |
| Farge og flater | Varm papirbakgrunn `--sp-canvas: #faf9f5`, kvite avgrensa flater, mørkt koltema, citronaksent | Global `main`-CSS frå `assets/index.html` treff biblioteket sin `<main class="sp-main">` med `background: var(--paper)`, ramme, runding og skugge. Biblioteket nullstiller ikkje alle desse eigenskapane. Dette er ein konkret kandidat til feil hovudflate; stadfest med computed styles i nettlesaren. |
| Avsendar og rytme | Tett byline, sparsam metadata, 8 px meldingsmellomrom og ein kort 56 px strek over neste avsendar | Global `header`-CSS tilfører 18 px padding og fullbreidd understrek til `.sp-message-meta`. `.sp-message + .sp-message` verkar ikkje når kvar melding er pakka i ein eigen `div[data-message-id]`. |
| Meldingshandlingar | Demoen brukar `variant="quiet"`, eit lite reaksjonssymbol og «Svar» | Integrasjonen legg reaksjon, eigen emoji, rediger, slett, svar og reaksjonsdetaljar ut samtidig. Standardknappar med rammer tek merksemda frå samtalen. |
| Metadata | Kort klokkeslett og status ved behov | `toLocaleString("nn-NO")` gir full dato og klokkeslett på kvar melding; normal sendestatus tek òg plass. |
| Skrivefelt | Éi linje i kvile, diskret pluss og små valfrie verktøy | Lange tekstknappar vart supplerte med horisontal scrolling og pil. Dette løyser breidd teknisk, men ikkje det ønskte uttrykket. |
| Media | Avgrensa ukutta førehandsvising og diskret detalj-/fullstorvising | Ustyla `figcaption`, filnamn og vanleg blå lenkje blir framtredande. |
| Appskal | Tydeleg Sprøyt-merke og få overordna element | Desktop har ei rad med globale tekstknappar og manglar originalen sin merkeprofil. Mobilrettinga innfører eit separat uttrykk. |

SHA-256 for original `src/styles.css` og installert `dist/styles.css` er identisk: `33AADD870804C720260C26FCB47A58EDA0AA7C91AEE7E75E1821E01708658287`. Avviket kjem dermed ikkje av ein annan bibliotek-CSS-versjon. Host-styling, DOM-struktur og val av komposisjon er dokumenterte skilnader. Nettlesaren må stadfeste den endelege CSS-kaskaden; originaldemoen er ikkje rendra på nytt i denne planfasen.

## Visuell fasit

- Bruk original React-demo (`examples/react.tsx`), `examples/shared/demo.css`, komponentkjeldene og token som fasit for farge, typografi, strekar, avstand og hierarki. Demodata, utviklarverktøy og test-sendefeil skal ikkje kopierast til produktet.
- Behald varm papirflate, mørk ink, citron og det tilhøyrande mørke temaet. Kvitt skal ha same rolle som i originalen, ikkje fylle hovudfeltet via legacy-CSS.
- Meldinga er hovudinnhaldet. Rutinehandlingar og metadata er sekundære; sletting og avanserte val skal ikkje vere permanent framheva.
- Bruk eit konsekvent sett enkle ikon for kjende handlingar. Ikonknappar får tilgjengelege namn og hjelp ved fokus/hover; menyval og uklare handlingar får tekst. Touch må fungere utan hover eller langt trykk som einaste inngang.
- Alle funksjonar skal vere tilgjengelege, men treng ikkje vere synlege samtidig.

## Gjennomføring i rekkjefølgje

### 1. Etabler ein direkte visuell samanlikning

Køyr originaldemo og applikasjon lokalt med deterministiske, tilsvarande samtalar: korte og lange meldingar, bilete, reaksjonar, tråd og utkast. Ta bilete i lyst og mørkt tema ved 390×844 og 1440×900. Suppler med 320×568, kort tastaturflate og smal container på desktop.

Registrer computed styles for `.sp-main`, `.sp-message-meta`, `.sp-composer`, textarea, knappar og figcaption. Noter kjelderegelen for bakgrunn, padding, border, font og shadow. Skil verifiserte kollisjonar frå hypotesar.

Ferdig når originalen og appen kan samanliknast side om side, og avvika har konkrete eigarar i kode. Dette er grunnlaget for vidare arbeid, ikkje berre bilete av tom chat.

### 2. Fjern lekkasje frå det gamle grensesnittet

Avgrens gamle elementreglar i `assets/index.html` til legacy-rota. Kartlegg også `form`, `button`, `input`, `textarea`, lenkjer, dialogar og responsive reglar. React-rota skal ha eit eksplisitt, lite grunnlag og same token-eigarskap som originalen.

Kontroller renderarar som framleis bruker eldre klassar: Markdown, vedlegg, omtaleveljar og andre integrerte innhaldsflater må få medvite scoped styling. Unngå å stable fleire spesifikke overstyringar oppå kollisjonane. Behald eventuell legacy-reserveinngang avgrensa til si eiga rot.

Ferdig når papirflate, byline, rammer og typografi stemmer med originalen i begge tema, og ingen global legacy-elementregel påverkar React-chatten.

### 3. Gjenopprett meldingskomposisjonen

Tilpass `conversation-view.tsx` og meldingsrenderarane slik at originalen sin rytme og korte skiljestrek fungerer med nødvendig `data-message-id` og scrollankring. Ikkje introduser meldingskort eller talebobler.

Presisering frå brukaren: skiljestreken skal stå **over avsendarnamnet til den nye meldinga**, aldri mellom namnet og meldingsinnhaldet. Kvar melding skal lesast som éi samanhengande eining i denne rekkjefølgja: kort skiljestrek, namn og tid med diskrete handlingar, deretter meldingsinnhald og eventuelle reaksjonar/trådsvar. Fjern den utilsikta understreken på metadata-headeren. Avstanden mellom namn og innhald må vere mindre enn avstanden til førre melding.

Handlingane skal ha ei fast, føreseieleg forankring i den aktuelle meldinga. På stor skjerm skal dei liggje i same kompakte byline som originalen og innanfor meldinga si avgrensa innhaldsbreidd; dei skal ikkje hamne langt ute i eit tomt felt. Unngå ei brei `auto`-kolonne med tekstknappar som pressar namn og tid saman. Ved lange namn eller liten breidd må sekundærhandlingar samlast i meny framfor å skape lausrivne knapperader. Test med korte meldingar, lange namn, bilete og open sidetråd.

Vis kort klokkeslett i byline; full dato/tid skal framleis vere tilgjengeleg i detaljar og semantisk `time`. Samle datokontekst på tidslinja. Normal «Sendt» skal vere diskret; uviss levering, kø og feil må framleis vere forståelege.

Bruk ein liten, roleg inngang til reaksjon og svar, og ei fleirvalshandling for rediger/slett. Samle «Eigen emoji» i reaksjonsveljaren og «Kven reagerte?» på reaksjonsmerket/detaljvisinga. Behald fleirreaksjonar, teljing, rettar, tastatur og fokusretur. På touch finst ein synleg kompakt inngang; på desktop kan ekstra handlingar visast ved hover/fokus utan layoutskifte.

Hjartesymbolet frå originalen er konkret stilreferanse for reaksjonsinngangen. Gå gjennom symbolhandlingane i både originaldemoen og det førre Sprøyt-grensesnittet før dei blir omforma. Behald den effektive plassbruken og attkjenninga: eit lite symbol med roleg, normalt uinnramma uttrykk og tilstrekkeleg trykkflate. Ikkje erstatt eit kjent symbol med ein stor tekstknapp for å tilfredsstille eit generelt krav om etikettar. Bruk tilgjengeleg namn, fokus-/hoverhjelp og forklarande tekst inne i menyen der det trengst. Hjartet opnar framleis heile reaksjonsvalet, ikkje berre éin reaksjonstype.

Ferdig når ein kort tekst ikkje får fleire rader med administrasjon under seg, og alle eksisterande handlingar er nåbare.

Visuelle akseptkrav: I ei liste med minst fem korte meldingar skal det utan interaksjon vere eintydig kva innhald kvart namn, tidspunkt og handlingssymbol høyrer til. Skiljestreken skal liggje over neste namn på mobil og desktop. Handlingssymbol skal ha same relative plassering mellom meldingane og ikkje flytte seg til tilfeldige posisjonar når tekstlengd, namn eller medieformat varierer. Kontroller dette mot originalen ved 320, 390, 1440 og 1920 px breidd.

### 4. Bygg skrivefeltet i same stil, med ikon

Erstatt tekstkarusellen i `preview-composer.tsx` med kompakte ikon for emoji, omtale og vedlegg, og eit tydeleg val for biletegenerering i verktøypanelet. Bruk eitt kompakt panel når plassen krev det; ingen horisontal scrolling for å finne dei vanlege skriveverktøya.

Behald originalen si éinlinjes kvileform og kontrollert tekstvekst. Send skal vere lett å kjenne att. Verktøy skal ikkje tvingast fram berre fordi feltet får fokus dersom dette reduserer meldingsplassen unødig. Valde vedlegg, framdrift og feil er eigne synlege tilstandar.

**Innhaldet skal få mest mogleg av skriveflata.** Når utkastet berre inneheld tekst, skal tekstfeltet bruke den tilgjengelege breidda og vekse etter behov; ikkje reserver plass til tomme vedleggsområde, framtidige verktøy eller store handlingar. Éinlinjes kvileform er ein starttilstand, ikkje ei avgrensing som gjer lengre tekst vanskeleg å skrive. Avgrens veksten ut frå tilgjengeleg skjermhøgd og tastatur, med intern scrolling først når feltet elles ville fortrengt samtalen.

Når vedlegg eller anna innhald kjem inn, skal komposisjonen tilpasse seg det faktiske utkastet. Bruk kompakte førehandsvisingar, grupper fleire vedlegg og gjer detaljar større ved behov; bevar god plass til teksten ved sida av eller over/under etter breidda. Vis nødvendig opplastingsstatus, feil og fjern-/redigerhandling lokalt ved innhaldet. Store førehandsvisingar eller fleire vedlegg skal ikkje presse tekstfeltet eller sendekontrollen ut av skjermen. Tekst og andre innhaldsdelar skal opplevast som eitt samla utkast.

Behald cursor/selection, IME, omtaletastatur, paste, vedlegg utan tekst, separate kanal-/trådutkast og varig sending. Touchmål er minst 44×44 px sjølv om sjølve ikonet er mindre.

Ferdig når alle vanlege skrivehandlingar er forståelege og nåbare ved 320 px utan verktøyscrolling, og feltet fungerer med skjermtastatur.

Prøv eksplisitt rein tekst, lang tekst, berre vedlegg, tekst med eitt/fleire vedlegg, pågåande opplasting og feil med ope tastatur. Kontroller faktisk skriveplass og tilgang til innhaldet, ikkje berre om kontrollane ligg innanfor viewporten.

### 5. Samordne appskal, navigasjon og media

Gjeninnfør originalen sin merkeprofil og eit roleg hierarki i både desktop og mobil. Globale rutineval får ein samla inngang; innboks/ulest skal ha ein tydeleg, diskret indikator. Kanaloverskrift og gruppenamn skal ha originalen sine proporsjonar. Vurder bjelle per kanal opp mot behovet for ro; varslingsvala må framleis vere lette å finne.

Gi mediefigurane avgrensa ukutta format og diskret bildetekst frå designet. Trykk opnar stor vising; fullstor-/originalhandling og filnamn er tilgjengelege utan å dominere kvar melding. Kontroller dialogar, profil, innboks, biletegenerering og avanserte flater mot same grunnstil.

### 5a. Gjer plass for fleire typar meldingsinnhald

Sprøyt skal vidareutviklast med mange typar innhald i meldingar. Join-/medlemsinvitasjonar og Mermaid-diagram er eksisterande føringar for dette, ikkje særtilfelle som kan ofrast for enklare layout. Designet må romme både tekst, media, strukturerte opplysningar og innhald med eigne handlingar, også i same melding.

Skil den felles meldingsramma (avsendar, tid, reaksjonar og meldingsmeny) frå sjølve innhaldsvisinga. Definer eit lite, eksplisitt grensesnitt for innhaldsrenderarar og eventuelle tilhøyrande utkast-/redigeringsvisingar. Nye typar skal kunne leggje til eiga vising og lokale handlingar utan å byggje om heile tidslinja eller fylle den globale verktøyrada med nye knappar. Bruk eksisterande protokoll og trygg rendering; dette er ikkje ei bestilling på nye innhaldstypar eller ei generell pluginplattform no.

Felles krav er design-token, mobiltilpassing, tilgjengelege namn/fokus, trygg rendering og lenkjehandtering, lokal lasting/feil og stabil leseposisjon når innhaldet endrar storleik. Innhaldsspesifikke handlingar, som å bli med via ein invitasjon, skal liggje ved innhaldet og ha tydeleg status. Breie diagram får ei avgrensa vising og tilgang til større vising; dei skal ikkje gjere heile chatten breiare. Behald plass til rikare innhald utan å gjere kvar vanleg tekstmelding til eit kort.

Bruk dei eksisterande invitasjonane og Mermaid-støtta som konkrete akseptprøver saman med blanda tekst/media i kanal og tråd. Kontroller invitasjonsstatus og tilgang, diagramlasting/-feil, endra innhaldshøgd og retur frå større vising. Framtidige innhaldstypar treng ikkje implementerast for å validere denne utvidingsretninga.

### 6. Kontroller design og funksjon kvar for seg

Visuell port: samanlikn endelege skjermbilete med originaldemoen, ikkje berre førre produksjonsversjon. Kontroller farge, bylineavstand, ikon, rammer, metadata og faktisk innhaldsro. Ta med korte meldingar i tett historikk, medietung chat, lange namn, tråd, opne verktøy og feiltilstandar. Mål nok meldingsplass på mobil, men ikkje bruk høgdemål som einaste kvalitetsbevis.

Funksjonsport: køyr eksisterande frontend-/browser-kontraktar og målretta prøver for endra handlingar, rettar, tastatur, utkast, sending og media. Oppdater testar til nye tilgjengelege handlingar utan å fjerne funksjonskrava. Ta vare på visuelle referansebilete slik at framtidige endringar kan samanliknast.

Før produksjon må den visuelle samanlikninga vere ferdig og dokumentert. Deretter gjeld vanleg releaseport og produksjonsverifikasjon. Ein grøn utrulling er ikkje dokumentasjon på designlikskap.

## Ansvar og modellbruk ved seinare implementering

Éin ansvarleg agent eig originaltolking, appskal, meldingar og skrivefelt gjennom heile arbeidet. Dersom brukaren ønskjer å bruke Astra, er dette den delen ho bør eige. Sol kan uavhengig kontrollere funksjonar og samanlikne faktiske skjermbilete mot fasiten. Terra/Luna kan utføre klart avgrensa mekanisk arbeid etter at uttrykk og komponentkontraktar er fastlagde.

Unngå at fleire agentar kvar for seg bestemmer korleis handlingar skal sjå ut. Modellval åleine forklarer ikkje avviket; manglande isolasjon frå legacy-CSS og manglande visuell sluttkontroll er konkrete feil som arbeidsmåten må fange.

Bibliotekendringar skal gjerast i vedlikehalden kjelde med ny eintydig pakkeversjon ved behov, aldri direkte i `node_modules`. Den noverande untracked originalkatalogen skal bevarast som referanse; vel eit versjonert opphav før eventuelle bibliotekendringar. Planen krev ingen ny backend eller omskriving av runtime.

## Leveranse og stoppunkt

Første implementeringsleveranse bør vere eit samanhengande appskal med tidslinje og skrivefelt som liknar originalen, med før/original/etter-bilete og dokumentert funksjonsbevaring. Fullfør deretter sekundærflatene og releasekontrollen. Denne planleggingsoppgåva stoppar med dette dokumentet; gjennomføring og utrulling er ikkje starta.
