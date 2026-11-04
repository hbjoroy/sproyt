# Sprøyt Windows-laboratorium

Den same appen har no òg ein separat ekte Canary-modus. Start ei bygd pakke
med `--server https://sproyt-canary.bjoroy.me/` for innlogging, kanalar,
tekstchat og autorisert medielesing. Sjå [oppdatert klientstatus](../../../docs/native-client-status-2026-11-03.md)
og [paritetsplanen](../../../docs/native-canary-parity-plan.md).
Demo- og laboratorieavgrensingane nedanfor gjeld ikkje som status for alle
modusar. Windows er første produktprioritet, deretter Android.

For ein separat live-testprofil kan du leggje til
`--data-dir "$env:LOCALAPPDATA/Sproyt/WindowsCanaryTest"` etter serveradressa.
Bruk ein absolutt sti. Standardprofilen `WindowsCanary` blir då ikkje opna,
og gamle utkast eller kømeldingar der blir ikkje sende av testinstansen.

Ein lokal, køyrbar UI-pilot med **Microsoft.UI.Reactor 0.1.0-preview.16** og
ekte WinUI 3-kontrollar. Reactor er C# med MVU, ikkje React/JavaScript eller ein
nettlesar. Prøva er isolert frå dagens Sprøyt-klient og produksjon.

## Prøv sjølv

På Windows 11 med PowerShell 7, .NET SDK 10.0.301, Rust 1.96 og ein native
C-kompilator for SQLite:

```powershell
./native/probes/run-windows-reactor.ps1 -ArtifactsDirectory C:/Build/sproyt-reactor -UiSmoke -Launch
```

Runneren byggjer på den lokale maskina sin ARM64- eller x64-arkitektur, køyrer
lagrings- og UI-prøvene og opnar deretter appen. Bygg og testdata blir lagde
under `C:/sproyt-build/reactor`, eller mappa vald med `-ArtifactsDirectory`.
Han installerer ingen global Windows App Runtime. Den publiserte mappa inneheld
runtime, Rust-DLL og ressursindeks; opne `Sproyt.ReactorProbe.exe` der seinare.

- Byt mellom **Prat** og **Idear**; utkast og leseposisjon er separate.
- Skriv, byt kanal og start appen på nytt. Utkasta skal vente på deg.
- Enter gir ny linje. **Ctrl+Enter** eller pila legg teksten i den lokale køen.
- Byt tema med symbolet øvst, og gjer vindauget smalare for samtalelista.
- Historikken er fiktiv: 1 000 meldingar i kvar kanal. Berre synlege rader blir
  materialiserte som native kontrollar.

Vanleg køyring lagrar i
`%LOCALAPPDATA%/Sproyt/WindowsProbe/demo-v1.sqlite`. For ein ny testprofil kan
appen startast med `--data-dir C:/sproyt-build/reactor/min-profil`.
Testane bruker alltid nye mapper. Ingen runner slettar gamle databasar.

Dette er ei avgrensa lagringsprøve: maks åtte kølagde meldingar og 1 024
UTF-8-byte per utkast/melding. Køen sender ingenting til andre brukarar. Han
kan bli full; bruk ein ny testprofil for fleire prøvesendingar.

## Vindauge og skjermtastatur

Når du er ved botnen av samtalen, blir siste melding halden synleg ved endra
vindaugsstorleik. Justeringa ventar til geometrien har vore roleg i 180 ms,
slik at raske endringar blir samla. Har du rulla opp for å lese, blir
leseposisjonen bevart; musehjul, peikar og tastatur avbryt ei venta justering.

Desktop-innkoplinga til Windows InputPane reserverer faktisk overlappa
botnplass når skjermtastaturet dekkjer klienten utan å krympe vindauget.
Målinga bruker klientkoordinatar i DIP og ei ytre ramme som held seg stabil.
Har Windows alt krympa klienten, blir berre eventuell attståande overlapp
reservert; flytande tastatur gir ikkje fullbreidd botnplass.

