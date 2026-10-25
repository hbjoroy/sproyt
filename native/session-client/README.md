# N03a: delt økteigar og Windows-integrasjon

Dette er ein køyrbar, isolert del av native-appen. `NativeAuthSession` blir
kompilert frå same kjelde i identitetsprøva og Windows Reactor-appen. Han eig
legitimasjon og livsløp; `NativeSession` i Windows/Core eig framleis berre
Rust-lagringa for samtalen. UI får status og handlingar, aldri token.

## Levert

- DPAPI CurrentUser, éin eigar og atomiske journalar frå N01b blir gjenbrukte.
- Aktiv, fråkopla, innlogging ventar, logga ut og ny innlogging nødvendig er
  eksplisitte tilstandar. Nettfeil bevarer legitimasjon og utkast.
- Eitt fornyingsforsøk lagrar operasjons-ID før HTTP. Retry bruker same gamle
  token, generasjon og ID. Resultat blir lagra før det blir meldt aktivt.
- Utlogging skriv lokal sperre før nettarbeid. Ventande revoke blir lagra
  varig, og eit seint svar kan ikkje gjenopplive ei eldre lokal generasjon.
- Gyldige, seine innloggingsresultat blir køa for revoke utan å erstatte ei
  nyare økt. Køen har ei fast grense; han blir ikkje sletta ved nettfeil.
- Windows-grensesnittet bruker same eigar, same eksisterande handoff og ein
  separat samtaledatabase bunden til testkontoens kanoniske ID. Den gamle
  lokale demoen og utkastdatabasen hennar er urørte.

`Core/NativeAuthentication` i Windows-appen eig browserforsøket og ei varig
liste over kansellerte handoff-forsøk. Utlogging lagrar denne intensjonen
før ho sperrar den lokale økta. Proof blir verande i handoff-journalen til
serveren stadfestar kansellering. Nye innloggingar ventar på denne oppryddinga.
Ein lokal ticket utan journal kan berre ryddast når helperen er avslutta:
helperen lagrar proof/returtilstand før han kan opne nettlesaren.

## Serverkontrakt og tryggleiksgrense

Den eksisterande Rust-brokeren får eksplisitt `--session-lab --listen-port`.
Han avviser denne modusen saman med ekte Authentik-konfigurasjon. Ein ny
versjonert SQLite-labdatabase kan opnast att med same instance/opphav/issuer.
Vanleg N01-modus krev framleis ein ny database og har ikkje desse rutene.

| Rute | Autoritet og utfall |
| --- | --- |
| `/probe/exchange` | Same app-prov som N01; legg til ein familie og `native_session` i same transaksjon som handoff-kvitteringa |
| `/session/info` | Ikkje-hemmeleg scope, miljøreferanse, instance og issuer |
| `/session/probe` | Access-bearer; kontrollerer familie, generasjon, epoch, utløp og sperre |
| `/session/refresh` | Refresh-bearer + operasjons-ID/generasjon; atomisk rotasjon og eksakt retry innan 60 sekund |
| `/session/revoke` | Separat stabil revoke-kapabilitet; idempotent og kan ikkje gi ny tilgang |
| `/session/cancel-login` | Transaksjon + opphavleg app-verifier; kansellerer handoff og eventuell utferda familie atomisk, også etter utløp |

Retry-kvitteringar blir kontrollerte mot sperre og epoch før utlevering.
Kansellerte forsøk kan ikkje opnast av ein forseinka callback eller exchange.
Fixture-ruter med `X-Probe-Control` kan endre epoch/utløp berre i denne laben;
dei er ikkje eit produksjons- eller recovery-grensesnitt.

Token har eigne `n03a-*`-prefiks, eige scope og eige testmiljø. Dei gir ingen
tilgang til Sprøyt. Serverdatabasen inneheld **syntetiske** resultat i klartekst
for prøva; produksjon krev kryptert token-/retrylagring etter identitets-ADR-en.
Dette er ikkje PostgreSQL-migreringar eller ein produksjonsklar sessionserver.
Ein kontrollert brokerrestart bevarer ferdige familiar; pågåande upstream
OIDC-handoff over brokerrestart er ikkje godkjend av dette snittet.

## Verifisering og køyring

`run-identity-probe.ps1` køyrer Rust-testar og HTTP-kontraktar på Windows/Linux.
På Windows køyrer han også den faktiske DPAPI-eigaren med nettfeil, replay,
epoch, feil scope/konto, korrupte/låste journalar, serverrestart og faktiske
prosessdrap før/etter lokal lagring av ei rotert økt. Linux får ikkje rapportert
ein OS-lagringsaksept.

`run-windows-reactor.ps1 -UiSmoke` prøver no også faktisk WinUI i to prosessar:
innlogging, lokal utlogging, ny innlogging og omstart med bevart kanalutkast.
Denne automatiske UI-prøva simulerer nettlesaren mot den verkelege syntetiske
OIDC-provideren. Ho er ikkje Windows Hello-, touch- eller produksjonsaksept.
Berre ikkje-hemmelege kvitteringar blir lasta opp frå CI.

```powershell
./native/probes/run-identity-probe.ps1 -ArtifactsDirectory S:/Build/sproyt-native-identity
./native/probes/run-windows-reactor.ps1 -ArtifactsDirectory S:/Build/sproyt-reactor -UiSmoke
./native/probes/run-session-ui.ps1 -PublishDirectory <testa-Windows-pakke> -BrokerExecutable <testa-broker.exe> -ArtifactsDirectory S:/Build/sproyt-native-session/interactive -Launch
```

Den siste kommandoen opnar systemnettlesaren med den syntetiske Alice-kontoen.
Launcher-eigaren held brokeren i gang medan appen er open, stoppar berre den
prosessen han sjølv starta og bevarer labdata for neste oppstart.

## Vegen til ein nyttig app

N03a er sessionfundamentet. Neste Windows-etappe er native endepunkt i den
ekte backend-en, normal Authentik-innlogging og transportautorisasjon gjennom
HTTP/WS/SSE. Deretter koplar vi Windows-samtalen til ekte kanalliste, historikk,
tekstsend­ing og innkomande meldingar i ein avgrensa Canary-pilot. Ein full
N05-synkmotor og avansert media er ikkje føresetnad for den første nettbaserte
pilotprøva; lokal kø må likevel aldri kunne sende same melding to gonger.

Android-starten følgjer denne Windows-piloten. Android bruker den same
sessionkontrakten og dei same feilsituasjonane, med eigen OS-lagringsadapter.
C#-eigaren er Windows-adapteren, ikkje ein ny felles Kotlin/C#-kodebase.
Rust-/wirekontraktar blir gjenbrukte; tokeneigarskap blir ikkje kopiert inn i UI.
Recovery og generell produksjonsaktivering har framleis eigne opne portar.
