# Implementasjonsplan for personleg agentminne

Planlagt funksjon, 6. oktober 2026. Strategien er revidert av Astra mot
noverande agentmotor, databaseadapterar og tilgangskontroll. Planen legg opp
til små leveransar gjennom eksisterande CI/CD og GitOps.

M0-grunnlaget har no eigne Rust-kontraktar og testar i
`src/chatbot/memory.rs`. Kontekst og oppfølgingsanker får serverlagra aktørar,
proveniens og ein versjon av den rå meldinga. Denne metadataen blir ikkje
send til dagens vanlege svarprompt, så eksisterande promptbudsjett blir
bevart. Automatisk innsamling, lagring, modellbygging og minnebruk kjem i
M1-M6.

M1 har no additive migrasjonar og eigaravgrensa lagrings-/API-kontraktar.
Sjå [M1: lagring og eige minne-API](agent-memory-storage-api.md) for rutene,
gløyming, revisjonar og kva som framleis ventar på M2–M6. Innføring i kode
aktiverer ikkje innsamling eller minnebruk.

Kvar kretsagent får eit eige minne om kvar menneskeleg brukar. PostgreSQL
lagrar minnet varig, og Rust samlar og handsamar nye meldingar i avgrensa
arbeidsbolkar. Innsyn, retting og gløyming blir leverte før vi aktiverer
automatisk læring. Første pilot bruker Maria og uttrykkeleg påmelde brukarar.

M2 gir no [innsyn og styring av eige agentminne](agent-memory-user-controls.md)
for vanlege kretsmedlemmer. Læring og minnebruk er framleis avslått.

## Endringar etter Astra sin gjennomgang

| Punkt | Avgjerd i implementasjonsplanen |
| --- | --- |
| Visingsnamn er ikkje stabil identitet | Minneinnputt får avsendar-ID, proveniens, kanal, tråd og kjeldeversjon. Serveren vel minneeigar. |
| Meldingssekvens er per kanal | Cursor og arbeidstilstand blir per agent, brukar og kanal. Redigering og sletting får eigne ugyldiggjeringshookar. |
| Eit ferdig modellresultat kan vente på publisering | Gløyming, retting og avslag på minnebruk aukar ein tryggleiksepoke. Endeleg publisering kontrollerer denne, også ved retry av lagra svartekst. |
| Stadfesting gir ikkje større leserett | Notata bevarer kjeldekanalen og tilgangssjekkane. Etter pilotutvidinga kan minne frå opne kanalar brukast innan same krets, også inn i private kanalar. Private kjelder blir berre brukte i kjeldekanalen. |
| Oppsummeringar kan skjule gamle kjelder | MVP lagrar små notat. Nye notat blir utleidde frå avgrensa meldingstekst; gamle notat blir ikkje sende tilbake som nytt kjeldegrunnlag. |
| Ein arbeidar per pod er ikkje ei felles kvote | Minnekall får eit databaseautoritativt budsjett på tvers av replikaene. Svar har prioritet ved opptak av nye jobbar. |
| Tekststorleik er ikkje heile lagringskostnaden | Budsjettet omfattar notat, kjelder, sperrer, arbeidstilstand og indeksar. |

## Omfang og tilgang

Det logiske minnet høyrer til `(circle_id, agent_id, user_id)`. Notata har
eit `channel_id` som kjeldeomfang. Namnebyte påverkar ikkje identiteten.
Agenten må tilhøyre den aktuelle kretsen, og både brukar, kanal og agent må
framleis ha gyldig tilgang når minne blir henta eller brukt.

Agentoppsettet får `memory_enabled` med av som standard. Agentvalet og
brukarens eige påval må begge vere aktive. Utelating på PATCH bevarer
eksisterande val, og av-val skal kunne lagrast også når modellbygging er
utilgjengeleg. API-et viser tilgjengelegheit; brukarens første påval opnar
ikkje historisk innsamling før den varige sendestien er klar i begge miljø.