Showing/Hiding, vindaugsendringar og fokushendingar oppdaterer målinga.
Medan tekstfeltet har fokus eller vi kjenner til overlapp i eit aktivt vindauge,
tek éin native timer ei måling kvar 200 ms som vern mot manglande Showing-varsel.
Uendra geometri gir ingen layout- eller rulleoperasjon. Ingen polling går i
bakgrunnen når vindauget er inaktivt. Timer og hendingar blir kopla frå ved lukking.

UI-prøva kontrollerer at både skrivefelt og siste melding ligg over eit simulert
tastaturområde, at lukking gir plassen tilbake, at mindre klient ikkje gir dobbel
reservasjon og at lesing av eldre meldingar blir bevart. Dette erstattar ikkje
utprøving av faktiske Windows-tastaturhendingar med touch på maskina.

## Bygg, testing og avgrensingar

Kvart bygg bruker eigne managed mellomfiler og eigen C#-kompilatorprosess.
Dermed kan gamle byggfiler ikkje skjule kjeldeendringar når filklokkene avvik.

Sjå [resultat og arkitektur](../../../docs/native-client-windows-probe.md).
Den felles Rust-prøva eig SQLite. Den eksisterande C#-arbeidaren serialiserer
alle native kall; UI-et skriv aldri SQL. Ein avgrensa koordinator samlar opp
tekstendringar og tek vare på request-ID ved uviss kvittering.

`--self-test <ny-mappe>` køyrer dei deterministiske lagringsprøvene.
`--ui-smoke <ny-mappe>` køyrer integrasjon mot appen sitt eige native kontrolltre,
lagar kvittering og lukkar vindauget. Runneren startar deretter ein ny prosess
med `--ui-resume <same-mappe>` og kontrollerer at fleirlinjeutkast faktisk kjem
tilbake i skrivefeltet, saman med utkastet i den andre kanalen og lokal kø.
UI-prøva krev at eiga sending blir synleg utan manuell rulling.
Desse automatiske integrasjonsprøvene injiserer ikkje OS-input og erstattar
ikkje testing av fysisk tastatur, IME, touch, skjermlesar eller visuell kvalitet.

Reactor er eksperimentell. Denne prøva tek ikkje stilling til ein endeleg
Windows-produktplattform. Vidare produktarbeid følgjer paritetsplanen;
full produksjonssynkronisering står framleis att.

## Live-pilot: navigasjon, historikk og lesing

Live-visinga har søk og krets-/DM-faner, tilbake/fram med separate utkast og
meldingsanker, og ei handling for å laste eldre meldingar. Historikken er
avgrensa til 2000 røter i minnet; kapasitet og sidefeil blir viste i samtalen.
Berre faktisk synlege røter i eit aktivt vindauge kan bli melde lesne, og
ulestindikatoren bruker stadfesta servertilstand.

Uvisse sendingar bevarer ID og payload og ventar 2–60 sekund mellom automatiske
forsøk. «Prøv sending» er eksplisitt manuell retry med ei minstegrense på eitt
sekund. «Oppdater samband» og vindaugsaktivering omgår ikkje ventinga.

Live-fixture-prøva måler faktiske virtualiserte rader ved eldre historikk,
navigasjon og endra radhøgd, og kontrollerer at inaktivt vindauge ikkje sender
lesekvitteringar. Transport- og timerkontraktane ligg i `history-contracts`,
`navigation-contracts` og `delivery-contracts`; dei bruker syntetiske eigarar
og kontrollerte feil. Full live-aksept og fysisk input står framleis att.

## Trådar og meldingshandlingar

«Svar i tråd» opnar forelder og svar med eit eige varig utkast. Trådoversikta
kan òg opne røter som ligg utanfor den lasta kanalhistorikken. Tilbake/fram,
lukking og kontobytte held kanal- og trådutkast skilde. Trådlesing bruker målte
synlege svar; forelderen blir ikkje rekna som eit lese svar.

