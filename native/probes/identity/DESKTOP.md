# N01b: Windows-retur og varig innloggingsforsøk

Dato: 25. oktober 2026. Utviding av [N01a](README.md), revidert med Astra.
Dette er eit isolert laboratorium med fiktive kontoar og legitimasjon som ikkje
kan brukast mot Sprøyt. Den vanlege Windows-chatprøva er ikkje kopla til det.

## Ansvar og tryggleiksgrense

`DesktopHandoffClient` eig ein loopback-lyttar før nettlesaren blir opna.
Autorisasjonsadressa blir rekonstruert frå fast, kontrollert loopback-opphav og
transaksjon, ikkje opna direkte frå mottekne eller lagra URL-ar. Returen krev
rett metode, vert/port, sti og nøyaktig to avgrensa parameter. App-prov og
provider-PKCE er framleis separate.

`WindowsProtectedJournal` bruker DPAPI CurrentUser med UI_FORBIDDEN, utan
klartekstfallback. Éin prosess eig `owner.lock`; atomisk erstatting av `state.bin`
skjer frå ei kryptert mellomfil som er flusha først. Post og ciphertext er
avgrensa til 32 KiB. Dette er vern under Windows-brukaren, ikkje isolasjon frå
andre prosessar under same brukar. Det er ikkje ein lovnad om vern ved straumbrot.

App-prov blir lagra før nettlesaropning; returkode og stabil innløysings-ID før
exchange. Fullføring blir meldt først etter varig lagring av resultatet.
Posten er bunden til miljø, brokerinstans, transaksjon og forventa konto.
Oppstart/resume og callbackar deler same tilstandslås gjennom nettverkskall og
journaloppdatering, slik at eitt forsøk ikkje overskriv eit anna.

Falsk kode kan avvisast utan å øydeleggje legitim retur. Etter usikker levering
blir same kode, prov og innløysings-ID brukte. Ingen feil startar automatisk ein
ny familie eller fell tilbake til ukryptert lagring.

## Køyre prøva

```powershell
./native/probes/run-identity-probe.ps1 -ArtifactsDirectory S:/Build/sproyt-native-identity
```

Windows køyrer N01a og N01b; Linux køyrer N01a og byggjer .NET-koden, men blir
ikkje rapportert som DPAPI-aksept. CI lagrar berre kvitteringa, ikkje journalar,
databasar, callback-URL-ar, prov eller legitimasjon.

Legg til `-SystemBrowser` for manuell prøve i Windows sin standardnettlesar.
Vel Alice i den første fana og Bob i den andre. Berre denne interaktive
brokerstarten bruker ti minutt for innlogging og nettlesarcookiear. Den vanlege
automatiske prøva held to minutt; appen ventar maksimalt åtte minutt manuelt.
Kvar køyring får eit nytt lager. Utløpte faner skal ikkje brukast på nytt.

Kontoveljaren krev eksakt Origin og ein HttpOnly/SameSite-cookie. Sida bruker
`Referrer-Policy: origin`: `no-referrer` ville gjort Origin til `null` ved
HTML-POST. CSP tillèt berre eige opphav og det registrerte loopback-returopphavet
for formnavigasjon; Chromium kontrollerer også redirect-kjeda. Ingen query eller
kode blir delt som Referer. Andre svar bruker `no-referrer` og alle bruker
`Cache-Control: no-store`.

## Bevis og attståande portar

Automatisk Windows-prøve drep app-prosessen etter varig retur, etter
server-commit med tapt ACK, etter motteke resultat før lokal commit og etter
lokal commit. Ny prosess må få same resultat og nøyaktig éin familie. Ho testar
òg B-retur medan A blir gjenopptatt, falsk/dobbel retur, delt nettlesarcookie-lager,
eksklusiv eigar, korrupt/utilgjengeleg/utgått lager og feil miljø/brokerinstans.

Ein separat faktisk Edge-rundtur på Windows ARM64 fullførte Alice og Bob i to
samtidige forsøk. Dette beviser ekstern nettlesar, lokal retur og OS-verna
fullføring med syntetiske identitetar. HTTP-simulatoren åleine blir ikkje rekna
som dette beviset.

Broker lever gjennom appkrasja. Vi føreset ikkje at loopback startar ein død
app, at gammal port alltid kan bindast på nytt eller at ein retur som aldri
vart journalført, kan hentast att. Før varig retur kan trygg ny innlogging vere
naudsynt. Brokerkrasj, Authentik, passnøkkel, providerrotasjon, recovery og
Android/iOS-retur står framleis att. N01b frigir ikkje N03-porten eller aktiverer
noko i produksjon.

Referansar: [native OAuth](https://www.rfc-editor.org/rfc/rfc8252),
[DPAPI](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata),
[Origin/Referrer-policy](https://fetch.spec.whatwg.org/#append-a-request-origin-header),
[CSP form-action](https://www.w3.org/TR/CSP/#directive-form-action).
