# Plan for ryddigare Sprøyt UI

Avtalt 28. oktober 2026 etter gjennomgang av Paros / Prat og Reiseglede /
26-10 Ølesund i produksjon ved 320, 390, 430, 820 og 1440 px.
Vi beheld informasjonsarkitektur, fargar, merkevare og funksjonar.

## Rekkefølgje og utrullingsgrenser

1. Oppdater skill og designkontraktar, og lag denne planen.
2. Fullfør PR #314 (vekta direktemeldingsliste) og #315 (kompakte barresultat).
   Vent på relevante CI-resultat, merge og bygg eitt felles release-image frå main.
   Rull same digest/chart gjennom GitOps til Canary og produksjon. Stadfest
   Argo Healthy/Synced, replikaer, offentleg readiness og faktisk revisjon.
3. Først etter stadfesta produksjonsutrulling implementerer vi UI-et nedanfor.
4. Test, review og merge UI-endringane; bygg gjennom CI/CD og rull berre Canary.
   Produksjon skal bli verande på revisjonen frå steg 2 til ny beskjed.

Ingen skjemaendringar er planlagde. Eksisterande migrasjons- og media-worker-image
skal ikkje endrast utan at diffen krev det. Bruk scope-basert releasegate og eitt
bygg per release. Unngå parallelle, identiske releasebygg.

## U1 Toppfelt og kanalidentitet

- Fjern manuell forkorting av kanalnamn. Vis heile kretsnamnet i ei overlinje og
  kanalnamnet under, med kontrollert linjebryting.
- Behald logo/samtalar, innboks og innstillingar som direkte globale handlingar.
  Samle sekundære kanalhandlingar i den eksisterande samtaleinformasjonen på
  mobil, med tydeleg inngang og uendra kommandoar. Vis trådindikator når relevant.
- Samle handlingane i kanalhovudet på stor skjerm; ikkje fordel dei over heile
  breidda. Behald «Gå til siste» utan å ta plass frå kanalnamnet.
- Test lange namn, ulest-tal, tilkoplingsstatus, Escape/klikk utanfor og fokusretur.

## U2 Meldingshovud og vertikal rytme

- Erstatt dei flate flex-elementa med eit identitetsområde og ei samla
  handlingsgruppe. Status og redigeringsinformasjon får sekundær typografi.
- Behald badge, nøyaktig tid/dato og alle meldingshandlingar tilgjengelege.
  Små informasjonsmerke skal ikkje kvar krevje ei eiga 44px kolonne.
- Unngå ekstra handlingsrad før meldingsteksten. Sikre plass for lange namn og
  status utan tilfeldige ikonbrot; verifiser ekte grove peikarar separat.
- Rett dobbel Markdown-luft ved å bruke normal whitespace på renderte blokker,
  med bevarte kodeblokker og lesbare avsnitt. Bruk 8px mellom meldingar.
- Behald 16px meldingstekst og skrivefelt. Demp berre fokusramma; ikkje reduser
  skriveareal eller endre tastatur-/vedleggsflyt.

## U3 Sidefelt og felles justering

- Minst 240px sidefelt når liste og samtale blir viste saman. Bruk 900px som
  samordna liste/detalj-grense; behald 780px innhaldsgrense for delt tråd.
- Same tekstkant og fast ikonkolonne i menyar; ingen flytting av funksjonar til
  nye administrative nivå. Langt kretsnamn kan bryte kontrollert i sidefeltet.
- Samordne venstrekant for kanalhovud, meldingar og composer, med avgrensa
  lesebreidd på store skjermar.

## U4 Pakking, akseptanse og Canary

- Oppdater kjelda i packages/sproyt-ui, pakk ny versjon, installer den i frontend
  og oppdater skillens medfølgjande pakke og komponentkontraktar.
- Køyr komponenttestar, typekontroll og relevante nettlesartestar. Verifiser
  320/390/430/820/900/901/1440px, lys/mørk, coarse pointer og forstørra tekst.
- Eigne akseptansar: «Reiseglede / # 26-10 Ølesund» er lesbart utan ekstraklikk;
  handlingane i ei melding held saman; namn/status og tekst har klar tilknyting;
  ingen horisontal side-overflyt; menyar kan lukkast og fokus blir bevart.
- Bevar reaksjonar, trådstart, ulest, historikk, offline/reconnect, utkast,
  vedlegg og sending. Bruk isolerte testmeldingar, ikkje post i levande kanalar.
- Review av diff og skjermbilete før release. CI byggjer immutable image;
  GitOps oppdaterer berre Canary. Sjekk revisjon/readiness og den verkelege
  visinga i Canary. Dokumenter kva som er testa og kva som står att på fysisk mobil.

## Kvifor desse tiltaka

Ved 390px vart kanalnamnet «# 2…und» og ved 320px nesten berre «…und». Koden
forkorta først til 14 teikn før CSS klipte vidare. Ei vanleg melding brukte 88px
før teksten byrja fordi metadata og handlingar kvar fekk ei 44px rad.
Ved 820px vart «Reiseglede» klipt til «Reis…» i sidefeltet. Skrivefeltet var
allereie omtrent 55px høgt og er ikkje hovudkjelda til plassproblemet.

Designreglane ligg i packages/sproyt-ui/docs/design.md og er spegla i skillen.
Dette dokumentet er arbeidsrekkefølgja; designreglane er dei varige kontraktane.

## Gjennomføring 28. oktober

- Før UI-implementeringa vart #314 og #315 merga og publiserte som
  `bb02cb1672d159f1fd6a07f398551ac6a6ae4ff2` gjennom
  [release 40724888225](https://github.com/hbjoroy/sproyt/actions/runs/40724888225)
  og [GitOps #285](https://github.com/hbjoroy/rocket-applications/pull/285).
  Produksjon (2/2) og Canary (1/1) vart stadfesta Healthy/Synced med rett digest,
  offentleg readiness og revisjon før UI-koden vart endra.
- U1–U3 er implementerte. Kanalval ligg direkte i den eksisterande
  samtaleinformasjonen på mobil; namnet er ikkje lenger manuelt forkorta.
  Reaksjonar har eigen komponentplass under innhaldet i React og Vue.
- U4: pakke og skill-bundle er oppdaterte til 0.1.3. Sol har kontrollert planen,
  komponentgrensene, diffen og skjermbilete. 21 pakketestar, 207 frontendtestar
  og 31 målretta Chromium-scenario har passert, inkludert 320–1440 px,
  begge tema, touch, forstørra tekst, badge, reaksjonar og trådoversikt.
- Canary-utrullinga skal bruke CI/CD med `bb02cb1` som sist verifiserte baseline.
  Produksjon skal halde fram med denne baselinen. Fysisk mobil med reelt
  skjermtastatur er ikkje stadfesta av nettlesaremuleringa.

- WebKit fann at native dialogar ikkje vart synlege under den lukka
  mobilmenyen. Desse dialogane blir no renderte i den synlege temarota,
  med uendra komponenteigar og tilgangskontroll. Sol reviderte rettinga.
  Alle dei 42 målretta WebKit-scenarioa er no dekte av grøne testar (37 i
  første runde; seks i den målretta retesten, inkludert dei fem som feila).
  CI inkluderer no også dei nye layoutkontraktane og badge-/menytestane i WebKit.
