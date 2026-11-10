# Isolert Kubernetes-/Argo-prøve for Heart

10. november 2026. Knytt til #353/#345 og PR #404. Arkitektur og uavhengig lesing av hendingbevis: Astra. [Sanert prøvebevis med UID, resourceVersion og tidslinjer](evidence/heart-kubernetes-argo-2026-11-10.json). [Budsjett og driftsprosedyre](heart-rollout-budget.md).

Dette er ein rapport frå ei gjennomført, disponibel prøve. Han er ikkje ein installasjon av eit permanent utviklingskluster. Prøveressursane er rydda; lokale råloggar og arbeidsfiler ligg i den ignorerte `.local`-mappa i arbeidskopien. Ingen produksjonsressursar blei endra.

## Oppsett og avgrensing

| Del | Prøvd konfigurasjon |
| --- | --- |
| Vert | Windows ARM64, PowerShell 7, `wslc`; ingen Docker |
| VM | Eiga WSLC-økt `sproyt-heart-kube`, 4 CPU, 8192 MiB, 8 GiB dynamisk VHDX på S: |
| Kubernetes | K3s `v1.35.8+k3s1`, same versjon som kartlagd drift; éin testnode |
| Argo | Core `3.4.8`, pinna same image-digest som drift |
| Databasar | PostgreSQL `18.6-alpine`; separat Heart-rolle, ikkje superbrukar, `CONNECTION LIMIT 8` |
| Heart | To prod-liknande runtimepoddar og éin canary, pool 2, v2 aktivert; vedlikehald pool 1 |
| Sprøyt | Faktisk pinna ARM64-image; éin API-pod, eiga database, utviklingsautentisering, syntetisk brukar og løyndomar |
| Git/Argo | Lokal HTTP-Git med eigne chart-kopiar; AppProject avgrensa til denne repoen og `heart-prod`/`heart-canary` |
| Nett | Eigne pod-/service-CIDR `10.190.0.0/16` / `10.191.0.0/16`; test-pod-egress berre internt og til eigen API-server |
| Tilgang | Eigen kubeconfig, context `sproyt-heart-isolated`, localhost-port 64453; syntetisk app-API på 18080 |

Kube-system-UID-en blei lagra ved oppretting og kontrollert før kvar Python-klientsamling, saman med eksplisitt kubeconfig, context og endpoint. Ingen standardcontext blei brukt til mutasjon. Standard WSLC-økt blei halden utanfor; ulike Linux boot-ID-ar stadfesta at VM-ane var ulike. Namnerom, kvotar og NetworkPolicy kom i tillegg til VM-isolasjonen.

Prøva omfatta den faktiske gamle chart-revisjonen `0241b5b89b36cd00954e41a93a82f164f152ee08` og kandidaten `cde974480cc9c93bacda43cb6e9d0b12b5d539ed`. Dei lokale fixture-commitane og alle image-identitetar ligg i JSON-beviset. `wslc save` normaliserte manifestet til OCI og endra manifest-digesten. Importen kontrollerte **identiske image-konfigurasjons-SHA og rootfs-diff-ID-ar**, før han tagga/importerte dei til K3s sin eigen containerd-socket. Ingen ny kompilering eller nyare Heart-kode blei brukt.

Eksplisitte endringar berre i testkopiane: rollout-annotasjon, fem sekund preStop-sleep for runtime og utvalde vedlikehaldskommandoar, og ein parametriserbar jobbfrist som normalt var 180 sekund. Fristprøva brukte åtte sekund. Siste E2E-kopi fekk ein separat init-container med ti sekund sleep før ny Heart-runtime starta. Dette gav eit målbart vindauge etter gamle prosessar var avslutta. Poolgrenser, selektorar, Recreate og bootstrap-rekkjefølgje kom frå chart-kandidaten. Pausemålingane er difor ikkje estimat for nedetid i produksjon.

## Resultat og konkrete observasjonar

