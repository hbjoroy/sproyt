# Kretsrom

Kretsrom er den nye utgåva av designspråket for Sprøyt. Kretsrom brukar eige oppsett frå `@sproyt/ui` 0.2.0. Sprøyt deler samtaledata, handlingar og kladdar mellom begge visingane. Slå på **Prøv Kretsrom** under **Innstillingar → Utsjånad og bruk**. Valet gjeld denne nettlesaren og kan slåast av medan ein brukar appen.

Målet er at ein ven skal kjenne att gjengen, sjå kven som sa kva og svare med éi hand. Varm papirflate og kolgrått mørkt tema, bølgja og nynorsken er vidareførte kjenneteikn. Kretsen grupperer kanalane. Tekst, fokusmarkeringar og trykkflater skal halde seg lesbare når det vert trongt.

## Kva høyrer til denne utgåva?

| Status | Flater og åtferd |
| --- | --- |
| Innarbeidd | Tydeleg kretsnamn ved sida av kanalnamnet, samanhengande kanalgruppe, samanleggbare kretsar og «Skjulte kanalar». Bølgjesnarveg og synleg retur til kretsane på mobil. Rolegare meldingsrad, dato og ulestmarkør på same rad når dei fell saman. |
| Innarbeidd | Ein skjult bileteverkstad med ei aktiv eller ferdig bestilling kan opnast frå skrivefeltet i opphavssamtalen. Verkstaden opnar som sidepanel på stor skjerm og ark nedst på mobil. Eksisterande privat gjennomgang og godkjenning styrer vedlegget. |
| Prøvd i prototype | Grupperte meldingar frå same ven, førehandsvising av siste melding i kanallista, trådsitat ved kladden, og mediefliser. |
| Ope | Korleis finne biletebestillingar frå andre kretsar. Korleis vise direktemeldingar, omtalar og ferdige bilete samstundes. Feste reaksjonar og balansere bruk gjennom fleire veker mot nyleg bruk. |
| Importert | Alle andre flater og interaksjonar kjem frå den redaksjonelle utgåva. Det omfattar mellom anna Arbeid, arrangement, agentoppsett, medlemskap, innboks, innstillingar, reaksjonsveljar og handtering av opplastingar. |

**Importert** er status i designarbeidet. Ein importert komponent brukar dei same kjeldetypane, tilgangssjekkane og handlingane som før. Når ei flate vert vurdert, kan ein ta henne inn i Kretsrom slik ho er, eller tilpasse henne. Oppdater rada når endringa kan prøvast i sjølve appen.

Utgåvespesifikke stilreglar ligg i `frontend/src/ui/kretsrom.css` og gjeld berre når React-rota har `data-design="kretsrom"`. Delte komponentar og temafargar har framleis éin eigar i `packages/sproyt-ui`. Prototypedata og simulerte innsendingar høyrer til i skissene.

## Til neste drøfting

Prøv to kretsar som begge har ein kanal kalla #Prat. Byt krets med ein halvskriven kladd, opne ei skjult kanalgruppe og gå tilbake. Prøv også ein tråd med eit bilete på veg og ei omtale frå ein ven. Sjå etter om mottakaren, leseposisjonen og vegen attende er tydelege på telefon.