Meldingsval har reaksjonar, redigering av eigne meldingar og sletting med
stadfesting. Vedlegg blir bevarte ved redigering. Sletting viser ein tombstone
og skjermar gammal tekst/media, medan tråden framleis kan opnast. Ved uviss
kvittering tilbyr klienten «Kontroller endringa»; han sender ikkje same
reaksjon eller endring automatisk på nytt. Moderator-sletting står att.

Trådar er avgrensa til 2000 svar og 20 MiB per serversvar. Store svar blir
avviste med kapasitetsstatus utan reconnect-løkke. Dette er framleis eit
online pilotsnitt, med dei eksisterande grensene for utkast og sendekø.
`conversation-contracts`, `thread-feature-contracts` og
`message-action-contracts` supplerer den faktiske WinUI-prøva.

## Riktekst og opplasting

Meldingar i kanal og tråd viser Markdown/GFM med tabellar og kode. Bruk **Vis råtekst** for å sjå heile originalen. Mermaid blir vist som kjeldekode, HTML er inaktivt og eksterne bilete blir viste som plasshaldarar. Dokument av typen PDF, ZIP, UTF-8-tekst og CSV kan lagrast med ein filveljar.

Vel **Legg ved** eller lim inn filer og bilete. Utklippstavlebilete blir koda som PNG etter kontroll av ei grense på 16 × 1024 × 1024 pikslar. Windows tek ein privat kopi før overføring, med høgst 512 MiB totalt og åtte jobbar per konto. Køen viser framdrift, avbrot og nytt forsøk. Eit ferdig vedlegg blir lagt i utkastet som vart valt ved start. Du kan fjerne det der eller velje **Send**. Er utkastet for stort, kan du frigjere plass og prøve igjen. Tekst og markørar må til saman få plass i 1024 UTF-8-byte.

## Originalbilete og filslipp

Windows Canary har ein originalvisar med prosent, zoom inn/ut, tilpassing av
heile biletet og 1:1. 100 % tyder eitt orientert originalpunkt per logisk
skjermpunkt. Redusert dekoding av store bilete blir merkt tydeleg; **Lagre**
bevarer originalbyte. Bilete, video og dokument bruker typetilpassa filending.
Visaren lukkar og slepper den private originalfila ved konto-/familiebyte.

Filer kan sleppast på heile kanal- eller trådkomponisten. Filslipp kopierer
berre til det fanga utkastet og sender inga melding. Høgst åtte filer om gongen;
mapper og flytting blir ikkje tekne imot som vedlegg. Uavklart filslipp blir
avbrote dersom komponistfeltet blir avmontert. Fysisk input og autentisert
Canary-aksept står framleis att; sjå implementasjonsstatusen for prøveomfang.

## Personar, innboks og profil

**Personar** søkjer på namn eller brukarnamn og opnar ein ein-til-ein-samtale
etter stadfesta kanaltilgang. **Innboks** viser ulesne kanalar, dei nyaste
200 omtalingane og eigne tildelte oppgåver. Omtalingar blir merkte lesne med
eit eige val; oppgåver kan fullførast og opnast att. Ei kjent kjelde opnar
rett melding eller tråd. Elles er handlinga tydeleg merkt **Opne kanal**.

**Profil** lagrar eige visingsnamn og status berre ved eit uttrykkeleg val.
Namnet i toppfeltet kjem frå den stadfesta profilen. **Tøm status** fjernar
tekst og emoji. Ved uviss kvittering hentar **Kontroller** servertilstanden
utan å sende den same endringa på nytt. Usend samtaletekst og ulagra
meldingsredigering blir bevarte medan desse flatene er opne.

Gruppesamtalar, oppretting av oppgåver, lagra statusval og varslingsinnstillingar
står att. Kontraktprøvene og dei syntetiske WinUI-prøvene bruker ingen ekte
innlogging eller brukardata. Autentisert Canary-aksept står framleis att.