| Scenario | Resultat |
| --- | --- |
| Gammal canary → ny canary → ny prod | Begge applikasjonar friske; runtime-prosessar avslutta før erstatningspoddar blei oppretta |
| Samtidige fulle nye synkar | Fem rå DB-prøver med 6 runtime + 2 migreringar = **8**; begge friske etter frigjeving |
| Pod-retry i same Job | Ingen replacement under terminering; gammal pod terminal `rv2772`, ny oppretta `rv2782` |
| Sekvensiell bootstrap | Event-init avslutta med exit 0 før pilot-main starta (`rv2985` → `rv2989`) |
| Ekte Argo-avbrot og retry | Gate blokkerte med gammal hook; foreground-opprydding før ny migrering; éi pilotdefinisjonsrad etter retry |
| Job-frist | `DeadlineExceeded`, Argo stoppa i PreSync, runtime-UID uendra; korrigert retry frisk |
| Gyldig START og chat i prosessgap | Chat lagra; fire START-leveringsforsøk, éin fullført outbox-operasjon, éin startnøkkel/instans, éi started-hending; Heart `completed/done` |
| Opphavleg event-planlegging | **Feila**, eksisterande integrasjonsavvik beskrive under |

Ved første overgang var canary sine gamle prosessar terminale ved `rv1715` før ny pod ved `rv1722`; prod ved `rv1895` før `rv1902`. Alle desse runtime-prosessane avslutta med exit 0. Gamle **podobjekt** blei ikkje nødvendigvis garbage-collected før nye blei oppretta. Prosess- og sambandsbudsjettet krev terminale gamle prosessar, ikkje at objektet allereie er borte.

Job-retry og avbroten, blokkert bootstrap enda med exit **137** etter normal avsluttingsfrist. Dette er ikkje graceful hook-avslutting. Nye prosessar kom etter terminal forgjengar; ingen force-sletting blei brukt. Under abort-retry blei gamle Jobbar sletta ved `rv3259/3264`, gate passerte kl. 10:28:53.401 UTC, og ny migration kom ved `rv3307`, kl. 10:29:00.162.

Siste E2E: gamle runtime-prosessar var terminale ved `rv4092/4097`, nye poddar kom ved `rv4105/4111`, og dei nye Heart-containerane venta på init. Klienten sende START kl. **10:33:52.655 UTC**. Alle levande EndpointSlices for tenesta blei samla før ready-unionen blei vurdert; unionen var tom frå 10:33:45.997 til 10:34:03.614. Outboxen viser transportfeil under pausen og fullføring kl. 10:34:04.687. SQL-kontrollen viser éin startnøkkel som peikar til éin fullført Heart-instans, og éi `process.started`-hending. Sprøyt-lenkja sin status `active` er gjeldande lenkjesemantikk; Heart sin instansstatus er fullføringsasserten.

## Separat integrasjonsavvik: event-planlegging

Den opphavlege prøva kalla det reelle start-API-et med tittelmetadata. Sprøyt godkjende og lagra førespurnaden, men outbox-payloaden mangla `metadata.process_link_id`. [Definisjonen](../helm/sproyt/definitions/event-planning.yaml) krev denne verdien i Lua-steget `prepare`. [Prosesskontrakten](../processes/README.md) seier at applikasjonen leverer han; [PostgreSQL-adapteren](../src/db/postgres.rs) ved `enqueue_start` sender i staden `command.metadata` vidare etter at han har generert lenkje-ID-en.

Den prøvde Heart-revisjonen `11758e27a8db7fa22f7f3d81ff44cc0a01707b2a` kan persistere instansen før motoren feilar i Lua. Eit idempotent START-replay returnerer den eksisterande instansen. Resultatet var fullført START-outbox og `active` Sprøyt-lenkje medan Heart stod i `running/prepare`. Den faktiske payloaden og instansmetadataen ligg i beviset; instansen blei ikkje reparert eller manuelt stega.

