# N00: lokal verktybaseline for native Sprøyt

Dato: 24. oktober 2026. Dette er ei inventarliste for den lokale Windows-verten, ikkje ei bygg-, plattform- eller kapasitetsgodkjenning. Kjeldegrunnlaget for eksisterande app er `main` på `42cb3b6`; planarbeidet byggjer på dokumentrevisjon `2bd405c`. Sjå [implementasjonsplanen](native-client-implementation-plan.md) for N00–N02 og dei vidare akseptkrava.

## Reproduserbar inventarliste

Køyr frå rotmappa i repoet med PowerShell 7:

```powershell
pwsh -NoProfile -File native/probes/tooling-inventory.ps1
```

[Skriptet](../native/probes/tooling-inventory.ps1) skriv eitt JSON-objekt med avgrensa versjonssvar, kommandostiar, tre konvensjonelle SDK-mapper og ledig plass på C og S. Det les berre lokalt: ingen installasjon, bygging, nettverk, autentisering, clusterinspeksjon eller generell dumping av miljøvariablar. `present` tyder at kommandoen finst på PATH; eit versjonssvar med `exit_code: 0` stadfestar berre at den kommandoen kunne køyrast. Andre SDK-plasseringar blir ikkje søkte gjennom, så fråvær i denne lista er ikkje bevis for at eit verkty manglar overalt på maskina.

## Observert lokalt grunnlag

| Område | Observasjon | Kva dette faktisk viser |
| --- | --- | --- |
| Vert | Windows ARM64 | Lokal plattform for ei avgrensa Windows ARM64-prøve. |
| Rust | `rustc 1.96.0`; `rustc` og Cargo finst på PATH | Kompilatorverkty er tilgjengelege. Linkar, `cdylib`, pakking og framandspråk-binding er enno ikkje prøvde i denne baselinen. |
| .NET | SDK `10.0.301`, lokal vert `win-arm64`, runtime `10.0.9` | .NET er tilgjengeleg. Ein C#-konsollprøve er ikkje ei WinUI-prøve. |
| Java og Android | `java`, `javac`, `adb` og `gradle` ikkje funne på PATH. Konvensjonelle `C:/Program Files/Android`, `%LOCALAPPDATA%/Android/Sdk` og `C:/Program Files/Java` manglar. | Java/Android SDK er ikkje tilgjengelege gjennom dei undersøkte inngangane. Ingen Android-app er bygd eller prøvd. |
| Apple | Ingen macOS-byggvert eller Apple-signering tilgjengeleg i dette lokale arbeidet | Swift/iOS/macOS-bygg, bindingar, pakking og signering er uverifiserte. |
| Lagring | Om lag 3,37 GiB ledig på S og 359 GiB på C ved første kartlegging | Augeblikksbilete av plass; ikkje eit målt byggbudsjett. Ledig plass endrar seg, og skriptet gir oppdaterte byteverdiar. |

## Valideringsgrense og neste prøve

Rust `cdylib` kalla frå C# gjennom P/Invoke, med SQLite-utkast/journal og gjenopptaking etter omstart, er **ikkje testa i denne inventarbaselinen**. Resultatet skal dokumenterast i ei eiga N02-prøve når ho er køyrd. Baselineinventaret skal framleis skilje verktytilgang frå den konkrete prøva sitt resultat.

Den separate [N02-prøva](native-client-probe-results.md) er no køyrd på Windows ARM64. Ho dokumenterer faktisk DLL-last og lokal SQLite-gjenopptaking; det endrar ikkje inventarskriptet sine `not_tested`-felt, som berre skildrar kva det lesande skriptet sjølv kontrollerer. Rustup-autoinstall er slått av under versjonslesing slik at manglande toolchain ikkje blir installert av inventaret.

Kotlin-/Swift-bindingar, WinUI, Android-/iOS-pakking, signering og fysisk einingsåtferd er ikkje validerte. Det er ikkje gjort API-, database-, minne-, pool-, nettverks- eller kapasitetsmåling her. N00 er dermed berre delvis dekt; minimum OS-versjonar, einingsmatrise, byggvertar og signeringsløysing må framleis avklarast før dei aktuelle plattformportane kan godkjennast.
