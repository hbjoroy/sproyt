# Plan: ryddig oppsett og innstillingar i Sprøyt

Dato: 26. oktober 2026. Avstemt mot kode og leveransar 27. oktober 2026.
Status: navigasjonsleveransen, S2b/S2c og førstegongsoppleving/DM er i Canary.
Den synlege omlegginga av hovudmeny, innboks og innstillingssider er
implementert i [PR #306](https://github.com/hbjoroy/sproyt/pull/306).
CI-, utrullings- og akseptstatus for denne pakka blir førte i PR-en.
Grunnlag: `main` på `44bc12c9a0d000f87c82dec07a88d19b0371c9bb`.

Gjennomføring er starta etter nytt oppdrag 26. oktober. Det første snittet
utvidar navigasjonsgrunnlaget med prioriterte/samanfoldbare kretsar, eiga
direktemeldingsflate, adresserbare sider og personleg historikk. Sjå
[leveranse og vidare produktarbeid](product-navigation-implementation.md).
Resten av denne planen er framleis eit målbilete, ikkje levert funksjonalitet.

Astra har vurdert arkitektur og planutkast. Sluttreviewet er teke inn som
presiseringar om omfang, prosessinnsyn og binding av pågåande operasjonar.

Målet er at ein brukar finn rett val, forstår kven endringa gjeld og ser om ho
verkar. Nettappen og dei native klientane skal dele denne forståinga. Planen
krev ikkje fysisk tilgang til Windows-maskina eller endringar i produksjon.

## 1. Kva som skaper problemet i dag

Dette er observasjonar i koden, ikkje ein ny produksjons- eller brukartest:

| Observasjon | Konsekvens | Kjelde |
| --- | --- | --- |
| Hovudmenyen blandar personar, profil, varsling, Felles, kretsar, kanaldetaljar, teknisk agenttilgang og Heart. | Ein må vite kvar funksjonen vart lagt inn, framfor kva han gjeld. | `frontend/src/ui/react/preview-management.tsx` |
| Separate tilstandar opnar innstillings-, krets-, agent- og avanserte dialogar. Nokre opningar lukkar hovudmenyen; andre gjer det ikkje. | Tilbake, lukking og fokus varierer. Fleire modalflater kan vere opne. | Same fil og `navigation-scope-actions.tsx` |
| Agentlista, redigering, kanaltilgang, videresending og alle ferdigheitene bur i same dialog. | Enkle endringar krev mykje rulling og kunnskap om avhengigheiter. | `circle-chat-agents.tsx` |
| «På i kretsen» og kanalval blir viste kvar for seg. | På-brytaren forklarar ikkje om agenten kan svare i den aktuelle kanalen. | `circle-chat-agents.tsx`, `channel-chat-agents.tsx` |
| «Agenttilgang» opprettar kortliva tilgang for ein ekstern klient. | Namnet blir lett forveksla med samtaleagentar som Maria. | `preview-advanced.tsx` |
| Heart bruker vald krets til aktivering, men aktiv samtale til oppstart. | To forskjellige mål kan vere synlege i same arbeidsflate. | `preview-advanced.tsx` |
| Funksjonstilgang blir delvis avleidd frå om legacy-DOM-element er skjulte. | Synlegheit blir kopla til gammal presentasjon og kan ikkje delast med native. | `frontend/src/app.ts`, oppretting av `createAdvancedHost` |

Bevar det gode grunnlaget: eksisterande API/host-eigarskap, revisjonskontroll,
personleg minnesamtykke, kanalautorisasjon og at opning av meny ikkje skaper
nøklar, invitasjonar eller prosessar.

## 2. Mønster frå andre program

| Referanse | Det vi tek med | Tilpassing til Sprøyt |
| --- | --- | --- |
| [Slack: personlege preferansar](https://slack.com/help/articles/212596808-Adjust-your-sidebar-preferences) og [workspace-oppsett](https://slack.com/help/articles/360000355143-Review-your-workspaces-settings-) | Skil personlege val frå felles administrasjon. | Kretsen er det sosiale administrasjonsnivået; Felles er eit eige område. |
| [Discord: server- og kanalrettar](https://support.discord.com/hc/en-us/articles/206029707-Setting-Up-Permissions-FAQ) | Rettar og kanaloppsett høyrer til konkrete ressursar. | Vis krets, kanal og eiga rolle før ei tilgangsendring. Bevar våre eksisterande reglar, ikkje kopier Discord sin rettsmodell. |
| [Android: innstillingar](https://developer.android.com/design/ui/mobile/guides/patterns/settings) | Grupper, liste/detalj og forklarte avhengigheiter; respekter systemval. | Éi side om gongen på mobil. Forklar blokkerte OS-varsel og tilby relevant systeminngang der plattforma støttar det. |
| [WinUI: innstillingssider](https://learn.microsoft.com/en-us/windows/apps/design/app-settings/guidelines-for-app-settings) | Fast inngang, eigne sider og kortfatta grupper. | Innstillingar nedst i native navigasjon. Bruk Sprøyt sine rolege rader og semantiske token. |

Dette er vår designvurdering basert på kjeldene. Dei ulike plattformene er
ikkje samde om alle detaljar, som plassering av konto og appinformasjon. Vi
vel ei sams produktinndeling og bruker native presentasjon på kvar plattform.

## 2a. Avstemming og ny kjeldekontroll – 27. oktober

Dette vidarefører planen frå 26. oktober. Det erstattar ikkje vedtaka med
ein ny plan for å flytte symbol. Status er kontrollert mot kode og PR-ar;
vurderingane nedanfor er tilrådingar for den attståande gjennomføringa.
Astra har gjort ein ny, avgrensa gjennomgang av plan og presentasjonskode.

Tabellen og kodeobservasjonane i denne avstemminga skildrar utgangspunktet
`4ab7e8d` før gjennomføringa av PR #306. Pakka implementerer dei ordinære
sidene, funksjonsplasseringane, innbokskontrakten og sambandsvisinga nedanfor.
Ho gjer ikkje heile S3–S5 eller alle framtidige skjemaforbetringar ferdige.

| Del av planen | Faktisk status | Det som står att |
| --- | --- | --- |
| S0: handlingskart og skisser | Delvis. Kjeldemønster og målarkitektur er dokumenterte. | Full sporbar plassering av eksisterande handlingar og tre konkrete flytar. |
| S1a: navigasjonsgrunnlag | Levert til Canary. | Bevare adresser, historikk, utkast og eigarskap når presentasjonen blir bytt. |
| S1b: personlege sider | Delvis. Søk og grupper finst. | Vanlege sider; hovudinngangen blandar framleis personlege val og administrasjon. |
| S2a: krets og kanal | Delvis. Kontekstsnarvegar og eksplisitte mål finst. | Fullføre eigne ressursflater og fjerne avhengigheit av aktiv samtale i global meny. |
| S2b/S2c: agentoppsett | Canary, PR #304 / GitOps #271. | Brukaraksept og eventuell seinare produksjonspromotering. |
| Førstegongsoppleving og DM | Canary, PR #305 / GitOps #272. | Brukarprøving; ikkje ein uferdig implementasjonsetappe. |
| S3/S4/S5 | Framleis attståande. | Prioriter synleg fullføring av S1b/S2a og relevant menyopprydding før breiare prosess-/native-arbeid. |

Konkret kodegrunnlag: `HeaderActions` er framleis ein panelmeny med knappar;
`PreviewManagement` blandar personlege val, ny DM, Felles, kanaloppsett og
prosess-/integrasjonsval. Oversikt og mange detaljflater bruker `Dialog`.
At berre éin modal er aktiv, oppfyller ikkje vedtaket om vanlege sider.
`PreviewInboxes` legg uleste meldingar, uleste omtalar og opne personlege
oppgåver saman i eitt merke. Det er tre ulike storleikar, og omtalar kan
overlappe med ulesttalet. `ConnectionIndicator` viser eit passivt symbol og
utleier gjenoppretting frå statustekst; han tilbyr inga manuell handling.

### Mønster som er undersøkte på nytt

Primærkjeldene er lesne 27. oktober. Dette er dokumenterte produktmønster,
ikkje påstandar om at alle klientversjonar har identisk plassering.

| Kjelde | Dokumentert mønster | Vår vurdering for Sprøyt |
| --- | --- | --- |
| [Slack Activity](https://slack.com/intl/en-gb/help/articles/19693583638803-Get-your-work-done-from-the-Activity-view) | Eiga aktivitetsflate, uleste varsel som merke, filter og tett/detaljert presentasjon. | Fast inngang og kompakte rader. Sprøyt skal ikkje få eit nytt sett uavhengige lesemarkørar eller ei brei feed utan datagrunnlag. |
| [Teams Activity](https://support.microsoft.com/en-us/teams/notifications-settings/explore-the-activity-feed-in-microsoft-teams) og [personlege innstillingar](https://support.microsoft.com/en-us/teams/personal/get-started/getting-to-know-microsoft-teams-personal) | Aktivitet og profil/innstillingar er forskjellige inngangar. | Skil det som krev merksemd frå korleis brukaren konfigurerer appen. |
| [Teams kanal-/kretsadministrasjon](https://support.microsoft.com/en-gb/teams/teams-channels/create-a-standard-private-or-shared-channel-in-microsoft-teams) | Medlemmer og kanalval blir opna ved den aktuelle gruppa/kanalen. | Vidarefør våre eksisterande ressursmenyar og rettar; ikkje kopier Microsoft sin organisasjonsmodell. |
| [Discord Inbox](https://support.discord.com/hc/en-us/articles/360045027712-Inbox-FAQ) | Desktop/nett skil uleste frå omtalar og tilbyr hopp til kjeldesamtalen. | Bevar skiljet og presis tilbakeveg. Ordet «Innboks» er ikkje i seg sjølv feil; innhald og oppførsel må avklarast før omdøyping. |
| [Slack tilkoplingsfeil](https://slack.com/help/articles/205138367-Troubleshoot-connection-issues) | Synlege feilmeldingar, banner ved WebSocket-problem og handling for omstart ved tilkoplingssvikt. | Diskret normaltilstand; forståeleg tekst og relevant handling ved avvik. Logoen kan supplere dette, men skal ikkje vere einaste forklaring eller skjult reparasjonsknapp. |

### Neste levering: fullfør den eksisterande arkitekturen

1. **Spor kvar handling før flytting.** Bruk tabellen under som startkart og
   kontroller mot kode/testar. Lag tre klikkbare flytar: personlege val,
   krets/kanaladministrasjon og innboks → kjeldemelding → tilbake. Vis mobil
   og stor skjerm før detaljane i toppfeltet blir låste.
2. **Felles sideskall for S1b/S2a.** Innstillingar og innboks får ordinære,
   adresserbare sider med overskrift, Back og liste/detalj. Bevar samtale,
   tråd, utkast, vedlegg og leseposisjon i bakgrunnen. Korte val og
   stadfestingar kan framleis vere dialogar. Gjenbruk eksisterande mål og
   host-/serverkontraktar; inga ny rettigheits- eller prosessløysing.
3. **Fullfør plasseringa, deretter fjern overgangsmenyen.** Personlege sider
   viser personlege val. Krets og kanal viser si administrasjon. Ny DM,
   kanaloppdaging og historikk blir verande ved samtalenavigasjonen.
   På stor skjerm kan innstillingar/hjelp få fast plass i sidefeltet; på
   mobil skal den faste inngangen vere tilgjengeleg utan ei ny permanent
   verktøylinje som tek unødig plass frå meldingane.
4. **Rydd innboks og teljing.** Bevar dei eksisterande visingane Uleste,
   Omtalar og personlege Oppgåver med eigne, forklarte tal. Tilråding for
   første snitt: hovudmerket tel stadfesta uleste omtalar og blir omtala som
   nettopp det. Uleste-visinga viser til dømes «12 uleste meldingar i 3
   samtalar», medan personlege Oppgåver viser tal på opne oppgåver. Tomt og
   enno ikkje henta er forskjellige tilstandar. Prosessarbeid blir i kanalane
   sine; eksisterande personlege oppgåver med prosesslenkje blir bevarte.
   Ingen ny generell arbeidskø eller ny lesemarkør blir laga.
   Behald «Innboks» inntil innhaldsavgjerda er prøvd; «Aktivitet» er aktuelt
   dersom vi seinare faktisk har fleire personlege hendingstypar å vise.
5. **Gjer sambandet forståeleg og mogleg å gjenopprette.** Logoen kan vise
   diskret tilstand med form, ikkje berre farge. På mobil er han framleis
   Samtalar-knapp. Ved vedvarande brot viser vi kort tekst og «Kopla til att»;
   utløpt økt tilbyr rett innloggingshandling. Ei fungerande SSE-tilkopling
   er tilkopla, også medan ein usynleg WS-probe går. Bruk eksisterande
   samband-/økttilstand og gjenopprettingsveg, ikkje nye helseprober eller
   nye parallelle transportar. Manuelt forsøk får ventestatus og vern mot
   gjentekne klikk. «Last inn på nytt» er ein reserveveg som sikrar bevarbare
   utkast og handsamar pågåande vedlegg/ufullførte skjema før reload.

| Handling i dagens overgangsmeny | Fast heim i målarkitekturen |
| --- | --- |
| Profil/status, varsling, personvern og eige agentminne | Personlege innstillingar; kretsval for minne opnar same personlege mål. |
| Byt tema og rå/formaterte meldingar | Utsjånad og bruk, med korrekt klient-/kontomerking. |
| Logg ut | Konto / kort personleg meny. Ingen ny øktadministrasjon. |
| Ny DM og finn person | Direktemeldingsoversikta. |
| Finn/opprett krets, bruk invitasjon og finn kanalar | Samtaleoversikta med tydelege neste handlingar. |
| Medlemmer, invitasjonar, kanalar og agentar i ein krets | Den namngjevne kretsen; «Administrer krets» kan stå som kort snarveg. |
| Felles-kanalar og global invitasjon | Felles med eksisterande tilgangsreglar. |
| Kanaldetaljar, medlemmer, kanalvarsel og integrasjonar | Den namngjevne kanalen eller «Om samtalen» for DM. |
| Mellombels ekstern agenttilgang og prosessoppsett | Integrasjonar/prosessoppsett ved vald kanal. Teknisk inspeksjon under Drift berre ved eksisterande rett. |
| Gå til siste, Back/Forward og Nyleg | Samtale-/historikknavigasjon. |
| Kom i gang, versjon, oppdatering og sambandsdetaljar | Hjelp / Om Sprøyt. Avvik med nødvendig handling blir òg synlege ved chatten. |
| Uleste, omtalar og personlege oppgåver | Innboks med tydelege visingar; prosessoppgåver blir verande i kanalen. |
| Til fullt grensesnitt | Berre den eksplisitte utviklarførehandsvisinga; ingen ordinær produktinngang. |

Dette er ei presentasjons- og navigasjonslevering. Ho krev ikkje ny database,
fleire prober eller ombygging av alle skjema. Før gamle inngangar blir
fjerna, skal handlingskartet vise at kvar eksisterande funksjon har ein
testa heim. Aksepten omfattar dei tre komplette flytane, korrekt teljing,
offline/reauth/SSE, mobil/tastatur, kontobyte og bevarte utkast. Ny kode og
Canary-utrulling blir ei eiga gjennomføring etter denne planavstemminga.

## 3. Informasjonsarkitektur

Det skal vere éin kanonisk destinasjon for kvar oppgåve. Søk, hovudinngang og
kontekstmeny er ulike vegar til den same sida, med same ressurs-ID og utkast.

| Inngang | Undersider | Kven vala gjeld |
| --- | --- | --- |
| **Innstillingar** | Profil og status; Varsel; Utsjånad og bruk; Personvern og minne; Konto og tryggleik | Deg, med tydeleg merke for konto eller denne eininga. |
| **Krets → Om kretsen / Kretsinnstillingar** | Oversikt; Medlemmer og roller; Kanalar; Invitasjonar; Agentar; Prosessar og integrasjonar | Den namngjevne kretsen. Administrasjon krev eksisterande rettar. |
| **Kanal → Kanaldetaljar / Kanalinnstillingar** | Om og medlemmer; Tilgang; Agentar; Prosessar og integrasjonar; Dine kanalvarsel | Den namngjevne kanalen, eller dine personlege val for kanalen. |
| **Felles → Om Felles / Administrasjon** | Kanalar og invitasjonar med dagens autorisasjon | Sprøyt sitt fellesområde, ikkje ein oppdikta vennekrets. |
| **Hjelp og om Sprøyt** | Hjelp; tilbakemelding; versjon; forståeleg tilkoplingsstatus | Appen og denne klienten. |
| **Drift**, berre ved nødvendig operatørtilgang | Avansert diagnostikk og teknisk prosess-/integrasjonstilstand | Tenesta. Drift og utrulling held fram i eksisterande GitOps-rutine. |

Tabellen er eit målbilete, ikkje ei påstand om at alle undersidene finst.
S0 merker kvart punkt **finst / blir flytta / framtidig forslag**. S1–S2 leverer
kartlagt funksjonalitet og nødvendig navigasjon/status; nye kontofunksjonar,
personlege kanalval eller andre utvidingar krev eiga avklaring dersom dei
ikkje finst frå før.

Forståeleg prosessstatus, relevante feil og neste handling blir verande ved
prosessen etter eksisterande innsynsrettar. Berre infrastrukturdiagnostikk
høyrer til operatørflata. «Drift» gir ikkje mandat til å innføre ei ny rollemodell.

Hovudinngangen får innstillingssøk og ein kort snarveg **Administrer krets**
som opnar ein kretsveljar. Han skal ikkje utvide alle kretsar til store
knappelister. Krets- og kanalmenyane bevarer `…`, kjende symbol og få handlingar.

Vanlege handlingar som ny melding, ny samtale og oppgåvebehandling ligg nær
innhaldet. Oppgåver held fram som meldingar/kort i kanalar som er aktiverte
for den aktuelle prosessen. Dei skal ikkje samlast i ei ny generell oppgåveliste
under innstillingar.

Skil særleg mellom:

- **Agentar:** samtaleagentar i kretsen.
- **Mellombels tilgang for ekstern agent:** eksisterande kortliva credential,
  under integrasjonar/tilgang for den konkrete kanalen.
- **Prosessoppsett:** kva kanalen kan starte og kva applikasjonar han gjeld.
- **Prosessarbeid:** registrering, behandling og svar i den aktuelle kanalen.

Konto og tryggleik viser berre implementerte funksjonar og eksisterande
identitetsflytar. Øktadministrasjon, recovery eller native passnøkkelkontroll
skal ikkje sjå ferdige ut før dei faktisk er leverte. Utsjånad og hjelp skal
vere tilgjengelege også før innlogging.

## 4. Navigasjon, layout og designsystem

Mobil bruker **oversikt → detalj → redigering**, med vanleg tilbakeknapp.
Større flater kan vise kategoriliste og innhald ved sida av kvarandre.
Innhaldet får avgrensa lesebreidd. Innstillingar skal ikkje bli ein stabel av
dialogar; dialogar blir brukte til korte val og stadfestingar.

Alle administrasjonssider viser til dømes **Paros / Prat / Agentar** og rolla
di. Ressurs-ID-en blir bunden når sida blir opna. Kanalbyte i chatten skal
ikkje endre målet for eit ope skjema eller ei seinare lagring.

Søk har eit lite, lokalt register over destinasjonar og alias som «bjelle»,
«varsling», «bot», «agent», «invitere» og «ferge». Treff viser kva krets/kanal dei
gjeld. Treff og titlar må filtrerast etter faktisk innsyn; søk skal ikkje
avsløre private kanalar eller andre sine minne. Eit søketreff opnar ei side,
ikkje ei muterande handling.

SettingsContext, SettingsSection, SettingsRow og SettingsSummary er
appkomposisjonar av eksisterande `@sproyt/ui`, ikkje nye eksportar frå pakken.
Dei bruker same rekkjefølgje, semantiske token, lys/mørk tema, tastaturfokus
og minst 44 px trykkflate, med lågt visuelt støynivå.

Web, Compose, SwiftUI og WinUI deler namn, bestemmingar og tilstandar. Dei
skal bruke sine eigne navigasjons- og kontrollmønster. Dette innfører ikkje
ein felles nettlesarflate i native eller eit nytt UI-rammeverk.

## 5. Agentoppsett som første prioriterte arbeidsflyt

S2b/S2c-agentarbeidet er i Canary (PR #304 / GitOps #271).
Flyten er adresserbar frå liste til agent og vidare til éin vald del:
Oppførsel, Minne, Bilettolking, Biletskaping, Vêr, Sjøtemperatur, Ferger,
Samarbeid eller tilgang for vald kanal. Konto/servermerking viser eigarskap.
Nye agentar blir oppretta avslått som standard.

Agentutkast er eigarbundne i minnet, avgrensa til 24, og ikkje profilsynkroniserte.
Back, lukking og før-unload vernar uferdige utkast. Små CAS-oppdateringar med
revisjon bevarer uendra delar. Ved timeout krevst tilbake-lesing før ny endring;
ved konflikt kan brukaren behalde utkastet eller velje lagra oppsett. Dette gjer
ikkje tilsvarande arbeid for andre profil- og innstillingsskjema ferdig.

Eksempel: **Paros → Agentar → Maria**. Lista viser formål og ei kort
statusoppsummering. Detaljsida har desse delane:

1. **Oversikt:** ønskt av/på, kvar agenten kan svare, hinder og neste handling.
2. **Oppførsel:** namn, omtale/triggerfrasar og svarføringar.
3. **Ferdigheiter:** minne, bilettolking, biletskaping, vêr, sjøtemperatur og
   ferger. Kvar rad viser gjeldande oppsett og opnar ei avgrensa redigering.
4. **Kanalar og tilgang:** valde kanalar og kva som faktisk er tillate.
5. **Samarbeid:** eksisterande videresending og samarbeid mellom agentar.

Ingen stor tabell med alle agentar mot alle kanalar blir lasta på oppstart.
Hent relevant oversikt når sida blir opna, og detaljar ved behov. Bruk
eksisterande status og kort, avgrensa cache; ikkje nye modellkall eller
hyppige prober berre for å vise innstillingar.

Kanalstatus forklarer konfigurasjon og tilgang for den valde kanalen, med
relevant neste handling. Han er ikkje ein helseprobe eller garanti for svar;
triggarreglar og køyrereglar gjeld framleis. Faktisk tenestetilstand kan
vurderast separat etter eksisterande innsynsrettar. Ein feil i ei valfri
ferdigheit skal ikkje framstillast som total stans dersom resten kan fungere.

Private kanalar bevarer eksplisitt agentval og eksisterande innsynsgrenser.
Ei samleoversikt skal aldri slå på tilgang som sideverknad. **Mitt agentminne**
er personleg, med vidareføring av eksisterande krets-/agent-/kanalgrenser og
samtykke. Det er tilgjengeleg frå Personvern og frå kretsen, men opnar same
destinasjon. Kretsadministrasjon gir ikkje innsyn i andre sine personlege minne.

**Kontroller oppsett** kan i eit seinare delsteg forklare konfigurasjonen for
ein vald kanal utan å køyre modellen eller sende meldingar. Ei faktisk
prøvemelding krev ei eiga, synleg brukarhandling.

## 6. Lagring, rettar og felles kontraktar

- Merk kvar innstilling med eigar og lagring: konto/server, krets/server,
  kanal/server eller denne klienten. Dagens lokale temaval blir ikkje
  automatisk gjort kontofelles. Synkronisering krev ei eiga avgjerd og kontrakt.
- Bruk eit lite typa destinasjonssett med eksplisitte konto-, krets- og
  kanal-ID-ar. Bevar heile returmålet, chatutkast og leseposisjon ved navigasjon.
- Serveren eig autorisasjon. Erstatt DOM-avleidd funksjonstilgang med eksplisitt
  domenetilstand og, der det manglar, ei avgrensa serverkontrakt for tillatne
  handlingar og strukturerte årsaker. Ikkje lag ein generell rettigheitsmotor.
- Skil manglande rett, funksjon slått av i utrullinga og mellombels tenestefeil.
  Forklar utilgjengelege val. Vis ikkje skjerma informasjon for å forklare eit
  avslag. Konto- eller rollebyte må stengje gammal administrasjonsvising.
- Bind lesing og lagring til konto, ressurs og revisjon når operasjonen startar.
  Seine svar frå før eit konto-/ressursbyte får ikkje oppdatere ny vising,
  nytt utkast eller ny kontocache. Serveren kontrollerer gjeldande rettar.
  Avbrot i klienten lovar ikkje at ein alt motteken servermutasjon er angra;
  ved retur må den opphavlege ressursen lesast tilbake.
- Skjema med fleire felt bruker **Lagre / Avbryt** og vern mot tap av utkast.
  Ei enkel brytarhandling kan lagrast direkte med ventetilstand og stadfesta
  resultat. «Lagra» kjem først etter serverkvittering.
- Bevar revisjon og konfliktkontroll. Vis «Endra av andre» og la brukaren
  behalde/samanlikne utkast med siste versjon før nytt forsøk. Ei endring i
  vêr skal ikkje overskrive ferdigheiter, kanalval eller biletoppsett.
- Ved timeout er utfallet uviss. Les tilbake aktuell tilstand før ny mutasjon;
  ikkje opprett credential, invitasjon eller prosess ein gong til automatisk.
- Ikkje kø tilgangsendringar, sletting eller prosessoppstart stille offline.
  Ikkje-sensitive utkast kan bevarast konto-/ressursvis med avgrensa levetid;
  nøklar og token skal aldri i vanleg utkastlager, søk, lenkjer eller loggar.
- Bevar eksisterande domenekommandoar. Ikkje innfør generisk «lagre alle
  innstillingar»-API, dynamiske JSON-skjema eller ei ny database/cacheteneste.
  Nye status-/kontraktfelt blir additive og versjonerte ved behov.

Serverflagg og infrastrukturkonfigurasjon held fram under GitOps. Appen
forklarer kva som er tilgjengeleg og kva operatøren må endre, utan å tilby
tilsynelatande brytarar som ikkje kan påverke tenesta.

## 7. Granulert leveringsplan

| Steg | Avgrensa levering | Ferdig når |
| --- | --- | --- |
| **S0 – kart og vedtak** | Lag komplett handlings-/eigar-/lagringskart frå eksisterande menyar, API og testar. Avklar namn og teikn tre klikkbare flytar: personlege val, krets/agent og kanal/prosess. | Alle eksisterande handlingar har ny plass og rolle; vanlege brukarar og moderatorar kan gjennomgå skissene remote. Ingen dataendring. |
| **S1a – navigasjonsgrunnlag** | Typa destinasjonar, eksplisitt ressursmål, éi navigasjonshistorikk og trygt konto-/rollebyte. Definer minimal capability-/feilkontrakt. | Meny og kontekstsnarveg opnar same side; ingen modalstakk eller implisitt aktiv-kanal-mål. Tilgang kan ikkje utleiast frå DOM. |
| **S1b – personlege sider og søk** | Ny innstillingsoversikt, eksisterande profil/varsel, tydeleg lokal/servermerking og lite tilgangsfiltrert søk. Dokumenter designmønstra. | Profil, varsling og tema er lette å finne; tilbake bevarer chat og utkast. Avslag frå OS blir forklart. Ingen ny kontosynkronisering. |
| **S2a – krets og kanal** | Flytt medlemmer, invitasjonar og kanaloppsett til faste sider. Bevar gamle inngangar som snarvegar under overgangen. | Felles, krets og kanal er eintydige; eksisterande medlems-/invitasjonsreglar og private grenser er uendra. |
| **S2b – agentoversikt og forklaring** | I Canary: agentliste/detalj, konfigurasjonsstatus og same tilgangsside frå begge inngangar. | Oktett-/Paros-scenario: brukaren finn kvifor agenten ikkje svarar og får rett neste handling utan hjelp frå utviklar. |
| **S2c – agentredigering** | I Canary: del opp oppførsel, ferdigheiter og samarbeid. Bevar revisjon, delvise oppdateringar og ukjende/gamle val. | Endring i éin del bevarer andre delar; feil, timeout, konflikt og tilbake bevarer utkast. Ingen automatisk melding eller modellkall. |
| **S3 – prosessar og integrasjonar** | Skil prosessoppsett frå oppstart/oppgåvearbeid. Gje ekstern agenttilgang presist namn og plass. Flytt teknisk inspeksjon til drift. | Prosess blir starta i eksplisitt vald kanal; kanalen kan berre starte konfigurerte prosessar/applikasjonar. Oppgåvene blir verande i sine kanalar. |
| **S4 – native paritet** | Implementer først personlege innstillingar og éi representativ krets-/agentflyt i Windows; bruk same kontrakt i Android-arbeidet. | Same eigarskap og feiltilstandar med native navigasjon. Manglande native funksjon opnar presist nettmål og blir ikkje framstilt som implementert. |
| **S5 – opprydding** | Fjern gamle opningsvegar og legacy-DOM-avhengigheiter som no er erstatta. Oppdater hjelp og paritetskart. | Kvar gammal funksjon er spora til testa ny plass; ingen ny menyparallell blir ståande. |

Robust lagring og autorisasjon er akseptkrav frå S1, ikkje ein sluttpolering.
S0–S2b er første nyttige pakke. Ikkje utset ryddigare agenttilgang i påvente av
alle native klientar eller ei full ombygging av avanserte integrasjonar.

Sol 6.1 styrer implementeringa. Små, eintydige flyttingar og dokumenterte
komponentoppgåver kan delegerast rimeleg. Astra tek ei kort vurdering av
destinasjons-/rettigheitskontrakten før S1 og agent-/tilgangsresultatet før
første Canary-release. Bruk eit nytt review ved nye tryggingsgrenser, ikkje
ei brei omlesing etter kvar etikettendring.

## 8. Validering og utrulling når implementering blir bestilt

- Karakteriser dagens domeneoppførsel før flytting. Test semantiske handlingar,
  ikkje berre nye komponentnamn. Mappinga frå S0 fungerer som paritetssjekk.
- Prøv medlem, moderator og eigar; same kanalnamn i to kretsar; privat kanal;
  konto-/rollebyte medan eit skjema er ope; konflikt, timeout og offline.
- Prøv smal mobilflate, stor skjerm, begge tema, tastatur, fokus og tilbake.
  Ingen uventa sidevegs rulling, skjult lagrehandling eller tap av chatutkast.
- Bruk testdata og eksisterande CI for kontraktar og nettlesarprøver. Native
  fixtures kan kontrollere kontrakt/layout; dei beviser ikkje fysisk touch,
  OS-løyve eller Windows Hello. Desse akseptpunkta blir dokumenterte separat.
- Skisser, review, kode, CI og Canary-utrulling kan gjerast remote utan at
  brukaren må opne eit lokalt program på maskina i Noreg. Full fysisk/native
  aksept kan vente utan å blokkere weboppryddinga.
- Følg PR → CI → testa immutabelt image → GitOps → Canary. Gjenbruk same
  digest ved seinare produksjonspromotering. Denne planbestillinga gir ikkje
  mandat til implementering eller produksjonsendring.
- Bevar kompatible API-ar under overgangen. Tilbakeføring av UI skal bevare
  konfigurasjon og eksisterande rettar; ingen massereset eller datamigrering
  er føresett for navigasjonsoppryddinga.

## 9. Tilknytte planar

- [Native klientarkitektur](native-client-architecture-plan.md) og
  [implementasjonsplan](native-client-implementation-plan.md).
- [Designrestaurering](sproyt-ui-visual-restoration-plan.md) og
  [UI-paritetskart](sproyt-ui-parity-audit.md).
- [Samtaleagentar](circle-chat-agents.md),
  [personlege minnekontrollar](agent-memory-user-controls.md) og
  [prosess-/arbeidssakplan](work-items-and-heart-plan.md).