Den minimale, gyldige fixture-definisjonen `fixture-start` (`start → done`, der `done` er `type: end`) blei brukt separat for å prøve leveringskontrakten under utrulling utan denne Lua-føresetnaden. Han ryddar ikkje avviket frå event-planlegging. Den separate oppfølginga har no retta den servergenererte korrelasjonsmetadataen i begge databaseadapterane og [prøvd den opphavlege flyten mot reell Heart på nytt](evidence/heart-event-planning-2026-11-10.json). Ho består for tittelmetadata, klientforfalska ID og utelaten metadata, inkludert retry etter Heart-pause. Dette endrar ikkje den historiske feilen eller beviset frå Kubernetes-økta. Retting av Heart sitt generelle replay etter motorfeil er ikkje del av Sprøyt-rettinga.

## Observasjonskvalitet og feila prøveforsøk

DB-prøver med `pg_stat_activity` blei tekne om lag kvart halvt sekund. Dette er observerte prøver, ikkje eit løfte om at polling ser kvar kortvarig topp. Chart-grensene og den tvinga samtidigheitsprøva støttar budsjettet i tillegg. Ingen observerte prøver var over åtte, og PostgreSQL-loggen hadde null «too many connections for role»-feil. Niande klient blei ikkje prøvd i Kubernetes; avvisinga er prøvd i det tidlegare [containerbeviset](evidence/heart-rollout-budget-2026-11-10.json).

Ein pod-watch stoppa kl. 10:27:07 UTC. Gjenopptaking frå siste resourceVersion fekk `410 Expired`; det manglande vindauget blir ikkje framstilt som gjenoppretta. Job-, Argo-, EndpointSlice- og DB-observasjon heldt fram. Abort/fristsjekkane brukte òg levande gate-/UID-snapshot. Pod-watch blei reparert med resourceVersion-basert gjenoppkopling, og siste E2E blei gjenteken med komplett podhistorikk. Rålogg-hash, dekning, feilmeldingar og dei utvalde vitnehendingane er lagra i JSON-beviset.

Tidlege prøveskript hadde for kort observasjonsfrist på foreground-sletting, handsama nullable EndpointSlice-lister feil, og den første business-asserten venta feilaktig at Sprøyt-lenkja skulle bli `completed`. Ny full sync etter avbrot blei flytta frå direkte patch av operation til den offisielle `argocd app sync --core --async --prune`; dette nullstiller gammal operasjonstilstand korrekt. Desse forsøka er bevarte som prøvefeil. Dei blir ikkje rekna som godkjende scenario.

## Gjenta oppsettet

Dette er oppsettsnotat for ein ny disponibel fixture, ikkje eit ferdig installasjonsskript. Bruk berre eit nytt isolert kluster og syntetiske data. Kontroller ledig plass på S: og C: og les `wslc --help` og relevante underkommandohjelper først.

