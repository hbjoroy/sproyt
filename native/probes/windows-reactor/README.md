# Sprøyt Windows-laboratorium

Ein lokal, køyrbar UI-pilot med **Microsoft.UI.Reactor 0.1.0-preview.16** og
ekte WinUI 3-kontrollar. Reactor er C# med MVU, ikkje React/JavaScript eller ein
nettlesar. Prøva er isolert frå dagens Sprøyt-klient og produksjon.

## Prøv sjølv

På Windows 11 med PowerShell 7, .NET SDK 10.0.301, Rust 1.96 og ein native
C-kompilator for SQLite:

```powershell
./native/probes/run-windows-reactor.ps1 -UiSmoke -Launch
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

## Bygg, testing og avgrensingar

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
Windows-produktplattform. Android er framleis første produktmål; innlogging,
ekte historikk og produksjonssynkronisering følgjer den eksisterande native-planen.