Etter pilotutvidinga kan eit agentsvar bruke målbrukaren sitt minne frå
opne (`public`/`local`) kanalar i same krets, også inne i private kanalar.
Minne frå ein privat kanal blir berre brukt i den same private kanalen.
Kanaltype og tilgang blir sjekka på nytt før publisering av lagra svar.
Det finst ingen generell innhenting av minnet til alle som er
til stades. Private kanalar krev eksisterande uttrykkeleg agenttilgang;
direktemeldingar, Felles og andre kretsar er utanfor.

Eit notat kan til dømes seie at Harald og Roald diskuterte ferjer. Det er
ei kjeldebunden hending med stabile deltakar-ID-ar. Ei utsegn frå Roald om
Harald blir ikkje automatisk ei stadfesta opplysning om Harald. Modellen
skal ikkje utleie vennskap, konflikt, diagnosar eller personlegdomsprofilar.

Innsyn og eksport viser eigne notat innanfor gjeldande kjeldetilgang. Ved
mista kanaltilgang kan brukaren sjå ei innhaldsfri melding om utilgjengelege
notat og slette dei; avleidd samtaleinnhald og kjeldetekst blir ikkje vist.
Ved utmelding frå kretsen blir brukarens agentminne der sletta. Andre
brukarar sine relasjonsnotat med vedkommande blir trekt tilbake eller
ombygde utan opplysningane. Vanlege kretsadministratorar får ikkje eit API
for å lese andre sine personlege agentminne.

## Første datakontrakt

Felta under er forslag til kontrakt, ikkje ferdige migrasjonar. Neste
ledige migrasjonsnummer blir valt når implementeringa startar.

| Lagring | Ansvar |
| --- | --- |
| `agent_memory_profiles` | Unik agent/krets/brukar, uttrykkeleg brukarval, tryggleiksepoke `memory_epoch`, revisjon for brukarendringar og samla budsjett. |
| `agent_memory_scopes` | Unik profil/kanal, startgrense, handsama sekvens, siste ventande sekvens, kjeldegenerasjon, tidspunkt, lease, forsøk og retry. Same rad samlar gjentekne arbeidsønske. |
| `agent_memory_notes` | Profil, kanal, type, strukturert innhald, serverkontrollert evidensstatus, opphav, revisjon og eventuell utløpstid. Brukarstyrte notat er verna mot automatisk overskriving. |
| `agent_memory_note_sources` | Kjelder og avhengnader: meldings-ID, versjon/hash og relevante deltakarar. Indeks på kjeldemelding for direkte ugyldiggjering. Deling av kjeldegrupper kan redusere duplisering dersom det gir ein enklare kontrakt. |
| `agent_memory_exclusions` | Kjelde-ID-ar som ikkje skal brukast igjen for dette minnet etter gløyming. Ingen kopi av gløymd tekst. |
| Felles modellbudsjett | Fleirreplica-kvote/lease, tidsfrist og avgrensa token-/arbeidsbudsjett for minnekall. |
| Utvida svarjobb | Referansar til brukte minneprofilar, epokar, notat og kjeldeversjonar. Retry av `reply_body` må bevare desse avhengnadene. |

