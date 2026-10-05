# Del til Sprøyt: første etappe

MVP-et er laga for Sprøyt installert gjennom Chrome på Android (WebAPK).
Brukaren kan dele tekst/lenkje og eitt JPEG-, PNG-, GIF-, WebP-, HEIC- eller
AVIF-bilete på høgst 35 MiB. ChromeOS og Edge på Windows har dokumentert
plattformstøtte, men er ikkje fysisk aksepterte her. Safari på iPhone/iPad/Mac
registrerer ikkje dette PWA-delingsmålet. Reservevegen er å opne Sprøyt og lime
inn tekst/lenkje eller velje biletet i vedleggsknappen.

Mottaket lagrar tekst og originalfila privat i nettlesaren før det stadfestar
at delinga er teken imot. Delinga går ikkje i URL, logg eller automatisk til
serveren. Ved innlogging kan ei anonym deling takast uttrykkeleg i bruk med
den aktuelle kontoen. Eigarbundne delingar blir ikkje viste til andre kontoar.
Utlogging ryddar delingsinnboksen og aukar ein lokal auth-generation i same
transaksjon, slik at eit eldre pågåande mottak ikkje kan gjenopprette innhaldet.
Ein feil i identitetskontrollen blir aldri tolka som anonym innlogging.

«Motteke deling» opnar ei kompakt kontrollflate. Brukaren vel ein krets/kanal
med skriverett, kan redigere teksten, og trykkjer «Send delinga». Vanlege
samtaleutkast blir tekne vare på. Serveren kontrollerer skriverett og den
faktiske fila ved opplasting/sending. Feil bevarer delinga og viser ein lokal
retry. Etter at sendinga er journalført er request-ID og innhald låste;
retry og gjenoppretting nyttar same ID og payload. Ingen posting skjer før
brukaren har valt Send. Kvitteringslaus sending kan gjenopptakast etterpå.

Innboksen har høgst fem usende delingar og 70 MiB bilete totalt. Ubrukte
delingar går ut etter 48 timar; uttrykkeleg journalførte sendingar blir
bevarte i opptil sju dagar som meldingsjournalen. Identisk framleis ubehandla
OS-levering blir samla i éi deling. Etter stadfesta sending blir innhaldet
rydda; ei kort dedup-kvittering hindrar umiddelbar dobbel levering. Dette er
ikkje permanent innhaldsdedup: ei ny tilsikta deling seinare er mogleg.

Automatiserte prøver brukar ekte service-worker multipartPOST og IndexedDB,
innloggingsavbrot, original File-payload, rettigheitsfeil, uendra samtaleutkast,
to faner, same request-ID etter reload og logout-race. WebKit-prøva kontrollerer
at manglande File-lagring aldri gir falsk mottakskvittering; ho beviser ikkje
OS-mottak på iPhone.

Fysisk Android-aksept står att: installer appen med gjeldande manifest,
del tekst/lenkje frå ei anna app og eitt bilete frå biletsamlinga, prøv
utgått innlogging, vel kanal og Send, og kontroller éi melding med rett bilete.
Prøv òg å avbryte og dele same innhald to gonger. Før denne prøva er utført
skal dokumentasjonen ikkje hevde at registrering i OS-delingsarket er verifisert.

Primærkjelder: [Chrome sitt PWA-kurs](https://web.dev/learn/pwa/os-integration),
[Microsoft Edge](https://learn.microsoft.com/en-us/microsoft-edge/progressive-web-apps/how-to/share),
[WebKit 194593](https://bugs.webkit.org/show_bug.cgi?id=194593).
