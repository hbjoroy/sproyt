# Produktnavigasjon: første leveranse og vidare arbeid

Dato: 26. oktober 2026. Gjennomføring av
[innstillingsplanen](settings-and-administration-plan.md), utvida etter
brukaren sitt oppdrag om eit meir profesjonelt og lettbrukt produkt.
Dette dokumentet gjev ikkje mandat til produksjonspromotering.

## Produktkrava

Ein ny brukar skal forstå kvar samtalane og dei viktigaste vala finst.
Ein brukar med mange kretsar skal kunne prioritere dei han faktisk bruker.
Direktemeldingar treng si eiga tydelege flate. Brukaren skal kunne finne
att staden han nettopp var, og varsel/lenkjer skal opne rett melding.

Dette krev samanheng mellom navigasjon, adressering, leseposisjon og oppsett.
Det er ikkje nok å flytte knappane i den eksisterande hovudmenyen.

## Første implementasjonssnitt

| Område | Endring |
| --- | --- |
| Kretsar | Kretsar kan foldast saman. Aktiv krets er synleg; favorittar og eksplisitt bruk prioriterer rekkjefølgja. Ved mange kretsar blir resten samla under «Andre kretsar». |
| Søk | Treff blir viste mellombels utan å overskrive lagra opningsval. Søk bevarer tilgjengelege kretsar/kanalar i domenemodellen. |
| Direktemeldingar | Eiga fane med eksisterande samtalar og «Ny direktemelding». Nyleg besøkte direktemeldingar blir viste først, med stabil rekkjefølgje for resten. |
| Innstillingar | Søkbart oversyn med eigne grupper og ein kretsveljar. Kontekstsnarvegar opnar same kontrollerte sider. Éi aktiv administrasjonsflate erstattar dialogstakkane. |
| Historikk | Tilbake/fram bruker nettlesaren sin historikk. «Nyleg» er eit avgrensa oversyn over besøkte stader, ikkje ein ekstra tilbake-stakk. |
| Lesing | Kvar historikkoppføring kan ha eiga meldingsforankring og offset, også ved to besøk i same kanal. Tråd og kanal held separate posisjonar og utkast. |
| Adresser | Kanal, melding, tråd, innboks, krets-/DM-oversyn og innstillingssider har eksplisitte mål. Vanlege interne lenkjer går i same app; eksterne lenkjer held trygg eksisterande opning. |
| Meldingslenkje | Meldingsmenyen kan kopiere ei lenkje til akkurat meldinga, inkludert trådkontekst. Lenkja gjev ikkje tilgang utover medlemskapet mottakaren har. |

