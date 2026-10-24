# ADR N02: varig endringsreparasjon og éin lokal lagringseigar

Dato: 24. oktober 2026. Status: vald kontraktretning; konkret schema og wire-format skal prøvast i N02/N05 før aktivering. Sjå [implementasjonsplanen](native-client-implementation-plan.md). Ingen migrering eller ny protokoll er implementert her.

## Problemet som må løysast

`sequence` på ei melding er rekkjefølgja til nye meldingar i ein kanal. Dagens WS/SSE-etterslepsreparasjon kan ikkje generelt hente inn ei redigering, sletting eller reaksjon på ei eldre melding. Ein native klient med lokal historikk treng ein separat markør for synlege endringar, konsistent grunnkopi og ei definert handling når historikken til markøren er utgått.

Vi skal ikkje byte ut meldings-ID-ar, gjere gamle meldingar til nye meldingar eller endre meininga til eksisterande `sequence`. Gamle nettklientar og nye appar må kunne fungere samtidig.

## Avgjerd 1: delte, varige endringar i eksisterande database

Innfør ei revisjonsrad per kanal og ein delt journal med kanal-ID, revisjon, hendingstype, kjelde-ID, entitetsversjon og nødvendige endringsdata. Første implementasjon bør vere enkel og avgrensa: kanoniske små metadata/etterbilete, slettemarkørar og fil-ID-ar; aldri bilete-/videobyte. Det skal ikkje finnast ei kopi av alle journalhendingar per brukar eller apparat.

Kvar transaksjon som endrar synleg samtaleinnhald må:

1. Ta kanalens revisjonslås før dei relevante mutable radene, etter ein dokumentert felles låserekkjefølgje.
2. Kontrollere handling og tilgang, oppdatere domenetilstanden og auke revisjonen transaksjonelt.
3. Skrive tilsvarande journalhending(ar) i same transaksjon.
4. Committe før han gir varig kvittering eller signaliserer ei ny revisjon.

Låsen blir halden til commit/rollback. To transaksjonar i same kanal kan då ikkje få ein «tidleg» revisjon som blir usynleg når klienten flyttar seg forbi henne. `BIGSERIAL`, klokkeslett eller global `nextval()` utan denne commit-kontrakten er ikkje nok. Fleirkanaloperasjonar tek låsar i stabil kanalrekkjefølgje. Endeleg låserekkjefølgje må kontrollerast mot alle faktiske skrivestiar og eksisterande presence-/medlemskaps-/agentlåsar, med concurrency-test og avgrensa deadlock-retry.

Transaksjonelle skrivehjelpar i databaseadapterane er første retning. DB-triggerar er eit separat alternativ dersom kodebasert dekning ikkje kan bevisast; dei skal ikkje innførast som ein utesta snarveg med skjult låserekkjefølgje. PostgreSQL og SQLite får same kontrakttestar, med ulik teknisk låsing.

Journalen får endeleg byte-/retensjonsbudsjett frå N00/N05. Sju dagar er ein mogleg pilotverdi, ikkje ein lovnad om uavgrensa offline-delta. Batchvis opprydding flyttar ein eksplisitt lågaste gyldig markør. Ingen stadig større etterbilete eller full originalhistorikk skal bevarast berre fordi journalen finst.

## Avgjerd 2: skil samtaleendringar frå tilgang og privat brukartilstand

Tre logiske omfang skal ha eigne, dokumenterte versjonar:

- **Kanalendringar:** nye meldingar, redigeringar, slettingar, reaksjonar og nødvendige endringar i rike kort/vedleggsstatus som er delte mellom autoriserte medlemmer.
- **Tilgang og oversikt:** kva kretsar/kanalar denne kontoen framleis kan sjå, og relevante metadata-/medlemskapsendringar. Konto-/kretsversjonar og små autoriserte snapshots skal vurderast framfor å kopiere kvar hending til kvar deltakar.
- **Privat kontotilstand:** lesemarkørar, varselpreferansar og liknande, med rett konto som eigar. Ho skal ikkje sendast i den delte kanaljournalen.

Ein markør er eit synkroniseringspunkt, ikkje tilgangsbevis. Backend kontrollerer gyldig økt og dagens tilgang før nye data blir tekne opp for utlevering. Gjenoppretta tilgang kan krevje ei ny grunnkopi; ein klient kan ha kasta historikken sin. Avbroten tilgang skal ikkje kunne omgåast med ei gammal filadresse eller ein gammal journalmarkør.

## Avgjerd 3: kort og konsistent snapshot, deretter delta

Første synkronisering returnerer eit avgrensa recent-history-snapshot med kanal, aktuelle meldingar/kort, reaksjonar, nødvendige trådmetadata, skjemaversjon og revisjonshorisont. N05 fastset tal- og bytegrenser; 100 meldingar er eit utgangspunkt for måling, ikkje ein uavgrensa fullhistorikk.

