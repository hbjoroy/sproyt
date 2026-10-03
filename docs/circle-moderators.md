# Kretsmoderatorar

Kretseigaren kan gje eller fjerne moderatorrolla frå eit eksisterande menneskeleg medlem via kretsmenyen «Medlemmer og roller». Eigarar og agentkontoar kan ikkje endrast gjennom denne handlinga. Rolleendringa blir lagra og auditert med aktør, krets, medlem og ny rolle. Kretsinvalidering oppdaterer rolla hos tilkopla klientar; ny tilkopling les den lagra rolla.

Ein kretsmoderator kan konfigurere kretsens samtaleagentar og slette andre sine meldingar i opne kretskanalar (Public og Local/Prat) der moderator faktisk er kanalmedlem med skriverett. Kretsrolla gir ingen ekstra lesing eller sletting i private kanalar, og blir aldri kopiert til kanalrollene. Kanalens eksisterande eigar-/moderatorrettar og retten til å redigere/slette eigne meldingar gjeld framleis. Andre sine meldingar kan aldri redigerast.

Moderatorar kan opprette kanalar og forlate kretsen som vanlege medlemmer. Dei kan ikkje endre kretsnamnet, slette kretsen, oppnemne moderatorar eller administrere generiske MCP-agentgrants i kraft av kretsrolla.

Samtaleagentar med provider `sproyt-circle-chat` blir berre styrte gjennom gjeldande kretsrettar. Dei får ingen generiske API-nøklar og kan ikkje opprettast, få grants, rotere credentials, tilbakekallast eller få eigarskapsbasert provenance-godkjenning gjennom MCP-agent-API-et. Slik behaldar ikkje ein avsett opprettar personleg tilgang til ein kretsbot. Kretsbotens opprettar blir framleis bevart som historisk `owner_id`/`created_by`.

PostgreSQL låser aktuelt kretsmedlemskap før kanalmedlemskap ved moderert sletting, i same rekkjefølgje som kretsutmelding. Gjeldande tilgangspredicate blir vurdert etter låsane. Agentmutasjonar låser gjeldande owner/moderator-medlemskap i same transaksjon; agentlista har tilgangspredicate i sjølve dataspørringa.

Migrasjon 0050 utvidar PostgreSQL CHECK-constrainten og byggjer SQLite-medlemskapstabellen på nytt med same nøklar, FK, joined_at og indeks. Eksisterande rader blir kopierte før innmeldingstriggeren blir gjenoppretta, slik at migrasjonen ikkje produserer falske innmeldingshendingar. Ho må leverast saman med migrasjon 0049 frå emoji-batchen og ein ny verifisert releasebackupgate. Heart-data og -image blir ikkje endra.

Validering: delt repositorykontrakt i minne/SQLite/PostgreSQL; SQLite migrasjon med eksisterande rader og audit; PostgreSQL tilbakekalling medan sletting ventar på medlemskapslås; SQLite/PostgreSQL moderatoroppretta bot → avsett → avvist config og generiske eigarruter; uendra vanleg MCP-credentiallivssyklus; desktop/mobil i Chromium og WebKit.
