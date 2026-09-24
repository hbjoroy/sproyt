# Vurderingsplan for arkitektur og skalering

Status: plan for avgjerd, ikkje vedteken implementering. Oppretta 24. september 2026.

Grunnlag: det private driftsnotatet
`S:/Source/rocket/inventory/SPROYT-ARKITEKTUR-SKALERING-2026-09-23.md`,
Sprøyt-koden og driftsrapporten
`S:/Source/rocket/inventory/SPROYT-DELT-POOL-2026-09-23.md`.
Driftsnotatet er ein analyse frå før den første poolendringa vart levert. Det
skal ikkje lesast som ein spesifikasjon eller som dokumentasjon på gjeldande
kode. `ARCHITECTURE.md` beskriver framleis berre arkitektur som faktisk er
implementert; avgjerder her kan førast dit når dei blir kode.

## Kjent utgangspunkt

- Delt PostgreSQL-pool er versjonert i Sprøyt-commit `997eec6`, på grein
  `codex/shared-database-pool`. Han deler pool mellom repository, push og
  bilethandsaming, set maksimum og ventetidsgrense og har PostgreSQL-prøver.
  Dette er den avgrensa delen av tiltak 1 som alt er implementert.
- Kjelde-PR [#161](https://github.com/hbjoroy/sproyt/pull/161) er open oppå
  [#160](https://github.com/hbjoroy/sproyt/pull/160). At poolen ikkje synest
  i `main` eller i ei arbeidsmappe på den eldre SSE-greina, tyder ikkje at han
  er lagt inn manuelt utan Git. Før vidare arbeid må PR-stabelen og
  merge-vegen avklarast, utan å duplisere endringa.
- GitOps-PR [#155](https://github.com/hbjoroy/rocket-applications/pull/155)
  og [#156](https://github.com/hbjoroy/rocket-applications/pull/156) er
  fletta. Driftsrapporten dokumenterer utrulling av det uforanderlege imaget
  frå `997eec6` til canary og produksjon. Gjenta lesande kontroll av faktisk
  image/chart-revisjon når vurderinga startar; ein eldre lokal klone av
  Rocket-GitOps er ikkje kjelde til noverande driftstilstand.
- Pool-commitet fullfører ikkje namngjeving av databaseklientar, eit samla
  tilkoplingsbudsjett for clusteret eller måling av poolventing under last.
  Dei rapporterte 13 Sprøyt-sambanda er eit konfigurasjonstak ved normal drift,
  ikkje ein kapasitetstest eller eit budsjett for Authentik og andre appar.

## Steg 1: kjelde og utrulling kontrollert 24. september 2026

Dette er ei avgrensa, lesande kontroll av GitHub, GitOps og Kubernetes-context
`default`, pluss offentlege versjons-/readiness-endepunkt. Han omfattar ikkje
autentisert chat, lasttest eller måling av PostgreSQL under belastning.

| Kjelde | Stadfesta tilstand |
| --- | --- |
| Sprøyt `main` | `59e06d5`. Pool-commitet `997eec6` er ikkje fletta dit. |
| Open kjelde-PR-stabel | [#158](https://github.com/hbjoroy/sproyt/pull/158) GFM → `main`; [#159](https://github.com/hbjoroy/sproyt/pull/159) React-avslutning → #158; [#160](https://github.com/hbjoroy/sproyt/pull/160) SSE → #159; [#161](https://github.com/hbjoroy/sproyt/pull/161) delt pool → #160; [#162](https://github.com/hbjoroy/sproyt/pull/162) denne planen → #161. Alle var opne og mergeable ved kontrollen. Dei to vanlege CI-testjobbane viste grønt; fleire release-jobbar var markerte som hoppa over i PR-køyringane. |
| GitOps `main` | `b89af0b`; [#155](https://github.com/hbjoroy/rocket-applications/pull/155) og [#156](https://github.com/hbjoroy/rocket-applications/pull/156) er fletta. Både produksjon og canary har chart-kjelde låst til `997eec6` og image-digest `sha256:97e7191f2a5a8b6fad3e4dad84990cfcef7206ac09fdac9cc1a64d441212e73c`. Migreringsimaget er separat pinna. |
| Levande Argo og Kubernetes | Begge applikasjonar er `Synced/Healthy`. Argo viser GitOps `b89af0b` og chart `997eec6`. Produksjon har 2/2 klare poddar og canary 1/1; alle tre brukar image-digesten ovanfor. ConfigMap-taka er høvesvis 4 og 2. |
| Ekstern teneste | `/versionz` returnerte `997eec6` og `/readyz` HTTP 200 for begge miljø. |

**Avviket:** Kode og chart er versjonerte og utrulla frå ein uforanderleg
commit, men denne commiten er enno ikkje del av Sprøyt `main`. Det er ikkje
ein uversjonert produksjonspatch. Likevel er utrullinga avhengig av at Git
framleis kan levere commitobjektet når Argo treng ny rendering; ikkje slett
eller omskriv greinene i stabelen før kjeldehistoria er integrert i `main`.
Den lokale Rocket-klonen er eldre enn GitOps `main` og må ikkje brukast som
bevis for noverande produksjonskonfigurasjon.

**Trygg veg til `main` (forslag, ikkje utført):** Gå gjennom PR-diffane i
rekkjefølgja #158 → #159 → #160 → #161 → #162. GitHub-repoet tillèt
merge-commit; bruk det i denne stabelen for å bevare dei opphavlege commitane
og forfedretilhøvet, særleg GitOps-pinna `997eec6`. Etter kvart steg: rett
basen for neste PR til oppdatert `main`, kontroller at diffen berre inneheld
den PR-en si eiga endring, køyr relevante testar og sjekk at GitOps-pinna
chart-commit framleis er tilgjengeleg. Stopp ved konflikt eller uventa diff
og avklar han før neste samanslåing. Endeleg `main` skal innehalde `997eec6`,
og GitOps skal framleis peike på same verifiserte image til ei eiga leveranse
blir bestilt. Ingen samanslåing eller utrulling er gjort i dette steget.

## Vurdering i rekkjefølgje

1. **Lukk kjelde- og driftsbiletet.** Kartlegg PR-status, kva som ligg i
   `main`, chart-revisjon, applikasjonsrevisjon, image-digest og konfigurasjon
   i produksjon og canary. Skil Git-fakta, levande observasjon, eldre rapport
   og ukjent forhold. Dokumenter ein konkret, trygg veg for å få eksisterande
   kjelde-PR-ar inn i `main`; ikkje flett eller rull ut som del av vurderinga.
2. **Fullfør kapasitetsrekneskapen.** Tel poolar og langlevde listenerar for
   produksjon, canary, Heart, migrering, backup, Authentik og andre klientar.
   Ta med samtidige utrullingar og terminerande poddar. Mål poolventing,
   timeoutar, aktive/idle tilkoplingar og påverknad på Authentik over tid.
   Vurder klientnamn og ein eksplisitt driftsreserve før nytt pooltak blir
   foreslått. Dersom delt pool skaper intern konkurranse, undersøk først kva
   arbeid som held ei tilkopling lenge.
3. **Kartlegg leveringskontraktane.** Spor kvittering mot database-commit,
   `NOTIFY` etter commit, listener-reconnect, innhenting av etterslep,
   edit/delete/reaction, request-id og utgåtte/overtatte leases. Skil ei
   forseinka varsling frå ei tapt lagra melding. Definer kva som faktisk må
   vere varig, og kva som kan vere eit signal for å vekkje mottakarar.
4. **Kartlegg driftsroller og køar.** List alt som blir starta i `server.rs`,
   kva ein rein API-prosess og ein rein worker faktisk treng, og korleis dei
   skal ha helse, avslutting, pool og eigne samtidigheitsgrenser. Vurder
   push-drenering, Heart og bilete kvar for seg. Den globale biletlåsen kan
   vere eit medvite kostnadstak; fleire workers er ikkje åleine grunn til å
   fjerne han. Vurder ei minste rolleendring før ein felles køkontrakt.
5. **Lag ein isolert prøveplan før lastprøver.** Bruk representative data og
   1/2/4 API-replikaer med fast worker-tal, deretter fleire workers med fast
   API-tal. Mål throughput, p95/p99, poolventing, køalder, feil, DB-bruk og
   Authentik-helse. Start med eksisterande driftsmål i `docs/operations.md`:
   99 % aksepterte sendingar innan 750 ms og 99 % reconnect-innhenting innan
   fem sekund. Prøv fleire kanalar samtidig før eventuell chat-partisjonering.
   Ingen last- eller feilinjeksjonsprøver i produksjon i denne fasen.
6. **Ta ei avgjerd per forslag.** For pool-etterarbeid, API/worker-roller,
   push-drenering, lease/retry, varig realtime-replay, kanalpartisjonering og
   eventuell broker: vel *implementer no*, *mål først* eller *utset*. Før opp
   problemet, venta gevinst, enklare alternativ, driftskostnad, risiko,
   verifikasjon og tilbakeføring. Broker krev dokumentert behov for
   uavhengige mottakarar/replay eller mottak utan PostgreSQL; han løyser ikkje
   automatisk levering mellom database og broker.

## Krav til avgjerdsgrunnlaget

- Commit, PR, chart, image og faktisk utrulling heng saman; avvik har eigar
  og ein plan. Eit grønt Argo-statusfelt åleine er ikkje sluttbrukarprøve.
- Eit samla tilkoplingsbudsjett viser normal drift, realistisk overlapp under
  utrulling og reserve for identitetsteneste og vedlikehald.
- Feilscenarioa er spesifiserte: krasj før/etter commit, tapt `NOTIFY`,
  listener-reconnect, duplisert request-id, full lokal kø, lang sideeffekt,
  utløpt lease og rullering av API/worker/canary.
- Ingen kvitterte meldingar skal gå tapt. Retry skal ikkje skape ei ny logisk
  melding. Rekkefølgje per kanal må halde på tvers av replikaer. Eksterne
  sideeffektar kan bli prøvde på nytt; idempotens eller avgrensa duplikat må
  vere eksplisitt, ikkje lovnad om «exactly once».
- Dersom roller blir valde, må API-tal og worker-tal kunne endrast uavhengig.
  Dersom lokal kanalpartisjonering blir vald, må databasekoordinering framleis
  sikre rekkefølgje mellom poddar.

Etter denne vurderinga skal det finnast ei kort avgjerd per tiltak. Først då
kan ein skrive ein konkret implementerings- og utrullingsplan. Miljøspesifikke
innstillingar høyrer heime i GitOps-repoet; clusterbudsjett og drift av
PostgreSQL må dokumenterast hos infrastruktureigaren. Det private Rocket-
inventaret skal ikkje kopierast ukritisk inn i dette repoet.
