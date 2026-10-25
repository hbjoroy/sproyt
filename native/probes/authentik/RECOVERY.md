# N01f1: e-postbevis og feilgrenser for recovery

Dato: 25. oktober 2026. **Recovery-porten er ikkje godkjend.** Denne etappen
har reproduserbare SMTP-/lenkjeprøver, men fann ein feilbane der ei
policybasert sperre kan hoppast over. Ingen produksjonsflyt er endra, og
prøva gir ikkje grunnlag for å aktivere native recovery eller N03.

## Avgrensa leveranse

`start-lab.ps1 -Recovery` lagar ein ny localhost-lab med same pinna Authentik
2026.8.2/PostgreSQL som N01c–e, og Mailpit
`v1.32.0@sha256:0790692be935ea4420d085694f7985b1f692ca6a8771ab0e0bd8ba75ac180498`.
Mailpit-imaget er eit kontrollert OCI-index med ARM64 og AMD64. Lokal Windows
bruker WSLC; Linux CI bruker byggverten sin Docker.

SMTP går berre på internt nettverk til Mailpit:1025. Fangaren har ingen
relékonfigurasjon. Berre web/API på `127.0.0.1:19093` blir publisert, og alle
adressene er syntetiske `example.invalid`-adresser. Meldingsinnhald, lenkjer,
token og cookiear blir ikkje skrivne til logg eller CI-artefaktar. Fangaren
har ei minnegrense på 128 MiB og ei halv CPU; stopprutina kontrollerer eksakt
namn/eigarlabel for dei fire eigne containerane og bevarer evidens/volum.

To eigne recovery-flytar går frå identifikasjon til e-postbevis og deretter
Authentik si WebAuthn-registrering. Ingen av dei krev den tapte nøkkelen eller
eit passord, opprettar brukarar eller reaktiverer ein deaktivert konto.
Automatiske prøver stoppar ved registreringsutfordringa: dei lagar ikkje
ein nøkkel eller eit forfalska WebAuthn-svar. Endeleg ny passnøkkel, fersk
innlogging og autentisert recovery-kvittering er ikkje godkjende her.

## Mottakar og e-postbevis

Autoriteten i denne laben er ei administratorprovisjonert allowlist i den
versionerte policyen: eksakt syntetisk Alice-UUID og adresse. Dette er ikkje
ein produksjonsmodell for lagring av stadfesta adresser. Brukarredigerbare
profilattributes er med vilje ikkje autoriteten.

Innleiande opptak kontrollerer aktiv konto og samsvar med den lagra adressa,
og set mottakaren frå serverdata. Ukjend, ikkje godkjend eller deaktivert
brukar får same «e-post sendt»-utfordring utan faktisk SMTP-sending. Ved
lenkjeretur blir brukar, flow og opphavleg mottakar kontrollerte på nytt mot
ferske serverdata. Ei endra adresse skal ikkje arve eit gammalt e-postbevis.

Den ordinære Authentik EmailStage, worker-kø og SMTP-backend blir brukte.
Testen les berre den eigne Mailpit-API-en. Retur og samtykke går gjennom
FlowExecutor med dei verkelege serverutferda tokena; denne test-API-en blir
ikkje lagd inn i native klienten.

FlowExecutor-prøvene bruker DjangoClient mot dei verkelege API-visningane og
databasen. Dei er serversidekontraktar, ikkje browser-/CSRF-aksept eller
bevis for faktisk passnøkkelregistrering.

Den konfigurerte tokenlevetida er fem minutt. Pinna EmailStage legg til eitt
minutt for visingsavrunding; dette må takast med når endeleg policy blir vald.
Tre innleiande sendingar og tre resendingar har separate femminuttsbudsjett.
Dette er ikkje marknadsført som eitt felles budsjett på tre e-postar.

## Observerte utfall

| Scenario | Målt utfall |
| --- | --- |
| Godkjend Alice, også med injisert mottakarfelt | Berre rett adresse får faktisk SMTP-melding |
| Samtykke frå gyldig e-postlenkje | Når registrering av ny WebAuthn-nøkkel utan gammal MFA |
| Ukjend, ikkje godkjend Bob eller deaktivert Alice | Same utfordring, ingen SMTP-sending |
| Utgått/brukt lenkje, endra adresse eller feil pending user | Når ikkje nøkkelregistrering |
| To restaurerte kopiar før noko samtykke, standard EmailStage | **Begge** når nøkkelregistrering |
| Same fork med ekstra databaseclaim | Berre eitt opptak |
| To HTTP-forsøk med barrier rett før verkeleg claim-innsetjing | Berre eitt opptak |
| Vanleg evaluatorfeil eller feil i claim-lagringa | Deny, ingen ny device |
| Bindingstimeout eitt sekund, faktisk policyarbeid to sekund | Timeout blir ikkje handheva; setup blir vist |
| Feil i den ytre PolicyProcess-wrapperen | **Deny-sperra blir hoppa over; setup blir vist** |

Ingen av feilprøvene oppretta ein device. Dei observerer om ein nettlesar
ville blitt sleppt til registreringsutfordringa. Tilstanden og dei mellombelse
policy-/bindingendringane blir tilbakeførte etter kvart feilforsøk.

Den første raceprøva bruker to restaurerte planar før sekvensielle POST-ar.
Ei separat prøve bruker to HTTP-trådar og ein barrier før den verkelege
`get_or_create`-operasjonen mot PostgreSQL. Ho verifiserer opptak under
overlappande forsøk, utan å erstatte databaseinnsetjinga med ein mock.