Første snitt av S1 og S2a er implementert. Agentarbeidet i S2b/S2c er
utrulla i Canary gjennom [PR #304](https://github.com/hbjoroy/sproyt/pull/304)
og [GitOps #271](https://github.com/hbjoroy/rocket-applications/pull/271). Andre profil-, skjema- og
administrasjonsarbeid står att.

Agentflyten er adresserbar frå liste til agent og vidare til éin del:
Oppførsel, Minne, Bilettolking, Biletskaping, Vêr, Sjøtemperatur, Ferger,
Samarbeid eller tilgang for vald kanal. Konto/servermerking forklarer eigarskap.
Kanalstatus viser oppsett og tilgang, ikkje resultatet av ein helseprobe.
Nye agentar blir oppretta avslått som standard.

Utkast er avgrensa til 24 per eigar i minnet og blir ikkje profilsynkroniserte.
Back, lukking og før-unload vernar uferdige agentutkast. Små revisjonsbundne
CAS-oppdateringar bevarer uendra delar; ved timeout blir tilstanden lesen tilbake,
og ved konflikt kan brukaren behalde utkastet eller velje lagra oppsett.

SettingsContext, SettingsSection, SettingsRow og SettingsSummary er
appkomposisjonar av eksisterande `@sproyt/ui`, ikkje nye eksportar frå pakken.
Dei held fast rekkjefølgje, semantiske token og minst 44 px trykkflate.

## Kanoniske mål

`ViewTarget` skil mellom samtale, samtaleoversyn, innboks og administrasjon.
`ManagementTarget` bind ei konkret innstillingsside til eksplisitte ressurs-ID-ar.
Namn er presentasjon, ikkje identitet. Dei eksisterande meldingsadressene blir
vidareførte:

```text
/?channel=<kanal>
/?channel=<kanal>&message=<melding>&sequence=<sekvens>
/?channel=<kanal>&message=<svar>&thread=<rot>&sequence=<sekvens>
/?view=circles
/?view=direct
/?view=inbox&inbox=mentions
/?view=settings&section=circle&circle=<krets>
/?view=settings&section=chat-agents&circle=<krets>
```

Ein delbar URL inneheld ikkje kontonøkkel, innloggingstilstand eller
invitasjonstoken. Konto og navigasjonsgenerasjon høyrer til den lokale økta.
Mottakaren blir framleis kontrollert av eksisterande autorisasjon.
Kalde lenkjer gjennom innlogging og øktfornying bevarer desse måla gjennom
faste lokale returruter, kvitelista ruteverdiar og normaliserte UUID-ar.
Invitasjonar og enrollment har framleis sin eksisterande prioritet.
Serveren opplyser eksplisitt om aktiverte agent-/Heart-flater i bootstrap;
visinga av ein gammal DOM-knapp blir ikkje brukt som funksjonskontrakt.

Brukarstyrte målskifte legg til historikk. Tilstandsoppdatering, innkommande
meldingar, paginering og reconnect skal ikkje gjere det. Back/Forward gjenopprettar
ei eigd oppføring utan å leggje henne inn på nytt. Ei fersk meldingslenkje
prioriterer målmeldinga; ei verifisert gjenopptaking av same nettlesaroppføring
kan gjenopprette den seinare leseposisjonen. Ein ny fane utan lagra oppføring
bruker framleis den stadfesta lesemarkøren.

Innstillingar og innboks kan halde samtalen montert under seg. Dette bevarer
utkast og lesing. Lukking går til faktisk opphav når det finst ei eigd
historikkoppføring; kalde lenkjer har ein definert forelder/samtaleoversyn.
Nøklar og kortliva credentials blir aldri del av historikken.

## Lagring og datavern

- Favorittar blir lagra som ei privat kontoinnstilling i Sprøyt sin database,
  med opptil 12 kretsar. Dei følgjer kontoen mellom einingar og nettlesarøkter;
  dei er ikkje del av den offentlege profilen eller Authentik-attributta.
  Opningsval og bruksteljing er lokale per konto, med opptil 128 kretsoppføringar. Bruksteljinga kjem frå eksplisitte val,
  ikkje frå å lese eller rendera ei liste.
- Nylege stader er avgrensa til 30 og 30 dagar. Dei inneheld ressurs-ID-ar og
  små posisjonar, ikkje meldingsinnhald, namn, løyve eller credentials.
  Gjeldande namn og innsyn blir henta frå den autoriserte visingsmodellen.
- Den vanlege leseposisjonscachen er avgrensa til 100 oppføringar per
  kontrollar. Automatisk opphenting for ein manglande historikkforankring er
  avgrensa; manglande/sletta innhald får trygg fallback.
- Tekstutkast og kanalhistorikk blir kontobundne etter autentisert identitet.
  Eldre utkast utan eigar blir ikkje automatisk tildelte neste innlogga konto.
  Dei blir bevarte og kan hentast eksplisitt i rett samtale etter stadfesting.
- Kontobytte tømmer visingscache, vedlegg, lokale opplastingsstatusar og
  uteståande reveal-intensjonar. Pågåande opplasting sjekkar både opphavleg
  konto og kontogenerasjon før framdrift, nye forespurnader, retry og innlegging
  i skrivefeltet. Ei gammal fullføring skal ikkje fylle ein ny konto sitt utkast.
- Lagringsavslag skal ikkje hindre lesing og navigasjon. Back/Forward-kontrollar
  blir ikkje annonserte som tilgjengelege dersom historikkopptaket feilar.

Første navigasjonsleveranse kravde ingen databasemigrasjon. Oppfølginga for
favorittar legg til `is_favorite` på kretsmedlemskap i migrasjon 0069. Når
medlemskapet blir fjerna, forsvinn favoritten òg. Opningsval, bruksteljing,
historikk og utkast blir framleis ikkje synkroniserte mellom mobil og PC.

Favoritt-API-et bruker innlogga identitet og krev dessutan forventa konto i
`x-sproyt-account`; headeren gjev ingen rettar. Kvar stjerne blir endra med
ein idempotent PUT/DELETE, ikkje ved å overskrive heile lista. Medlemskap og
12-grensa blir kontrollerte atomisk. Klienten hentar på innlogging og retur
til appen, bevarer stadfesta data ved nettfeil og avviser gamle GET-resultat
etter ei endring. Konto og generasjon bind òg køyrande batchar og øktretry.
Favorittane er med i personleg dataeksport.

Eldre lokale favorittar blir ikkje automatisk lagde over profilen, sidan det
kunne gjenoppretta ein favoritt fjerna på ei anna eining. Brukaren får eit
avgrensa, eksplisitt «Overfør lokale favorittar»-val dersom slike finst.
Stadfesta overføringar blir fjerna som lokale importkandidatar.

DM-lista har kompakte rader utan mellomstrekar; namn og metadata held éi
linje med avkorting. Personveljaren bruker same tette rytme. Trykkflater er
minst 44 px på touch og 36 px med presis peikar.

## Validering

Astra har vurdert arkitektur og dei kritiske kodevegane. Funn om same-kanal-
navigasjon, kontobytte/vedlegg, gamle tråd-reveal og reload av meldingslenkjer
er retta. Akseptansen omfattar faktiske DOM-posisjonar, ikkje berre ein testa
historikkklasse:

- To besøk i same kanal, Back/Forward og gjenoppretta meldings-ID/offset.
- Trådar, parallelle utkast, intern meldingslenkje og innboks → kjelde → tilbake.
- Direkte reload av kretsinnstillingar med berre éin aktiv dialog.
- Mange kretsar, favorittar, søk, kontobytte og 320 px breidd.
- Ferdig og sein opplasting frå konto A som aldri kan sendast av konto B.
- Oppdatering med ekte service worker, tekstdraft, leseposisjon og varige køar.

Agentpakken S2b/S2c har lokalt bestått 156 frontend-kontrakttestar og 21
målretta prøver i kvar av Chromium og iPhone/WebKit. Desse dekkjer delvise lagringar, konflikt/uviss
lagring, utkast over Back, konto- og rollebyte under pågåande operasjonar,
gamle GET-svar og privat kanaltilgang. Rust-prøver dekkjer kontobinding på
alle sju agent-HTTP-rutene og kalde innstillingslenkjer gjennom innlogging.
Astra har kontrollert dei kritiske kodevegane etter rettingane. Mørk mobil
på 320/390 px og stor skjerm er visuelt kontrollerte. Endeleg CI og utrulling
er førte i release-evidensen nedanfor.

Agentrelease på `517290ccfbc1e773e8a81ec9f49e88e82d0ff3c1` bestod 233
Chromium- og 109 WebKit-prøver i CI. Argo og offentleg readiness er verifiserte.

## Førstegongsoppleving og direktemeldingar

Neste pakke er implementert og under validering:

- Tom konto får ei roleg velkomstflate når både kretsar og kanalar er ferdig
  henta. Lasting, feil og tomt søk blir ikkje framstilte som ein tom konto.
  «Kom i gang» er òg ein eksplisitt, adresserbar hjelpeinngang for etablerte
  brukarar. Ingen obligatorisk vegvisar eller nytt profilflagg blir oppretta.
- Guiden forklarer krets, kanal og direktemelding og opnar eksisterande
  kanal-/kretsoversikt, personveljar, profil og eit eige invitasjonsskjema.
  Kalde invitasjonar og meldingslenkjer bevarer prioriteten sin.
- Personveljaren bruker kompakte rader med namn og brukarnamn, søk, ventestatus
  og lokal feil/retry. Ei eksisterande 1:1-samtale blir opna med same kanal-ID.
- Direktemeldingar deler tittelresolvar; grupper får ei lita gruppeindikering.
  «Om samtalen» viser medlemmer utan vanleg kanaladministrasjon. Medlemmer
  blir henta for den valde samtalen, ikkje for alle DM-ar ved oppstart.
- Å omtale ein ny person med @ kan framleis starte ei **ny** gruppesamtale.
  Historikken og utkastet i opphavleg 1:1 blir ikkje delte med den nye personen.
- Konto- og sambandsgenerasjon vernar ventande personlesingar, DM-opning,
  invitasjonslevering og automatisk Prat-oppretting. Nye navigasjonsintensjonar,
  inkludert trådar og meldingslenkjer, hindrar seine svar i å ta over visinga.

Ingen databaseendring eller nye medlemsreglar. Fysisk touch og brukartesting
blir tekne i Canary; syntetiske prøver er ikkje ein påstand om slik aksept.

Endelege testtal og release-evidens skal førast etter ferdig CI. Syntetisk
nettlesarakseptanse er ikkje ei fysisk touch-/native-akseptprøve. Brukaren kan
likevel prøve nettpakken remote i Canary utan å starte Windows-klienten.

## Neste små leveransar

1. **Førstegongsoppleving:** fullfør validering og Canary-prøving av tomtilstandar, kort forklaring av krets,
   kanal og DM, og direkte neste handling etter ein invitasjon. Prøv med ein
   heilt ny brukar, ikkje berre administratoren sin konto.
2. **Direktemeldingar:** fullfør validering og Canary-prøving av den eigne oversikta med konsekvente
   person-/gruppetitlar, medlemsvising og betre personval. Bevar eksisterande
   meldingar og kanal-ID-ar; ikkje opprett ein parallell DM-modell.
3. **Agentoppsett:** prøv S2b/S2c i Canary før
   eventuell produksjonspromotering. Autorisert tenestestatus kan vurderast
   separat; oppsettsstatus er ikkje ein helseprobe.
4. **Skjemaarbeid:** vidarefør utkastvern og konfliktvising til andre profil- og
   innstillingsskjema; agentflyten gjer ikkje dette arbeidet ferdig.
5. **Native:** overfør mål, Back/Forward- og eigarskapskontraktar til Windows og
   Android, med native kontrollar. Dette snittet endrar nettappen, ikkje den
   allereie installerte Windows-pakken.

Kvar pakke får målretta testar og ei avgrensa Astra-vurdering ved nye
tryggingsgrenser. Følg vanleg CI/CD til Canary og seinare eksplisitt
produksjonspromotering. Ikkje bruk fysisk Windows-aksept som sperre for
nettleveransen eller gjer ei ny full arkitekturrunde for kvar menyetikett.
