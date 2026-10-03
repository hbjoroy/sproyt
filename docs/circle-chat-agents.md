# Samtaleagentar på kretsnivå

Status: arkitektur for første implementasjon, 2026-09-30. Astra har vurdert
eksisterande agent-, meldings- og vLLM-kode. Dette er ein avgrensa chatbot,
utan verktøy, nettsøk eller høve til å starte andre handlingar.

## Åtferd og omfang

Ein kretseigar opprettar ein agent med namn, triggerord/-frasar og
svarord/-setningar. Agenten kan aktiverast og deaktiverast. Ei ny
menneskemelding som treff eitt eller fleire triggeruttrykk, kølegg høgst
éin svarjobb for denne agenten og meldinga. Fleire aktive agentar kan
reagere på same melding.

Agenten les berre tekst frå dei siste 20 minutta i **same kanal og tråd**,
fram til og med meldinga som utløyste jobben. Han svarar fyrst og fremst
denne meldinga og kan bruke avsendaren sitt viste namn. Ei melding som
allereie er meir enn 20 minutt gammal når jobben blir handsama, får ikkje
eit forsinka svar. Eit trådsvar kjem i same tråd; eit kanalsvar i kanalen.

Opne kretskanalar (`public` og `local`) har tilgang som standard.
Private kretskanalar krev eit uttrykkeleg kanalval. Direktemeldingar,
gruppedirektemeldingar og kanalar i Felles er utanfor.
Ingen eksisterande meldingar vert kølagde når ein
agent blir aktivert. Agentmeldingar er merkte som genererte og kan aldri
trigge ein annan agent.

## Ansvar og eksisterande grunnlag

`agent_profiles`, `users.kind=agent` og meldingsproveniens finst allereie;
den nye funksjonen brukar desse. Chatmotoren har varig melding, sekvens,
realtime og idempotens. Santorini vLLM kan nåast gjennom
`SPROYT_VLLM_URL` og `SPROYT_VLLM_API_KEY`, med modelloppdaging frå
`/models`. Biletspesifikk art direction og nettsøk blir ikkje gjenbrukt.

Ein eksisterande kretsgrant opprettar ikkje kanalmedlemskap, så han gir
ikkje i seg sjølv ein serverintern agent rett til å sende. Ein snever
publiseringsveg for gyldige, leigde svarjobbar må kontrollere agent,
konfigurasjonsversjon, krets, kanal, tråd og kjeldemelding i same
transaksjon som han lagrar svaret. Han må ikkje vere ein generell veg
for å omgå kanalrettar. Ingen agent-credential vert utlevert til
nettlesaren eller til modellen.

## Lagring og API

`circle_chat_agents` bind éin eksisterande agentidentitet til éin krets.
Namnet ligg på brukaren. Konfigurasjonen har validerte JSON-lister for
trigger- og svaruttrykk, `enabled`, stigande `revision`, opprettar,
oppdaterar og tidspunkt. Oppretting av brukar, profil og konfigurasjon
er atomisk. Ei slått av eller tilbakekalla konfigurasjon kan ikkje
publisere eit seinare svar; historiske agentmeldingar blir bevarte.

`circle_chat_agent_jobs` bind agent og kjeldemelding unikt og lagrar
konfigurasjonsversjon, status, avgrensa forsøk, lease-token, tidspunkt,
eventuell godkjend svartekst og endeleg meldings-ID. Køen kopierer ikkje
heile samtalehistorikken. Arbeidarar bruker atomisk claim og lease-fencing,
så fleire replikaer ikkje publiserer same svar to gonger.

HTTP-kontrakt: `GET/POST /api/v1/circles/{id}/chat-agents` og
`PATCH /api/v1/circles/{id}/chat-agents/{agent_id}`. Oppretting og endring
krev eigarrolla i kretsen, kontrollert i databasen. `PATCH` tek venta
`revision` og gir konflikt ved ei forelda endring. Namn og lister blir
avgrensa og normaliserte på serveren. Deaktivering er første versjons
«sletting».

UI viser **Agentar** i kretsadministrasjonen, med éin verdi per linje
for triggeruttrykk og svarfrasar, lagring og ein aktivbrytar. Når
modellarbeidaren er utilgjengeleg, skal det visast; aktivering skal
ikkje late som eit svar vil kome. Kortliva «Agenttilgang» for MCP er
ein annan funksjon og vert verande skild.

## Trigger, kontekst og prompt

Triggertekst vert trimma, whitespace normalisert og samanlikna utan
skilnad på store/små bokstavar. Heile uttrykket må treffast med
bokstav-/talgrense rundt; brukerdefinert regex blir ikkje køyrd.
Tomt eller duplisert uttrykk vert avvist/fjerna. Tal, lengd og samla
promptbudsjett får faste servergrenser. Berre ferske menneskemeldingar
i tillatne kanalar vert kandidatar; redigering og replay kølegg ikkje
nye jobbar.

Konteksten kjem frå ein serverstyrt, avgrensa SQL-spørjing med same
kanal/tråd, `deleted_at IS NULL`, siste 20 minutt og sekvens høgst lik
kjeldemeldinga. Ho blir sortert stigande. Ved bytegrense fell eldste
melding ut fyrst; sjølve kjeldemeldinga må vere med. Hent berre vist
namn, tekst, meldings-ID og tidspunkt. Ingen e-post, vedlegg, nøklar
eller andre kretsar vert sende til vLLM.