Autoritet, identitet, tilstand, kjelder og revisjonar får relasjonelle felt.
Notatinnhaldet bruker `jsonb` i PostgreSQL og validert JSON-tekst i SQLite,
med same Rust-typar og adapterkontrakt. PostgreSQL dokumenterer denne
kombinasjonen av vanlege kolonnar og strukturerte dokument i
[JSON types](https://www.postgresql.org/docs/current/datatype-json.html).
Første versjon treng B-tree-indeksar for eigar, kanal, ventande arbeid og
kjelder. Redis, vektorsøk og JSON-innhaldsindeksar blir ikkje innførte.

Notattypane er eigne uttrykte preferansar, tidsavgrensa samanheng og konkrete
samhandlingshendingar. Tidspunkt, usikkerheit og opphav blir bevarte. Modellens
forslag er ikkje etablerte fakta. Utløp blir styrt av validerte kategoriar og
servergrenser, ikkje av vilkårlege modellval.

## Innsamling og framdrift

Når ein ny, kvalifisert menneskemelding blir lagra, blir ventande arbeid
markert i same transaksjon. Det skjer før og uavhengig av val av trigger,
mention eller oppfølging. Idempotent replay markerer ikkje meldinga på nytt.
Agentmeldingar blir ikkje nye minnekjelder.

Det eksisterande taket er ti agentar per krets. MVP kan derfor gjere ei
avgrensa oppdatering per aktiv minneagent med påmeld brukar, framfor å
innføre ein eigen distribusjonskø. Kostnaden på sendestien skal målast.
Meldinga skal aldri vente på eit modellkall.

Arbeidaren les inntil tjue nye kvalifiserte meldingar frå målbrukaren etter
cursoren, i stigande sekvensrekkjefølgje. Nærståande menneskemeldingar i dei
aktuelle samtalespora kan gi avgrensa samhandlingskontekst. Kjelder frå andre
kanalar, før startgrensa eller under gløymsperre blir ikkje tekne med.

Arbeidaren fangar ein endeleg sekvens og ein kjeldegenerasjon før modellkallet.
Minneskriving og flytting av cursor skjer atomisk etter ny tilgangs- og
leasekontroll. Eit gyldig resultat med ingen nye notat flyttar også cursoren.
Nye meldingar medan modellen arbeider blir ståande som meir arbeid. Seksti
uhandsama meldingar krev fleire bolkar; vi hoppar ikkje direkte til dei siste
tjue. Ingen uavgrensa historisk innlesing skjer ved aktivering.

Redigering gir ikkje ny meldingssekvens. Derfor trekkjer redigering og
sletting tilbake avhengige notat, aukar relevant kjeldegenerasjon og markerer
avgrensa ombygging i same mutasjonstransaksjon. Tilgangsendringar får
tilsvarande sperrer. Ein vanleg cursor åleine løyser ikkje dette.

## Minnebygging og modellbudsjett

Byggaren returnerer strukturerte kandidatar med avgrensa tekst, type,
tidspunkt og kjeldetilvisingar. Rust kontrollerer både skjema og at ID-ar og
opplysningstypar høyrer til den faktisk leverte innputten. Modellen kan ikkje
velje minneeigar, kanal, SQL, nettverksadresse, credential eller publisering.

Første byggar får nye meldingar og avgrensa samtalekontekst. Han får ikkje
gamle minnenotat som faktagrunnlag. Serveren handterer duplikat og erstatning
utan å innføre ein kjede av oppsummeringar. For nye kandidatar må kjeldeavhengnadene
omfatte relevant innputt konservativt; ei modelloppgitt kjeldeliste åleine
beviser ikkje kva teksten faktisk byggjer på. Ugyldiggjering av ei nødvendig
kjelde trekkjer tilbake heile notatet.

Eksisterande vLLM kan tilby strukturert utdata; støtta på den installerte
versjonen og modellen blir verifisert før bruk. Rust-validering gjeld uansett.
Sjå [vLLM structured outputs](https://docs.vllm.ai/en/latest/features/structured_outputs/).
Ugyldig svar gir avgrensa retry og synleg etterslep, ikkje eit fritekstnotat.

Førebelse pilotgrenser er tjue eigne meldingar per bolk, høgst ti meldingar
som samtalekontekst, 16 KiB samla notattekst og høgst 24 aktive notat per
agent/brukar på tvers av kanalar. M0 set 1 KiB per notat, høgst 720 aktive
kjeldetilvisingar, 1 024 gløymsperrer og 64 kanalskop per profil. Den samla
serialiserte kontrakten har i tillegg eit tak på 256 KiB; ikkje alle
enkeltgrenser kan brukast som ein lovnad om faktisk diskforbruk.
Ein stor fixture med notat, kjelder, sperrer og arbeidstilstand bruker
248 491 JSON-byte. PostgreSQL-kontrakten måler òg `pg_column_size` av denne
fixturen som JSONB. Relasjonelle rader, indeksar, WAL og backup blir målte i
M1, når tabellane finst. Desse målingane skal avgjere om pilotgrensene må
senkast før aktivering. Når eit tak blir nådd,
skal systemet synleg avgrense ny læring; det skal ikkje kaste gløymsperrer eller
brukarstyrte notat for å halde fram i det stille.

M0 set også 24 KiB samla modellinnputt, høgst 600 utdata-token og opptak av
høgst to nye minnekall per minutt på tvers av replikaene. M4 må handheve
desse grensene; dei reserverte brytarane aktiverer ikkje ein arbeidar i M0.
Pilotens standardutløp er sju dagar for mellombels samanheng og nitti dagar
for samhandlingshendingar. Uttrykte preferansar har ikkje automatisk utløp,
men er avhengige av medlemskap, kjeldegyldigheit, eigarval og gløyming.

Nye minnekall får låg prioritet og høgst éin gyldig felles kvote om gongen.
Ein lokal semaphore per pod er ikkje tilstrekkeleg. Ingen database-
transaksjon eller pooltilkopling blir halden gjennom modellkallet. Lease,
modellfrist og avbrot må dimensjonerast og testast saman; ein modellførespurnad
som allereie er sendt, kan ikkje reknast som fullstendig avbroten berre fordi
den lokale leasen går ut.

Etter kvar bolk stiller ein travel brukar bak andre ventande brukarar.
Lågtrafikkbrukarar skal kunne bli handsama etter ein tidsfrist utan tjue
meldingar. Pilotutgangspunktet er handsaming etter fem nye meldingar eller
fem minutt utan ny aktivitet, med eit mål om å plukke opp ventande arbeid
innan femten minutt når modellbudsjettet har kapasitet. Under overlast viser
vi etterslep; dette er ikkje ein garanti om fast ferskleik. Eit minnekall
som allereie køyrer kan framleis påverke svartid.

## Bruk i agentsvar og gløyming

Svararbeidaren hentar eit lite relevant utval av gyldige notat for
målbrukaren og kjeldekanalen. Innhaldet er serialiserte data, ikkje nye
systeminstruksjonar. Det blir sett eit separat promptbudsjett for minne;
den ferske kjeldemeldinga og faktisk verktøydata har prioritet.

Vanleg læring aukar notatrevisjonar. Brukarretting, gløyming, nullstilling og
avslag på minnebruk aukar også `memory_epoch`. Endeleg publisering sjekkar
epoke, aktuelle notat/kjelder og tilgang i same transaksjon som svaret blir
lagra. Det omfattar svartekst som allereie ligg i ein jobb for retry.
Eit utdatert resultat blir forkasta; eit nytt svar kan berre køleggast dersom
den opphavlege samtalejobben framleis er gyldig. Ingen automatisk publisering
av den gamle teksten er tillaten.

Gløyming av eitt notat slettar det, ugyldiggjer avleidde resultat og sperrar
dei aktuelle kjeldene mot ny bruk i byggaren, også som bakgrunnskontekst.
Nullstilling slettar notata og flyttar startgrensa for kvar kanal til den
kontrollerte nullstillinga. Nyare meldingar kan gi nye minne. Avslått minne
stoppar både innsamling og bruk; ei framtidig læringspause kan vere eit eige val.

Desse vala slettar ikkje dei opphavlege chatmeldingane eller allereie
publiserte agentsvar. Det skal stå tydeleg i brukarflata. Ei permanent sperre
mot eit tema i framtidige meldingar er ei eiga funksjon. Backupretensjon og
gjenoppretting må dokumentere korleis seinare gløyming blir handheva etter
restore; sletting i hovuddatabasen åleine er ikkje ein full restore-prosedyre.

Ei trygg, enkel MVP-regel ved restore av ein eldre database er å nullstille
alt gjenoppretta agentminne, også brukarstyrte notat, stoppe minnebaserte
svarjobbar og setje nye startgrenser før minnearbeidarar får starte. Minne
blir sett av og krev nytt påval. Chatmeldingar blir bevarte. Dette kostar
oppbygd minne, men hindrar at ein gammal backup vekker gløymde opplysningar.
Bevaring av minne gjennom restore krev seinare ei separat, oppdatert
slettingsjournal og skal ikkje lovast av første versjon.

## Innsyn og brukarflate

Vanlege medlemmer får inngangen **Kva hugsar agenten om meg?** innanfor
aktuell krets, uavhengig av om dei kan administrere agenten. Flata viser
agent, kanal, notat, opphav, tidspunkt, kjelder brukaren framleis kan lese,
minnestatus og etterslep. Handlingane er rett, stadfest, gløym, nullstill og
minne av/på. Kjeldemeldingar blir ikkje kopierte inn i driftsloggar.

Eigaravgrensa API-ruter kan liggje under
`/api/v1/me/circles/{circle_id}/chat-agents/{agent_id}/memory`.
Innlogga menneske blir vald frå den autentiserte principalen; ein klientvald
brukar-ID gir ikkje rett til andre sitt minne. Endringar krev venta revisjon.
Svar blir merkte `no-store`, og sein HTTP-respons etter kontoskifte eller
dialoglukking skal ikkje fylle ei ny flate.

Den eksisterande snapshot-konsistente `/api/v1/me/export` blir utvida med
same eigar- og kjeldetilgangsreglar. Brukaren skal få med notat, bruksomfang
og opphav; eksporten skal ikkje opne ei skjult kanal eller andre sine profilar.

## Granulerte leveransar

| Etappe | Konkret arbeid | Ferdig når |
| --- | --- | --- |
| M0 Kontrakt og grenser | Fastset Rust-typar, notatkategoriar, startgrenser, samla budsjett, utløp og tre uavhengige aktiveringsflagg. Utvid minneinnputt med stabile aktørar og proveniens. | Namnebyte og like namn gir korrekt attribusjon; datakontrakt og målt storleiksbudsjett er dokumenterte. Ingen modelljobb er aktivert. |
| M1 Lagring og eige API | Additive PostgreSQL/SQLite-migrasjonar; profil/scope/notat/kjelder/sperrer; eigaravgrensa les/rette/stadfest/gløym/nullstill/av-på. Utvid eksport. | Begge adapterar dekkjer eigar, konflikt, kjeldegrenser og atomisk sletting. Administrator/annan brukar blir avvist. |
| M2 Brukarstyring | Frontend-kontraktar og kompakt Sprøyt-dialog for vanlege medlemmer; kanalmerking, kjeldelenkjer, status, kontrollar og val av læring. | Chromium/WebKit og mobilbreidde fungerer, også kontoskifte, sein respons, feil og konflikt. Ingen læring før kontrollane finst. |
| M3 Varig innsamling og ugyldiggjering | Marker ventande arbeid på begge meldingssendestiane utan triggerkrav. Hook redigering, sletting, utmelding, kanalval og profiltilbakekalling. | Triggerfri melding, idempotent replay, gamal redigering og samtidige tilgangsendringar blir handterte utan hol eller stale notat. Modellbygging er framleis av. |
| M4 Bakgrunnsbyggar | Claim/lease, tjue-meldingsbolkar, strukturert vLLM, validering, kjelder, atomisk cursor, felles kvote og rettferdig kø. | Faktisk PostgreSQL testar to replikaer, restart, utgått lease, ny melding under arbeid, nullresultat, ugyldig JSON og tidsfrist for låg trafikk. |
| M5 Minne i svar | Relevant utval, separat promptbudsjett, avhengnader i svarjobb og transaksjonell publiseringssperre. | Gløym/rette/kjeldesletting etter modellkall og før publisering stoppar også lagra `reply_body`. Minne av gir vanleg svar utan minnebruk. |
| M6 Canary og produksjon | Full CI/release, immutable image, backup/restore, kompatible workerar i begge miljø og avgrensa pilot. | Maria hugsar ulike preferansar for to menneske, fangar samtale utan trigger, respekterer privat kanal og gløyming, med målt kostnad og svartid. |

M1 og M2 si brukarstyring er ein aktiveringsføresetnad for M3 og M4, sjølv
om utvikling av dei seinare etappane kan skje med flagga av. Ein målretta
Astra-kontroll bør skje etter M3 og før M5 blir aktivert: det er her sletting,
lease og publikasjon må fungere som éin kontrakt.

## Integrasjonspunkt og kontrollar

- `src/chatbot.rs`: kontekstidentitet, konfigurasjon, svarjobbar og
  endeleg publisering. Eit eige `src/chatbot/memory.rs` kan halde
  minnearbeidet avgrensa utan å byggje eit generelt agentrammeverk.
- `src/db/postgres.rs` og `src/db/sqlite.rs`: begge sendestiar,
  redigering/sletting, medlemskap og den eksisterande brukar-eksporten.
- `src/web/chatbot.rs`, `src/server.rs` og ny minnerute: autentisert eige API,
  tilgjengelegheit og separat minneaktivering.
- `frontend/src/chat-agents.ts` og React-flater: typed dekoding,
  kretsmedlem sin inngang og eksisterande designspråk.
- Migrasjonar og `.github/workflows/ci.yml`: eigne minnekontraktar må faktisk
  veljast i jobben med PostgreSQL. Null testar eller manglande database skal
  ikkje kunne gi ei falsk grøn akseptanse.

Kontrollmatrisa skal omfatte namnekollisjon, menneske/agent-proveniens,
Harald/Roald-isolasjon, private kanalar, av/på, utmelding, endra kjelder,
gløyming under modellkall og under retry, to replikaer, nye meldingar under
arbeid, kjeldetilvising som modellen finn på, promptinjeksjon og eit fullt
minnebudsjett. Vanleg chat skal halde fram ved feil i modellen.

Mål køalder, handsama/hoppa over/feila jobbar, modellkall, rapporterte token
der dei finst, varigheit, kvoteventing og faktisk lagringsstorleik. Driftsmålingar
skal ikkje ha brukar-ID, agent-ID, kanal-ID eller minneinnhald som labels.

## Aktivering og vidare arbeid

Innsamling, modellbygging og bruk i svar får separate flagg som er av som
standard. Prod og canary deler database. Før første aktivering må alle
arbeidarar som kan publisere, køyre kode som kjenner dei nye sperrene.
Eksisterande CI/CD, full backup/restore og GitOps blir brukte; minneval for
Maria endrar ikkje triggerord, personlegdom eller eksisterande funksjonar.

Helm har desse reserverte verdiane med standard `false`:
`config.chatAgentMemoryCollectEnabled`, `config.chatAgentMemoryBuildEnabled`
og `config.chatAgentMemoryUseEnabled`. Dei gir respektive miljøvariablar
`SPROYT_CHAT_AGENT_MEMORY_COLLECT_ENABLED`,
`SPROYT_CHAT_AGENT_MEMORY_BUILD_ENABLED` og
`SPROYT_CHAT_AGENT_MEMORY_USE_ENABLED`. Dei skal vere av fram til dei aktuelle
etappane og brukarstyringa er klare.

Flagga styrer opptak av nytt arbeid, ikkje rett til å omgå lagra avhengnader.
Ein prodarbeidar som tek ein svarjobb frå canary, må handheve jobben sine
minnesperrer sjølv om minnebruk er av i hans miljø. Eit flaggskifte kan
ikkje publisere forelda `reply_body` utan kontroll eller vekkje gamle jobbar.

Pilotbrukarane vel minne på med synleg startpunkt. Aktiver først avgrensa
innsamling, deretter bygging og til slutt bruk i svar. Ved behov kan nye
modelljobbar stoppast medan innsyn og gløyming framleis er tilgjengelege.
Etter aktivering skal eventuell rollback bruke kompatibel kode som handhever
lagra epokar, kjeldesjekkar og brukarval.

Bruk av private notat på tvers av kanalar, vektorsøk, bilete som varige
personopplysningar, generell kunnskapsgraf og autonome minnebaserte handlingar
er seinare etappar. Ei framtidig kretsomfattande sjølvopplysning blir oppretta
uttrykkeleg med synleg bruksomfang, og kan ikkje løfte privat samtaleinnhald
om andre deltakarar.
