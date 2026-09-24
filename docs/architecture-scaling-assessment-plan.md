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

- Delt PostgreSQL-pool er versjonert i Sprøyt-commit `997eec6`, no i `main`
  via [#161](https://github.com/hbjoroy/sproyt/pull/161). Han deler pool mellom repository, push og
  bilethandsaming, set maksimum og ventetidsgrense og har PostgreSQL-prøver.
  Dette er den avgrensa delen av tiltak 1 som alt er implementert.
- Kjelde-PR-ane #158–#161 var opphavleg ein stabel. Dei er no fletta i
  `main` med merge-commitar som bevarer dei utrulla commitane. Ei arbeidsmappe
  på ei eldre grein vil framleis ikkje vise poolen utan å oppdatere grein.
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

Dette starta som ein avgrensa, lesande kontroll av GitHub, GitOps og
Kubernetes-context `default`, pluss offentlege versjons-/readiness-endepunkt.
Kontrollen omfattar ikkje
autentisert chat, lasttest eller måling av PostgreSQL under belastning.

| Kjelde | Stadfesta tilstand |
| --- | --- |
| Sprøyt `main` før samanslåing | `59e06d5`. Pool-commitet `997eec6` var ikkje fletta dit. |
| Kjelde-PR-stabel ved første kontroll | [#158](https://github.com/hbjoroy/sproyt/pull/158) GFM → `main`; [#159](https://github.com/hbjoroy/sproyt/pull/159) React-avslutning → #158; [#160](https://github.com/hbjoroy/sproyt/pull/160) SSE → #159; [#161](https://github.com/hbjoroy/sproyt/pull/161) delt pool → #160; [#162](https://github.com/hbjoroy/sproyt/pull/162) denne planen → #161. Alle var opne og mergeable ved første kontroll. Dei to vanlege CI-testjobbane viste grønt; fleire release-jobbar var markerte som hoppa over i PR-køyringane. |
| GitOps `main` | `b89af0b`; [#155](https://github.com/hbjoroy/rocket-applications/pull/155) og [#156](https://github.com/hbjoroy/rocket-applications/pull/156) er fletta. Både produksjon og canary har chart-kjelde låst til `997eec6` og image-digest `sha256:97e7191f2a5a8b6fad3e4dad84990cfcef7206ac09fdac9cc1a64d441212e73c`. Migreringsimaget er separat pinna. |
| Levande Argo og Kubernetes | Begge applikasjonar er `Synced/Healthy`. Argo viser GitOps `b89af0b` og chart `997eec6`. Produksjon har 2/2 klare poddar og canary 1/1; alle tre brukar image-digesten ovanfor. ConfigMap-taka er høvesvis 4 og 2. |
| Ekstern teneste | `/versionz` returnerte `997eec6` og `/readyz` HTTP 200 for begge miljø. |

**Samanslåing av kjeldehistorikken:** #158, #159, #160 og #161 vart fletta i
denne rekkjefølgja med merge-commit. Basen for neste PR vart sett til `main`
etter kvart steg; filsettet i PR-diffen var likt før og etter. Sprøyt `main`
er no `c0de238` etter dokument-PR #162 og inneheld både `ab7531f` (SSE) og `997eec6` (delt pool)
som forfedrar. `src/`, `frontend/` og `helm/sproyt/` i `main` er identiske
med det utrulla `997eec6`-treet.

Dette lukkar avviket der CD peika på kode utanfor `main`. GitOps peikar framleis
på den same verifiserte chart-commiten og image-digesten; ingen ny appversjon
vart rulla ut ved samanslåinga. Den lokale Rocket-klonen var eldre ved første
kontroll, men vart fast-forwarda til GitOps `main` (`b89af0b`).

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
   [Første samla måling](database-capacity-assessment-2026-09-24.md) er gjord;
   dokumenterte tak og poolventemålingar for alle klientane står att før
   budsjettet kan godkjennast.
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