1. Opprett ei ny WSLC-VM med det offisielle SDK-et `Microsoft.WSL.Containers` **3.0.3**. `wslc system session enter --datafolder` oppretta ikkje den nye disken i denne prøva. Ein liten .NET-host med target `net10.0-windows10.0.26100.0` brukte `SessionSettings(sessionName, storagePath)` med 4 CPU/8192 MiB og `VhdRequirements = new VhdOptions("storage", 8UL*1024*1024*1024, VhdType.Dynamic)`. Køyr `Session.Start()`, hald hosten levande, og avslutt økta eksplisitt etter prøva. Legg lagring og NuGet-cache på S:.
2. Last ned offisiell K3s ARM64-binær og checksum frå release `v1.35.8+k3s1`; verifiser SHA før køyring. Legg binær, data, kubeconfig og kubelet-root på den persistente disken `/var/lib/docker/heart-kube`. Køyr `k3s server` utan systemd, med `--prefer-bundled-bin`, eiga node/tls-san og CIDR-ane over; slå av Traefik, servicelb, metrics-server og local-storage. På denne WSLC-VM-en måtte `ip rule add priority 0 to 127.0.0.1/32 lookup local` gje gjesten lokal loopback før serverstart. Dette blei gjort berre inne i test-VM-en.
3. Publiser eigen API-port gjennom ein liten, merkt socat-container. Skriv separat kubeconfig og registrer kube-system-UID. Gjer eksplisitt kubeconfig/context/UID-kontroll til ein føresetnad for alle mutasjonar. Opprett fire namnerom, kvotar og egress-policy før applikasjonane.
4. Installer pinna Argo Core frå `https://raw.githubusercontent.com/argoproj/argo-cd/v3.4.8/manifests/core-install.yaml`, lokal Git-teneste og PostgreSQL. Opprett Heart-rolle med cap åtte og separat Sprøyt-rolle/database. Importer kvar pinna app-image separat og kontroller konfigurasjon/rootfs-identitet. Bruk K3s-socket `/run/k3s/containerd/containerd.sock` og namespace `k8s.io` eksplisitt; standard `ctr` såg feil image-store i denne VM-en. Legg til kanonisk image-alias dersom CRI normaliserer `import-*` til `docker.io/library/import-*`.
5. Lag lokal Git-kopi av berre chartet frå dei to kjelderevisjonane over. Registrer alle testinjeksjonar eksplisitt. Opprett to manuelle Argo-applikasjonar med foreground-pruning, replika 2/1 og `HEART_RUNTIME_V2_ENABLED=true`. Sprøyt bruker utviklingsautentisering; chart-valideringa kravde likevel ikkje-tomme OIDC-felt og syntetisk client secret/session key. Ingen driftsnøklar skal kopierast.
6. Start separate watch-ar for Pods, Jobs, Applications og EndpointSlices og DB-polling før første gamle sync. Lagre UID, owner-UID, resourceVersion, container start/terminal state og tidsstempel. Gjenoppta watch frå siste resourceVersion; marker eventuelle hol eksplisitt. Samle alle levande slices per teneste før ready-settet blir vurdert.
7. Køyr gammal baseline, stegvis overgang, samtidige nye fulle synkar, same-Job retry, Argo-avbrot/oppryddingsport og tidsfrist med korrigert retry. Tabellåsar i den syntetiske databasen held runtime- og migreringssamband opne; ein avgrensa trigger/advisory-lås kan halde pilot-bootstrap. Frigjev berre eigne låseklientar. Slett hooks med foreground og verifisert UID, aldri force.
8. Køyr ekte chat-/prosess-API under eit verifisert prosessgap. Kontroller outbox-status og forsøk, éi start-hending, éin Heart-startnøkkel/instans og faktisk Heart-node/status etter recovery. Bruk både original forretningsflyt og ein separat minimal gyldig prosess; før resultata kvar for seg.
9. Render og valider dei faktiske fixture-prod/canary-verdiane med `tools/verify-heart-rollout-budget.py`. Arkiver sanert bevis før `wslc --session <eiga-testøkt> system session terminate`. Kontroller eksakt eigd VM-sti og at økta er avslutta før sletting av VHDX. Kontroller standardøkt og ledig plass etterpå.

Kjelder: [WSLC SDK](https://learn.microsoft.com/en-us/windows/wsl/wsl-container), [sesjonsinnstillingar](https://learn.microsoft.com/en-us/windows/wsl/wsl-container-settings), [K3s konfigurasjon](https://docs.k3s.io/installation/configuration), [Kubernetes Recreate](https://kubernetes.io/docs/concepts/workloads/controllers/deployment/#recreate-deployment), [Job-replacement](https://kubernetes.io/docs/concepts/workloads/controllers/job/#delayed-creation-of-replacement-pods), [Argo sync-waves](https://argo-cd.readthedocs.io/en/release-3.4/user-guide/sync-waves/).