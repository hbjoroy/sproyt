# Frå tilbakemelding i Felles til avslutta GitHub-sak

## Mål og brukarforløp

Ein deltakar i **Felles → Tilbakemeldingar Sprøyt** skal kunne melde eit konkret behov med éi vanleg melding. Etter publisering får meldinga ein reaksjon og eit trådsvar med lenkje. Under **Fleire meldingsval → GitHub-sak** kan deltakaren sjå den publiserte teksten, siste kjende GitHub-status og steget i Heart. Menyen verkar både på kjeldemeldinga og på agenten si kvittering.

| Kva kjem inn | Kva agenten gjer | Kva avsendaren ser |
| --- | --- | --- |
| Prat utan eit konkret produktbehov | Avsluttar etter første modellvurdering | Ingen kvittering |
| Ny brukarhistorie | Opprettar ei sak i `hbjoroy/sproyt` med behovsorientert tittel, brukarhistorie og akseptansekriterium | Reaksjon, trådsvar og saksvisning |
| Ny opplysning om ei open sak i Backlog eller Todo | Les sak og eksisterande kommentarar. Formulerer kva den nye meldinga konkret tilfører | Reaksjon og lenkje til same sak |
| Heile innhaldet er allereie dekt | Knyter meldinga til saka utan ein ny GitHub-kommentar | Kvittering om at innhaldet er dekt |
| Fleire moglege treff, arbeid i gang eller uavklart status | Lagrar meldinga til manuell avklaring | Ingen kvittering om publisering |

Kjeldemeldinga sitert i bestillinga er eit døme på eit konkret produktbehov. Ved aktivering blir startpunktet sett til siste tildelte meldingssekvens. Det gir eit skilje mellom dømet som alt ligg i kanalen og meldingar som blir publiserte etter aktivering.

## Fire arbeidspakkar og akseptanse

1. **Vurdering og GitHub.** Gjenbruk standardagenten sin vLLM-klient og den repoavgrensa GitHub App-installasjonen. Første modellkall vurderer om meldinga kvalifiserer. Ved kvalifisering samanliknar eit nytt kall behovet med opne saker. Ved treff les agenten kommentarane før han formulerer eit tillegg. Kandidatnummer, tekstgrenser og publiseringsmål blir kontrollerte på tenarsida. Ei ufullstendig samanlikning går til avklaring.
2. **Kanal og varig kvittering.** Slepp berre gjennom nye menneskeskrivne rotmeldingar i den uttrykkeleg bundne Felles-kanalen. Sjekk kjeldetekst, agenttilgang, produktansvar og GitHub-binding på nytt før publisering. Lagra føremålet med skrivinga før nettverkskallet. Ved tapt svar må rett bot, repo, tekst og markør kunne stadfestast før arbeidaren kvitterer. Reaksjon og trådsvar får éi felles transaksjonskvittering.
3. **Heart og produkteigar.** Ei ny sak startar som GitHub **Backlog**, vist som «Føreslått». Kvar ny sak får éin Heart-instans og ei vurderingsoppgåve tildelt produkteigaren. Eit utfylt vurderingsfelt og «Utført» registrerer vurderinga. Heart går vidare når kvitteringa samsvarar med oppgåva. Eit ferskt GitHub-oppslag etter vurderinga kan sende ventesteget til **Løyst** (`completed`) eller **Avvist** (`not_planned`). Ei anna eller ukjend sluttårsak står til avklaring. Ei gjenopning før stadfesta lukking held prosessen open. Eit tillegg til ei sak som denne agenten har oppretta, viser den same prosessen.
4. **Brukarflate og drift.** Prøv innsyn via kjeldemelding og kvittering, produkteigarrettar, begge sluttgreiner, tidleg lukking, gjenopning, avbrot mellom skriving og svar og to arbeidarar som prøver å kvittere samtidig. Kontroller status med GitHub-lesing, og prøv SQLite, PostgreSQL og mobilvisning. Eit delt bygg eller testbevis skal publiserast i `oci.bjoroy.me` med identifiserbar revisjon og digest. Phoenix kan køyre bygg ved behov.

## Ansvar og tilstand

Sprøyt eig kanaltilgang, den bearbeidde publikasjonen og koplinga mellom melding og sak. GitHub eig saksnummer, arbeidsstatus og sluttårsak. Heart eig oppgåvetildeling og overgangar. Santorini har berre tekstdata og avgjerdsalternativ. Meldingar og GitHub-tekst kan ikkje velje mottakar, repo eller API-kall. Denne flyten brukar eksisterande Sprøyt-prosess og database med eigne jobbar og ein versjonert Heart-definisjon.

