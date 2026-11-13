# Plan for arrangement i kanal og chat

Dato: 13. november 2026. Status: A–C er implementerte og verifiserte lokalt på `codex/channel-events-ux`, med utgangspunkt i oppdatert `origin/main` (`93e4957`). D står att til brukarprøve. Ingen produksjonsdata er endra.

Sprøyt skal gjere det lett å opprette fleire arrangement i same kanal, sjå kva som nærmar seg medan ein les chatten, og gå mellom arrangement og tilhøyrande meldingar. Eksisterande kanalval, arrangement, påmeldingar, bilete og meldingsreferansar er grunnlaget for arbeidet.

## Grunnlag og faste krav

Brukarflata er undersøkt med Londonfestival under Filosofene. Implementasjonen som er undersøkt, ligg i arbeidskopien `S:/Build/sproyt-channel-events`, commit `ccc31db`: `docs/channel-events-contract.md`, `frontend/src/ui/react/channel-events.tsx`, `src/events.rs` og migreringane 0075/0077. Den opphavlege arbeidskopien i `S:/Source/sproyt` manglar denne arrangementsimplementasjonen. Før implementering må ein kontrollere gjeldande release og bruke ei oppdatert arbeidskopi med kanalarrangement og tilgjengekontrakten.

- Arrangement er valfri støtte per kanal, styrt av eksisterande `events_enabled`. Eigar/moderator styrer aktivering; vanlege kanalmedlemmer kan opprette når støtta er på.
- Eksplisitt av/på-val blir respektert. Eksisterande standard for uavklart innstilling blir bevart: kanalar med arrangement har støtte, tomme kanalar har henne av.
- Avslåing bevarer data. Vanleg chat skjuler arrangementsnavigasjon, indikator og referanseforslag; autoriserte gamle lenkjer er lesbare som arkiv. Endringar er sperra når funksjonen er av.
- Fleire sjølvstendige arrangement kan høyre til same kanal, også med overlappande tidspunkt eller lik tittel. Kvart har eigen UUID, kode, svar, bilete og revisjon.
- Eksisterande direktelenkjer, `#arr`-kodar, reserverte alias og meldingsfilter skal framleis fungere. Ingen automatisk flytting av eksisterande meldingar til nye trådar.
- Dei fire svarverdiane blir bevarte: deltek, deltek ikkje, følgjer og ikkje svart. Endring av svarmodellen er ei separat produktavgjerd.
- Utkast, vedlegg, leseposisjon, medlemsgrenser og tilgangskontroll blir bevarte gjennom navigasjon og oppdatering. Arrangementsarbeidet innfører ingen ny avhengnad til Heart.

## A – Rett flyten for fleire arrangement først

Datamodellen har unikheit på kanal og kode, ikkje på kanal åleine. Backend har allereie testkode som opprettar fleire arrangement i same kanal. I den undersøkte brukarflata finst «Nytt arrangement» berre i oversikta; arrangementsfana gjenbrukar sist vald detaljvising. Dette er observert friksjon, men ikkje eit endeleg bevis på årsaka til problemet med å opprette fleire. Det skal først gjenskapast i isolert testmiljø.

1. Prøv oppretting av arrangement A, gå til chat, opne arrangementsfana og opprett B. Prøv òg frå direktelenkje til A og med like titlar/kolliderande kodar. Skil navigasjonsproblem, valideringsfeil og faktisk lagringsfeil.
2. La «Arrangement» opne kanaloversikta. Ei eksplisitt arrangementslenkje opnar framleis rett detaljside.
3. Gjer «Nytt arrangement» tilgjengeleg både frå oversikta og detaljvisinga når kanalen er aktivert. Handlinga startar eit nytt utkast utan ID frå det opne arrangementet. Avbryt returnerer til utgangspunktet.
4. Lag ein kode automatisk ved oppretting; la brukaren endre han under «Fleire val». Handter kollisjonar mot både aktive kodar og reserverte alias på serveren. Like titlar er lov. Endring av tittel endrar ikkje eksisterande kode automatisk.
5. Skil tydeleg mellom ny oppretting og redigering i klient/API. Ved feil blir utkastet bevart. Undersøk tapt kvittering og gjenteken innsending, slik at retry ikkje opprettar ekstra arrangement; bruk ein stabil opprettingsnøkkel dersom eksisterande kontrakt ikkje dekkjer dette.

