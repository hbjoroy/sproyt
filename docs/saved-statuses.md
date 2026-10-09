# Tidlegare statusar (#246)

Profil og status har ei samanbretta liste med tidlegare kombinasjonar av emoji og tekst. Eit val fyller berre ut skjemaet; brukaren må trykkje «Lagre status» for å publisere. Lista følgjer kontoen mellom nettlesarar og einingar. Mest brukte kombinasjonar kjem først, med sist brukt som sekundær sortering.

Kvar konto har høgst 20 kombinasjonar. Berre vellukka, ikkje-tomme statuslagringar blir registrerte. Teljaren tel vellukka lagringar, ikkje garantert unike brukarhandlingar: transporten har ikkje ein idempotensnøkkel for status. Tøm status beheld historikken; «Gløym status» fjernar berre eitt forslag og endrar ikkje gjeldande status. Utløpstid blir ikkje lagra i forslaget.

Historikken er privat, inngår i kontoeksport og blir kaskadesletta med kontoen. GET og DELETE `/api/v1/me/statuses` bruker den innlogga identiteten utan valfri brukar-ID. Profiloppdatering, historikk og avgrensing skjer i same transaksjon. Sletting tek same brukarlås.

Additiv migrasjon 0062 må køyrast før nye workerar. Gamle workerar toler tabellen, men registrerer ikkje historikk under blanda utrulling. Verifiser PostgreSQL-kontrakten og backup/full restore før utrulling. Ved applikasjonstilbakerulling skal tabellen stå att.

Validering: delte minne-/SQLite-/PostgreSQL-kontraktar; HTTP-eigarisolasjon, eksport og idempotent sletting; SQLite rollback, varig lagring og kontosletting; frontend-dekoding; Chromium og iPhone WebKit for gjenbruk utan publisering, rangering, reload og sletting utan profilendring.
