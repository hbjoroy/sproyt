# Sjøtemperatur – utrulling og tilbakeføring

Oppfølging av #243, 12.–13. oktober 2026.

## Kontrakt

Ferdigheita er uavhengig av luftvêr. Eigar/moderator vel offentlege
stad-ID-ar og standardstad i den eksisterande agentdialogen. Agenten kan
slå opp høgst to av sine tillatne stader i éin native tool-call. Alle kall
blir validerte før første eksterne oppslag. Tenaren skriv marinefakta frå
normaliserte kjeldedata; modellen får ikkje skrive om temperaturane.
Kjelde, stad, tidspunkt og regional modellstatus følgjer svaret.
Sjå `sea-temperature-sources.md` for levande dekningstest og kjeldevilkår.

## Utrullingsrekkjefølgje

1. CI kontrollerer Rust/Clippy, PostgreSQL, Chromium/WebKit, backup/restore
   og den publiserte ARM64-revisjonen. PostgreSQL-testen ventar på den faktiske
   publiseringslåsen til marinefristen er utgått; inga melding/receipt kan kome ut.
2. GitOps tek eit nytt uforanderleg backup-checkpoint med full gjenoppretting
   og SHA256, før den additive migrasjonen 66. Nye kolonnar er nullable.
3. Prod og canary får identisk kompatibel webkode med
   `seaTemperatureAgentsEnabled: false`. Media-worker og Heart er uendra.
4. Kontroller alle tre delte databasearbeidarane før GitOps slår på flagget
   i begge miljøa. Eldre arbeidarar må vere borte før agentkonfig blir aktivert.
5. Konfigurer pilot/ordinære agentar først etter barrieren; registrer revisjon,
   kjelde-/modellprøver og faktisk utrullingsstatus i release-kvitteringa.

Prod og canary deler chatdatabase og agentjobbar. Canary er ikkje eit isolert
miljø for denne funksjonen. Eksisterande HTTPS-egress dekkjer dei faste
marineendepunkta; ingen nye tenester, nøkkel eller cache-database trengst.

## Tilbakeføring

Slå av sjøtemperatur på agenten og auk konfigrevisjonen. Slå av globalflagget
i begge miljøa og drener/avvis marinejobbar før eldre webkode kan rullast tilbake.
Inga skjema-/meldingsnedmigrering: migrasjon 66 og alle brukarmeldingar blir
bevarte. Gamle marine-snapshot får ikkje ny frist; publisering kontrollerer
DB-klokka igjen etter venting på låsar.

## Kontrollgrunnlag

Lokalt: 105 chatbot-testar før siste marine-followup/tool-regresjon,
116 frontend-grensetestar, TypeScript/build og Clippy. CI og levande
release-kvittering avgjer endeleg produksjonsstatus; lokal grønt er ikkje
bevis på at ei bestemt revisjon køyrer i clusteret.