Aksept: A og B kan opprettast frå same brukarøkt og ligg att etter ny innlogging. Redigering, svar, avlysing, bilete og meldingsfilter for B påverkar ikkje A. Ein annan kanal kan bruke same kode utan konflikt. Prøva omfattar både SQLite og PostgreSQL for endra lagringskontraktar.

## B – Komande arrangement synlege i eksisterande navigasjonsrad

Bruk rada med «Meldingar / Arrangement». Tal og neste dato blir del av denne rada, med fast høgd og utan automatisk utviding. Første versjon treng ikkje eit ekstra banner, biletkarusell eller ein ny permanent rad.

Illustrasjon med to komande arrangement; det andre er berre eit døme:

```text
Brei flate:    Meldingar    Arrangement · 2 komande     Neste: 4. feb. · Londonfestival
Smal flate:    Meldingar    Arrangement · 2
Ingen komande: Meldingar    Arrangement
Støtte av:     vanleg chat utan arrangementsrada
```

- Talet på komande arrangement omfattar alle framtidige, ikkje-avlyste arrangement i kanalen. Det blir ikkje forveksla med dagens tal på endra arrangement eller med talet på deltakarar.
- Pågåande arrangement med sluttid får prioritet i samandraget, merkte «Pågår». Talet på komande blir framleis vist når det er plass. For arrangement utan sluttid blir eksisterande tidsklassifisering brukt; vi legg ikkje inn ei gjetta varigheit.
- Dato og kort tittel blir viste berre når breidda tillèt det. På smale flater blir samandraget fjerna før kanalnamn, tabbar eller primærhandlingar blir pressa saman. Ingen automatisk linjebryting til ei ekstra rad.
- Trykk på arrangementsfana opnar oversikta med «Nytt arrangement». Eit eventuelt klikkbart neste-arrangement-samandrag opnar akkurat det arrangementet. Sekundære handlingar skal vere utanfor sjølve tablista.
- «Endra» er eit separat, roleg signal. Tilgjengeleg namn opplyser til dømes «2 komande arrangement, 1 arrangement endra». Det skal ikkje blinke eller opne noko automatisk.
- Aktivert kanal utan komande arrangement har framleis arrangementsfana, historikk og oppretting. Avslått kanal får ingen ny arrangementsindikator.
- Ved lastingsfeil blir talet vist som ukjent eller førre stadfesta verdi med feilstatus, aldri som falsk null. Oversikta tilbyr lokal retry.

Talet må vere korrekt også ved paginering. Utvid eksisterande lesekontrakt med eit lite autorisert samandrag for heile kanalen dersom nødvendig: komande tal, pågåande tal, næraste relevante arrangement og eksisterande attention-tal. Ikkje hent alle historiesider eller deltakarlistene for å teikne navigasjonen. Del same klienttilstand mellom oversikt og chat, med oppdatering etter eigne endringar, ved gjenopptaking og avgrensa oppfrisking medan flata er synleg. Ny sanntidsinfrastruktur er ikkje ein føresetnad.

Aksept: 0, 1 og fleire komande arrangement, pågåande, avlyste, gamle og endra arrangement gir korrekt indikasjon. Arrangement utanfor første listeside blir talde. Test 320/390 px, brei desktop, lange namn, større tekst, tastatur og begge tema. Rada får ikkje større høgd og chatten får ikkje ny horisontal rulling.

## C – Skriv og les i arrangementskontekst

Detaljsida får «Vis samtalen» og «Skriv om arrangementet». Begge nyttar eksisterande kanal og meldingsfilter. Skrivefeltet viser ei tydeleg, avtakbar merking med arrangementsnamnet. Brukaren kan velje A eller B frå ein søkbar veljar, og kan gå tilbake til vanleg kanalmelding.