For dette repoet gir GitHub sitt `status.category` operativt grunnlag for matching. `backlog` og `planned` er aktuelle, `started` er under arbeid. Oppretting ber om `Backlog`. GitHub opnar for fleire lukkegrunnar enn dei to som er avtalte i første versjon. [GitHub sine planleggingsstatusar](https://docs.github.com/en/rest/repos/planning-statuses) og [GitHub sine statushendingar](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/issue-status-webhooks-and-workflows).

## Aktivering og observasjon

1. Bygg og verifiser den aktuelle Sprøyt-revisjonen. Bruk lokalt bygg eller Phoenix og OCI for delte artifaktar. Den eksisterande generelle CI-flyten brukar GitHub Actions-artifaktar. Ho må få eit kompatibelt leveranseløp før ho kan nyttast med denne avgrensinga.
2. Rull ut migreringane 0080 og 0081 og same app-/chartrevisjon i vald installasjon. Set `config.feedbackEnabled: true` i éi installasjon med tilgang til standard vLLM, eksisterande GitHub App og den varige Heart-databasen. Kontroller Heart-rutinga ved delt appdatabase. [Heart i clusteret](heart-cluster.md).
3. Bruk operatørskriptet mot Sprøyt-databasen. Standardkøyringa prøver binding, rettar og agentoppretting i ein transaksjon og rullar tilbake. Med `apply=on` blir agent og kanalbinding lagra. Ved første aktivering byrjar arbeidaren ved neste melding. [Operatørskriptet](../tools/configure-felles-feedback.sql).

```sh
psql -X -f tools/configure-felles-feedback.sql
psql -X -v apply=on -f tools/configure-felles-feedback.sql
psql -X -v stop=on -v apply=on -f tools/configure-felles-feedback.sql
```

4. Status blir normalt henta om lag kvart andre minutt så lenge ei sak ventar. For raskare oppdatering kan GitHub App sende `issues`-hendingar til `/api/v1/integrations/github/feedback` på den valde installasjonen. Set `github.feedbackWebhookSecret` til namnet på eit Kubernetes-Secret med nøkkelen `webhook-secret`, og bruk same hemmelegheit i GitHub. Etter kontroll av HMAC-signaturen vekker ei statusendring, lukking eller gjenopning lesinga for rett installasjon, repo og sak. Eit nytt API-oppslag stadfestar tilstanden før Heart får sluttårsaka. Periodisk avstemming fangar opp forseinka eller tapte hendingar.
5. I første levande prøve, følg éi medviten tilbakemelding frå den rette kanalen, kontroller den faktiske publiserte teksten, gjer ei produkteigarvurdering og stadfest sluttgreina når saka blir lukka.

Ein operatør kan telje `feedback_jobs` gruppert på `status` og `feedback_heart` gruppert på `stage`. `needs_review` betyr at avgjerda eller kjeldene krev avklaring. Ved avklara grunnlag kan ein operatør sende éin slik jobb til ny vurdering ved å setje `status='pending', attempts=0, available_at=0` berre dersom han manglar stadfesta `issue_id` og har ledig lease. Ei lagra `issue_id` krev avstemming av den eksisterande saka. `uncertain` betyr at GitHub kan ha akseptert ei skriving. Kontroller markør og innhald i det pinna repoet før ei manuell retting. Arbeidaren held seinare publiseringar til same repo tilbake medan utfallet er uvisst.

## Verifisert 25. november 2026

Lesande kontroll fann éin synleg kanal med dette namnet i Felles, aktiv appbinding til rett repo og ein produkteigar med kanal-, vurderings- og eksportrett. Av sju opne saker hadde tre GitHub-statusen Backlog. Dei fire andre mangla status på kontrolltidspunktet.

På den konfigurerte Santorini-tenesta avviste standardmodellen ei helsing, kvalifiserte eit ønske om kanaldemping og trekte ut eit nytt tidskrav frå ei oppfølging. Testdata var syntetiske. Ei eiga integrasjonsprøve med simulerte leverandørsvar gjekk gjennom mottak, publisering, tapte svar, avstemming og trådkvittering. Signaturkontrollen og oppvekking av rett sak er prøvde lokalt.

Begge sluttgreinene gjekk gjennom reell, isolert Heart v2. Kanalopptak, reaksjon og trådsvar vart prøvde på SQLite og PostgreSQL 17. Operatørskriptet vart prøvd med tørrkøyring, start, gjenteken start, stopp og gjenopptak. Saksvisning og ny innsending etter tapt svar vart prøvde i Chromium og WebKit med simulert saks-API. Produksjonsprøva i steg 5 høyrer til aktiveringa.
