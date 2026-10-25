# N01d: kva tilbakekalling faktisk stoppar

Dato: 25. oktober 2026. Avgrensa framhald av [N01c](README.md), etter Astra-review.
Same pinna Authentik 2026.8.2 og PostgreSQL, syntetiske kontoar og lokal issuer.
Ingen produksjonskonfigurasjon blir endra. Passnøkkel og recovery er eigne steg.

## Reproduserbar prøve

```powershell
./native/probes/authentik/start-lab.ps1 -ArtifactsDirectory S:/Build/sproyt-authentik
./native/probes/authentik/run-lab.ps1 -RunDirectory <ny-labmappe> -Revocation
./native/probes/authentik/stop-lab.ps1 -RunDirectory <same-mappe>
```

På Linux krev prøva det uttrykkelege valet `-HttpOnly -Revocation`. CI køyrer
først N01c, så N01d mot den same isolerte tenaren. Byggcache blir gjenbrukt.
Kvar køyring har eiga kvittering; berre `evidence/receipt.json` blir lasta opp.

Kvar av dei tre delprøvene lagar to uavhengige Authentik-browserøkter og grants
for Alice, og éi for Bob. Alle har fungerande OIDC, refresh, access-/refresh-
introspection og ny autorisasjon med eksisterande cookie før tiltaket. Nye økter
og grants blir laga før neste tiltak. Ingen øydelagd tokenfamilie blir gjenbrukt
som kontrollgrunnlag.

SSO-kontrollen utferdar nye grants. Alle baseline-SSO-kontrollar blir derfor
køyrde **før** siste refresh/introspection-baseline. Etter tiltaket blir alle
tokenutfall målte **før** nye SSO-kontrollar. Kontrollane bruker opphavleg
cookie for kvar økt og kontrollerer den forventa kanoniske brukar-ID-en ved
fullført OIDC/appbyte. Ei rendera innloggingsflyt betyr at ny interaksjon trengst;
det blir ikkje kalla tokenrevokasjon.

## Målte utfall med denne policyen

| Tiltak for Alice si første økt | Gammalt access-token aktivt | Refresh-token aktivt og godteke | Nytt access-token etter refresh | Opphavleg cookie gir ny OIDC utan interaksjon | Alice si andre økt / Bob |
| --- | --- | --- | --- | --- | --- |
| Tilbakekall refresh-token | Ja | Nei | Ikkje utferda | Ja | Uendra / uendra |
| Slett første Authentik-browserøkt via admin-API | Nei | **Ja** | **Aktivt** | Nei | Uendra / uendra |
| Deaktiver Alice via admin-API | Nei | Nei | Ikkje utferda | Nei | Sperra / uendra |

Desse utfalla er faste kontraktassertar for det pinna imaget, ikkje berre ei
bestått målerutine. Kvitteringa inneheld tidspunkt, tid til tokenmåling og tid
til SSO-måling for kvar rad. Målingane er sekvensielle, utan polling; dei beviser
ikkje ei øvre grense for forplantingstid. Endra provider-versjon eller policy
krev ny vurdering av kontrakten.

Introspection blir kontrollert før refresh. Dersom refresh blir godteke, blir
også det nye access-tokenet kontrollert. Dermed blir det synleg at sletting av
browserøkta ikkje stoppar dette grantet frå å skaffe ny tilgang. Eit HTTP 200 frå
revokasjonsendepunktet er berre eit godteke kall; det er etterfølgjande
introspection og avvist refresh som beviser verknaden på det konkrete tokenet.

## Testgrense og vern

`revocation-fixture.py` køyrer berre i den eigarmerkte lab-workeren og kontrollerer
labmarkør, applikasjon, klienthemmelegheit og dei eksakte syntetiske brukarane.
Han lagar førehandsautentiserte Django-prøveøkter med Authentik si signerte
cookieinnpakking. Administrative tiltak går gjennom dei offentlege API-visningane
for brukar-PATCH og authenticated-session-DELETE med ein førehandsautentisert
testadministrator. Vanlege signal/visningsreglar blir køyrde; admininnlogging,
CSRF og administrasjonsgrensesnittet er ikkje testa av denne fixturen.
Alice blir reaktivert i `finally`; gamle grants blir ikkje gjenoppretta.

Øktsletting i denne prøva er **administrativ tilbakekalling**, ikkje ein prøve av
RP-initiated logout. Sistnemnde og endring/fjerning av passnøkkel står att saman
med faktisk passnøkkelinnlogging og e-postrecovery. Ingen privat Authentik-
flow-API er lagd inn i appen.

Brokerkontrollane bruker faste same-origin-endepunkt, konfidensiell klientauth,
ingen proxy/redirect, ti sekund timeout og svargrense 16 KiB. Token kjem frå
transaksjon + brukar i broker-minnet, aldri frå kontrollkallet. Introspection
returnerer berre boolsk `active`; verken claims, token eller cookiear kjem i
kvitteringa. Nettfeil gir ikkje eit gyldig revokasjonsresultat.

Windows-prøva stadfestar at den verna handoff-prøveposten held same resultat etter
tiltaket. Dette er ikkje integrert aksept for samtalecache og utkast. HTTP-prøva
har `nativeWindowsHandoff=false` og `localStatePreserved=null`; ho bruker ingen
DPAPI-fallback. Testøkter og administrative stimuli er ikkje produksjons-API-ar
for ein native Sprøyt-klient.

## Følgje for N03

Browserøkt, providergrant og Sprøyt si komande einingsøkt er ulike levetider.
Administrativ sletting av ei Authentik-browserøkt kan ikkje åleine vere Sprøyt
sin regel for å sperre appfornying. Introspection av eit access-token beviser
heller ikkje at ein ressursserver med berre lokal JWT-validering avviser det
straks. Ein eigen tilbakekallbar Sprøyt-einingsøkt og den avtalte providerkoplinga
er framleis nødvendige.

N03 må definere kva «logg ut denne eininga» og global tilbakekalling skal stoppe,
korleis provider- og Sprøyt-sperring blir samordna, og kva ei eksisterande gyldig
access-periode tillèt. Eit aktivt provider-refresh-token er ikkje åleine prov på
at ei tidlegare browserøkt framleis er aktiv. Støtta providerlogout/recovery-
signal og passnøkkelåtferd står framleis som opne akseptpunkt. N01d opnar ikkje
produksjonsporten for N03.

Kontrollert mot kjelda for den pinna versjonen:

- [Introspection](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/providers/oauth2/views/introspection.py)
- [Tokenrevokasjon](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/providers/oauth2/views/token_revoke.py)
- [Administrativ øktsletting](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/core/api/authenticated_sessions.py)