I første versjon blir koplinga lagra gjennom den eksisterande `#arr`-kontrakten. Klienten handterer innsetjing, førehandsvising og duplikatvern; den sendte meldinga skal ikkje få ein skjult eller overraskande mottakar/kontekst. Manuelt skrivne kodar og meldingar med fleire arrangementsreferansar skal framleis fungere. Ein strukturert referanse kan vurderast seinare som ei bakoverkompatibel utviding, ikkje som eit vilkår for betre flyt.

Koplinga er eksplisitt: å opne eit filter skal ikkje i løynd endre kva eit eksisterande utkast gjeld. Ved «Skriv om» med eit påbegynt utkast skal teksten bevarast og konteksten vere synleg før sending. Ved kanalbyte, avslåing eller tap av tilgang blir gammal arrangementskontekst rydda utan å sende eller slette utkastet. Feila sending bevarer både tekst og vald kontekst for kontrollert retry.

Vis referansar med eit forståeleg arrangementsnamn. Bevar direktelenkja og den underliggjande koden. Vis tilhøyrande meldingar frå detaljsida med eksisterande meldingskomponent, slik at reaksjonar, trådar, redigering og bilete held fram med å fungere.

Aksept: Ei melding knytt til A kjem i filteret for A, ikkje B; ein vanleg melding blir verande utan arrangementskopling. Alle ligg framleis i kanalhistorikken. Gamle kodar og alias verkar. Utkast, vedlegg, leseposisjon og val blir prøvde gjennom navigasjon, sendefeil og ny tilkopling.

## D – Detaljar og oppfølging etter grunnflyten

- Samle tittel, tid, stad, ditt svar og deltakaroppsummering øvst. Gjer omtale/program lettare å finne, med bilete som støtte.
- Tilby forståelege tidssoneval og vis lokal tid eintydig. Eksisterande tidspunkt skal ikkje flyttast ved berre å opne og lagre skjemaet.
- Bygg vidare på revisjonsmarkeringa med ei konkret forklaring av kva som er endra. Dette krev verifiserbar endringshistorikk; noverande revisjonsnummer åleine gir ikkje før/etter-verdiar.
- Vurder målretta påminningar og push som ei eiga leveranse med respekt for varselinnstillingar. Dagens kontrakt har ingen automatisk push for arrangementsendringar.
- Utset ny svarmodell, gjentaking, kapasitet, eksterne gjester og datoavstemming til A–C er prøvde med faktiske brukarar.

## Leveranserekkjefølgje og vern av eksisterande bruk

| Del | Avhengnad | Leveranse og kontroll |
| --- | --- | --- |
| A | Oppdatert arbeidskopi og reproduksjon | Rett oppretting/navigasjon for fleire arrangement; målretta regresjonsprøve med to arrangement. |
| B | Avklart samandrag og tidsklassifisering | Kompakt indikasjon i eksisterande rad; korrekt ved paginering og kanalval. |
| C | A og stabil arrangementsidentitet | Synleg skrivekontekst og direkte veg mellom detaljar og meldingsfilter. |
| D | Brukarprøve av A–C | Prioriter detaljforbetringar og oppfølging ut frå observerte problem. |

Lever i små, bakoverkompatible endringar. Bevar data og URL-ar ved oppgradering og tilbakeføring. Nye API-felt er additive; gamle klientar skal framleis kunne bruke eksisterande funksjonar. Test av/på, eksisterande autoriserte arkivlenkjer, tap av medlemskap og uendra RSVP/bilete/alias i tillegg til dei nye brukarfløtane.

Londonfestival er referanse for visuell og lesande kontroll. Oppretting av fleire arrangement, avlysing og feilscenario blir prøvde med testdata i isolert miljø, ikkje ved å endre Londonfestival. Før utrulling skal ein stadfeste at nye og gamle arrangement kan brukast side om side i same kanal. Planen inneber ingen sletting eller konvertering av eksisterande arrangement.

Mål for brukarprøva: ein brukar finn neste arrangement utan å forlate chatten, opprettar arrangement nummer to utan forklaring, skriv ei melding i rett arrangementskontekst og finn henne att. Synlegheit blir vurdert mot faktisk plassbruk, ikkje berre om ein indikator finst.

