# Vêragent (#212)

Sprøyt hentar vêrdata serverinternt frå Weather-Service og brukar Santorini vLLM til å formulere svaret. Dette er ei avgrensa lesande funksjon, ikkje eit generelt verktøyrammeverk. Modellen vel aldri URL, autentisering eller kanal.

## Bruk og avgrensing

Under kretsmenyen → Agentar kan ein eigar/moderator velje **Vêrdata**. Stadnamn og koordinatar er eksplisitte; nye oppsett foreslår Parikia (37.085,25.148). Sprøyt les ikkje GPS. Svaret skal namngje staden. Eit spørsmål om ein annan stad endrar ikkje koordinatane: modellen skal forklare avgrensinga. Administrator må førebels endre oppsettet for ein annan stad.

Triggerord startar samtalen. Oppfølging utan trigger krev spørsmålsteikn eller eit avgrensa vêruttrykk (UV, lufttrykk, trykk, vind, vêr/vær, weather, pressure, rain). Berre same menneske, agent, kanal og identiske tråd-ID kan følgje opp innan 20 minutt etter eit faktisk publisert svar. «Skål!» får ikkje automatisk eit vêrsvar. Agentmeldingar triggar aldri nye jobbar. Vanlege samtaleagentar får ikkje denne oppførselen.

Publiseringsbevis kjem frå den atomiske `command_receipts → messages`-koplinga, ikkje berre workerstatus. Dermed fungerer oppfølging også ved krasj etter meldingens commit, før workerens `finish()`. Den valde ankermeldinga blir lagra på jobben; retry vel aldri eit nytt anker. Endra konfig/kanaltilgang, sletta/redigert anker eller opphavleg menneskemelding og utgått vindauge stoppar jobben. PostgreSQL låser kanal og relevante meldingar i stabil ID-rekkjefølgje før endeleg autorisasjon; tidssjekken bruker fersk klokke etter låseventing.

## Vêrkontrakt

Serveren gjer høgst to samtidige GET-kall: `/current` og `/forecast`, koordinatar som `location`, tre dagar, timeprognose på, AQI/alerts av. Ingen brukarstyrt baseadresse. Kvar respons har 256 KiB grense, 12 sekund timeout og ingen redirect. Stadkoordinatar og tidssone må samsvare mellom observasjon og prognose.

Adapteren bevarer UTC-epoch og oppgitt tidssone. Observasjonstid er ikkje prognosens utgjevingstid. Aktuell observasjon må vere høgst to timar gammal og ikkje meir enn fem minutt fram i tid. Timar blir avgrensa til neste 72 timar (prognosens tre lokale dagar kan gi kortare dekning). Time-UV og trykkfelt må finnast i tenestekontrakten. Manglande/ugyldige tal er `null`, aldri nulltal som late som observasjonar. Trykk er hPa; serveren reknar endring frå aktuell observasjon. UV, temperatur, vind og regnsannsyn har eksplisitte einingar.

Vêrsnapshotet og generert svar blir lagra på eksisterande jobb og kan brukast i maksimalt fem minutt etter henting. Publisering kontrollerer utløpet på nytt, også for eit lagra svar. Tenestefeil blir prøvde avgrensa på nytt med eksisterande jobblease; ugyldig eller forelda data feilar utan oppdikta standardsvar. Det blir førebels ikkje publisert ei eiga feilmelding i kanalen. Loggar inneheld feilkode, ikkje samtaletekst, koordinatar eller leverandørnøkkel.

## Lagring og bakoverkompatibilitet

0052 legg til nullable `circle_chat_agents.weather` (JSON-tekst) og jobbkolonnane `followup_anchor_message_id`, `weather_snapshot`, `weather_valid_until`. Eksisterande agentar er vanlege samtaleagentar. Same eigar-/moderatorrettar, revisjonar, privatkanalval og lease-fencing gjeld. API bruker `weather: {location,latitude,longitude}` eller `null`; utelate felt på PATCH bevarer eksisterande vêrkonfig, eksplisitt `null` fjernar ho. Konfigendring aukar agentrevisjonen.

## Utrulling og faktisk status

Weather-Service PR1 er merga på `b8a714002897fb0a2ce5add9f63cd081a969c499` og kontrakt-CI er grøn, men eksisterande live-versjon 1.0.26 manglar UV/trykk i timeprognosen. Tenesta er manuelt Helm-forvalta og har ikkje ein imagepubliserings-workflow, registry-secret eller Actions-environment. Dette er eit konkret aktiveringshinder; #212 er ikkje ferdig eller produksjonsaktivert.

GitOps PR203 gir snever egress frå berre Sprøyt-app-podar til Weather-Service-podar i namespace weather, TCP8080. Intern Service-base er `http://weather-stack-service.weather.svc.cluster.local/` (Serviceport80, podport8080). Heart og backup blir ikkje valde.

Før aktivering:

1. Etabler ein avtalt CI/CD-veg for Weather-Service, publiser/deploy den merga kontrakten og verifiser UV/trykk/stad/tid med ei avgrensa live-lesing.
2. Rull ut Sprøyt 0052 og kompatible workerar til både prod og canary med `config.weatherAgentsEnabled: false`. Begge deler DB; gamle workerar må vere drenerte før eit aktivt vêroppsett kan lagrast.
3. Set `config.weatherUrl` til intern Service-base og merge den snevre egress-policyen. Verifiser faktisk HTTP frå appens nettverk.
4. Aktiver `config.weatherAgentsEnabled: true` i eit separat GitOps-steg. Denne er ein **aktiveringsgate for konfigmutasjon**, ikkje ein brytar for eksisterande workerar. Workerar handhevar lagra konfig/tilgang uavhengig av gate. AV åleine gjer ikkje rollback til gammal worker trygg. Slå av den aktuelle agenten (aukar revisjon) for å stoppe svar; URL/manglande adapter stoppar også ny generering.
5. Prøv eitt faktisk Santorini-svar og ei oppfølging i avtalt testkanal før #212 blir lukka. Ingen agent blir automatisk oppretta eller aktivert av migrasjonen.

API viser `weather_available` for aktiveringsgaten og `worker_available` per agent. Utilgjengeleg vêragent kan lagrast deaktivert; grensesnittet forklarar kvifor han ikkje kan aktiverast.

## Verifikasjon

SQLite/PostgreSQL-kontraktprøver dekker CRUD, committed-receipt-krasjgap, menneske/tråd-isolasjon, anchor-delete, revisjonsendringar og snapshotutløp. HTTP-adapterprøver dekker faste ruter, parameter, redirect og storleiksgrense. Frontend dekker gamle API-svar, rundtur av stad/koordinatar, ugyldig oppsett, utkast ved feil og utilgjengeleg teneste i Chromium og WebKit.
