# Journal for sletting og tilbakekalling ved gjenoppretting

Drift eig journalen og attspelinga. Produkt/personvern godkjenner omfanget,
haldbarheita til saksvurderinga og kva opplysningar som skal handhevast.
Tryggleik stadfestar identitet, tilbakekalling av andre tilgangar og aktive
sperrer. Krav ved gjenoppretting gjeld begge API-miljø og alle arbeidarar som
kan bruke den delte Sprøyt-databasen. [#415](https://github.com/hbjoroy/sproyt/issues/415)

## Lagring og tilgang

Journalen køyrer i eit eige namnerom på ein PostgreSQL-disk med `Retain` med to
Longhorn-replikaer. Han har eigne roller, ingen Kubernetes Service og ingen
lyttar på pod-adressa. Sprøyt-backupen omfattar aldri journalen. Drift kan
byte journalpoden utan å miste journalinnhaldet. Tap av heile klynga kan
ramme begge databasane. Ei uavhengig arkivkopi av journalen krev framleis
eit verifisert, tilgangsstyrt mål utanfor klynga.
[GitOps og tilgang](https://github.com/hbjoroy/rocket-applications/blob/e428b4f5ae4cf21f58cfb3b665cf843daef27044/manifests/sproyt-privacy-journal/journal.yaml)

- Berre journaladministratorar får administrativ pod-tilgang og lokal
  peer-autentisering. Ein administrator kan lese og endre journalen, og må
  haldast skild frå ordinære lese-/skrivefullmakter.
- Bind namngjevne, godkjende operatørar til rolla
  `privacy-journal-connect` for lesing av podstatus og port-forward. Rolla
  gir inga shell- eller hemmelegheitstilgang. Bruk lokal bindeadresse og
  eksplisitt podnamn. Ein administrator opprettar ein personleg, tidsavgrensa
  PostgreSQL-innlogging med medlemskap i anten `privacy_journal_writer` eller
  `privacy_journal_reader` og set SCRAM-passordet med `\password`. Bruk
  tilgangsstyrt passordlager og libpq-passordfil. Ingen permanent brei
  rollebinding blir oppretta av GitOps.
- Skriv journalen før den godkjende endringa i applikasjonen. Skrivaren kan
  leggje til konto-UUID, handling og eventuell teknisk øktfamilie-ID.
  Lesaren kan hente kontrollgrunnlaget. Skrivaren kan ikkje lese, endre
  eller slette rader. Lesaren kan ikkje skrive. Unngå namn, grunngjeving,
  meldingar, nøklar og økttoken i journalen.
- Behald ei plikt så lenge ei gjenopprettbar eldre kopi, PITR-intervall
  eller klone kan innehalde den råka tilstanden. Drift og produkt/personvern
  stadfestar at alle slike kopiar er ute av omløp før administrativ sletting
  av ei samanhengande eldste radrekkje. I same transaksjon flyttar drift
  dekningsgrensa forbi den siste fjerna plikta og aldri forbi ei bevart rad. Uavklarte eller eldre kopiar forlengjer vernet.
  Journalen har inga automatisk sletting.

Dekninga byrjar 20. november 2026 kl. 05:18:30 UTC. Ein backup med eldre
eller ukjend starttid må haldast isolert til to ansvarlege har rekonstruert
og kontrollert det eldre handhevingsgrunnlaget frå uavhengige kjelder. Den
ordinære attspelinga stoppar ved manglande dekning. Ved tap av journalen må
drift gjenreise og verifisere kjelda før trafikk kjem tilbake.

## Attspeling og signering før trafikk

1. Steng trafikk og alle skrivarar frå gjenopprettingsdatabasen, inkludert
   begge API-miljø, arbeidarar og OIDC-callbackar. Bruk eige namnerom,
   tilkoplingsrettar og nettverksreglar. Gjenopprett til ei isolert database
   med namn som byrjar `sproyt_restore_` og køyr framovermigreringar.
   Noter start på backupvindauget frå dokumentert backupbevis.
2. Produkt/personvern og drift kontrollerer private saksvedtak, opne
   plikter og ekstern identitet. Set journalskriving på pause, vent på
   påbegynte transaksjonar, noter tidspunktet i UTC, og hald pausen fram til
   signering. Journalen handhevar lokal avgrensa kontosletting og
   tilbakekalling av ein konkret native øktfamilie. Andre tiltak, som
   Authentik- og nettlesarøkter, agentar, eksterne mottakarar og uavklarte
   eigaroverføringar, må kontrollerast separat før trafikk.
3. Journal-lesaren hentar eit nytt kontrollgrunnlag med
   `psql -X -qAt -v ON_ERROR_STOP=1 -d service=privacy_journal -f tools/privacy-restore-manifest.sql`.
   Omdiriger til ei fil med avgrensa lesetilgang. Ein annan operatør sjekkar
   radtalet og høgaste ID direkte i den uavhengige journalen. Bruk ID 0 når
   journalen er tom. Hent dei aktive sperrene frå det autoritative
   saksgrunnlaget og den aktuelle `blocked-account-ids`-verdien frå
   `sproyt-account-restrictions` i både prod- og canary-namnerom.
   Hald alle tre kjeldene som separate tilgangsstyrte filer med
   kommaskilde konto-UUID-ar. Ei eksplisitt tom kjelde er gyldig.
4. Køyr attspelinga frå eit privat operatørmiljø. Bruk den faktiske
   produksjonsskjemaidentiteten `sproyt` og den verifiserte lineagen frå
   driftsjournalen. Den etablerte minne-/posisjonsresetten køyrer etter
   journalhandlingane. Returkode ulik null betyr at kopien framleis er
   isolert. Ved delvis gjennomføring kan same journal spelast på att.
   Journal, sperrefiler og SQL-feil kan innehalde sensitive metadata og
   skal haldast ute av felles loggar.

   ```sh
   python tools/replay-privacy-restore.py \
     --journal-json "$journal_file" --lineage "$lineage" \
     --backup-start "$backup_start" --frozen-at "$journal_frozen_at" \
     --expected-count "$checked_count" --expected-last-id "$checked_last_id" \
     --required-blocked "$approved_restrictions" \
     --prod-blocked "$prod_restrictions" --canary-blocked "$canary_restrictions" \
     --restore-target service=privacy_restore --restore-db "$restore_db" \
     --restore-schema sproyt
   ```

5. Drift verifiserer måltilstanden i den isolerte kopien. Kontroller at dei
   journalførte handlingane er handheva, andre brukarar framleis har rett
   medlemskap og data, og at minne og posisjon ikkje er gjeninnførte. Ein
   annan operatør kontrollerer same journaltal og siste ID fram til trafikk
   blir opna, og at begge innstillingane matchar saksgrunnlaget. Vent med
   trafikk til begge miljø brukar ein versjon som handhevar desse
   innstillingane, og har lasta dei ved oppstart. Stadfest ekstern
   identitet og tilbakekalling før signering. Verktøyet opnar aldri trafikk.
   Arkiver berre nødvendig driftsbevis og rydd arbeidskopiane av journal og
   sperrer.

## Prøve 20. november 2026

Den målretta PostgreSQL-jobben køyrer same kontroll på syntetiske data.
[CI-prøve](../tools/test-privacy-restore.sh)

To lokale PostgreSQL 18.6-containerar utan nettverk heldt journal og
syntetiske applikasjonsdata kvar for seg. Applikasjonstabellane låg i
`sproyt`. Prøva tok eit ekte `pg_dump`, journalførte og gjennomførte
sletting og økttilbakekalling i kjelda etter backup, og brukte `pg_restore`
til ei anna database før attspeling. [Syntetiske data](../tools/seed-privacy-restore.sql) · [Etterkontroll](../tools/verify-privacy-restore.sql)

| Kontroll | Resultat |
| --- | --- |
| Eldre kopi før attspeling | Tre aktive øktfamiliar, éin minneprofil og eitt posisjonsløyve |
| Borte eller ulikt sperregrunnlag, eller borte journal | Avvist før nokon av dei tre kontrollverdiane endra seg |
| Backup før dekningsgrensa eller feil stadfesta radtal | Avvist før attspeling |
| To gjennomføringar med same journal | Begge passerte. Slettemarkør og privat sletting heldt, tilbakekalla økt opna seg ikkje, og gyldig økt, medlemskap, status og melding til annan brukar stod att |
| Minne og posisjon | Ingen gjeninnførte profilar eller løyve etter reset |
| Produksjonsjournal | Synced/Healthy, 1 GiB PVC med `Retain`, to køyrande replikaer på to nodar. Dekningsrada overlevde podbyte. Ingen testplikter eller prøveinnloggingar stod att |
| Produksjonstilgang | Separat SCRAM-lesar og -skrivar passa SQL-rettane. TCP-superbrukar vart avvist. Avgrensa Kubernetes-rolle gav port-forward og avviste pod-exec og lesing av hemmelegheiter |
