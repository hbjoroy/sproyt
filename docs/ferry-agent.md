# Fergeoppslag for kretsagentar

Ein kretsagent kan bruke dagens planlagde anløp i Paros (Parikia). Dette er
rutetider frå den eksisterande Ferry-Schedule-tenesta og GTP, ikkje AIS,
stadfesta ankomst, forseinkingar eller kanselleringar. Modellen får normaliserte
data og skal seie tydeleg kva som er planlagt. Han får ikkje nettverkstilgang.

## Konfigurasjon og tilgang

Det eksisterande eigar-/moderator-API-et for kretsagentar tek `ferry_port: "paros"`
eller `null`. Utelate felt på PATCH bevarer oppsettet. Andre hamner blir avviste.
Konfigendring aukar revisjonen og ugyldiggjer ventande jobbar. Privatkanalval,
medlemskap, tilbakekalling og lease-fencing gjeld som for andre agentsvar.
`@Maria` brukar dei eksisterande reglane for direkte adressering.
Valet «Fergeruter for Paros» ligg i redigeringa av ein kretsagent.

`SPROYT_FERRY_URL` er ein serverstyrt HTTP/HTTPS-base utan brukarinformasjon,
query eller fragment. Adapteren gjer berre GET til den faste Paros-ruta, med
dagens dato i Europe/Athens. Redirect blir nekta; respons og tidsbruk er avgrensa.
`SPROYT_FERRY_AGENTS_ENABLED` er ein konfigurasjonsgate, ikkje ein snarveg rundt
lagra krav i workerar. API-et rapporterer `ferry_available`.

## Datagrunnlag og levetid

Kjelda sitt `fetched_at` blir bevart. Feil hamn/dato, framtidige eller for gamle
data, ugyldige tider og for store svar blir avviste. Manglande tider og hamner
blir ikkje fylte inn. Dato og klokkeslett bruker Europe/Athens, inkludert sommartid.
Snapshot og svar blir lagra atomisk på den leasa jobben. Snapshot er gyldig i
høgst fem minutt og aldri over dato- eller kjeldealdergrensa. Retry og publisering
kontrollerer fristen igjen. Feil i kjelda gir ikkje oppdikta fergefakta.

## Utrulling

0054 legg til nullable `ferry_port`, `ferry_snapshot` og `ferry_valid_until`.
Eksisterande agentar held fram utan fergeoppslag. Rull ut kompatible workerar i
både prod og canary med gaten av, sidan dei deler database. Bruk ordinær CI/CD,
fersk backup og full restore-kontroll. Verifiser at gamle workerar er borte før
gaten blir slått på og Maria får `ferry_port: "paros"`.

Helm har `config.ferryUrl` og `config.ferryAgentsEnabled`, med snever egress til
Ferry-Schedule-podar på TCP8080 når URL er sett. Ikkje endre eller rull ut
fergetenesta som del av denne integrasjonen. Ved tilbakeføring må fergeagentar
deaktiverast og jobbar drenerast før ein går tilbake til workerar utan 0054-støtte;
ein AV-gate åleine stoppar ikkje lagra agentkonfigurasjon.

## Neste steg

Ei kjelde med dokumentert ferske AIS-posisjonar for Paros må verifiserast før vi
kan svare på kva ferge som faktisk kom inn. Rutetider kan berre peike på kva som
var planlagt på det tidspunktet. AIS-posisjon åleine stadfestar heller ikkje anløp.
