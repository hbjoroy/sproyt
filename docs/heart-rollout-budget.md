# Heart sitt utrullingsbudsjett

Dato: 10. november 2026. Arkitekturvurdering: Astra. Status: **chart-kandidat med isolert Kubernetes-/Argo-budsjettprøve; ikkje utrulla. Prosessintegrasjonsavviket er retta og prøvd separat**. Forelder #353/#345. [Kapasitetskartlegginga](database-capacity-assessment-2026-11-10.md) er historisk grunnlag for avviket.

## Avgjerda og budsjettet

Behald Heart sitt pooltak to og databaserolla sitt tak åtte. Bruk `Recreate` for Heart, sekvensiell definisjonsbootstrap og høgst éin vedlikehaldspod per release. Alternativet `maxSurge: 0` med rullerande oppdatering avgrensar ikkje samband frå terminerande poddar. Større rolletak eller ein ny global koordinator er ikkje nødvendig for dette kontrollerte scenarioet.

| Samtidige forbrukarar | Sambandstak | CPU-/minnetak |
| --- | ---: | --- |
| Produksjon: 2 runtime × pool 2 | 4 | Kvar 500m / 256 MiB |
| Canary: 1 runtime × pool 2 | 2 | Kvar 500m / 256 MiB |
| Høgst 1 vedlikehaldsprosess per release | 2 | Kvar 500m / 256 MiB |
| **Planlagd topp** | **8** | **2,5 CPU / 1280 MiB samla Heart-tak** |

Vanleg drift har to ledige rolleplassar til vedlikehald. Ved toppen åtte er det **ingen reserve for ein ekstra klient av denne rolla**. Backup eller administrator må ikkje låne Heart-rolla utan eit nytt budsjett. Dette er ikkje eit godkjent budsjett for heile PostgreSQL-instansen med 97 vanlege plassar.

For kvar release er rekkjefølgja migrering (wave -4) → definisjons-ConfigMap (-3) → bootstrap (-2). Med pilot på fullfører ein vanleg init-container event-planlegging før hovudcontaineren bootstrapp-ar piloten; utan pilot finst berre event-steget. Begge jobbar har `parallelism=1`, `completions=1`, `restartPolicy=Never`, `podReplacementPolicy=Failed`, 180 sekund samla tidsfrist og 30 sekund avsluttingsfrist. Retry ventar på fullført terminering. Jobbar nyttar same ressursramme og pinna Heart-image som runtime, med eksplisitt `HEART_DB_MAX_CONNECTIONS=1`.

Runtime-poddar får `component=runtime`; Service og PDB vel denne komponenten. Vedlikehald får `component=maintenance`. Deployment sin eksisterande immutable selector er bevart. NetworkPolicy omfattar framleis begge, slik at jobbar kan nå databasen.

## Føresetnader og konsekvensar

- Budsjettet gjeld kontrollerte **fulle Argo-synkar**, éin operasjon per release. Under ein normal oppdatering ventar Recreate på gamle runtime-poddar før nye blir laga. Manuell pod-sletting, force-sletting, ekstra klientar og utskifting etter nodefeil ligg utanfor denne garantien.
- Det blir ein Heart-pause under oppdatering. PDB gjer ikkje Recreate til ei oppdatering utan avbrot. Vanleg chat held fram; prosessoperasjonar ventar i den varige outboxen og skal fullføre etterpå utan duplikat.
- Etter avbroten eller feila sync: vent til **alle** gamle vedlikehaldspoddar i begge miljø er borte, også ei definitions-jobb frå ei seinare wave. `BeforeHookCreation` gjeld same ressursnamn; det er ikkje ein lås mellom migrering og gamle definition-jobbar.
- Effektiv Argo sync-policy må bruke foreground-sletting. `PrunePropagationPolicy=background/orphan`, `Force`, `Replace`, selektiv sync eller fleire parallelle Helm-operasjonar er ikkje del av det prøvde budsjettet. Chart- og appnivå-overstyringar må kontrollerast.
- Ein vedlikehaldsfeil stoppar sync før runtime-oppdatering. Ved tidsfrist/retryfeil blir årsaka retta før ny full sync; ikkje tving fram neste wave eller auk databasetaket.

## Overgang og tilbakeføring

Dette er planen for ei seinare driftsendring. **Den isolerte controller-/budsjettprøva er no gjennomført**, med gammal → ny canary → ny produksjon, samtidige synkar og avbroten bootstrap med full opprydding før retry. Original event-planlegging avdekte eit separat integrasjonsavvik; rettinga og ei ny reell prosessprøve er dokumenterte nedanfor. Ingen driftsendringar er utførte her.

