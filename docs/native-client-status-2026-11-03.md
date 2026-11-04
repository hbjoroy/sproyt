# Native-klienten: status 3. november 2026

Windows-klienten er ein fungerande, avgrensa Canary-pilot med ekte innlogging,
kanalar, tekstchat og autorisert medielesing. Han har eit langt mindre
funksjonsomfang enn Canary-nettappen. Den viktigaste neste leveransen er eit
produktgrunnlag og ei kontrollert utviding av daglege brukarforløp, med
funksjonslikskap målt per handling og utfall.

Denne statusen byggjer på kodekartlegging og historiske kvitteringar. Det er
ikkje køyrt nye bygg eller klientaksept som del av kartlegginga. Sjå
[funksjonsmatrisa](native-canary-parity-matrix.md),
[gjennomføringsplanen](native-canary-parity-plan.md) og
[den daterte Canary-baselinen](evidence/native-canary-baseline-2026-11-03.json).

## Kjeldegrunnlag og dagens Canary

- Lokal kjelderevisjon: `8715726e9fe11290f215a13cbc71261993942313`.
- Køyrande Canary-revisjon: `142f71888cdde11dec42a698432530e208f577bb`.
- Baselinekontrollen fann `/readyz` med HTTP 200 og Argo `Synced`/`Healthy`.
  Native-, chatagent- og kanalagentfunksjonane er aktiverte i denne Canaryen.
- Det er ingen native-kjeldediff mellom den lokale revisjonen og
  Canary-revisjonen. Det stadfestar kodeinnhaldet; historiske kvitteringar
  beviser framleis berre sine oppgitte klient-/serverkombinasjonar.
- Siste native-relaterte merge er `32186d5`, 26. oktober, PR #303.
  PR #297–303 innførte ekte innlogging/chat, retta klokkeskeivheit, la til
  kretsgrupper/media og retta scrolling og biletproporsjonar.

Noverande driftsobservasjonar er samla i baselinefila. Ingen ny native
innlogging, melding eller kontobytte mot dagens Canary er utført i denne
statuskartlegginga.

## Plattform og struktur

