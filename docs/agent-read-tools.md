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