1. Før ei endring: sjekk at begge Argo-applikasjonane har avslutta siste operasjon; ingen gamle vedlikehalds-/terminerande Heart-poddar og ingen ekstra klientar av Heart-rolla. Stadfest faktiske pooltak, rollegrense åtte og effektive sync-options. Køyr render-validatoren mot **dei faktiske prod/canary-verdiane**, ikkje berre chart-standardar.
2. Lever først til **canary åleine**, med produksjon i ro. Vent på ferdige hooks, fjerna gamle poddar, rette Service-endepunkt og frisk ny Heart. Lever deretter produksjon på same måte. Gamle og nye hook-former må ikkje oppdaterast samstundes i denne første overgangen.
3. Første nye PreSync køyrer medan den **gamle Service-selectoren** framleis gjeld og kan velje ein vedlikehaldspod. Når den nye selectoren blir brukt, fell gamle umerkte runtime-poddar ut av endepunkta. Rekn denne første overgangen som ein Heart-pause frå PreSync til nye runtime-poddar er klare; selector-rettinga vernar først seinare synkar.
4. Etter begge kontrollerte driftsstega: stadfest rette endepunkt, rolletopp ≤8 og chat/prosess-recovery. Seinare samtidige synkar er berre innanfor budsjettet når begge miljø brukar den nye kontrakten og den isolerte samtidigheits-/avbrotsprøva allereie består.
5. Tilbakeføring bevarer Recreate, sekvensielle jobbar, pooltak og komponentveljarar; eit kompatibelt image kan pinnast tilbake. Ikkje bruk blind chart-/Deployment-rollback som gjeninnfører gamle veljarar eller rullerande overlapp. Ingen datamigrering eller databasegrense blir endra av dette chart-steget.

Lesande sjekk før/etter sync:

```powershell
kubectl get applications -n argocd sproyt sproyt-canary -o json
kubectl get pods -n sproyt -l app.kubernetes.io/name=heart -o wide
kubectl get pods -n sproyt-canary -l app.kubernetes.io/name=heart -o wide
kubectl get jobs -n sproyt -l app.kubernetes.io/name=heart -o json
kubectl get jobs -n sproyt-canary -l app.kubernetes.io/name=heart -o json
kubectl get endpointslices -n sproyt -l kubernetes.io/service-name=sproyt-sproyt-heart -o json
kubectl get endpointslices -n sproyt-canary -l kubernetes.io/service-name=sproyt-canary-sproyt-heart -o json
```

Avbryt utrullingssteget dersom føresetnadene ikkje held. Ikkje force-slett poddar for å få rekneskapen til å sjå rett ut. Drift/Heart-eigar eig utrullingsporten og må namngjevast før driftsendring.

## Prøvebevis og det som står att

Det eksakte utrulla Heart-imaget `sha256:fe2eb544a14006504dbbb20e2c0a3489a0035eababefc5c93dba9f10c7ec1955` har entrypoint `heart-api`, ARM64 og revisjon `11758e27a8db7fa22f7f3d81ff44cc0a01707b2a`. Kjelda ved denne revisjonen avgrensar migrering og bootstrap til eitt samband; dette er ikkje ei slutning frå nyare lokal kode. Ingen privat Heart-kode eller nytt image trengst.

[Isolert image-/databasebevis](evidence/heart-rollout-budget-2026-11-10.json): PostgreSQL 18.6, eigen ikkje-superbrukarrolle med `CONNECTION LIMIT 8`, tre Heart-runtimeprosessar med pool to og v2-arbeidar aktivert som i drift. Kontrollerte tabellåsar held to databasekall per runtime og begge vedlikehaldskommandoane samtidige. Migrering, event-bootstrap og pilot-bootstrap når åtte; eit niande samband blir prøvd under migrering og avvist. Etter frigjeving fullfører alle blokkerte HTTP-kall og jobbar. Feila bootstrap og retry etter prosessavslutting gir ingen ekstra definisjon. Gamle runtimeprosessar blir haldne av trege kall, avsluttar med exit 0 og frigjev sambanda før erstatningane startar. Prøveinstansar, nettverk og anonyme volum er rydda.

Ti render-kontraktar dekkjer pilot av/på, samtidige budsjett, usikre replika-/strategi-overstyringar, retry, tidsfristar, sekvens, pool-/ressursgrenser, endepunkt og immutable selector. Streng server-dry-run på dagens Kubernetes 1.35.8 godtek Deployment-/Service-/PDB-endringane og begge jobbar. Den eksisterande Rust-kontrakten `heart_unavailable_does_not_interrupt_chat_and_recovers_once` består på nytt.

[Isolert Kubernetes-/Argo-bevis](evidence/heart-kubernetes-argo-2026-11-10.json) og [prøveoppsett/metode](heart-kubernetes-argo-test.md): ei eiga ARM64 WSLC-VM køyrde K3s **1.35.8+k3s1** og Argo **3.4.8**, med lokal Git, syntetiske løyndomar og PostgreSQL 18.6. Hjarta i prøva var dei faktiske chart-revisjonane og dei same kompilerte Heart-/Sprøyt-imaga som i drift; WSLC si manifestnormalisering er dokumentert med identisk image-konfigurasjon og rootfs-digestar.

