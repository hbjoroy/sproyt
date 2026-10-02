# Informasjonsoverlevering i work-item-vurdering (#190)

Heart-definisjonen `sproyt/work-item-review:1.1.0` har ein avgrensa flyt:
`review` → val → eventuelt `provide-information` → `followup-review` → slutt.
Det er høve til éin informasjonsrunde. Den siste vurderinga kan berre ende
med planlagt, løyst eller avvist. Nye rundar krev ei seinare utviding.

`1.0.0` er urørt. Migrasjon 0046 festar eksisterande saker til denne
versjonen. Ny registrering kopierer versjonen frå kanalbindinga; endring
av bindinga endrar ikkje pågåande saker. Begge versjonar kan startast og
avstemmast av same Sprøyt-versjon.

## Oppgåver, kanalar og rettar

- `review` og `followup-review` går til den behandlarkanalen som vart
  lagra ved registrering. Den tildelte behandlaren må framleis vere
  kanalmedlem med skrivetilgang, ha `can_review` og rolla `product-handler`
  for applikasjonen. Andre kanalmedlemmer ser kortet utan innsending.
- `provide-information` går til kjeldekanalen, som allereie er eksplisitt
  aktivert for work-item-prosessen. Oppgåva er tildelt personen som
  registrerte saka. Han må framleis ha skrivetilgang i denne kanalen;
  han treng ikkje behandlarrolle. Andre medlemmer kan lese spørsmål/svar.
- Spørsmålet frå behandlaren er offentleg for kjeldekanalen. Skjemaet
  spør uttrykkeleg etter «Spørsmål til innmeldar». Det publiserer ikkje
  intern behandlarhistorikk, kategori eller prioritet i kjeldekortet.
- Kopiering av makroen gir ikkje tilgang: API-et bind oppgåva til den
  faktiske meldinga og kanalmedlemskap. Nye aktiveringars kanal, node og
  tildeling vert kontrollerte mot den lagra saka.

Korta er samanfalda som standard. Informasjonskortet viser berre eit
svarfelt. Siste behandlarkort viser spørsmål og svar og har dei vanlege
kategori-, prioritets- og avgjerdsfelta. Tekst blir vist som tekst; HTML
frå spørsmål eller svar vert ikkje køyrt. Utkast blir verande ved feil.

## Framdrift, kvitteringar og historikk

Sprøyt lagrar request-ID, venta revisjon og nøyaktig innsending i same
transaksjon som revisjonsauken. Spørsmål og svar er avgrensa til 8000
byte. Ein retry må ha same request-ID, revisjon og innhald. Ei motstridande
innsending vert avvist. Nettlesaren tek vare på den første revisjonen
og nøkkelen ved usikker respons, også etter at eit nytt statuskall har
vist ein høgare revisjon.

Heart eig framdrifta. Godteken lokal innsending står som ventande til
Heart har fullført rett aktivering med rett resultat. Ein tapt completion-
respons vert avklart ved å lese Heart. Eit fullført steg med eit anna
resultat vert avvist ved avstemming. Ein allereie godteken kommando kan
leverast etter at brukarrettar er trekte attende; nye kommandoar kan ikkje
godtakast då.

Avstemming handterer dei tre historiske aktiveringane i prosessrekkjefølgje.
Kvitteringa og kanalmeldinga vert lagra saman; gjenteken avstemming eller
restart gir ikkje fleire meldingar. Heart må ha fullført forgjengaren før
Sprøyt viser neste steg. Saka står som `needs_information` etter den første
vurderinga, `reviewing` etter svaret og endeleg status etter siste vurdering.
Oppgåvemeldingane blir verande som lesbar historikk i begge kanalane.

## Canary-aksept

Aktiver berre ein avgrensa testapplikasjon og testkanalar. Bruk
`definition_version: 1.1.0` i kjeldekanalbindinga og ei review-rute med
`product-handler`. Det skal vere to forskjellige brukarar: ein innmeldar
og ein behandlar. Ingen global aktivering eller produksjonspromotering
er del av denne etappen.

1. Registrer ei kjeldemelding og kontroller eitt review-kort i rett kanal.
2. Send eit spørsmål som behandlar; kontroller eit informasjonskort i
   kjeldekanalen, med innmeldaren som einaste person med svarfelt.
3. Svar som innmeldar; kontroller eit nytt followup-kort hos behandlaren
   med det rette spørsmålet og svaret. Avslutt saka.
4. Kontroller read-only for andre, ingen nye meldingar etter restart/retry,
   og blokkert innsending etter tap av kanal- eller behandlarrett.
5. Verifiser den direkte ruta utan informasjonsbehov og at ei sak festa
   til 1.0.0 held fram med den gamle flyten.

Automatiske SQLite-/PostgreSQL-kontraktar køyrer same overlevering med
separate kanalar, mist respons etter Heart-aksept, retry, rettstap og
restart. Nettlesarkontrakten dekkjer mobilkort, read-only, utkastbevaring
og retry. Reell Heart/canary-aksept skal førast særskilt; ein mock er
ikkje dokumentasjon på den driftsøvinga.
