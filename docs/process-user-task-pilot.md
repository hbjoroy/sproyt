# Prosesstest: to brukaroppgåver

Dette er den avgrensa iterasjonen 0a i
[arbeidsplanen](work-items-and-heart-plan.md), ikkje eit generelt saksregister.

## Kontrakt og arkitektur

Heart PR #6 innfører reelle `user-task`-steg. Prosessdefinisjonen
`sproyt-user-task-pilot` 1.0.0 går start → first → second → end.
Begge oppgåvene vert tildelte den same UUID-en i `metadata.assignee_id`.
Heart lagrar oppgåveaktivering og atomisk fullføring/overlevering. Han
støttar førebels dette avgrensa sekvensielle forløpet, ikkje claim,
omfordeling eller vilkårleg blanding med automatiske noder.

Sprøyt finn oppgåver frå kjende pilotinstansar kvart tredje sekund,
validerer heile resultatet og opprettar éi vanleg melding per oppgåve.
Makroen `[[process-task:<id>]]` peikar på ei faktisk Heart-oppgåve.
Serveren kontrollerer også meldings-ID; ein kopiert makro får ikkje
gyldige oppgåvehandlingar. Meldinga har ein intern Heart-agent som
avsendar utan utferda innloggingscredential.

`process_pilot_channels`, `process_pilot_runs` og `process_pilot_tasks`
er kanalbindingar, start-/fullføringskvitteringar og meldingsprojeksjonar.
Dei avgjer ikkje kva prosesssteg som er neste. Same startnøkkel gjev same
instans, og same oppgåve gjev same melding. Fullføring vert først vist
som ventande, deretter som fullført etter Heart-stadfesting.

Arbeidarane deler PostgreSQL-poolen med resten av Sprøyt og bruker
varige, inngjerda leases og rettferdig batch-plukking. Éi feilande
instans skal ikkje blokkere seinare instansar. Terminal oppgåvestatus
kan ikkje regrediere ved ei forelda avstemming. Konfigurering, start
og akseptert fullføringskommando vert loggført i auditregisteret.

## Aktivering og manuell prøve

Piloten er av som standard. Canary-oppsettet må ha:

- Sprøyt-image som inneheld migrasjon 0040 og denne funksjonen.
- Heart-image frå PR #6, med migrasjon 005 og brukaroppgåvekontrakten.
- Eigen Heart-database/identitet for canary. Gamal produksjons-Heart
  skal ikkje lese dei nye prosessdefinisjonane/instansane.
- `config.processPilotEnabled: true` og
  `config.processPilotAssigneeId` med Harald sin faktiske Sprøyt-UUID.
- Heart med éi canary-replika og `heart.dbMaxConnections: 2`.

Sprøyt set miljøvariablane `SPROYT_PROCESS_PILOT_ENABLED`,
`SPROYT_PROCESS_PILOT_ASSIGNEE_ID` og `SPROYT_PROCESS_PILOT_HEART_URL` frå Helm. Canary set `config.processOutboxEnabled: false`, slik at den generelle arbeidaren ikkje tek produksjonskommandoar frå den delte Sprøyt-databasen.
Den nye Heart-versjonen bruker `HEART_DB_MAX_CONNECTIONS`; migrering
og definisjonsbootstrap har eigen grense på éi tilkopling.

I canary går Harald inn i **Rocket-admins → Prosesstest**, opnar
kanalmenyen og vel **Aktiver prosesspilot for meg**, deretter
**Start testprosess**. Aktivering er berre tillaten for konfigurert
testperson som er eigar av både testkanalen og kretsen. Det vert
ikkje starta prosessar eller oppretta medlemskap ved vanleg lasting.

Opne første oppgåvemelding og fullfør. Meldinga vert ståande som
fullført, og ei eiga melding for steg to skal kome. Fullfør denne.
Ein annan kanalmedlem skal kunne opne begge, men ikkje fullføre.
Samanfalding/utviding er visningstilstand; lesing og chatreply endrar
ikkje oppgåvestatus. Vanlege kanalvarslar og ulestmarkeringar gjeld.

## Verifikasjon

Vanleg CI køyrer SQLite-projeksjon, rettar, kommando-replay, forelda
arbeidar og batchrettferd. PostgreSQL-jobben køyrer same retts-/
projeksjonskontrakt med eksplisitt testdatabase. Frontend har
dekodar-/identitetstestar og to nettlesartestar for mobil/lesetilgang,
ventande fullføring, retry og bevart meldingsutkast.

Den ekte kontrakten mellom repoane køyrer separat fordi Heart-repoet
er privat. Start ny Heart API mot ei isolert Heart-database og set:

```powershell
$env:SPROYT_TEST_DATABASE_URL = '<dedikert Sprøyt-testdatabase>'
$env:SPROYT_TEST_HEART_URL = 'http://127.0.0.1:<testport>'
cargo test actual_heart_two_steps -- --ignored --test-threads=1
```

Testen registrerer den ekte definisjonen, utfører begge stega og
kontrollerer at Heart-feil held fullføring ventande, at ei ny Sprøyt-
teneste finn neste oppgåve, at eit gammalt steg ikkje kan fullføre det
neste, og at kanalen har nøyaktig to oppgåvemeldingar. Dette erstattar
ikkje den manuelle canary-prøva med ekte innlogging og kanaloppsett.

## Utrulling og tilbakeføring

Sprøyt-migrasjon 0040 er additiv, utan omskriving av eksisterande data.
Canary deler Sprøyt-databasen med produksjon; pilotmeldingane i den
avgrensa testkanalen vil difor også finnast i produksjon, der gamle
klientar kan vise rå makro. Ingen andre kanalar får aktivering.
Heart sin nye database har ikkje produksjonsinstansar. Eksisterande
databasebackup er grunnlaget for saksdata; unngå destruktiv migreringsrollback.

Deaktiver `config.processPilotEnabled` for å stoppe pilotarbeidaren og
API-handlingane; bevar tabellar og Heart-database for avstemming.
Deaktivering av berre kanalbindinga stansar nye starter, medan arbeid
som allereie er akseptert kan leverast. Reaktivering finn varige køar
att. Bevar den same Heart-databaseidentiteten for registrerte pilotinstansar; flytting til ein annan motor krev ei eiga migrering. Produksjonsimage og produksjons-Heart vert ikkje endra av piloten.

Status for GitOps, image-digest og faktisk canary-prøve skal førast
her ved utrulling; denne kjeldedokumentasjonen åleine stadfestar ikkje
at piloten er aktiv i clusteret.