Systemprompten vert bygd deterministisk av validert konfigurasjon:
agentnamn, triggeruttrykk som tema for kvifor agenten vart kalla inn,
svarfrasar som føringar, eit kort og naturleg svar på den
utpeikte siste meldinga, og høve til å nemne avsendaren. Eldre meldingar
er bakgrunn. Svarfeltet blir vist som stikkord og svarføringar i
grensesnittet. Lengre ferdigskrivne svar som modellen kopierer ordrett,
blir prøvde på nytt éin gong med krav om å svare på kjeldemeldinga; ved
ny kopiering blir jobben feila utan å publisere eit standardsvar. Korte
helsingar kan framleis vere identiske når det er naturleg. Denne regelen
garanterer ikkje kvalitet: canary-prøver med ulike kjeldemeldingar må
vise at svaret faktisk varierer.

Samtaletekst og konfigurasjonsverdiar vert sende som
serialiserte data; dei kan ikkje endre API-rollene, velje kanal eller
opne verktøy. Modellen får ingen tools eller nettverkstilgang gjennom
Sprøyt. Berre eit ikkje-tomt, lengdeavgrensa `assistant.content` blir
publisert; tenkje-/reasoning-felt blir aldri vist. Prompt og svar blir
ikkje skrivne til driftsloggar.

## Feil, drift og verifikasjon

Kølegging skjer i same transaksjon som meldingsinnsettinga, både for
vanleg og idempotent sendesti. LLM-kall skjer i ein arbeidar og blokkerer
aldri chat. Manglande modell eller deaktivert global brytar stoppar
arbeidaren. Timeout/429/5xx kan prøvast om att med same jobb og avgrensa
backoff; ugyldig/tomt svar blir feil utan kanalsøppel. Deaktivert agent,
endra/sletta kjelde eller utgått tidsvindauge blir hoppa over.

Før aktivering i canary skal SQLite- og PostgreSQL-prøver dekke eigarrett,
kretsisolasjon, private kanalar, triggergrenser, 20-minuttsvindauge,
trådar, endring/sletting medan modellen arbeider, idempotens, restart og
parallelle arbeidarar. Ein falsk vLLM gir deterministiske testar;
ein avgrensa manuell test mot Santorini provar modelloppdaging og eitt
faktisk svar. Eit seinare driftssteg bør leggje til teljarar for kø,
fullførte/hoppa over/feila jobbar og modell-latens utan meldingstekst.

## Agentval per kanal (#205)

Migrasjon 0051 legg til `channel_chat_agent_settings`, ein stigande
`channels.chat_agent_access_revision` og den fangste revisjonen på kvar
svarjobb. Manglande val betyr på i opne kretskanalar og av i private;
eit uttrykkeleg av-val vinn. Agenten må alltid tilhøyre same krets.
Kanalval endrar ikkje agenten sin globale aktivbrytar eller konfigurasjon.

`GET /api/v1/channels/{id}/chat-agents` viser namn, global aktivstatus,
kanalval, tilgangsrevisjon og `selection_available`; ikkje trigger/prompt.
`PATCH /api/v1/channels/{id}/chat-agents/{agent_id}` tek `enabled` og venta
`access_revision`. Forelda revisjon gir konflikt; kanalval og revisjonsauke
blir lagra atomisk med aktør i auditloggen.

Begge API krev faktisk kanalmedlemskap. Kanalowner/moderator kan velje.
I opne kretskanalar kan også kretseigar/moderator med skrivetilgang velje.
Kretsrolla gir ikkje tilgang til private kanalar. Lesing sjekkar rett og
data i same statement; endring låser autoritetsmedlemskap i transaksjonen.

Jobben bind både konfigurasjons- og kanaltilgangsrevisjonen. Tilgang blir
kontrollert ved kølegging, kjelde-/kontekstlesing og publisering. Av→på
kan ikkje vekkje gamle jobbar. Endring av éin agent kan konservativt stoppe
andre ventande jobbar i kanalen. PostgreSQL publisering låser kanal før
agent; SQLite serialiserer skrivinga. Ein modellførespurnad som allereie er
send, kan ikkje trekkjast attende, men eit seinare svar kan stoppast.
Historiske svar blir bevarte.

### Første aktivering og tilbakerulling

Prod og canary deler chatdatabasen. Første imageutrulling skal ha
`config.channelChatAgentsEnabled: false`
(`SPROYT_CHANNEL_CHAT_AGENTS_ENABLED=false`) i begge miljø. Dette sperrar
HTTP-endringar og deaktiverer kanalvala i UI; workerane handhever alltid
lagra val. Med ingen tidlegare kanalval bevarer default revisjon 1 dei
historiske, opne svarjobbane, medan private kanalar framleis er av.

Kontroller at alle gamle workerprosessar i begge miljø er avslutta, og at
alle app-replikaer køyrer ny image/revisjon, før ei eiga GitOps-endring
set flagget til true. Ein gammal worker kan elles ta ein privat jobb og
skippe han, eller ignorere eit nytt av-val. Etter aktivering er av-flagget
åleine ikkje trygg tilbakerulling til kode utan kanalval; bruk ein kompatibel
image som handhever lagra val og revisjonar.

## Vêrdata (#212)

Ei avgrensa serverstyrt vêrfunksjon kan knytast til agentoppsettet. Sjå [vêragent](weather-agent.md) for stadval, grounded data, oppfølging, kontrakt og aktiveringshinder. Modellen får framleis ikkje generelle verktøy eller valfrie nettverksadresser.
