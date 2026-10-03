# Leseposisjon og lesemarkør

Kanalens lagra lesesekvens og nettlesaren sitt lokale scrollanker er to ulike
ting. Serveren lagrar ein monoton sekvens per brukar og kanal. Klienten held
melding-ID og avstand frå toppen i minnet for retur i same økt.

Plassering ved opning har denne prioriteten:

1. Ei eksplisitt meldingslenkje eller eit varsel peikar på meldinga, også i tråd.
2. Retur i same økt gjenopprettar melding og offset, også nær den gamle botnen.
3. Ei ny økt brukar lesesekvensen ved opning og viser første uleste rotmelding
   med inntil 120 px kontekst. Når alt er lese, viser ho siste melding.

Det valde opningsmålet står fast under lasting, kanalbyte og endringar frå ein
annan klient. Klienten lastar eldre sider til målet og konteksten finst, eller
til den faktiske starten er nådd. Tomme renderingar, svar utan røter og seint
komande historiesvar skal ikkje erstatte målet. Den viste ulestgrensa er grensa
ved opning; kvitteringar flyttar ikkje denne skiljelinja medan brukaren les.

Ved kanalretur kan meldingane vere i DOM før bilete og diagram har fått høgd.
Om nettlesaren avgrensar scroll til ein mellombels botn, held klienten det
opphavlege ankeret og kvitter ikkje denne plasseringa. Neste layoutendring
prøver same offset igjen. Ekte scrollinput eller **Gå til siste** overstyrer
restaureringa, også om endra innhald gjer den gamle offseten umogeleg.

Historikklasting flyttar ikkje lesemarkøren. Etter ferdig plassering og ved
scroll/fokus måler klienten meldingar som overlappar det synlege vindauget med
minst 24 px (eller heile høgda for kortare meldingar). Dokumentet må vere
synleg og ha fokus; skjult mobilnavigasjon, skjult kanal bak tråd og bakgrunn
bak ein dialog blir ikkje kvittert. Synlege svar kan flytte både kanalmarkøren
og markøren for den tråden. Ei rotmelding eller talet på svar er ikkje bevis på
at svara er lesne.

Lesemarkøren betyr «lese fram til den høgaste synlege sekvensen», ikkje ei
samling av individuelle meldingar. Ei eksplisitt lenkje eller **Gå til siste**
kan difor passere tidlegare meldingar. Framdrift blir først stadfesta av serveren;
feila eller tapte kvitteringar kan prøvast igjen ved neste synlege måling, fokus
eller reconnect. Gamle/lågare markørar får ikkje flytte sekvensen bakover.

**Gå til siste** finst i kanalhovudet på desktop og under **Meny** på mobil.
Når brukaren er ved den aktuelle botnen, følgjer nye meldingar normalt etter.
Ved lesing bakover held nye meldingar, reaksjonar og seint lasta bilete ankeret.
Berre denne klienten sin ventande sending gir automatisk avsløring av ei eiga
melding; ei melding frå same brukar på ein annan klient gjer ikkje det.

`ui-react-reading.spec.ts` prøver opning, paging til ulestgrensa, reload,
kanalretur, reconnect, sein layout, to klientar og skjulte/synlege trådsvar i
Chromium og WebKit iPhone. `ui-react-history.spec.ts` held på rå cursor,
retry/timeout og svar utan røter frå #182. `ui-react-scroll.spec.ts` prøver
reell WebSocket/SQLite-paging, medieskalering og lokal sending i tråd.
