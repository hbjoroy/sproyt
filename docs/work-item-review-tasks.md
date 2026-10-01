# Behandlaroppgåver for work items (steg 3)

Dette byggjer på [registreringssteget](work-item-registration.md). Ein
akseptert sak har framleis eit Sprøyt-eigd saksnummer og kjeldesnapshot.
Heart v2 avgjer når `review`-oppgåva er aktiv og når ho er fullført.
Sprøyt spør Heart regelmessig og lagrar ein leveringskvittering saman med
éi faktisk melding i den konfigurerte oppgåvekanalen. Meldingskroppen er
`[[work-item-task:<Heart-task-id>]]`; ein kopi av teksten i ein annan kanal
gir ingen oppgåvetilgang. Repetert avstemming, restart eller fleire
Sprøyt-replikaer skal ikkje lage fleire meldingar for same aktivering.

Kortet er samanfalda som standard og viser tittel, applikasjon, status og
behandlar. Alle medlemmer i oppgåvekanalen kan lese det. Berre den
Heart-tildelte brukaren som framleis er kvalifisert behandlar med aktiv
`product-handler`-rolle og kanalmedlemskap, får sende kategori, prioritet
og avgjerd. API-et kontrollerer oppgåve-ID, faktisk meldings-ID, gjeldande
rettar og venta saksrevisjon. Same request-ID og same avgjerd er ein retry;
ei motstridande avgjerd vert avvist.

Etter innsending står kortet som «avgjerd lagra, ventar på Heart».
Arbeidaren sender completion med stabil idempotensnøkkel, les deretter
Heart sin autoritative status og oppdaterer saka fyrst ved stadfesta
fullføring. Dette skil lokal aksept frå prosessframdrift. Ved Heart-feil
blir avgjerda verande for ny prøve; ved terminal feil er levering merkt
som feila. Ingen GitHub-eksport eller automatisk utviklingsstart finst her.
Om behandlarrett eller kanalmedlemskap blir trekt attende, er kortet framleis
synleg for kanalmedlemmene og merkt blokkert; serveren avviser nye avgjerder.

Før canaryaksept må vi konfigurere ei avgrensa kjeldekanalbinding,
applikasjon, review-rute og behandlar. Test med to kanalmedlemmer:
registrering, éi oppgåvemelding i riktig kanal, read-only for den andre,
dobbel innsending, Heart-nedetid/restart, og at meldinga held fram som
historikk etter fullføring. Fleire kvalifiserte behandlarar kan
konfigurerast, men MVP vel éin deterministisk ved registrering; dynamisk
felleskø/omfordeling ved bortfall er ikkje implementert.
