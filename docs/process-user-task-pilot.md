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
## Canary-førebuing 28. september 2026

Harald sin verifiserte Sprøyt-identitet er `d293b6d3-32b5-5979-bf12-94266342cd29`.
Prosesstest finst allereie (`01a0e7c1-5cae-7b71-afaf-bc74ae67c3cc`), og
Harald er eigar av kanalen og Rocket-admins. Ingen medlemskap må endrast.

Den nye databasen `heart_sproyt_canary` får ei eiga rolle med høgst fire
samstundes tilkoplingar; runtime bruker to, og bootstrap/migrering éi kvar.
Dette legg til ei tom database utan å endre eksisterande roller eller
prosessdata. Secret-referansen er `sproyt-canary-heart` i `sproyt-canary`;
passord vert ikkje lagra i Git. PostgreSQL sin eksisterande nattlege backup
køyrde ferdig same dag; nye pilotdata må også vere med i vidare backup.
Ved feil vert piloten deaktivert, medan database og Secret vert bevarte.
Det er ikkje naudsynt eller planlagt å slette data for tilbakeføring.
## Utrulla canary — 28. september 2026

GitOps PR [161](https://github.com/hbjoroy/rocket-applications/pull/161) er
merga som `2a06298aa3cd10ac323203a4a2ef32b66746d500`.
Canary-chart/app er frå `edf559b2d8b934de270a0ed4c9cdedac812e3d78`.
Sprøyt-image: `sha256:bf7f9fff915b3751570e5092b57488dbdb5c913da70fccd67ccabe1b9b1a129e`.
Heart-image: `sha256:0fbae5d490ded24ecdfab9e86a313277687c7c44309c1dcc6184e1c4ff9dc5f5`,
frå `efaa7f80a55b7b6526de0ae7247d44a1c2de4d70`.

Levande kontroll stadfestar begge image-digestane, klare podar, gjennomført
Sprøyt0040 og Heart005, og begge definisjonane registrerte i den separate
Heart-databasen. Canary ConfigMap har pilot=true og generell outbox=false.
Argo rapporterer begge applikasjonane Synced/Healthy; offentleg readyz
svarar200 for canary og produksjon. Ingen pilot er starta ved utrullinga;
Harald må aktivere bindinga og velje Start testprosess i kanalmenyen.
Den manuelle prøva med ekte innlogging, særleg på telefon, står att.

Produksjonen sin migreringsjobb er pinna til det same Sprøyt-imaget, fordi
SQLx elles vil avvise0040 som ukjend ved ein seinare sync med eit gammalt
migreringsimage. Render mot eksakt produksjonschart356fa06f viste at berre
migreringsjobben endra image. Produksjonsappen står på digest7b23e18e… og
Deployment-generasjon120, som før denne utrullinga; Heart der er urørt.
Bevar den nye migreringspinnen også ved eventuell rollback av appen.

Heart-publiseringa bruker ein valfri, manuell CI-veg med fast registrymål,
kontrollert privat TAR-sjekksum, revisjon, arkitektur og ikkje-root-identitet.
Den mellombelse signerte URL-en vart berre lagra som kryptert miljøsecret
og er sletta etter import. Import og digest-evidens lukkast; CI-runda
36426375395 feila berre under sletting av root-eigd mellombels loginmappe.
Den avgrensa oppryddingsrettinga er pusha som fcfcef6 og syntakskontrollert.
Dette var ein publiseringsjobbfeil, ikkje ein feil i det utrulla imaget.

Framtidige fleirkjelde-Argo-endringar bør først synkronisere ny chartpin
med gamalt image og pilot av, og deretter aktivere nytt image/oppsett.
Parent Application-pin og main-values er ikkje ei atomisk endring.
ReplicaSet-kontrollen for denne utrullinga viste berre den rette nye
chart-konfigurasjonen; ingen mellomliggjande gamal-chart/nytt-image-utrulling.

## Heart v2-etappe 1/2 i canary — 28. september 2026

Heart PR6/7/8 er merga; endeleg masterrevisjon er
`9f0925a8aabd28296036a37fe5d4f65c58c5bf56`.
[Heart CI36458710117](https://github.com/hbjoroy/heart/actions/runs/36458710117)
passerte 144 testar, faktiske PostgreSQL-migreringar, native ARM64-containerprøve,
SBOM og sikkerheitsskanning.
[Publiserings-CI36460233926](https://github.com/hbjoroy/sproyt/actions/runs/36460233926)
passerte alle kontrollar og publiserte det verifiserte imaget.

GitOps PR[162](https://github.com/hbjoroy/rocket-applications/pull/162) førebudde
chart3a7a608 med gammalt image/v2=false. Deretter aktiverte
PR[163](https://github.com/hbjoroy/rocket-applications/pull/163), merge
`27af07e3a4329803b42a803d580cf0e59510dfe1`, v2 med Heart-image
`sha256:36eb8181e55487bacfa0466faba2ceefe5dd9662ca46eb27bbb54ab6e30c1671`.
Sprøyt-image, databaseidentitet og produksjonsoppsett er uendra.

Full canary-backup vart lesen, sjekksummert og restaurert på isolert PostgreSQL18.
Prøva avdekte kjende CRLF-sjekksummar frå det eldre Windows-bygget, medan SQL-
innhaldet i Git er uendra. Det avgrensa GitOps-operator-scriptet
`scripts/heart/reconcile-legacy-crlf-checksums.sql` vart først prøvd på restore-
kopien: ukjend hash avbryt atomisk, kjende hashpar blir retta, replay er uendra.
Levande retting og migrering006/007 bevarte eksakte SHA-256-fingeravtrykk av
v1-instansen, begge oppgåvene, definisjonane, startkvitteringane og dei andre
felta i eksisterande migreringshistorikk. Heart migreringsfiler er no bundne til LF.

Levande privat v2-prøve i namespace `ci-v2-smoke` passerte med to brukaroppgåver
for same person, Lua/XOR-overgang, idempotent start/fullføring og terminaltilstand.
Resultatet er éin fullført prøveinstans, to fullførte oppgåver og null aktive steg.
V1-piloten kan framleis lesast via API. Argo er Synced/Healthy; canary/prod readyz
svarar200. Produksjonsgenerasjonar120/1 og image-digestane er uendra.

Sprøyt Prosesstest bruker framleis v1-adapteren; denne utrullinga prøver den nye
v2-motoren direkte, utan å flytte eksisterande kanalprosessar. V2-adapter og
fork/join står att som seinare etappar. Paus v2 ved å sette
`heart.runtimeV2.enabled=false` med same nye image og database. Bevar schema
og canonical-LF migreringsimage; ikkje bruk gammalt CRLF-image som blind rollback.
