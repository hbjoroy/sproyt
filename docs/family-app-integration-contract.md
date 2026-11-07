# Appintegrasjon API og Open Drinks

Dato: 2026-10-29. Status: føreslått v1-kontrakt, ikkje eit tilgjengeleg API.
Denne rettleiinga skal ferdigstillast med OpenAPI og køyrbare kontrakttestar i I0.
Sjå [arkitektur og etappar](family-app-integrations-plan.md).

## Tilkopling

Alle kall går frå appserver til Sprøyt over HTTPS. Bruk
`Authorization: Bearer <service-credential>`. Credential blir utlevert gjennom
secret-handtering, aldri som melding eller i nettlesaren. Sprøyt identifiserer
appen frå credentialet, ikkje frå eit sjølvvalt `app_id` i payload.

Kvar aktiv binding avgrensar app, kanal og ekstern sesjon. Ein gyldig credential
utan binding gjev ingen data. MVP-scopes: `responses:read`, `attributes:write`,
`messages:write`, `media:write`. Ingen historikklesing eller generell medlemsliste.
Appen oppdagar sine eigne bindingar gjennom lista under. Bruk separate credentials
og bindingar i canary og produksjon.

## Føreslåtte endepunkt

Prefiks: `/api/v1/app-integrations`.

| Metode og sti | Føremål |
| --- | --- |
| `GET /bindings` | Paginert liste over eigne bindingar, ressurs-ID, revisjon og status |
| `GET /bindings/{id}/responses` | Konsistent snapshot av gjeldande, delte svar frå noverande medlemmer |
| `GET /bindings/{id}/changes?cursor=...` | Ordna endringar, inkludert tilbakekalling, etter snapshot |
| `PUT /bindings/{id}/members/{subject}/attributes` | Erstatt appens deklarerte attributt for ein delt respondent |
| `POST /bindings/{id}/messages` | Publiser Markdown og ferdige vedlegg |
| `POST /bindings/{id}/media-uploads` | Opprett avgrensa opplasting |
| `PUT /bindings/{id}/media-uploads/{upload}/chunks/{index}` | Overfør byte til den eigde opplastinga |
| `POST /bindings/{id}/media-uploads/{upload}/complete` | Start kølagd prosessering |
| `GET /bindings/{id}/media-uploads/{upload}` | Les status og ferdige media-ID-ar |

Desse er nye adapterruter. Dei eksisterande menneskeautentiserte medieendepunkta
kan ikkje utan vidare brukast med appcredential. Chunk-storleik, bytegrenser,
sjekksumformat og statusfelt skal koplast til eksisterande mediekontrakt i I0.

## Første Open Drinks-binding

Sprøyt opprettar bindinga etter moderatoren si handling og serverautorisert
sesjonsval. Open Drinks skal ikkje opprette ein deltakar frå berre kanalmedlemskap.

```json
{
  "binding_id": "b_123",
  "app_id": "open-drinks",
  "scope": {"type": "channel", "id": "c_123"},
  "resource": {"type": "session", "id": "od_session_456"},
  "manifest_version": 1,
  "revision": 1,
  "status": "active"
}
```

ID-ane er illustrative, opake strenger. Ikkje baser logikk på prefiks eller namn.
Sesjons-ID er uforanderleg i bindinga. Ny sesjon krev ny binding og nye svar.

MVP-brukarfeltet er `participation` med `yes` eller `no`; ikkje-svara personar
har inga eksportert rad. Open Drinks får alle som har svart og framleis deler,
og må sjølv skilje ja frå nei. Ingen e-post, OIDC-identitet eller andre
krets-/kanalopplysningar blir inkluderte.

```json
{
  "binding_id": "b_123",
  "snapshot_id": "snapshot_opaque",
  "items": [
    {
      "subject_id": "subject_opaque",
      "display_name": "Harald",
      "response": {"participation": "yes"},
      "app_attributes": {},
      "revision": 3,
      "updated_at": "2026-10-29T12:00:00Z"
    }
  ],
  "next_page_token": null,
  "changes_cursor": "cursor_opaque"
}
```

Snapshot-sider må representere same konsistente uttrekk. `changes_cursor` blir
utlevert når siste sida er lesen. Klienten erstattar den lokale kopien for denne
bindinga og les deretter endringar. Snapshot-token har avgrensa levetid; nytt
snapshot ved utløp. Serveren må sjekke gjeldande binding og deling også under
paginering; tilbakekalte data skal ikkje bli utleverte frå ein forelda snapshot.

Endringar bruker monoton `sequence` per binding, `event_id`, `kind`,
`subject_id` og `revision`. `response.upserted` ber det gjeldande eksportformatet;
`response.revoked` ber berre identifikator/revisjon, ikkje gammalt svar eller namn.
Endringslesing må kontrollere gjeldande deling: historiske upsert-hendingar for
ein tilbakekalt respondent skal ikkje utlevere dei gamle dataa. Lever minimal
tilbakekallingsstatus i staden. Appattributtendringar får eiga hendingstype,
`attributes.updated`, slik at dei ikkje utløyser ei løkke med nye brukarsvar.
Visingsnamnendring må òg oppdatere eksportrevisjonen. Levering er minst éin gong;
lagre ny cursor først etter lokal commit. Ikkje reaktiver frå ei eldre revisjon.
Ved utgått cursor: `410 cursor_expired`, deretter nytt fullstendig snapshot.

