# Arkitektur og leveranseplan: brukarskjema i Heart-prosessar

Dato: 2026-09-30. Status: plan; ingen skjemastøtte er implementert av dette
dokumentet. Grunnlaget er [designnotatet](process-forms-design-note.md),
[saksplanen](work-items-and-heart-plan.md) og den noverande Heart v2-kontrakten.

## Grense og rekkjefølgje

Når ein brukar vel **Lag Issue** frå ei melding, skal Sprøyt lagre saka:
tittel, kategori, prioritet, historikk og kven som får lese henne. Heart
får ei referanse til saka og styrer kva brukaroppgåve som er aktiv, kven
som kan utføre henne og kva som skjer etterpå. Sprøyt viser oppgåva som
ei melding i rett kanal.

Kvar opplysning har éin eigar. Dersom eit skjema spør om noko som berre
trengst for neste prosesssteg, til dømes «godkjenn eller avvis», kan Heart
lagre svaret. Dersom behandlaren endrar sjølve saka, til dømes prioritet,
skal Sprøyt lagre endringa; Heart treng berre resultatet som styrer flyten.
I ein annan prosess kan eit fagsystem eige saksdataa i staden for Sprøyt.

Skjemadefinisjonane kan høyre logisk til Heart utan ei eiga teneste.
JSON Forms kan vise dei i Sprøyt, men Heart skal òg kunne brukast av
andre brukarflater utan JSON Forms.

To arbeidsspor må ikkje blandast:

- **S-sporet:** første applikasjonssak med `Lag Issue`, register og
  produktbehandling etter steg 1–3 i saksplanen. Kategori, prioritet og
  saksstatus blir verande Sprøyt-eigde data. Eit lite, formålsbygd skjema
  kan levere dette utan å vente på JSON Forms.
- **F-sporet:** første Heart-brukaroppgåve som faktisk treng eit
  prosessnært, definisjonsstyrt skjema. Då blir JSON Schema/JSON Forms
  innført for akkurat denne oppgåva. Skjemaet lagrar ikkje ein ekstra kopi
  av Sprøyt-saka i Heart for å demonstrere teknologien.

F-sporet startar med eit konkret feltsett og ein eigar for svara. Det kan
gå parallelt med S-sporet etter at oppgåve-/datakontrakten er avklart;
det er ikkje ein generell føresetnad for S1–S3. Ein seinare prosess kan
kombinere eit Sprøyt-eigd sakssteg og eit Heart-eigd skjemasteg, men kvar
verdi har éin autoritativ eigar.

## Kontrakt mellom definisjon, Heart og brukarflate

Ein versjonert definisjon er skrive i YAML. `schema` er JSON Schema og
`uischema` er JSON Forms UI Schema, bevarte som standard JSON-strukturar.
Definisjonsinnlesinga avviser duplikate YAML-nøklar, tvitydig typekonvertering,
ikkje-JSON-verdiar og ikkje-støtta skjemafunksjonar. Val av JSON Schema-dialekt,
støtta felt/format og JSON Forms-versjon blir pinna og dokumentert i F0/F1.

Heart-definisjonen viser til ein *uforanderleg* skjemaversjon (ID, versjon og
innhaldsdigest) for kvar brukaroppgåvenode som brukar skjema. Aktiveringa
bevarer referansen. Oppgåve-API-et må eksponere han saman med
oppgåve-/aktiverings-ID. Ein serverstyrt lesekontrakt returnerer det pinna
`schema`, `uischema` og berre dei oppgåvedataa den aktuelle aktøren har
rett til. At ein kanalmedlem får lese oppgåvemeldinga, gjev ikkje automatisk
rett til heile skjemaet, svara eller saksdataa. Ei klientvisning kan
ikkje velje eit anna skjema eller ein annan aktivering ved innsending.
Skjemareferansen må vere stabil sjølv om ein ny definisjons- eller
bibliotekversjon blir publisert medan ei oppgåve ventar.

