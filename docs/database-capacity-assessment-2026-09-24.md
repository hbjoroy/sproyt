# Databasekapasitet: første samla budsjett

Status: kartlegging 24. september 2026, **ikkje eit godkjent kapasitetsbudsjett**.
Dette er steg 2 i [arkitekturvurderinga](architecture-scaling-assessment-plan.md).
Kjelde er Sprøyt `main`, gjeldande GitOps-konfigurasjon og avgrensa, lesande
målingar i PostgreSQL og Kubernetes-context `default`. Det private
Rocket-inventaret har meir historikk om utfallet 23. september. Ingen
lastprøve, konfigurasjonsendring eller produksjonsutrulling er gjort her.

## Grense og observert bruk

PostgreSQL rapporterer `max_connections=100`, `reserved_connections=0` og
`superuser_reserved_connections=3`. Dermed har vanlege klientar høgst **97
plassar**. Ingen innloggingsrolle har ei eiga `rolconnlimit`. Bakgrunnsprosessar
er ikkje talde som vanlege klientar nedanfor.

Tre punktprøver kring 22.55–22.57 CEST fann **64–68 vanlege
klienttilkoplingar**, etter at kontrollklienten er trekt frå. Ved den siste
prøva var fordelinga slik:

| Gruppe | Observerte samband | Kjent konfigurasjonstak |
| --- | ---: | --- |
| Sprøyt API/realtime, produksjon og canary | 9 (10 i ei tidlegare prøve) | 13 normalt: 2 × (4 i delt pool + 1 lyttar) + 1 × (2 + 1) |
| Sprøyt Heart | 2 | Ukjent pooltak; to poddar og éin ekstra ved rullering |
| Authentik | 14 | Ukjent pooltak; éin server og éin worker, kvar med mogleg ekstra pod ved rullering |
| Grafana | 4 | `max_open_conn=10` i levande konfigurasjon |
| Andre applikasjonar | 39 (34 i ei tidlegare prøve) | Ikkje samla; fleire separate tenester og databasar |

Den siste punktprøva gir 97 − 68 = **29 ledige vanlege plassar i det
augeblikket**. Ho seier ikkje kor høgt samtidige toppar blir, og er ikkje eit
kapasitetsbevis. Ei tidlegare hending tømde dei vanlege plassane og felte
Authentik. Ein fast reserve for identitet, backup og drift er enno ikkje
fastsett. Eksisterande overvaking har heller ikkje ein PostgreSQL-exporter
eller historisk serie for tilkoplingsbruk.

## Kva utrulling kan krevje

Sprøyt sitt Helm-chart tillèt éin ekstra API-pod ved rullering i produksjon
og éin ekstra canary-pod. Deira kjende konfigurasjonstak kan då auke frå 13
til **18** med berre produksjonsrullering, eller **21** dersom begge overlappar.
Dette er app-poolar og realtime-lyttarar, ikkje Heart eller migreringsjobbar.

Som *illustrasjon*, ikkje prognose: start ved den siste målte bruken på 68;
ein samtidig produksjons- og canary-rullering kan leggje til inntil 8 Sprøyt-
samband. Om nye Authentik-poddar samstundes held like mange samband som dei
nolevande, blir summen 68 + 8 + 14 = **90** og berre 7 vanlege plassar står
att. Dette reknar ikkje med migrering, Heart-rullering, backup eller endringar
i andre appar. Verken Authentik-observasjonen eller låg Sprøyt-bruk er øvre
grenser for dei tenestene.

`sproyt migrate` hadde tidlegare ein vanleg SQLx-pool med standardtak 10 og
ein separat realtime-lyttar. **11 var eit konfigurasjonstak, ikkje målt eller
sannsynleg normalbruk:** SQLx-migratoren nyttar normalt éi pooltilkopling om
gongen, pluss lyttaren. Ein eigen migreringspool med maks éi tilkopling og
utan lyttar avgrensar no kvar Sprøyt-migreringsjobb til éi tilkopling. Dette
sparer normalt berre éi tilkopling per jobb og løyser ikkje clusterbudsjettet.
Produksjon og canary kan ha kvar sin pre-upgrade-jobb; Heart har dessutan ein
eigen migreringsjobb. Desse må framleis reknast med før fleire samtidige
utrullingar blir tillatne.

## Målehol og konklusjon

- Sprøyt-poolen har fem sekund acquire-timeout, men `/metrics` eksponerer
  ikkje poolstorleik, ventarar, ventetid eller timeoutar. PostgreSQL kan ikkje
  sjå førespurnader som ventar *inne i* klientpoolen. Vi kan derfor ikkje
  stadfeste at taket på fire held sendemålet under reell last.
- `application_name` er tom for dei fleste sambanda, også Sprøyt. Podmapping
  var mogleg ved å samanlikne kortlevde interne klientadresser med Kubernetes,
  men dette er ikkje ei robust driftsmåling. Ingen IP-adresser eller Secret-
  verdiar er lagra i dette dokumentet.
- Authentik, Heart og fleire andre applikasjonar manglar dokumenterte,
  kontrollerte makstal i denne kartlegginga. Fleire éin-pod-deployments tillèt
  ein ekstra pod kvar ved rullering. PostgreSQL har ikkje rollegrenser som
  reserverer plass for Authentik.
- Avgrensa søk i tilgjengelege Sprøyt- og Authentik-loggar fann ingen nye
  treff på tilkoplingsplass- eller pool-timeout-feil. Lokal logghistorikk er
  ikkje komplett; null treff er ikkje prov på null feil.

**Avgjerdsgrense:** Ikkje auk Sprøyt-replikaer eller pooltak på grunnlag av
desse punktprøvene. Eit godkjent clusterbudsjett må få dokumenterte tak for
Authentik, Heart, banktenestene, backup og migrering, eit realistisk
utrullingsscenario og ein uttrykkeleg driftsreserve under 97-plassgrensa.

Første avgrensa oppfølging er eit eige tilkoplingstak for Sprøyt-migrering
utan realtime-lyttar. Vidare trengst måling av poolventing/timeouts og
namngjeving av Sprøyt-sambanda. Etterpå kan ein samle tidsseriar og gjere ei
kontrollert kapasitetsprøve utan å risikere produksjonsdatabasen. Val av
konkrete tal og eventuell eksportør/varsling høyrer til ei eiga implementerings-
og driftsavgjerd; denne kartlegginga endrar ikkje dei tala.