- Første stegvis selector-overgang og seinare samtidige fulle synkar blei `Synced/Healthy`. Gamle runtime-**prosessar** var terminale med exit 0 før nye poddar blei oppretta. Gamle podobjekt kunne framleis vente på garbage collection.
- Samtidig prod/canary nådde **åtte samband**: seks runtime og to migreringar. Fem rå databaseprøver målte denne toppen; ingen av alle databaseprøvene gjekk over åtte, og PostgreSQL logga ingen rollegrensefeil.
- Same Job fekk ny pod først etter at den gamle var terminal. Ekte `argocd app terminate-op --core`, full foreground-opprydding og ny full sync bestod; piloten hadde éi definisjonsrad etter retry. Blokkerte hooks enda med exit 137 etter normal avsluttingsfrist. Ingen force-sletting blei brukt.
- Ein testforkorta åttesekunds Job-frist stoppa Argo i PreSync og bevarte runtime-UID-en. Korrigert retry med normal frist 180 sekund blei frisk.
- Under ein controllerstyrt pause sende klienten ei chatmelding og ein gyldig minimal `start → end`-prosess **etter** at begge gamle Heart-prosessane hadde stoppa. Chatmeldinga blei lagra; START venta, fekk fire leveringsforsøk og gav éin fullført outbox-operasjon, éin startnøkkel/instans og éi start-hending. Heart nådde `completed/done`; Sprøyt-lenkja blir verande `active`, som er gjeldande lenkjesemantikk.

**Separat avvik:** den opphavlege event-planlegginga bestod ikkje. Sprøyt sende START utan `metadata.process_link_id`, som Lua-steget `prepare` krev. Den prøvde Heart-revisjonen kan då lagre ein instans som blir ståande `running/prepare`; eit idempotent START-replay kan returnere den eksisterande instansen utan å fullføre flyten. Den feila instansen og outbox-payloaden blei bevarte uendra. Den minimale START-prøva godkjende ikkje denne forretningsflyten. Sjå det konkrete funnet og kjeldestadene i prøverapporten.

**Rett og prøvd på nytt:** begge databaseadapterane lagrar no servergenerert `process_link_id` i START-payloaden, overskriv klientverdien og bevarer ID/payload ved replay. [Ny event-planleggingsprøve](evidence/heart-event-planning-2026-11-10.json) med fersk Sprøyt-binær, faktisk pinna Heart og PostgreSQL fullfører den uendra originale definisjonen for tittelmetadata, falsk klient-ID og utelaten metadata. Alle når `wait-for-decision`, tek imot «ja» og endar `completed/completed`. Eitt scenario beviser varig retry og chat under Heart-pause. Dette er ei ny isolert app-/prosessprøve; Kubernetes-controllerprøva er ikkje køyrd om att. Historiske feila instansar/outbox-rader er ikkje skrivne om.

Observasjonsavgrensing: pod-watch stoppa kl. 10:27 UTC, og gjenopptakinga fekk `410 Expired`. Job-, Argo-, EndpointSlice- og databaseobservasjon heldt fram. Avbrot/fristscenarioet er støtta av desse straumane og eksplisitte levande gate-/runtime-UID-sjekkar. Den siste E2E-prøva blei gjenteken etter reparert pod-watch og har komplett podrekkjefølgje. Tidlege feil i prøveskriptet er førte i beviset. Dette er ikkje ein påstand om uavbroten poddekning gjennom heile økta.

Den disponible VM-en, Kubernetes-ressursane, testdatabasane og proxyane er avslutta og rydda. Standard WSLC-økt og produksjonscontext er urørte. #353 står framleis open for samla PostgreSQL-budsjett og ansvarleg driftseigar; integrasjonsrettinga må følgje med ved ei seinare levering av event-planlegging. Ingen driftsutrulling er utført.

Gjenta lokalt (PowerShell 7, `wslc`; les relevant underkommandohjelp fyrst):

```powershell
python -m pip install --target .local/heart-rollout-python -r tools/requirements-heart-rollout-tests.txt
$env:PYTHONPATH=(Join-Path (Get-Location) .local/heart-rollout-python)
python tools/test-heart-rollout-budget.py
python tools/verify-heart-rollout-budget.py <rendert-prod.yaml> <rendert-canary.yaml>
wslc pull --quiet oci.bjoroy.me/sproyt/heart@sha256:fe2eb544a14006504dbbb20e2c0a3489a0035eababefc5c93dba9f10c7ec1955
node tools/heart-rollout-probe.mjs --confirm-isolated
```

Grunnlag: [Kubernetes Recreate](https://kubernetes.io/docs/concepts/workloads/controllers/deployment/#recreate-deployment), [Job-replacement](https://kubernetes.io/docs/concepts/workloads/controllers/job/#delayed-creation-of-replacement-pods) og [Argo sync-fasar](https://argo-cd.readthedocs.io/en/release-3.4/user-guide/sync-waves/).