## Gjennomført og verifisert

Arbeidet ligg i `S:/Build/sproyt-channel-events`. Tre parallelle utviklingsagentar brukte GPT-6.1 Sol; integrasjon og samla verifikasjon vart gjennomførte i same arbeidskopi.

- A: fana opnar oversikta; oppretting finst òg i detaljvisinga. Ny kode blir laga automatisk, og kvart opprettingsutkast har ein stabil kvitteringsnøkkel. Migrering 0078 lagrar nøkkelen atomisk med arrangementet. Uviss kvittering held på same nøkkel og innhald ved nytt forsøk.
- B: heile kanalen blir tald på serveren, uavhengig av paginering. Komande tal, pågåande arrangement og neste dato/tittel nyttar eksisterande fanerad. Bakgrunnsoppdatering bevarer innlasta historiesider. Chatfilter viser avkorta arrangementsnamn, med full tittel og tidspunkt tilgjengeleg.
- C: «Vis samtalen» og «Skriv om arrangementet» opnar eksisterande chat/filter. Vald skrivekontekst er synleg, søkbar og avtakbar. Manuelle kodar og alias blir bevarte; automatisk innsetjing brukar same parser som meldingsvisinga og opnar ikkje autofullføring ved ny sending.

Verifikasjon 13. november 2026:

- Typekontroll og frontend-bygg: grøne. Klientens kontraktprøver: 227 bestått.
- Rust-formatkontroll og Clippy med åtvaringar som feil: grøne. Full backend-suite: 484 bestått, 12 eksisterande prøver ignorerte, køyrd med fire testtrådar utan ekstern database.
- Eiga arrangements-suite med PostgreSQL 17 aktiv: 18 bestått, inkludert delte SQLite/PostgreSQL-kontraktar for fleire arrangement, samandrag, søk, tilgang og samtidig gjenteken oppretting.
- Klientkjernen: 12 bestått og WebAssembly-kontroll grøn.
- Nettlesar: 55 bestått i samla køyring av arrangement, skrivefelt og vanleg chat. Den eine prøva mot ekte server fekk tidsavbrot under ny lasting i samlekøyringa og bestod uendra separat på 26 sekund. Ho kontrollerer mellom anna varig lagring av arrangement B, melding berre i B-filteret og bevarte svar/bilete/alias på A. Totalt 56 relevante scenario er verifiserte.
- Skjermbilete er inspiserte ved 320/390 px og brei desktop, i lyst og mørkt tema. Lange namn og automatisk kode er prøvde i skrivekontekst og veljar.

To eksisterande testproblem på Windows vart òg retta: multiline-kontrollar toler CRLF, og testopprydding ventar avgrensa ved SQLite si delingslås. Ingen produksjonslogikk vart endra for desse rettingane. Den mellombelse PostgreSQL-containeren er stoppa.

Utrulling er ikkje gjennomført. Bruk den ordinære migreringsrutinen med migrering 0078 før nye applikasjonsinstansar blir starta, og bevar migratoren ved eventuell tilbakeføring. Eksisterande arrangement krev inga konvertering. Londonfestival er ikkje endra.

## Produktreferansar

Spond er referanse for forståeleg oppretting og oppfølging; Luma for kort veg mellom arrangement og samtale; Discord og WhatsApp for synlegheit i ei samtaleflate. Desse mønstera skal tilpassast den eksisterande kanalmodellen i Sprøyt.

- [Spond: oppretting i grupper](https://help.spond.com/app/en/articles/127163-creating-and-managing-events-in-spond-groups)
- [Spond: arrangementsinnstillingar](https://help.spond.com/app/en/articles/129730-features-in-events)
- [Luma: arrangementschat](https://help.luma.com/p/event-chat)
- [Discord: planlagde arrangement](https://support.discord.com/hc/en-us/articles/4409494125719-Scheduled-Events)
- [WhatsApp: arrangement i grupper](https://about.fb.com/news/2024/05/events-in-whatsapp-communities/)
