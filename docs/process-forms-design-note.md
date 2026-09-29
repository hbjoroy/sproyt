# Notat: brukarskjema i Sprøyt og Heart

Dato: 2026-09-28. Status: designretning frå diskusjon, ikkje implementert kontrakt.
Implementasjonen vert utsett til eit konkret behov oppstår. Notatet legg ikkje
til funksjonalitet eller avhengnader, og seier ikkje at dagens pilot allereie
har JSON Forms-støtte.

Den konkrete ansvarsdelinga og leveranserekkja er ført vidare i
[arkitekturplanen](process-forms-architecture-plan.md). Dette notatet
bevarer grunngjevinga og dei seinare utvidingsretningane.

## Avgjerd og første omfang

Vi vil støtte meir samansette skjema for Heart sine brukaroppgåver med
JSON Forms. Definisjonane skal kunne skrivast i YAML, medan datamodellen
følgjer JSON Schema og JSON Forms sitt UI Schema. YAML er skriveformatet,
ikkje eit nytt skjemaspråk som erstattar desse modellane.

Når skjemastøtta vert teken inn, kan første leveranse avgrensast til
prosessnære data lagra i Heart. Vi treng ikkje bygge fleirkjeldebinding,
eksterne saksregister eller virtuelle funksjonar no. Enkle felt, val,
gruppering og validering er eit naturleg utgangspunkt; det konkrete
feltutvalet vert bestemt av den første brukaroppgåva som treng skjema.

Utvidingsretninga nedanfor skal hindre unødig kopling, men er ikkje eit krav
om å implementere eit generelt integrasjonsrammeverk på førehand.

## Uavhengige ansvar

Heart skal kunne drive prosessar utan å føresetje Sprøyt som brukarflate
eller som eigar av saksdata.

| Del | Ansvar |
| --- | --- |
| Heart | Prosessdefinisjon og versjon, instans, aktive oppgåver, tildeling, framdrift og den avgrensa datakontrakten prosessen treng |
| Brukarflate | Presentere og behandle oppgåver; Sprøyt er eitt alternativ, eit fagsystem eller annan programvare er andre |
| Saks-/fagsystem | Eige fagdata, tilgangsreglar, revisjonshistorikk og reglar for lagring og sletting |
| Skjemamodul | Versjonerte skjemadefinisjonar og innlesing/validering av definisjonane, logisk knytt til Heart utan krav om eiga teneste |

Sprøyt kan eige saksregisteret for Sprøyt sine eigne saker, slik
[arbeidsplanen](work-items-and-heart-plan.md) skisserer. Dette er ikkje ein
universell modell for alle prosessar. Til dømes kan ei sak om antihvitvasking
høyre til i eit separat fagsystem med eigne datagrenser.

Ein prosess kan etter kvart ha oppgåver i ulike brukarflater. Ei spesialbygd
flate kan oppfylle oppgåvekontrakten utan å bruke JSON Forms. Heart skal
ikkje vere avhengig av React eller av Sprøyt sine visningskomponentar.

## YAML og JSON Forms

Innlesing av YAML skal gje JSON-kompatible objekt. Klienten får `schema`,
`uischema` og oppgåvedata gjennom eit definert API. Vi bevarer standardnøklar
og struktur slik at JSON Forms kan brukast utan ein særskild omsetjar for
vårt eige skjemaspråk.

Illustrasjon av ei mogleg innpakking, ikkje ein fastsett API-kontrakt:

```yaml
id: vurder-sak
version: "1.0.0"
schema:
  type: object
  required: [prioritet]
  properties:
    prioritet:
      type: string
      enum: [laag, normal, hoeg]
    grunngjeving:
      type: string
      maxLength: 2000
uischema:
  type: VerticalLayout
  elements:
    - type: Control
      scope: "#/properties/prioritet"
      label: Prioritet
    - type: Control
      scope: "#/properties/grunngjeving"
      label: Grunngjeving
      options:
        multi: true
```

Ved implementering må vi velje ein eksplisitt JSON Schema-dialekt og
samordne valideringa i klient og server. YAML-innlesinga skal avvise
duplikate nøklar og verdiar som ikkje kan representerast eintydig som JSON.
Versjonar, datoar og referansar må ikkje få utilsikta typekonvertering.

Oppgåver skal bindast til ein uforanderleg skjemaversjon. Oppgradering av
JSON Forms-biblioteket og endring av eit konkret skjema er ulike endringar.
Eigne renderarar kan bruke `@sproyt/ui`; slike renderarar og særutvidingar
vert vårt vedlikehaldsansvar. Støtteomfanget må vere eksplisitt, slik at
ikkje-støtta definisjonar vert oppdaga før bruk.

