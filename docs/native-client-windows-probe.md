# Native Windows-prøve med Reactor

Dato: 25. oktober 2026. Dette er eit isolert, lokalt UI-laboratorium og ei
utviding av N02. Det leverer ikkje N01-identitet, N06-produksjonskjerne eller
ein ferdig Windows-klient. Android er framleis første produktleveranse.

## Retning og omfang

[Microsoft.UI.Reactor](https://github.com/microsoft/microsoft-ui-reactor) er
Microsoft si eksperimentelle C#/MVU-løysing over ekte WinUI 3. Ho blir prøvd
avgrensa etter ønske om å utforske Windows parallelt med Android-planen.
Versjon `0.1.0-preview.16` er låst i prosjekt og NuGet-låsfil. Ingen webview,
ny konto, serverkall eller produksjonsendring inngår.

Prøva har to demokanalar, 1 000 syntetiske meldingar per kanal, virtualisert
historikk, separat leseposisjon og utkast, lokal meldingskø, lyst/mørkt tema
og kompakt liste/detalj under 800 DIP. Sprøyt sitt papir/blekk-uttrykk,
citronaksent og korte skiljelinje **over** namn/tid er omsette til native
penslar. Samtalen bruker plassen; meldingsinnhaldet er ikkje innramma i kort.

[Køyring og installasjonsfri prøve](../native/probes/windows-reactor/README.md).

## Arkitektur og ansvar

- `PilotComponent` eig presentasjon og les siste uforanderlege snapshot ved
  render. Utkastet ligg i koordinatoren. Ugyldiggjering frå bakgrunnsarbeid
  blir flytta til UI-køen; eit gammalt snapshot kan ikkje overskrive ny tekst.
- `ChatWorkspace` har to kanaltilstandar, eitt sendeforsøk og eitt signal til
  ein asynkron pumpe. Tekstendringar blir samla opp; det blir ikkje starta eit
  databasearbeid eller ein ny Task for kvart tastetrykk.
- `NativeSession` kallar eksisterande C-ABI gjennom den dedikerte
  `ProbeWorker` med to ventande oppgåver. Lasting, kall, lukking og frigjering
  av DLL skjer på denne arbeidaren.
- Rust eig SQLite, transaksjonar, kapasitetsgrenser, eksklusiv databaseeigar,
  konto/generasjonsgjerde og meldingsjournal. UI og koordinator har ikkje SQL.
- Same request-ID og byte blir brukte etter tapt kvittering. Retry skriv
  ikkje eit gammalt, fanga utkast over eit nyare utkast. Utkast blir berre
  fjerna frå UI når både innhald og koordinatorrevisjon framleis svarar til
  den aktuelle sendinga.
- Ved lukking blir nye UI-kommandoar stengde, ventande TextChanged-hendingar
  får nå koordinatoren, og siste utkast blir lagra før arbeidaren blir lukka.
  Reparerbar lagringsfeil held vindauget ope. Feil etter disponering gjer det
  lesbart og lukka for nye skriveforsøk; neste lukking stadfestar avslutting.

## Verifisering

Lokal native ARM64-prøve på Windows med 200 % skalering har verifisert:

- Reell Rust-DLL, eksklusivt eigarskap, omstart, UTF-8-grense, åtte
  køelement, idempotent sending og kontrollert drenering/lukking.
- Deterministiske kappløp mellom autosave, sending og retry: nyare utkast,
  endring bort og tilbake til same tekst, ukjent resultat før/etter commit,
  tapte kvitteringar og identitet bevart gjennom fleire feil.
- Ugyldig/stort utkast under lukking, retting etter feilen, lukking medan
  sending går og feil etter at lagringsadapteren er disponert.
- Appen sitt native WinUI-kontrolltre: 60 materialiserte TextBlock-kontrollar
  med 1 000 meldingar per kanal, stabil tekstkontroll/markør ved autosave,
  kanalutkast, leseposisjon ved fleire og raske kanalbyte, faktiske
  lyse/mørke penslar, UI-sending, minimumsbreidda 360 DIP og siste tekst lagra
  ved umiddelbar avslutting.

Runneren krev både vellukka UI-kvittering, native `Closed`-hending etter
lagringslukking og prosessavslutting med kode 0. Hosted Windows x64 køyrer same
runner i `Native interoperability probe`; CI-kvitteringa, ikkje eit ARM64-bygg,
er evidensen for x64. Kvitteringar inneheld revisjon, rein/endra kjeldestatus,
arkitektur og hash av exe, DLL og den nødvendige samanslåtte ressursindeksen.

Første x64-køyring avdekte at kollaps av eit virtualisert viewport kan flytte
leseposisjonen. Kanalbyte fangar no posisjonen før kollaps og gjenopprettar
etter faktisk måling, med generasjonsgjerde og avgrensa venting. Ny navigasjon,
lukking eller brukarinput i historikken avbryt utsett restaurering. Berre
første reelle vising utan lagra posisjon rullar til botnen.

Den lokale UI-automatiseringa kunne lese kontrolltreet, men kunne ikkje
aktivere det fanga vindauget. Derfor er fysisk tastatur/IME, touch,
skjermlesar og visuell skjermbiletgodkjenning **ikkje verifiserte**. Den
interne UI-prøva er ikkje ført som slike testar. Desse blir del av manuell
utprøving av den køyrbare appen.

### Rettar etter den første brukarprøva, 25. oktober

Ctrl+Enter vart fanga i bubbling-hendinga `KeyDown`, etter at WinUI-tekstfeltet
kunne ha handsama Enter. Han blir no fanga i `PreviewKeyDown`. Både knapp og
tastesnarveg sjekkar siste koordinator-snapshot før sending. Dei les ikkje ein
gammal, enno ikkje tømd TextBox tilbake som nytt utkast etter sendesuksess.
Ein regresjonstest sender på nytt og byter kanal straks etter suksess, utan
å vente på at rendereren tømmer tekstfeltet.

Godteken eiga sending ber no om botnplassering etter at historikken er målt.
Den same avgrensa restaureringa og generasjonskontrollen gjeld. Vanleg render,
skriving og temabyte flyttar framleis ikkje leseposisjonen. Sending i ein kanal
som ikkje lenger er aktiv, flyttar ikkje visinga i den nye kanalen.

Rapportert tap av buffer ved omstart kunne ikkje reproduserast; brukaren
stadfesta at neste forsøk fungerte. Lagringsløysinga er uendra. Testdekninga
omfattar no ein **ny OS-prosess** med det same testlageret: fleirlinjeutkast
må vere synleg i den faktiske editoren, utkast i andre kanal må finnast, og
godteken lokal sending må framleis vere der. Å berre opne SQLite frå den
første prosessen blir ikkje rekna som denne aksepten.

Med det oppdaterte CUA-verktøyet vart native Windows-input seinare prøvd i ein
isolert profil på ARM64: Ctrl+Enter gav éi sending utan linjeskift, vanleg Enter
gav to linjer, og send-knappen viste den nye meldinga. Etter Alt+F4 og ein ny
prosess var begge sendingane og eit usendt fleirlinjeutkast tilbake. Dette er
syntetisk native OS-input; fysisk tastatur/IME, touch og skjermlesar står att.

## Kjende grenser før ein ekte klient

Rust-prøva lagrar utkast etter innhald, utan varig utkast-revisjon. Når ei
uviss sending fjernar eit nyare utkast med identisk tekst, reparerer
koordinatoren det med ei ny lagring. Dette er prøvd etter at pumpa har falle
til ro og ved ordinær lukking. **Hard prosessdød mellom fjerning og reparasjon
er ikkje atomisk sikra.** N06 må ha ein varig revisjon/forventa-revisjon i
same transaksjon før denne koordinatoren kan bli produksjonsløysinga.

Ingen nett-ACK, synkronisering, token, utlogging mot server, ekte medlemskap
eller innhald frå Sprøyt blir prøvd her. Fiktiv lokal kø har framleis den
eksisterande prøva sine grenser; han blir ikkje tømd av ein nettarbeidar.

## Byggfunn

Reactor-previewen krev Roslyn 5.9 for generatorar/analyser. Prosjektet låser
`Microsoft.Net.Compilers.Toolset 5.9.0` som privat byggavhengigheit i staden
for å endre SDK-en på maskina. Full Windows App SDK 2.2.0 vel dei samsvarande
WinUI 2.2.1-komponentane.

Kodebasert WinUI genererer framleis ein samanslått app-PRI. Standard publish
tok med komponent-PRI-ane, men utelét app-PRI-en og gav temakrasj ved oppstart.
Prosjektet tek no eksplisitt med den genererte app-PRI-en. Frisk publish og
faktisk appstart er verifikasjonen; ingen global runtime-registrering trengst.

## Neste port

Astra har revidert binding/arbeidar, koordinatoren sine retry-/lukkegrenser og
UI-koplinga. Den lokale prøva kan brukast for UX-feedback medan N01 og
servergrunnlaget i [implementasjonsplanen](native-client-implementation-plan.md)
går vidare. Ho er ikkje grunn til å hoppe over identitetsprøva eller låse
Windows-previewen som endeleg rammeverk.
