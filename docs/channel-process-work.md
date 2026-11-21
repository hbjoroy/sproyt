# Arbeid ved kanalen og på tvers av kanalar

## Val for #386

Brukarutprøvinga i [#386](https://github.com/hbjoroy/sproyt/issues/386) viste at prosesshistorikken under kretsadministrasjonen var vanskeleg å finne. Ein deltakar skal kunne sjå kva som krev handling, utføre handlinga og gå vidare. I [Camunda Tasklist](https://docs.camunda.io/docs/components/tasklist/userguide/using-tasklist/) er køen og oppgåvehandlinga eigne delar av grensesnittet. Camunda skil òg mellom [lesing, overtaking og fullføring](https://docs.camunda.io/docs/components/tasklist/user-task-authorization/). Desse skilja ligg til grunn for utforminga i Sprøyt.

1. Gjer **Arbeid** til ei fane saman med Meldingar og eventuelt Arrangement. Vis tildelte oppgåver først. Opne oppgåvehandlinga i lista. Bevar utkast og leseposisjon ved fanebyte.
2. Bruk same kø i **Området mitt → Mine oppgåver**. Hent oppgåvene med éin førespurnad på tvers av kanalane, med opphavskanal og krets på kvar rad. Kanalvisinga brukar eit kanalfilter. Denne kontrakten kan også brukast til ei framtidig kretsvising.
3. Vis prosessane i kanalen under oppgåvene. Eit valt element viser registrerte hendingar, den konkrete definisjonsversjonen, aktive steg og moglege vegar vidare.
4. Prøv vanleg deltakar, tap av tilgang, fullføring, parallelle steg, eldre instansar og feil i Heart. Prøv fanebyte med tastatur og på smal skjerm.

## Ansvar og tilgang

Heart eig instansar, definisjonsversjonar og oppgåvetilstand. Sprøyt eig kanalmedlemskap, ruting og fagskjemaa for mellom anna saksbehandling. Køen les dei eksisterande lokale oppgåveprojeksjonane. Ei fullføring brukar same autoriserte handling og varige levering til Heart som oppgåvekortet i samtalen. Chatten treng ikkje vere open. Dette krev ingen ny teneste eller databasepool.

Dei integrerte menneskeoppgåvene har i dag ein namngjeven mottakar. Kanalmedlemskap åleine gjev ingen rett til å ta eller omfordele ei oppgåve. For ein framtidig felles arbeidskø må Heart eige ei uttrykkeleg kandidatliste eller grupperegel, atomisk overtaking ved konkurranse og sporbar frigjeving. Sprøyt må framleis kontrollere levande tilgang. Å tilby alle kanaldeltakarane eit «Ta oppgåva»-val for dagens oppgåver ville ha endra tildelingsreglane.

Detaljvisinga lastar ei avgrensa strukturell utgåve av definisjonen etter tilgangskontroll. Identiteten må stemme med definisjonen instansen vart starta med. Eit registrert steg, eit aktivt steg og ei mogleg grein har ulik tyding. Ved manglande kjelde viser flata den stadfesta informasjonen. Skript, interne vilkår, rå metadata og private oppgåvesvar høyrer ikkje heime i medlemsvisinga.