UI-reglar for synlegheit og redigering er ikkje tilgangskontroll eller
prosesslogikk. Serveren må validere data og rettar før oppgåva vert fullført.

## Seinare: samla arbeidsflate, ulike datakjelder

Eit skjema skal kunne behandle data frå både prosessinstansen og det
tilhøyrande fagsystemet. Eit samla datasett i brukarflata krev ikkje at alle
opplysningane vert lagra i Heart. Skjemadefinisjonen og dei utfylte svara
kan ha ulike eigarar.

Vi skil mellom tre kontraktar:

- Skjemakontrakten definerer felt, validering og presentasjon.
- Databindinga definerer kvar felt vert henta frå og kvar endringar skal
  lagrast. Dette er eit eige lag rundt JSON Forms, ikkje standardfunksjonalitet
  vi føreset at JSON Forms leverer.
- Prosesskontrakten definerer dei dataa og operasjonane Heart faktisk treng
  for å drive flyten.

Bindingar må vise til godkjende grensesnitt med serverstyrt tilgang, ikkje
gje skjemadefinisjonen fri tilgang til URL-ar eller databasefelt. Tilgang til
ei prosessoppgåve skal ikkje automatisk gje tilgang til alle tilhøyrande
fagdata. Berre naudsynte felt skal leverast til den aktuelle brukarflata.

Innsending må koordinerast på serveren. Nettlesaren skal ikkje ha ansvaret
for å få uavhengige lagringar og prosessfullføring til å henge saman.
Ein framtidig integrasjon må handtere delvise feil, dataversjonar og
idempotent gjentaking, og ikkje fullføre oppgåva før nødvendig fagresultat
er varig lagra. Den konkrete mekanismen vert vald saman med integrasjonen.

## Seinare: avgrensa spørsmål til fagsystemet

Prosessen kan ha behov for eit svar utan å ha behov for grunnlagsdataa.
Til dømes kan `person.atLeastAge(18)` erstatte overføring av fødselsdato
til Heart. Eit tidlegare skjema kan ha registrert fødselsdatoen direkte i
fagsystemet, medan prosessen berre får vurderinga han treng.

Lua er ein mogleg måte å uttrykkje kallet på, ikkje eit vedteke teknologival.
Sjølve funksjonen bør vere ei kontrollert evne eksponert av vertssystemet
eller ein adapter, med autorisasjon og avgrensa resultat. Grunnlagsdataa
skal ikkje måtte hentast inn i Lua-miljøet for å rekne ut svaret.

Ei slik kontrakt må ta høgd for:

- Ja, nei og ukjent som ulike resultat; transportfeil må heller ikkje bli
  tolka som eit negativt fagleg svar.
- Tidspunkt, relevant dataversjon og eventuelt regelversjon, slik at
  avgjerda kan forklarast med minst mogleg lagra informasjon.
- Registrering av det avgrensa resultatet når det vert brukt i ei avgjerd,
  slik at retry ikkje utilsikta vurderer nye data. Ny vurdering må vere
  eksplisitt.
- Kontroll av kva spørsmål prosessen får stille. Vilkårlege gjentekne
  aldersgrenser kan til dømes røpe meir enn det eine alderskravet krev.

Også eit avleidd svar som «over 18» kan vere persondata. Dette er
dataminimering, ikkje ein garanti for anonymitet.

## Datagrenser og isolasjon

Arkitekturen skal tillate ulike saksregister og ulik grad av isolasjon.
Sensitive fagdata skal ikkje automatisk kopierast til prosessdata,
chatmeldingar, hendingar eller loggar. Saksreferansar og prosessmetadata
kan òg vere sensitive. Datagrenser må omfatte tilgang, backup og sletting,
ikkje berre primærlagringa.

Separate lager og ved behov separate Heart-instansar skal vere mogleg.
Eit felt for fagområde er ikkje i seg sjølv tilstrekkeleg isolasjon.
Konkrete personvern- og lagringskrav må avklarast for kvart bruksområde;
dette notatet fastset ikkje at ei bestemt løysing oppfyller GDPR.

## Når vi tek dette vidare

Første konkrete behov utløyser implementering av JSON Forms for prosessnære
data. Fleirkjeldebinding og virtuelle funksjonar vert tekne opp når eit
fagsystem faktisk treng dei. Då må API, eigarskap til innsendinga,
feilhandsaming og isolasjonsnivå konkretiserast mot det systemet.

Bakgrunn: [brukaroppgåvepiloten](process-user-task-pilot.md),
[JSON Forms React-integrasjon](https://jsonforms.io/docs/integrations/react),
[UI Schema](https://jsonforms.io/docs/uischema/) og
[validering](https://jsonforms.io/docs/validation/).
