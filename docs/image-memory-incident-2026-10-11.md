# Biletebehandling og minnemangel 2026-10-11

## Stadfesta funn

Prod-poden `sproyt-sproyt-59949b6bcf-qsgl6` vart OOMKilled
2026-10-11 kl. 15:10:44 UTC, på revisjon `a35a5a7`. Kjerneloggen viser
262144 KiB brukt mot ei grense på 262144 KiB. Grafana sitt omstartvarsel
stemmer med denne hendinga. Den andre prod-poden og canary vart ikkje
starta om. Begge prod-podane vart kontrollerte som friske etterpå.

Overvakinga viser eit brått minnehopp, ikkje jamn vekst fram mot hendinga.
Det var bileteaktivitet rundt tidspunktet, mellom anna ei registrert
3000 × 4000 JPEG-opplasting kl. 15:10:01 UTC. Dette peikar mot
biletebehandling, men loggane identifiserer ikkje den konkrete operasjonen
som utløyste minnetoppen. Eit normalt komprimert mobilbilete kan krevje
mange gonger filstorleiken under dekoding, orientering og skalering.

## Korrigert retning og utrullingsstatus

Brukaren avviste kravet om å redusere store bilete manuelt. Tiltaka nedanfor
beskriv PR #258, som vart merga, men ikkje publisert eller utrulla.
Berre minnegrensa på 512 MiB er i prod og canary; apprevisjonen er framleis
`a35a5a7`. Grensene i PR #258 er ikkje den godkjende endelege løysinga.

Ny retning er uendra originalar, blokkvis opplasting/nedlasting og ei varig
kø med separat worker for tung behandling. Sjå
[implementasjonsplanen](media-upload-queue-plan.md), laga etter Astra-review.
Den avviste opplastingsgrensa må erstattast før neste kodeutrulling.

## Første tiltak, avvist som endeleg løysing

- Éin stor opplasting, referansekonvertering eller godkjenning av eit
  generert bilete per prosess. HTTP-admission skjer før store kroppar eller
  originalar blir lesne. Når opptatt: 503 med Retry-After og forklaring;
  ingen ubunden kø av komprimerte originalar.
- Éin tung dekodings-/skaleringsjobb per prosess, delt av opplasting,
  biletereferansar, genererte bilete og agentens syn. Budsjettet blir halde
  av blocking-worker heilt til han er ferdig, også om HTTP-klienten forsvinn.
- Opplasting og referansekonvertering kontrollerer dimensjonar før
  pikseldekoding: høgst 24 megapikslar, 8192 pikslar per side og 96 MiB
  dekodarallokering som best-effort-grense i image-biblioteket. JPEG/WebP
  sine interne arbeidsbuffer er ikkje fullt dekte av denne grensa.
  Agentens syn beheld sine strengare grenser.
- App-containeren får 512 MiB som minnegrense. Dekodargrensa er ikkje ei
  grense for total RSS: filbuffer, rotasjon, mellomresultat, SQL og andre
  førespurnader krev òg plass. Headroom og serialisering må brukast saman.

Originalar som er større enn grensene må reduserast før opplasting eller
bruk som referanse. Eksisterande meldingar og bilete blir ikkje endra.
Databaseskjema og migrasjonsimage treng inga endring. Vidare oppfølging
ved ny OOM: kontroller dimensjonar, operasjon og samtidige nedlastingar;
ikkje anta at denne endringa beviser den opphavlege årsaka.

## Verifikasjon

Testane skal dekkje avvising av eit lite JPEG med eit falskt 48 MP-header,
uendra pikselinnhald for eit gyldig bilete, og at kansellering ikkje frigjer
minnebudsjettet medan blocking-worker framleis arbeider. Eksisterande
orienterings-, preview- og biletegenereringstestar skal passere i CI.
Utrulling går gjennom immutable image og GitOps; kontroller revisjon,
512 MiB i podspesifikasjonen, readiness og omstartteljar i begge miljø.
