# N01f2: eigarskap og integrasjonsportar for recovery

Dato: 25. oktober 2026. Status: arkitekturretning vald; recovery er framleis
**ikkje godkjend for aktivering**. Byggjer på
[identitets-ADR-en](native-client-identity-adr.md) og
[N01f1 sine feilprøver](../native/probes/authentik/RECOVERY.md).

## Avgjerd

Sprøyt skal eige den varige recovery-transaksjonen, e-postbeviset og sperra
mot gamle appøkter. Authentik skal framleis eige WebAuthn-registrering,
verifisering og den kanoniske identiteten. Native klientar bruker
systemnettlesaren og får ikkje eit privat FlowExecutor-API.

Dette er ei retning, ikkje ein påstand om at integrasjonskontrakten finst.
Vi implementerer ikkje ein full recovery-koordinator før desse to portane
har ei dokumentert og prøvd løysing:

1. Ei feillukka eingongsoverlevering til registrering for eksakt eksisterande
   brukar, utan den tapte autentikatoren. Overlappande nettlesarforsøk og
   feil i policykøyringa skal ikkje opne fleire opptak.
2. Eit autentisert, varig fullføringsbevis som bind recovery-ID, same brukar,
   den faktisk nye WebAuthn-nøkkelen, ei fersk innlogging med denne nøkkelen
   og det eksakte OIDC-grantet som Sprøyt tek imot.

Ingen innlogga produksjonsbrukarar eller eksisterande innloggingsflytar blir
endra av denne avgjerda.

## Kva offentlege grensesnitt gir oss

Den pinna Authentik-versjonen er 2026.8.2 med same image-digest som N01c–f1.
`POST /api/v3/core/users/{pk}/recovery/` er eit offentleg server-API som
utferdar ei lenkje til den konfigurerte recovery-flyten. Det er ikkje i seg
sjølv ei stadfesting av e-posteigarskap; ein framtidig koordinator må
verifisere e-postbeviset før han bruker denne kapabiliteten.

Prøva bruker ein ekte bearer-token for ein mellombels tenestebrukar.
`reset_user_password` avgrensa til Alice var ikkje nok: standard POST-
objektkontroll krev også `add_user` på det same brukarobjektet. Dette er eit
semantisk grovt løyvenamn, ikkje eit behov for global brukaroppretting.
Løyva skal avgrensast til den avtalte kontomengda; superbrukar er ikkje ein
akseptabel snarveg. Løyve til å administrere providergrants eller browserøkter
blir ikkje gjeve av denne prøva.

Det ordinære WebAuthn-device-API-et gir metadata som ID, namn, opprettingstid,
type, AAGUID og brukar. Det gir ikkje ein dokumentert recovery-ID eller ei
binding til det eksakte innloggingsgrantet. Ei endra deviceteljing eller eit
tidsstempel er ikkje fullføringsbevis. N01e si direkte ORM-kopling var
laboratorieevidens, ikkje ein etablert produksjonskontrakt.

Ein generell login-event/webhook kan transportere eit signal, men kan ikkje
åleine bevise denne kjeda. Eit brukarredigerbart profilflagg, klienten sitt
`recovered=true` eller e-postlikskap blir heller ikkje autoriteten.

## Målt API-prøve

[Proben](../native/probes/authentik/recovery-api-contract.py) køyrer mot dei
verkelege API-visningane og PostgreSQL i den isolerte localhost-laben.
Han bruker ein mellombels recovery-flow med innebygd WebAuthn-setup og login,
og tilbakefører Brand, flyt, rolle, brukartilgang og token etterpå.

| Scenario | Observert på den pinna versjonen |
| --- | --- |
| Alice-avgrensa mint | 200 med dei to objektløyva |
| Bob-mint og global brukaroppretting | Avviste; ingen brukar oppretta |
| To mint før bruk | Same tokennøkkel, ikkje to uavhengige eingongslenkjer |
| To samtidige restaureringar av same lenkje | **Begge** når WebAuthn-registrering |
| Seinare replay frå ny nettlesar | Nektar tilgang til registrering |
| Identitet og opprettingar | Same Alice-UUID; ingen nye brukarar eller devices |