Ved deaktivert binding stoppar data- og skrivekall med `403 binding_disabled`.
Bindinglista viser deaktivert status, slik at appen kan stoppe bruk og rydde sin
kopi. Minimal kontrollstatus kan lesast med gyldig appcredential; ved tilbakekalt
credential gir alle kall `401`, og appen må stoppe synk og bruk av den aktuelle
tilgangen. Historiske meldingar i Sprøyt blir ståande.

## Appattributt

Open Drinks kan til dømes få deklarert `participant_ref` som streng i sitt
manifest. Feltet høyrer til denne bindinga, ikkje brukarens globale profil.
Nye felt eller sensitivt innhald krev ny manifestversjon og vurdering av kva
brukaren blir beden om å dele. Ikkje bruk generelle fritekstmetadata utan grenser.

```http
PUT /api/v1/app-integrations/bindings/b_123/members/subject_opaque/attributes
Authorization: Bearer <service-credential>
Idempotency-Key: od-participant-123-v1
If-Match: "3"
Content-Type: application/json

{"attributes":{"participant_ref":"od_participant_789"}}
```

Ukjende felt eller `participation` blir avviste. `If-Match` brukar gjeldande
medlemsrevisjon frå lesing; stale verdi gir `412 revision_conflict`. Uttrekking
av deling mellom lesing og skriving skal avvise skrivinga. Serveren returnerer
oppdatert revisjon; appskriving må ikkje sjå ut som eit nytt brukarsvar.

## Meldingar og vedlegg

```http
POST /api/v1/app-integrations/bindings/b_123/messages
Authorization: Bearer <service-credential>
Idempotency-Key: od-session-456-round-7
Content-Type: application/json

{
  "markdown": "## Neste runde\nSmakinga startar klokka 19.30.\n\n![Smaksoversikt](attachment:overview)",
  "attachments": [
    {"key":"overview","media_id":"media_opaque","alt":"Smaksoversikt"}
  ]
}
```

`attachment:` er føreslått API-input, ikkje lovnad om støtte i dagens renderer.
Serveradapteren skal omsetje dette til Sprøyt sitt autoriserte meldings-/medieformat.
Alle referansar må samsvare med ferdige vedlegg frå same binding. Video brukar
same eigarskapskontroll og blir vist med eksisterande videovisning. Markdown utan
vedlegg fungerer sjølvstendig. Ingen rå HTML eller klientstyrte oppgåvemakroar.

Ved ny melding: `201` med `message_id` og kanallenkje. Retry med same
idempotensnøkkel og same body returnerer same melding; endra body gir `409`.
Ikkje oppgi avsendar i body; han blir bestemt av tenesteidentiteten.
Opplasting og melding er separate operasjonar; ubrukte opplastingar blir rydda
etter eksisterande mediepolicy. Melding med uferdig medium gir ein tydeleg,
retrybar `409 media_not_ready`, ikkje ei stille tom melding.

## Feil, retry og samtidige kall

- Feil har `code`, ei trygg `message` og `request_id`; ingen secrets i feilsvar.
- `401`: credential ugyldig/utgått; stopp og varsle operatør.
- `403`: binding deaktivert eller scope manglar; ikkje prøv med brukarkonto.
- `404`: objektet finst ikkje eller er ikkje tilgjengeleg for appen.
- `409`: idempotenskonflikt eller oppgjeven ressurskonflikt.
- `412`: les oppdatert revisjon og avgjer om endringa framleis er relevant.
- `422`: ugyldige felt; korriger payload, ikkje blind retry.
- `429`/`503`: følg `Retry-After`, elles eksponentiell backoff med jitter.
- Nettverksbrot etter skriving: gjenta med same idempotensnøkkel og identisk body.

Idempotens er avgrensa til app, binding og operasjon og blir lagra atomisk med
effekten. Retensjon og maksimal replayperiode må dokumenterast i I0; klienten må
ikkje finne på ny nøkkel etter timeout. Credential-rotasjon endrar ikkje logisk
appidentitet eller kvitteringsnamespace. Store binære bodyar høyrer berre til
chunk-endepunkta, ikkje JSON-meldingsruta.

## Arbeidet på Open Drinks-sida

1. Tilby autorisert sesjonsval/-validering for Sprøyt. Avklar om driftsstyrt
   allowlist er nok for første prøve, eller om brukaren må godkjenne kontoåtgang.
2. Les credential frå secret. Finn eigne aktive bindingar; synk snapshot og
   endringar. Start med polling kvart 30. sekund med jitter og avgrensa batchar.
3. Upsert deltaking med `(binding_id, subject_id)` som unik nøkkel; visingsnamn
   er berre presentasjon. Handter nei, tilbaketrekking og endra revisjon separat.
4. Skriv eventuell `participant_ref`, og publiser éi prøvemelding med same
   idempotensnøkkel to gonger. Det skal berre bli éi melding i Sprøyt.
5. Prøv eit bilete og ein video gjennom køa, og følg status før publisering.
6. Bevis at tilbakekalling, credential-rotasjon, nettverksbrot og restart ikkje
   fører til ny deltaking, duplikat eller innsyn i andre bindingar.

Før aktivering skal denne rettleiinga supplerast med maskinlesbar OpenAPI,
eksakte grenser/retensjon, eksempelklient og resultat frå ein ekte canary-prøve.
Ingen av endepunkta over skal reknast som ferdig implementerte før det.