Klienten ligg under `native/probes/windows-reactor`. Han bruker ekte WinUI 3
med C# MVU gjennom Microsoft.UI.Reactor `0.1.0-preview.16`, Windows App SDK
`2.2.0` og .NET SDK `10.0.301`. Prosjektet har ARM64- og x64-mål og publiserer
ein sjølvstendig, upakka Windows-app. Sjå
[prosjektfila, linje 4–12 og 22–28](../native/probes/windows-reactor/Sproyt.ReactorProbe.csproj#L4)
og [SDK-låsen](../native/probes/windows-reactor/global.json).

Dette er framleis eit pilot-/prøveprosjekt. Ei endeleg Windows-produktplattform,
signert distribusjon, oppdatering og release-/tilbakeføringskontrakt er ikkje
godkjende berre fordi WinUI-piloten kan byggjast og køyrast.

Appen har separate startmodusar for lokal demo, syntetisk sessionlaboratorium
og ekte Canary. Live-modusen bruker `LiveChatComponent`, og live-opphavet er
eksplisitt avgrensa til `https://sproyt-canary.bjoroy.me/`. Sjå
[Program.cs:89](../native/probes/windows-reactor/Program.cs#L89) og
[LiveApplication.cs:82](../native/probes/windows-reactor/Core/LiveApplication.cs#L82).
Live-modusen bruker ein separat Windows-brukarprofil og kontoavgrensa lager.

Android og iOS har ikkje produktappar i kjelda. Kotlin/JVM og Swift/macOS har
bindings- og lagringsprøver mot Rust, med historisk CI-verifikasjon.
Android/ART, Compose, iOS-pakking, SwiftUI, signering og fysisk mobilbruk står
att. Gjeldande prioritering er Windows først, deretter Android på same
serverkontrakt; sjå
[implementasjonsplanen](native-client-implementation-plan.md) og
[språkprøvene](native-client-language-probes.md).

## Implementert funksjonsomfang

| Område | Kode som finst | Vesentlege hol mot full funksjonslikskap |
| --- | --- | --- |
| Identitet | Systemnettlesar, Authentik, kontostadfesting, loopback-handoff, eigen providergrant, DPAPI, refresh/revoke og omstart | Ny recovery, direkte native passnøkkel-API og samla aksept av dagens kombinasjon |
| Samtalenavigasjon | Felles, grupper per krets-ID, Direkte, kanalval og kompakt liste/detalj | Søk, favorittar, oppretting/administrasjon og dei øvrige navigasjonsflatene |
| Meldingslesing | Recent 100, innkomande meldingar, oppdateringshendingar og reconnect | Eldre historikk/paginering, varig historikkcache og full offlinesync |
| Meldingsskriving | Tekst, konto-/kanalutkast, varig outbox og same request-ID ved uviss levering | Svar/trådar, reaksjonar, redigering/sletting, omtalar og rik composer |
| Innhald | Tekst, avgrensa Sprøyt-markørar og kort med lenkje til originalmeldinga | Markdown/GFM, Mermaid og native handlingar/skjema for korta |
| Media | Autoriserte previews, native originalvising/video, framdrift og avbrot | Opplasting, deling, native filveljar, streaming og varig offline-media |
| Produktflater | Innlogging/utlogging, lyst/mørkt tema, samtaleliste og chat | Innstillingar, profil, agent-/minneflater, arbeids-/prosessflater og administrasjon |

Matrisa som er lenkja øvst, er arbeidslista for detaljert paritet. Til dømes
er eit oppgåvekort som opnar nettappen registrert som fallback, ikkje som ei
ferdig native oppgåvehandling.

Kjeldepunkt som avgrensar omfanget:

- [NativeChatClient.cs:94](../native/session-client/NativeChatClient.cs#L94):
  oppstart med `hello`, kretsar og kanalar; linje 153–165 har subscribe,
  recent-100 og `send_message` med `parent_message_id=null`.
- [NativeChatClient.cs:5](../native/session-client/NativeChatClient.cs#L5):
  den enkle meldingsmodellen har ingen tråd-, reaksjons- eller lesestatusmodell.
  Linje 278–285 handsamar historikk/oppdateringar og avgrensa reload.
- [LiveChatComponent.cs:99](../native/probes/windows-reactor/LiveChatComponent.cs#L99):
  live-presentasjonen har innlogging/tema, kanalrader, composer og sending.
- [NativeMessageContent.cs:29](../native/session-client/NativeMessageContent.cs#L29):
  avgrensa innhaldsparser; diagram og avanserte kort får fallback.
- [NativeMessageView.cs:56](../native/probes/windows-reactor/NativeMessageView.cs#L56):
  tekst blir native `TextBlock`; kort kan opne den aktuelle meldinga i nettappen.
- [NativeMediaLibrary.cs:45](../native/probes/windows-reactor/Core/NativeMediaLibrary.cs#L45):
  512 MiB originalbudsjett, éi originalhenting, åtte preview-leasar og to pipelines.

## Server- og lagringsgrenser

Native-sessionrutene finst i
[src/web/native.rs:18](../src/web/native.rs#L18), med additive tabellar i
migrasjon 0068. Native bearer er kopla til sessionkanten, native chattransport
og lesande media. Den generelle HTTP-hjelparen i
[src/web/http.rs:14](../src/web/http.rs#L14) autentiserer framleis med
eksisterande participant/cookie. Medielesing har ein eigen native-adapter i
[src/web/media.rs:29](../src/web/media.rs#L29); opplasting på linje 109–116
bruker den vanlege HTTP-hjelparen. Nye native HTTP-funksjonar må få eksplisitt
autoritet og kontrakttestar.

Live-historikken er ei minneprojeksjon som lastar recent-100 ved kanalval og
reparasjon. Rust eig lokal SQLite for prøvekjernen, utkast og outbox. Det
lageret har framleis prøvegrenser på åtte element og 1024 UTF-8-byte per tekst;
sjå [store.rs:6](../native/probes/client-core/src/store.rs#L6).
Det er ikkje eit produksjonslager for full historikk.

To konkrete grunnlagsproblem må få eigne arbeidspakkar:

1. **Definitivt avslag og uviss levering.**
   [NativeChatClient.cs:216](../native/session-client/NativeChatClient.cs#L216)
   kastar ei generell feil ved WS-serveravslag.
   [LiveApplication.cs:256](../native/probes/windows-reactor/Core/LiveApplication.cs#L256)
   klassifiserer alle leveringsfeil som `uncertain`. Rust har `rejected`, og
   pumpa hoppar over denne tilstanden, men ingen live-veg som skriv han vart
   funnen. Dette er eit kjeldefunn som treng ein målretta regresjonstest for
   validerings-/tilgangsavslag før fleire sendingstypar blir lagde til.
2. **Varig utkast-revisjon.** Lageret fjernar utkast etter kanal og tekst,
   utan ei varig forventa revisjon; sjå
   [store.rs:68](../native/probes/client-core/src/store.rs#L68) og linje 174.
   Dokumentert reparasjon av eit nyare utkast med identisk tekst toler
   ordinær lukking, men hard prosessdød mellom fjerning og reparasjon er
   ikkje atomisk sikra. Sjå
   [Windows-prøva:115](native-client-windows-probe.md#L115).

Full N05/N06-synkronisering har framleis eigne portar for journalrekkjefølgje,
reset, sletting, tilgangsendring, snapshot/delta, krasj og migrering. Ei
nettbasert UI-leveranse kan avgrensast utan full offlinesync, men må beskrive
dei faktiske grensene og bevare trygge utkast-/leveringsutfall.

## Historiske kvitteringar

Desse lokale filene er observerte historiske bevis, ikkje nye testresultat.
Stiane er skrivne ut slik at dei kan finnast på denne arbeidsmaskina.

| Kvittering | Revisjon og omfang | Praktisk grense |
| --- | --- | --- |
| `S:/Build/sproyt-native-session/real-canary-ui-receipt.json` | 26. oktober 05:07 UTC; klient `becadda43`, server `c0fc8efd`; ekte Edge-innlogging, loopback, kanalar/historikk, navigasjon, utkast og refresh generasjon 0→1 | `actualMessageSent=false`; inga aksept av ekte sending/tapt ACK |
| `S:/Build/sproyt-native-session/visible-previews/real-acceptance.json` | Klient `a2709c62`, merge `26b7519` (#302); ekte previews, originaldialog og eldre bilete etter scrolling | Null posta meldingar; beviser den oppgitte eldre pakken |
| `S:/Build/sproyt-native-session/visible-previews/ci-passed/b338cc349b814586aef2d97dc9c1af57/receipt.json` | Rein revisjon `c2ee6463`, x64; core, WinUI, omstart, session- og mediefixtures | Syntetisk transport/identitet; ingen fysisk eller ekte Canary-aksept |
| `S:/Build/sproyt-native-session/image-proportions/ui-cee03cd8519d427882c871bb8b39be43/live-ui-result.json` | WinUI-fixture med 24 bilete, proporsjonar, originalvising og responsiv breidd | `actualTransport=false`, `productionAuthentication=false`; denne resultatfila åleine bind ikkje resultatet til ein bestemt revisjon |
| `S:/Build/sproyt-reactor/runs/6c0d5ae8b9c84bb2b97325f04f7d6492/receipt.json` | Rein revisjon `1880372d`, lokal ARM64, 200 % skalering; core, UI, omstart og syntetisk session | Eldre enn #299–303; ingen ekte transportaksept |

Det finst òg historiske rolloutkvitteringar for #297 og #299. Dagens
servertilstand skal lesast frå den daterte baselinen, ikkje utleiast av desse.

## Attståande verifisering

- Same testa klientpakke mot dagens Canary: innlogging, omstart, refresh,
  revoke, konto-/kanalbyte og isolering av utkast/ventande sendingar.
- Ekte sending og mottak, brot før/etter servercommit, tapt ACK og retry med
  same ID; definitivt avslag skal få korrekt synleg og varig utfall.
- Eldre historikk, store samtalar, dei nye native funksjonsflatene og tilgang
  som blir endra medan appen lever.
- Fysisk tastatur/IME, touch og skjermlesar. Historisk OS-injisert input og
  interne kontrolltreprøver er avgrensa bevis for desse brukarforløpa.
- Signert pakke, installasjon/oppdatering/tilbakeføring, skjemaoppgradering,
  full disk og dei aktuelle krasjgrensene når klienten blir produktisert.
- Plattformportar på Android/ART og iOS når desse klientane faktisk blir bygde.

Relevante eksisterande kommandoar for målretta verifisering ved kodeendring:

```powershell
./native/probes/run-windows-reactor.ps1 -ArtifactsDirectory S:/Build/sproyt-reactor -UiSmoke
./native/probes/run-identity-probe.ps1 -ArtifactsDirectory S:/Build/sproyt-native-identity
cargo test --manifest-path native/probes/client-core/Cargo.toml --locked
cargo test native_sessions::tests --locked
```

PostgreSQL-samtidigheit har eit eksplisitt CI-steg i
[ci.yml:320](../.github/workflows/ci.yml#L320). WinUI x64 har ein eigen jobb i
[native-probe.yml:159](../.github/workflows/native-probe.yml#L159).
Ei grøn syntetisk prøve blir ikkje rekna som ei gjennomført ekte Canary-handling.

## Dokumentasjon som må tolkast med dato

Dei eldre plan- og laboratoriedokumenta har no fått lenkjer til denne
statusen. Avsnitt som legg ekte innlogging/servertransport fram i tid,
skildrar labomfanget på skrivetidspunktet. [Windows Canary-beskrivinga](native-windows-canary.md)
samsvarar betre med dagens implementerte pilot. Denne statusen,
funksjonsmatrisa og gjennomføringsplanen gir den daterte inngangen til vidare
arbeid; eldre prøvekvitteringar skal bevarast med sitt opphavlege omfang.
