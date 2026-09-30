# Saksflyt: konfigurasjon og tilgang (steg 1)

Dette er første serverdel av [saksplanen](work-items-and-heart-plan.md). Ho
registrerer applikasjonar og kanalpolicy, men lagar enno ikkje saker eller
Heart-instansar for saksflyten. Ingen applikasjon eller prosess er aktiv som
standard. `Rocket-admins → Prosesstest` held fram med sin eigen pilotadapter.

Ein applikasjon har stabil nøkkel, ID, namn og eigarkrets. Berre eigaren av
kretsen kan konfigurere applikasjonen, knyte henne til ein prosess i ein
kanal i same krets, tildele behandlarrettar eller gje prosessroller. Kretseigar
får ikkje automatisk behandlarrett. Behandlarrettane for vurdering, GitHub-
eksport og utviklingsstart er separate; eksport og utviklingsstart krev òg
vurderingsrett. Desse rettane blir først nytta av dei seinare saksstega.

Ein kanal–prosess-binding vel nøkkel, Heart-namespace, definisjonsnamn og
eksakt versjon. Ei oppgåverute bind ein oppgåvetype og prosessrolle til ein
kanal i same krets. Fleire applikasjonar og behandlarar kan konfigurerast.
Saksflyten skal bruke desse dataa til å velje applikasjon, tildele og vise
oppgåver; ein brukar si kanaldeltaking aleine gjev ingen utføringsrett.
Alle endringar i konfigurasjonen blir reviderte i `audit_events`.

Eksisterande generisk Heart-start krev no både det gamle
`heart.event-planning`-flagget og ei aktiv, eksakt kanal–definisjonsbinding.
Migrasjon 0043 tek vare på definisjonar som allereie er starta i den kanalen,
men opnar ikkje nye definisjonar. Replay med same request-ID returnerer det
eksisterande resultatet; gjenbruk av ID-en i ein annan kanal blir avvist.
HTTP og MCP brukar same repositorykontroll. Prosesstest bruker framleis si
servervalde definisjon og si eiga tilgangskontroll.

Administrasjons-API-et er autentisert og eigarkontrollert:

| Rute | Bruk |
| --- | --- |
| `POST /api/v1/circles/{id}/work-applications` | Opprett/oppdater applikasjon (`key`, `name`, `enabled`). |
| `POST /api/v1/channels/{id}/process-bindings` | Set `process_key`, `namespace`, `definition_name`, `definition_version`, `enabled`. |
| `POST /api/v1/channels/{id}/process-applications` | Tillat/fjern `application_id` for `process_key`. |
| `GET /api/v1/channels/{id}/process-applications?process_key=...` | List berre aktive val for skrivande kanalmedlemmer. |
| `POST /api/v1/work-applications/{id}/processors` | Set `user_id` og behandlarrettane. |
| `POST /api/v1/work-applications/{id}/process-roles` | Gje/trekk tilbake ei prosessrolle for ein brukar. |
| `POST /api/v1/channels/{id}/task-routes` | Set oppgåvetype, rolle og målkanal. |

For å aktivere den første saksflyten må vi enno velje konkrete kretsar og
kanalar for Sprøyt og Utpå. Steg 2 byggjer «Lag Issue», varig saksregister,
val/stadfesting av applikasjon og ein servervald Heart-start. Først då får
oppgåverutene og behandlarrettane operativ verknad. GitHub-eksport og
automatisk utviklingsstart er framleis avslått.
