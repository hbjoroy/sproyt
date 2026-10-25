# Windows Canary: ekte innlogging og tekstchat

Dette snittet koplar den native WinUI-appen til same konto, kanalar og
meldingsprotokoll som nettappen. Den gamle lokale demoen og N03a-laben er
eigne startmodusar. Android skal bruke den same serverkontrakten.

## Innlogging og økter

`SPROYT_NATIVE_ENABLED=true` / Helm `config.nativeClientEnabled` aktiverer
native-tenesta berre for den eksisterande HTTPS-Canary-callbacken. Standard
er avslått. Same OIDC-klient og eksakte issuer/subject blir brukte; Authentik
treng ikkje ein ny offentleg klient. Native får ein **eigen providergrant**.
Han deler ikkje nettlesaren sitt refresh-token eller set browserauth-cookiear.

Appen lagrar S256-prov før systemnettlesaren blir opna. Backend eig OIDC,
state, PKCE og den dedikerte granten. Etter innlogging stadfestar brukaren
kontoen på ei cookie-/Origin-/nonce-verna side. Returen går berre til den
registrerte IPv4-loopbackporten, med kortliva kode og transaksjon. Appen lagrar
retur og innløysings-ID før exchange og legg legitimasjonen i DPAPI CurrentUser.
Ein lukka app kan binde same port og vente på den opphavlege browserfana;
utgått eller ufullførbar innlogging blir kansellert før eit nytt forsøk.

PostgreSQL eig native forsøk, familiar, epoch og retry. Provider-/retrymateriale
er kryptert med formåls- og instancebinding og støttar eksisterande tidlegare
sessionnøklar. Native access varer høgst fem minutt og ikkje lenger enn den
underliggjande providerøkta. Native refresh fornyar den dedikerte providergranten.
Klienten toler 30 sekund klokkeskeivheit i formkontrollen av grantens utløp.
Dette forlenger ikkje serverens accesslevetid; serveren handhevar utløpet ved
HTTP- og WebSocket-opptak. Ein refresh frå ei klokke to sekund framfor klienten
og avvising av urimeleg framtidig utløp er dekte av klientkontrakten.
Uvisst upstreamutfall sperrar familien til ny innlogging; gammalt provider-
refresh-token blir ikkje prøvd på nytt. Lokal refresh-retry har same operasjons-ID
og får same krypterte resultat innan det avgrensa vindauget.

Lokal revoke/epoch blir kontrollert ved kvart HTTP-opptak, ved WS-kommandoar,
før nye utgåande WS-meldingar og periodisk på samband. Browserautentiseringa
er uendra. Native bearer og browsercookie blir ikkje blanda. Providerendringar
er avgrensa av access-levetida; dette snittet lovar ikkje umiddelbar tilbakekalling
etter vilkårleg endring i Authentik. Ny e-postrecovery er framleis utilgjengeleg.

## Windows-flyten

Start appen med `--server https://sproyt-canary.bjoroy.me/`. Metadata og
legitimasjon ligg under Windows-brukaren sin separate `Sproyt/WindowsCanary`-
profil. Kvar verifisert konto får si eiga lokale Rust-/SQLite-lagring.
Ingen fiktive kanalar eller meldingar blir lagde inn i denne modusen.

Kanalar, kretstitlar, siste historikk og nye tekstmeldingar kjem frå den
eksisterande `sproyt.chat.v1`-protokollen over autentisert WebSocket. Klienten
koplar til att og lastar historikk etter brot. Visingsprojeksjonen blir tømd
ved kontobytte. Utkast og sendekø blir verande i den opphavlege kontoen.
Transportopptak er bunde til konto og økt-/socketgenerasjon; ei gammal kømelding
kan ikkje bli send som kontoen som vart logga inn etterpå.

Køen blir lagra før sending og bruker same request-ID ved uvisst utfall.
Etter ny innlogging må brukaren stadfeste gamle kømeldingar før dei blir sende.
Vanleg tokenrotasjon i same øktfamilie krev ikkje denne stadfestinga.
Berre eksplisitt stadfesta meldingar kan fjernast frå den lokale køen. Det er
plass til åtte samtidige ventande meldingar og åtte kanalutkast i dette første
lagringssnittet. Stadfesta sendingar og sletta utkast frigjer plassen. Tekst er
førebels avgrensa til 1024 UTF-8-byte; UI forklarer grensa og bevarer teksten.
Rikt innhald, vedlegg, reaksjonar, full offlinesync og Android kjem etter
denne avgrensa tekstpiloten.

## CI og utrulling

- Native serviceprøver dekkjer kryptering, OIDC/account-confirm, CSRF-headerar,
  exchange/cancel-kappløp, epoch, lease og usikker providerfornying.
- CI køyrer PostgreSQL-samtidigheit eksplisitt, med ekte SQL-transaksjonar.
- Windows-klientkontraktar dekkjer scope, callback, DPAPI og transportopptak
  ved kontobytte. WinUI-fixturen prøver den nye visinga og utkast/sending i
  smalt vindauge; ho blir ikkje rapportert som ekte autentisering/transport.
- Faktisk Edge-forminnsending og Windows mot Canary er eit eige akseptpunkt.

Bruk vanleg PR → CI → testa immutabelt image → GitOps. Canary må oppgradere
**både app- og migrasjonsimage** til schema 0068 før flagget blir aktivert.
Den additive tabellutvidinga krev ikkje omforming eller sletting av meldingar.
Produksjonsflagget held fram som `false`. Ved tilbakeføring: slå native av;
bevar tabellane og dei kontoavgrensa lokale utkastfilene.