Horisont og innhald blir lesne i same korte database-snapshot. Ingen databaseøkt eller MVCC-transaksjon blir halden open over eit mobilnett eller fleire brukarrequestar. Vanleg paginering over tabellar som endrar seg, er ikkje «same snapshot» utan historikk-/snapshotstøtte. Første retning er derfor éin avgrensa respons som kan materialiserast kort på serveren.

Klienten installerer ei fullstendig verifisert grunnkopi og tilhøyrande markør atomisk i lokal lagring. Ufullstendig respons, ugyldig sluttmetadata, konto-byte eller full disk skal ikkje etterlate ein ny markør med halv historikk. Fram til installering er ferdig, blir førre lokale tilstand bevart.

Delta etter revisjon R er ordna, paginert og avgrensa i både byte og tal. Svaret oppgir startpunkt, nøyaktig handtert sluttpunkt, eventuelt meir-flag og gjeldande synkkapasitet. Klienten flyttar markør berre i same transaksjon som endringane er installerte. Entitetsversjonar og slettemarkørar hindrar at duplikat eller eit eldre history-svar skriv over nyare tilstand.

For gammal, feil kanal-/kontobunden eller ikkje støtta markør gir eit uttrykkeleg reset-resultat. Reset etablerer ein **ny lokal synkgenerasjon**. Ny grunnkopi installerer berre sitt avgrensa omfang; eldre cache utanfor dette blir uverifisert og er ikkje tilgjengeleg for vanleg vising, søk eller vedleggstilgang før ho er revalidert mot serveren eller kasta. Elles kan ei gammal sletting bli liggjande synleg for alltid. Utkast og outbox har eigne eigarskaps-/leveringsreglar og blir ikkje sletta som eit biprodukt av reset.

Eldre historikksider er aktuelle autoriserte visingar og blir fletta versjonsmedvite i den nye generasjonen, ikkje framstilte som ei forlenging av ei gammal MVCC-vising. Eit seint history-svar frå før reset kan ikkje gjere uverifisert cache gyldig igjen. Eit påkravd scenario er at lokal melding 101 blir sletta på serveren, klienten er fråkopla lenger enn journalretensjonen og reconnect berre gir dei siste 100 meldingane: den gamle lokale meldinga skal då ikkje visast eller finnast i søk.

Hendingar som kjem medan snapshot blir henta, blir henta att etter snapshot-horisonten. Abonnement, snapshot og første delta må ha ein konkret barriere slik at ei melding mellom «ferdig snapshot» og «ferdig abonnement» ikkje blir borte. Ukjende obligatoriske hendingstypar gir avklart kapasitets-/resetåtferd, ikkje stille skip med falsk framdrift.

## Avgjerd 4: signal er akselerasjon; framdrift blir kontrollert samla

Aktiv app bruker éi multipleksa sanntidstilkopling. WS er hovudsporet; SSE + POST er reserve der WS blir blokkert. Native adapterar må kunne autentisere begge. Dagens enkelte SSE-kanalstraum er ikkje allereie ein ferdig native multiplexer; konkret topic-/oversiktsstøtte skal spesifiserast i N05. Vi skal ikkje love éi straumtilkopling og så skjult opprette ei per kanal.

Nytt wire-format må definere korleis ein klient abonnerer frå eit stadfesta synkpunkt, får ordna endringar eller revisjonshint, og hentar etterslep. Gjenbruk eksisterande domeneutføring og tilgangsreglar; ikkje bygg ein parallell meldingsmotor. Nye felt/hendingar må vere kompatible eller eksplisitt kapasitetsstyrte.

NOTIFY og lokal broadcast vekkjer leveringa, men er ikkje kjelda til sanning. Ein full kø skal føre til avgrensa avbrot og reparasjon. Tal element, byte og tidsbruk skal vere avgrensa også før serialisering; ein treig klient skal ikkje halde uavgrensa RAM.

Tapt NOTIFY medan klienten framleis er tilkopla blir oppdaga gjennom avgrensa framdriftskontroll på API-instansen sine distinkte aktive kanalar. Kontrollen blir batcha og inkluderer versjonert konto-/tilgangstilstand. Tilrådd første prøve er eit spreidd intervall på 15–30 sekund, med maksimal etterslep og databasekostnad målt; dette er ikkje polling per telefon eller ein ny socket per kanal. Signal gir rask normal levering. Oppvakning, reconnect og reell brukaraktivitet utløyser også kontroll.

SLO for tapt signal blir målt separat frå normal meldingsforsinking og reconnect. Intervallet kan ikkje samstundes lovast å gi sub-sekund-reparasjon. Mobil bakgrunn køyrer ikkje denne løkka eller minuttlege WS-probar; native OS-varsling og gjenopptaking tek over. Manglande push skal aldri gi permanent tap, berre seinare innhenting.

## Avgjerd 5: éin lokal eigar og krasjtrygg sendekø

Retninga etter N02 er eit separat Rust-eigd native lagringslag med SQLite, éin kontrollert skrivar og kontoavgrensa databaser. Den reine `sproyt-client-core` skal framleis kunne byggjast med dagens WASM-/`no_std`-krav. Nettlesaren kan halde IndexedDB dersom han består dei same relevante kontraktane.