## Kvifor databaseclaim ikkje er nok

Labtillegget avleier claim frå serververifisert e-posttoken og eksakt brukar.
`Token.identifier` har databasehandheva unikskap. Eigar er bunden til
browserøkt og ein servergenerert forsøks-ID som først blir laga etter
restaurering; han blir ikkje kopiert inn i alle forkane frå e-posttokenet.
Same-plan-retry kan bruke same claim, medan ein annan plan blir avvist.
Tombstonen er ikkje tidsutløpande, slik at opprydding ikkje opnar gamle
restaurerte planar igjen. Dette er eit isolert admission-eksperiment, ikkje
ei recovery-fullføringskvittering.

Alle sperrene blir reevaluerte ved faktisk overgang. Pinna ReevaluateMarker
slår av policycache. Vanlege evaluatorfeil blir avviste gjennom negert
ALLOWED-policy på Deny-stage, og databasefeil blir eksplisitt handsama.
Men den ytre `PolicyProcess.run()`-feilbanen gir `PolicyResult(False)` etter
den vanlege negasjonen og `failure_result`-handsaminga. Då kan Deny-stage bli
fjerna frå planen. Dette vart demonstrert med avgrensa feilinjeksjon i nettopp
den wrapperen. Ein databaseclaim kan ikkje verne ei overgang som motoren
hoppar over.

Timeout-prøva bruker faktisk venting og ein ytre testfrist på ti sekund.
Inline-køyringa returnerte etter arbeidet, trass bindingstimeout eitt sekund.
Dette er ei separat observasjon frå å kaste ein `TimeoutError` i ein expression.

## Køyring og kvitteringar

```powershell
./native/probes/authentik/start-lab.ps1 -ArtifactsDirectory S:/Build/sproyt-authentik-recovery -Recovery
./native/probes/authentik/run-recovery.ps1 -RunDirectory <ny-labmappe>
./native/probes/authentik/stop-lab.ps1 -RunDirectory <same-mappe>
```

CI køyrer både N01e sin konfigurasjonskontrakt og N01f1 på denne eigne laben.
Berre ikkje-hemmelege `evidence/receipt.json` blir lasta opp. Kvitteringa har
`passed=true` for gjennomførte, pinna **observasjonstestar**, men uttrykkeleg
`recoveryGateApproved=false`, `fullRecoveryVerified=false` og
`hardwarePasskeyVerified=false`. Grøn CI betyr at funna er reproduserte; ho
godkjenner ikkje recovery-sperra. Ei seinare versjonsendring som endrar funna,
krev ny vurdering av assertane.

## Neste avgrensa avgjerd før meir implementasjon

N01f er framleis open. Før ein tilpassa serverutviding blir laga, må vi velje
ein vedlikehaldbar integrasjonsveg og få han revidert:

1. Avklar ei støtta, feillukka overgang til nøkkelregistrering som ikkje
   kan hoppast over som ein vilkårleg policystage. Vurder verifisert
   provideroppgradering eller ei avgrensa serverutviding som eig opptaket.
2. Bind e-postbevis, recovery-ID og den **faktisk oppretta** nye nøkkelen til
   kvarandre. Standard registreringsstage gir ikkje i seg sjølv Sprøyt ei
   autentisert, varig recovery-fullføringskvittering.
3. Bevis ny passnøkkelinnlogging og ei støtta serverkopling til Sprøyt.
   Events-API/webhook kan vere transport, men generelt login-event eller
   eit profilflagg er ikkje automatisk prov på fullført recovery.
4. Prøv idempotent epoch-auke, sperring av gamle app-/browserøkter og
   pending handoffs, retry og delvis svikt. Først deretter kan N01/N03-porten
   vurderast. Ingen av desse produksjonsstega er implementerte av N01f1.

Pinna kjelder: [EmailStage og tokenoppretting](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/email/stage.py),
[samtykke og tokenforbruk](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/email/flow.py),
[ytre policyfeil](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/policies/process.py),
[policykøyring](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/policies/engine.py).

## N01f2: offentleg API og vidare eigarskap

[Integrasjonsavgjerda](../../../docs/native-recovery-integration-adr.md)
fører N01f1 vidare med ei eiga, avgrensa API-prøve. Ein ekte bearer-brukar med
`reset_user_password` og `add_user` berre på Alice kan lage recovery-lenkje;
Bob og global brukaroppretting blir avviste. To mint før forbruk gir same
tokennøkkel. Ved to samtidige restaureringar når begge registreringssteget,
medan eit seinare replay blir avvist. Offentleg API åleine er derfor ikkje
godkjent som atomisk opptak til recovery.

```powershell
./native/probes/authentik/run-recovery-api.ps1 -RunDirectory <klar-recovery-lab>
```

Proben stoppar ved WebAuthn-utfordringa og kontrollerer at mellombelse objekt
er fjerna og Brand er tilbakeført. CI køyrer han i den same isolerte laben
etter N01f1, og lastar berre opp `recovery-api-checks/**/evidence/receipt.json`.
`passed=true` betyr at observasjonen er reprodusert, medan
`publicRecoveryApiApproved=false` og `fullRecoveryVerified=false` held
aktiveringsporten stengd. Ingen ny nøkkel eller produksjonsflyt er oppretta.