Begge HTTP-køyringane hentar tokenobjektet før ein barrier ved den verkelege
planlesinga. Barrierinjeksjonen endrar berre tidspunktet. Planlesinga,
tokenslettinga og databaseoperasjonane blir ikkje erstatta. Prøva stoppar ved
registreringsutfordringa; ho opprettar ingen passnøkkel og prøver ikkje ei
full recovery. Resultatfelt og kvitteringar skil observasjon frå godkjenning.

Den observerte sekvensielle eingongsbruken er dermed ikkje atomisk ved
samtidige forsøk. Det offentlege lenkje-API-et åleine oppfyller ikkje den
avtalte opptaksporten. Dette er ikkje bevis for kontoovertaking eller ein
fullført andre passnøkkel; begge forsøka må ha den same gyldige kapabiliteten,
og prøva stoppar før registrering. Vi har heller ikkje godkjent ein ekstern
koordinator som berre legg eit eingongssteg framfor denne lenkja: den
eksponerte providerkapabiliteten har framleis det målte race-vindauget.

Port 2 er framleis open etter kjelde-/API-gjennomgangen. Vi har ikkje køyrt
ein ekte ny-key/fullføringsprøve som latar som den manglande kontrakten finst.

## Val som ikkje er tekne

Vi har ikkje funne ein dokumentert, stabil tredjeparts plugin-kontrakt for
eigne Authentik-steg i denne versjonen. At Python-kode teknisk kan lastast i
eit eige image, gjer han ikkje til ein leverandørstøtta plugin. Dokumentasjonen
omtalar eigne Python-innstillingar som ustøtta.

Vi innfører derfor ikkje ein vedlikehalden Authentik-fork eller ei eiga
Django-utviding som eit skjult implementasjonsval. Dersom dei offentlege
grensesnitta ikkje oppfyller dei to portane, er neste avgjerd anten ein
verifisert upstream-rettelse/kontrakt eller ei uttrykkeleg eigd, avgrensa
providerutviding. Eit eventuelt utvidingsval må ha ansvarleg eigar, pinna
versjon, oppgraderingsprøver og ein drifts-/tilbakeføringsplan. Vanlege
innloggingsflytar skal framleis kunne brukast medan ny recovery er avslått.

## Gjennomføring etter at integrasjonsportane er løyste

1. Lag ein varig recovery-post bunden til eksakt issuer/subject og eit
   administrator-/serverforvalta register over stadfesta e-postadresse.
   Bruk tilfeldig, kortliva e-postbevis med hash ved lagring, rategrenser,
   nøytrale svar og ny kontroll ved adresseendring/deaktivert brukar.
2. Ta atomisk eigarskap til eitt forsøk etter gyldig e-postbevis. Ein
   uautentisert førespurnad eller berre utsending av e-post skal ikkje kunne
   stengje kontoen. Sperr nye sensitive opptak medan godkjend recovery blir
   avklart; fullføringsbeviset må kunne hentast att etter tapt svar.
3. Ved godkjend fullføring: bruk recovery-ID som idempotensnøkkel i ein lokal
   transaksjon som aukar sikkerheitsepoken og sperrar gamle native familiar,
   gamle browsercookiear og pending handoffs. Bind den autoriserte nye økta
   tydeleg til fullføringa. Retry skal ikkje sperre den nye økta om att.
4. Gjer nødvendig providerrevokasjon som eit separat, varig og retrybart
   arbeid. Sprøyt og Authentik deler ikkje transaksjon. Delvis svikt held
   opptak sperra og krev avstemming; ingen stille «alt ferdig»-tilstand.
5. Prøv gammalt Authentik-SSO, ferdige callbacks, tapt svar, prosessdød og
   dobbel fullføring. Usende utkast og lokal cache blir bevarte for same konto.
   Først deretter kan N01/N03/N04 og ei eiga produksjonsaktivering godkjennast.

## Pinna kjelder

- [Offentleg recovery-API og tokenoppretting](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/core/api/users.py).
- [FlowToken-restaurering og forbruk](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/flows/views/executor.py).
- [Objekt-RBAC](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/rbac/permissions.py).
- [WebAuthn-device-API](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/authenticator_webauthn/api/devices.py).
- [Offisiell status for eigne Python-innstillingar](https://docs.goauthentik.io/install-config/configuration/#custom-python-settings).