Kotlin/Swift/C# eig UI, nettverksadapter, OS-jobbar, nøkkellager og filtilgang. Rust eig transaksjonar, lokal migrering og sendelogikk. Ingen Room-/Core Data-/EF-migrering av dei same tabellane. Ingen tung DB-operasjon på UI-tråden. FFI-prøva må bevise små paginerte modellar, kontrollert minnelevetid, trådgrenser, kansellering, feil og ABI-/pakkeoppgradering.

Dersom ei native delingsutviding eller bakgrunnsjobb køyrer i ein annan prosess, kan ho ikkje berre opprette ein konkurrerande økteigar i minnet. N02 må velje og prove koordinering gjennom ein OS-teneste eller ein journal-/leasekontrakt som også toler prosessdød. Eit prosesslokalt mutex er ikkje nok. Fyrste pilot kan avgrense sensitivt arbeid til éin prosess framfor å late som om fleirprosessstøtte er ferdig.

Ei lokal sending blir handsama slik:

1. Utkastet og valde filreferansar høyrer til ein kanonisk konto og kanaltarget.
2. Klienten journalfører request-ID og eksakt, låst payload atomisk **før** han ryddar composer eller utfører servereffekten. Lagre berre filreferansar, ikkje videofiler i database eller FFI-payload.
3. Nettfeil og tapt kvittering held same ID og payload. Ei varig serverkvittering avsluttar sendinga. Ei definitiv domeneavvising blir ikkje automatisk replaya.
4. Ein response frå ei gammal økt-/kontogenerasjon kan ikkje endre den nye kontoen eller markere feil utkast sendt.
5. Lokalt format og replayalder må respektere serverens faktiske idempotens-/kvitteringsretensjon. Ei uviss sending utanfor dette vindauget skal ikkje få ny ID og sendast automatisk. Produktet må tilby ei forståeleg, uttrykkeleg avklaring.

Utlogging og konto-byte stoppar nye opptak i same konto/eining, avbryt relevante arbeidarar og gjer den gamle lokale tilstanden utilgjengeleg før ny konto blir vist. Pågåande arbeid må vere generasjonsbunde slik at eit seint resultat ikkje gjenopprettar cache eller legitimasjon etter rydding. N02 må definere ein avgrensa, krasjtrygg ryddekontrakt for database, vedlegg, delingar og søkjeindeks.

Cache har bytebudsjett og kan kaste historikk. Ei melding som er lesen lokalt, er ikkje eit nytt serverprov på leserett. OS-låst nøkkellager eller appprivat lagringsfeil gir venting/feil, ikkje automatisk sletting av utkast eller ny identitet.

## Innføring, kompatibilitet og stoppunkt

1. Legg til schema og skrivehjelpar utan å annonsere native synk. Lagre ingen ekte pilotdatabase på telefon før identitetsgrensa er prøvd.
2. Kartlegg og test **alle** skrivestiar: nettkommandoar, API/MCP/integrasjonar, agentpublisering, Heart-/arbeidskort, media-status og administrative endringar som påverkar visinga. Ein gammal separat arbeidar kan vere ein skrivar sjølv om API-poddane er nye.
3. Oppgrader lesarar og relevante skrivarar gjennom vanleg CI/CD. Gamle og nye klientar kan leve saman, men synkkapasiteten er avslått medan gamle skrivarar kan gjere uregistrerte endringar.
4. Bevis eit endepunkt for writer-overgangen: gamle prosessar og leasar er avslutta eller dekte, alle nye skrivestiar journalfører, og eit første konsistent snapshot blir laga etter denne barrieren. Ikkje lat som om ei tilbakelagd journal kan rekonstruere alle gamle redigeringar.
5. Aktiver berre pilotkontoar/-klientar i Canary. Nye native klientar installerer ei ny grunnkopi; gamle nettklientar bruker den eksisterande kontrakten sin.
6. Etter annonsering av synkkontrakten kan gamle skrivarar utan journalstøtte ikkje rullast tilbake inn i produksjon. Stopp pilotopptak, bruk minimum kompatibel versjon eller roll-forward. Avslag på native sync skal vere eksplisitt for eksisterande appar.

Påkravde bevis i N05/N06: commit-rekkjefølgje med pausert transaksjon; krasj etter commit; tapt NOTIFY utan socketbrot; snapshot under samtidige endringar; markørutløp; eldre edit/delete/reaction; tilgangstap; identisk replay; full disk; kontobytte; og avgrensa treg klient. Mål også journalkostnad, batchstørrelse, låseventing og spørjingar per apparattime. Eit stort append-berre-tal er ikkje full aksept av offline-synkronisering.

## Referansar

- [Meldingsprotokollen](protocol.md), [driftsmål](operations.md) og [mediaarkitekturen](media-upload-queue-architecture.md).
- [PostgreSQL NOTIFY](https://www.postgresql.org/docs/current/sql-notify.html).
- [UniFFI: støtta språk og bindinggrenser](https://mozilla.github.io/uniffi-rs/latest/).
