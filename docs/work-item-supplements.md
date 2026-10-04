# Tilleggsinformasjon frå innmeldaren (#208)

Innmeldaren kan leggje til informasjon på eige initiativ medan produkteigaren
vurderer saka. Opne «Arbeidssaker» frå menyen på den opphavlege meldinga; dialogen
viser også allereie registrerte saker og handlinga «Legg til informasjon».
Original beskriving og tidlegare tillegg blir ståande som historikk.

Tillegg er tilgjengelege for medlemmene i kjeldekanalen og behandlarkanalen.
Kjeldeoppslaget har ein eigen DTO med saksidentitet, tittel, beskriving,
applikasjonsnamn, status og tillegg. Det returnerer ikkje interne behandlarnotat,
kategori, prioritet, behandlaridentitet eller Heart-opplysningar.

## Avgrensa prosesskontrakt

- Berre personen som registrerte saka, med gjeldande skrivetilgang i
  kjeldekanalen, kan leggje til informasjon.
- Saka må ha ei klar og ventande `review`- eller `followup-review`-oppgåve,
  utan godteken avgjerd. Under `provide-information` bruker innmeldaren den
  eksisterande svaroppgåva. Ferdigbehandla saker tek ikkje imot nye tillegg.
- Eit tillegg fullfører ingen brukaroppgåve og endrar ingen Heart-node,
  definisjonsversjon eller sakstittel/-beskriving.
- Kvar tekst kan vere høgst 8000 UTF-8-byte. Ei sak kan ha høgst 100 tillegg.
  Kjeldeoppslaget viser høgst dei 100 nyaste sakene frå den valde meldinga.

## Samtidige innsendingar og retry

Migrasjon 0055 legg til `work_item_supplements` for PostgreSQL og SQLite.
Tillegg og auking av `work_items.revision` blir lagra i same transaksjon.
Avgjerda og tillegget konkurrerer om same venta revisjon: berre éi innsending
kan vinne. PostgreSQL låser saksrada og medlemskapet ved godkjenning.

Kvitteringa bind innmeldar, request-ID, sak, opphavleg revisjon og nøyaktig tekst.
Ein identisk retry aukar ikkje revisjonen og opprettar ikkje eit nytt tillegg,
også etter at saka er ferdig. Gjeldande tilgang blir framleis kontrollert.
Motstridande bruk av same request-ID blir avvist.

Behandlaren skal ikkje automatisk få ei ny revisjon godkjend av polling medan
skjemaet er ope. Nye tillegg blir viste med varsel; ei uttrykkeleg handling tek
inn den nye informasjonen før avgjerd. Utkast blir tekne vare på ved konflikt.
Tillegg kan inngå i eit nytt GitHub-utkast, men skal aldri endre ein allereie
redigert eller godteken eksport eller innhaldet i ein usikker retry.

Avgjerda må også innehalde `expected_supplement_id` for det siste tillegget
behandlaren har sett. Denne verdien blir lagra i avgjerdskvitteringa. Det hindrar
ein eldre, allereie open nettlesarklient i å hente ny revisjon og godkjenne ei
sak utan å vise dei nye opplysningane. Eksisterande saker utan tillegg fungerer
med den gamle klienten; saker med tillegg krev oppdatert klient før ny avgjerd.

## Utrulling

`SPROYT_WORK_ITEM_SUPPLEMENTS_ENABLED` / Helm `config.workItemSupplementsEnabled`
er av som standard og styrer berre nye tillegg. Rull ut migrasjon og alle
arbeidarar i prod og canary med flagget av; aktiver først når alle har den nye
kontrollen av avgjerder. Lesing og identiske retry-kvitteringar fungerer med
flagget av. Etter første tillegg må også ein eventuell rollback bevare den
nye avgjerdskontrollen; flagget av åleine gjer ikkje eldre kode kompatibel.

## Kontroll

Den felles SQLite-/PostgreSQL-kontrakten for informasjonsoverlevering dekkjer
tilgang, observer, feil kjeldemelding/kanal, bytegrense, innhald og historikk,
identisk/motstridande retry, foreldet avgjerd og samtidig tillegg/avgjerd.
Heile Heart-flyten skal framleis gi akkurat dei tre opphavlege oppgåvene.
Nettlesarprøvene dekkjer innmeldaren sitt utkast og retry samt behandlaren si
handtering av ei ny revisjon. Fysisk brukarakseptanse blir ført separat.