For prosessnære svar validerer Heart fullføringspayloaden mot det pinna
skjemaet, i tillegg til aktør, tildeling og oppgåvestatus. Sprøyt validerer
òg før innsending for god tilbakemelding, men klientreglar om synlegheit og
redigering gir aldri tilgang eller prosessrett. Dersom eit fagsystem eig
feltene, skal ikkje desse sendast som `result_metadata` til Heart; den
integrasjonen krev ein eigen serverstyrt lagrings- og fullføringskontrakt.
Formdefinisjonen får ingen rett til å starte prosessar, eksportere saker
eller kalle vilkårlege grensesnitt. Slike handlingar er typed kommandoar
med eigne rettar.

Heart v2 har i dag `POST /api/v2/user-tasks/{id}/complete` med `actor_id`,
valfri `result_metadata`, `X-Heart-Client` og `Idempotency-Key`. Same
klient/nøkkel/aktør/payload får historisk kvittering; endra payload
konfliktar. V2-oppgåve-ID er aktiverings-ID. Sprøyt sin avgrensa pilot sender
no tom `result_metadata` og lagrar berre kommandonøkkel. F-sporet må ta
vare på eksakt validert payload og skjemapin i ei varig kommando før
transport. Retry brukar same nøkkel og identisk payload; ved tapt svar
kan historisk kvittering hentast ved eksakt replay. Avstemming av berre
`status=completed` provar ikkje kva svar Heart godtok. Same nøkkel med
endra svar/skjema er konflikt, og ei ny aktivering får ny nøkkel.

Heart sitt v2-resultat frå ei sekvensiell brukaroppgåve ligg under
`results[node_id] = {activation_id, value}`. Parallelle greiner får
eigne resultat under den definerte `results_key` ved join. Skjema- og
prosessdesign må bruke desse eksplisitte resultata; dei skal ikkje
rekne med at siste grein overskriv felles metadata.

## Granulert gjennomføring

