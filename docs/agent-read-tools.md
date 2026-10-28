# Modellvalde leseverktøy

Kretsagentar får native OpenAI-compatible `tools` frå Sprøyt når dei allereie
har den aktuelle capabilityen. Agentnamn er ikkje ein tilgangsregel. Den
installerte Qwen-modellen på Santorini er kontrollert for native tool calls.

- `lookup_weather({location})`: eit eksplisitt stadnamn, høgst 80 teikn.
  Weather-Service resolver namnet; Sprøyt bind forecast til dei returnerte
  koordinatane. Resultatet namngir stad/region/land og skil requested/resolved
  frå konfigurert standardstad. URL, IP-lokasjon og modellgjetta GPS er avviste.
  Uttrykkelege Paros-oppslag må resolve til Hellas innanfor øyområdet.
  Aliki/Alyki på Paros brukar eit tenarstyrt, provider-verifisert lokalitetspunkt
  fordi namnesøket elles kan velje fastlandet. `lookup_basis` dokumenterer dette;
  svaret skal framleis namngje den faktisk resolverte staden.
- `ferry_calls({vessel?})`: alle normaliserte planlagde anløp i dagens tabell
  for Parikia, eventuelt filtrerte på eit fartøysnamn. Frå-/tilhamn, operatør,
  rute, planlagd ankomst/avgang og kjeldedato blir bevarte. Filteret er
  case-insensitivt delnamn, ikkje ei garanti for fuzzy-/forkortingsmatching.
- `vessel_observations({})`: same autoriserte, ferske AIS-utval som blei
  innhenta for denne meldinga. Ufullstendig dekning og posisjonsobservasjon er
  ikkje stadfesta kaiankomst, kansellering, forseinking eller live ETA.
- `find_bars(..., choose_random?)`: utfører eit Google Maps-søk og publiserer
  eit felles svar med kjelder og lenkjer i kanalen. Plan og rårespons blir ikkje
  lagra eller sendt til ein ny modellrunde; berre det ferdige svaret blir lagra.
  Tilgang, posisjonssamtykke, avgrensingar og nødvendige utrullingssteg er
  skildra i [agent-bar-search.md](agent-bar-search.md).

Modellen kan velje eitt kall per verktøy, høgst tre i éin batch. Heile batchen
blir validert før oppslag; ukjende felt, namn, duplikat eller ulovlege argument
blir avviste. Ingen rekursiv løkke. Deretter blir resultat sende som `tool`
messages og modellen lagar svaret utan nye verktøy. Vanleg prat utan oppslag
treng berre første modellrunde. Høgst 140 sekund gjeld for heile sekvensen,
innanfor det eksisterande globale 180-sekunds model-permitet.

Sluttrunda sender framleis definisjonane med `tool_choice: "none"`; tom
`tool_calls: []` med tekst er eit gyldig sluttsvar. Qwen-malen krev systemmeldinga
først, så det blir ikkje sett inn nye systemmeldingar mellom verktøy og svar.
Sluttrunda har 55 sekund for ei større dagsoversikt; vanleg modellkall har 35.

Tenarstyrte adapterar har faste base-URL-ar, timeout, storleiksgrense og nekta
redirect. Verktøysvar over 64 KiB blir eksplisitt utilgjengelege, ikkje stille
avkorta. Kjeldefeil gir aldri standardvêr som erstatning for etterspurd stad.
Om modellen såg både standardvêr og eit nytt stadoppslag, blir begge lagra på
jobben med den kortaste gyldigheitsfristen. Fergeutvalet er dokumentert på det
opphavlege snapshotet. Eksisterande lease-, konfigurasjons-, kanal- og
medlemskapsgjerde gjeld framleis før publisering. Ingen ny migrasjon er naudsynt.

`cargo test chatbot::tools` kontrollerer native call/result/reply-protokollen,
begge agentnamn, full rutefakta og ugyldige batchar. Den eksplisitt ignorerte
`live_model_and_sources_complete_tool_round` kan køyrast med lokale tenestetunnelar
og credentials i miljøet; han postar ingen melding og endrar ingen konfigurasjon.
Denne prøva passerte 9. oktober mot faktisk Qwen og begge cluster-tenestene:
Fogd valde heile dagslista, Maria valde Bergen, og det resolverte landet var Noreg.

## Terningkast

`roll_die({sides, count?})` kastar 1–10 uavhengige terningar; standard `count` er 1.
To separate `roll_die`-kall i same batch er også lov. Andre verktøy kan framleis
berre kallast éin gong per batch. Terningkast brukar operativsystemet si ekte
tilfeldigheit; sekundet i klokka skal ikkje brukast som seed eller som terning.

Eit reint kast blir formatert frå dei faktiske verdiane på tenaren, utan ny
modellrunde. Tal, sum og ein avgrensa `dice`-kodeblokk blir lagra som éi vanleg
melding gjennom den eksisterande idempotente jobbflyten. React viser kodeblokka
som lokale 3D-PNG-sprites; eldre klientar får tal og sum som lesbar tekst.
Spriteval skjer direkte frå dei lagra terningverdiane, ikkje frå ein animasjon.

`tools/render-dice-sprites.py` gjenskaper dei 12 PNG-filene med Blender 5:
seks valørar i to vinklar, 256×256 RGBA, transparente utanfor terningen.
Motståande sider summerer til sju. V1-filene blir serverte frå ei fast allowlist
på `/assets/dice/v1/`, med lang cachetid; endra grafikk må få eit nytt URL-versjonsnummer.
