# Driftsroller og avgjerd for #354

19. november 2026. Plattform/drift og varsling held push og dei andre innebygde arbeidarane i API-prosessen. Ved oppdatering kan Heart ta ein pause medan chat tek imot meldingar. Mediedekoding har allereie eigen prosess. Vi held på desse grensene og dei eksisterande standardinnstillingane.

## Kva køyrer kvar?

| Rolle | Ansvar og avhengnader | Oppstart, avslutting og helse | Ressursbudsjett |
| --- | --- | --- | --- |
| API | HTTP, WS, MCP, chataktør, tilgang og repository. PostgreSQL og ein eigen realtime-lyttar per replika. | Initialiserer repository og konfigurerte tillegg før HTTP bind. /healthz og /readyz blir prøvde av Kubernetes. Ved terminering blir shutdown varsla og HTTP drenert. | Historisk normalbudsjett prod 2 × (4 poolsamband + 1 lyttar), canary 1 × (2 + 1). Chart-tak 500m CPU og 512 MiB per pod. |
| Innebygde arbeidarar | Push/outbox, bilete, emne, agentdialog/minne, prosesspilot, arbeidsoppgåver og medieopprydding. Nyttar API-poolen og eventuelt push-, modell- eller Heart-tenesta. | Startar frå [serveroppstarten](../src/server.rs) etter konfigurasjon og får shutdown-varsel frå API-et. Kvar API-replika kan starte eigne løkker. Kontroller køalder, feil, retry og framdrift for den aktuelle funksjonen. API-readiness åleine seier lite om levering. | Innan API-poden sitt felles CPU-, minne- og pooltak. Ingen eigen reserve per løkke. |
| Mediearbeidar | Dekodar varige førehandsvisingsjobbar med eigen codec-prosess og mellombels lagring. | Eiga valfri Deployment, éin arbeidar, Recreate og 30 sekund termineringsfrist. Følg jobbtilstand og alder. Ved avbrot avgrensar arbeidaren codec-køyringa og publiserer gjennom MediaStore. | Pool 2. Chart-tak 500m og 512 MiB. |
| Heart | Privat teneste for prosessutføring. Sprøyt leverer frå varig outbox og brukar stabil startnøkkel. Heart har eiga databaserolle. | Eiga Deployment med /health og /ready, styrt av heart.replicaCount uavhengig av API-et sitt replicaCount. Recreate og sekvensiell bootstrap avgrensar overlapp. | 500m og 256 MiB per runtime-pod. Prod 2 × 2, canary 1 × 2 poolsamband. To vedlikehaldssamband fyller rolletaket 8. Sjå [utrullingsbudsjettet](heart-rollout-budget.md) før endring. |

## Kvifor push blir verande

I [den isolerte kapasitetsprøva](database-capacity-assessment-2026-11-10.md) heldt alle 18 prøvene ved 80 meldingar/s måla, også med éin emnearbeidar og fire rolege kanalar. Ved 160/s bestod seks av seks prøver med varmkanal. Ved 320/s nådde generatoren grensa for samtidige sendingar, og ventinga i chataktøren steig. Målingane identifiserte ingen push-flaskehals. Push var ikkje aktiv i prøva. Ei eiga pushrolle ville krevje eige poolbudsjett og ein overgang mellom konsumentar som deler outbox.

[Pushleveringa](../src/notification.rs) brukar no forsøksnummeret som eigarmerke. Claim returnerer nummeret frå sjølve oppdateringa. Delivered, Retry og Expired må endre den aktuelle, framleis leasa rada før dei får oppdatere eller slette abonnementet i same transaksjon. Etter overtaking kan ein sein arbeidar dermed ikkje endre den nye eigaren sitt arbeid. Ved krasj kan neste arbeidar ta over når den 30 sekund lange leasen har gått ut. Ekstern levering etter eit ukjent utfall kan gi duplikat.

Endringa brukar eksisterande kolonnar og krev inga migrering. Gamle binærfiler manglar eigarkontrollen. Under gradvis utrulling gjeld den gamle garantien til siste gamle pushkonsument i prod og canary er avslutta. Unngå å auke talet på konsumentar i overgangsvindauget. Verifiser image for alle arbeidsprosessar før den nye garantien blir lagd til grunn. Tilbakerulling til ein eldre binær innfører igjen risikoen for sein fullføring.

Plattform/drift vurderer prosessgrensa på nytt ved vedvarande aukande køalder, pushrelaterte API-oppstartsfeil, vesentleg auka trafikk eller behov for eigen oppdatering. Registrer då påverka funksjon, køalder, vanleg chatlatens, poolventing og faktisk CPU/minne gjennom same hending. Fram til då følgjer talet på innebygde arbeidarar framleis API-talet. Det er det konkrete avviket frå uavhengig skalering i #354.

## Delt arbeid og feilprøve

Prod og canary deler databasen og kan konsumere same arbeid. Ved endra jobbformat eller publiseringsvilkår må drift først oppgradere alle relevante konsumentar med ny funksjon avslått. Slå på produsenten etterpå. Ved tilbakeføring skal køyrande og attståande jobbar framleis kunne lesast og fullførast korrekt. [Minneprosedyren](agent-memory-worker-operations.md) krev kompatible publiseringskontrollar hos alle svararbeidarane før innsamling, bygging og bruk blir aktiverte i rekkjefølgje. Ved ei framtidig pushflytting gjeld [overgangen for gamle konsumentar](architecture-alignment-next-step-plan.md).

I [den isolerte Kubernetes-/Argo-prøva](heart-kubernetes-argo-test.md) var Heart utan klart endepunkt under oppdatering. Vanleg chat vart lagra. Fire leveringsforsøk for START gav éin fullført outbox-operasjon, éin Heart-instans og éi started-hending. Dette verifiserer den eksisterande Heart-grensa. Push- og modellfeil under API-oppstart er framleis ei eiga risiko.