| Del | Endring | Ferdig når |
| --- | --- | --- |
| A0 — eigarskap | Vel første reelle skjemabehov, felt og kven som eig kvart svar. Avklar synlegheit, sletting/retensjon, innsyn og om svaret er prosess- eller fagdata. | Ei konkret oppgåve og dataflyt er dokumentert; ingen fagfelt blir ført inn i Heart ved eit uhell. |
| S1–S3 — eige saksarbeid | Lever [saksplanen](work-items-and-heart-plan.md) med Sprøyt-validering og lagring for kategori/prioritet. Bind sak, Heart-instans, aktivering og meldingsprojeksjon. | Første saksflyt fungerer utan JSON Forms; dette sporet kan gå uavhengig av F0–F9. |
| F0 — profil og kontrakt | Vel JSON Schema-dialekt, tal-/datoformat, null/default, ukjende felt, lokale `$ref`, maksimal storleik/djupn og støtta UI-element. Lag eit felles valideringskorpus og kartlegg Heart v2-API mot ønskja formreferanse, input og svar. | Open API-/schemaendringar er skrivne ned; negative prøver viser kva dagens Heart ikkje kan validere. |
| F1 — definisjonsmodul | Legg til versjonert YAML-innlesing og immutable skjemaregister i Heart eller ein Heart-nær modul. Avvis duplikate nøklar, tvitydige YAML-typar, ugyldige referansar, fri nettverkslasting og ikkje-støtta UI-funksjonar. | Same ID/versjon kan ikkje få nytt innhald; ugyldige definisjonar blir stoppa før deploy. |
| F2 — task-binding og lesing | Pin skjemareferansen i brukaroppgåvenoden og aktiveringa. Eksponer referanse og avgrensa, autoriserte inputdata i oppgåve-API; hent `schema`/`uischema` frå pinna versjon. | Gammal aktiv oppgåve held på same skjema etter ny deploy; oppgåver utan skjema fungerer som før. |
| F3 — servervalidering | Heart validerer `result_metadata` mot pinna schema før atomisk completion, utan å endre eksisterande idempotens og historisk kvittering. | Ugyldig svar, feil aktør, forelda aktivering og endra payload med same nøkkel blir avvist. |
| F4 — varig innsending | Utvid Sprøyt-kommando med task/aktivering, aktør, skjemaref, eksakt payload og request-ID. Valider før kølegging; retry med same Heart-client/nøkkel/aktør/payload og bevar historisk kvittering. | Timeout, restart og dobbelt trykk gir éi avgjerd; ei anna fullføring blir ikkje feiltolka som kvittering for vår payload. |
| F5 — oppslag og avstemming | Serveren hentar berre autoriserte detaljar ved opning. Vis ventande, avvist, usikkert eller stadfesta utfall; samanlikn kvittering/kommando, ikkje berre task-status. | Ukjend skjema og konflikt blokkerer handling synleg; svar hamnar ikkje i chattekst, audit, feilmelding eller sanntidshending. |
| F6 — brukarflate | Render `schema`/`uischema` med JSON Forms i den samanfalda oppgåvemeldinga. Bruk `@sproyt/ui`, tastatur/touch og båe tema. Vel lokal eller varig utkastpolicy; andre kanalmedlemmer ser berre autorisert projeksjon. | Den konkrete oppgåva verkar på mobil og desktop; feil mistar ikkje utkast og ikkje-støtta skjema feilar trygt. |
| F7 — kontraktprøver | Prøv faktisk Heart og Sprøyt saman: gyldig/ugyldig svar, manglande rett, same retry/endra payload, timeout etter commit, restart, kansellering og ny skjemaversjon medan gamal oppgåve er open. | Kvittering og prosessresultat stemmer med innsendt svar utan dobbelt fullføring. |
| F8 — parallell regresjon | Prøv to skjemaoppgåver i ulike greiner, begge fullføringsrekkjefølgjer og samtidige innsendingar. | Join-resultat inneheld begge svar under rett `results_key`, utan overskriving eller datalekkasje mellom greiner. |
| F9 — canary og drift | Aktiver éin pinna definisjon i Prosesstest/canary. Legg til metrikker utan svarinnhald; dokumenter backup, retensjon, driftseigar, roll-forward og stans av nye starter medan aktive oppgåver blir avstemte. | Manuell aksept, same motoridentitet og pinna artefaktar er dokumenterte før breiare aktivering. |

F0–F9 kan delast i fleire små PR-ar på tvers av Heart og Sprøyt. Heart sitt
API/schema og kontrakttestar må vere på plass før Sprøyt-klienten blir
aktivert. Nye migrasjonar er additive; gamle prosessinstansar blir ikkje
omtolka. Canary deler Sprøyt-database med produksjon, så prøva må ha
eksplisitt kanal-/definisjonsbinding og ingen global aktivering.

## Seinare integrasjonar — eigne avgjerder

Fleirkjeldebinding kjem først når ein konkret fagprosess treng ho. Då
definerer ein serverstyrte lesar-/skrivaradapterar per felt, avgrensa
autorisasjon og koordinert innsending: fagresultat må vere varig lagra
før Heart-oppgåva blir fullført. Delvise feil, versjonar og idempotent
avstemming er del av den integrasjonen, ikkje av JSON Forms i seg sjølv.

Kontrollerte spørsmål som `person.atLeastAge(18)` krev ein separat,
godkjend evnekontrakt. Ja, nei, ukjent og transportfeil er ulike utfall;
grunnlagsdata skal ikkje automatisk kopierast til Heart. Eventuell Lua
er eit seinare implementeringsval, ikkje fri tilgang frå prosessdefinisjonen
til URL-ar, databasar eller personopplysningar.

For kvar slik integrasjon må tilgang, loggar, backup, sletting og behov
for separat lager eller Heart-instans vurderast konkret. Eit fagområdefelt
åleine gir ikkje isolasjon.
