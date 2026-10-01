# Saksregistrering frå melding (steg 2)

Denne leveransen byggjer vidare på [work-item-policyen](work-item-policy.md).
Ingen kanal er aktivert automatisk. Kanalen treng ei aktiv binding med
`process_key=work-item`, `namespace=sproyt`,
`definition_name=work-item-review`, `definition_version=1.0.0`, ei tillaten
applikasjon, ein aktiv `review`-rute med rolla `product-handler`, og minst éin
behandlar som har rolla, `can_review` og medlemskap i oppgåvekanalen. Om
fleire oppfyller vilkåra, vel første brukar-ID deterministisk; dette er ei
MVP-tildeling og ikkje ei felleskø.

Meldingsmenyen viser «Lag Issue» berre når serveren finn ei kvalifisert
applikasjon. Utkastet les den aktuelle meldinga, sender berre den avgrensa
kjeldeteksten til Santorini vLLM for tittelforslag dersom modellen er
konfigurert, og lèt brukaren rette tittel og beskriving. Modellen får ingen
verktøy eller kanalhistorikk. Ved modellfeil kjem eit redigerbart tekstutdrag.

`POST /api/v1/channels/{id}/work-items` gjer éin transaksjon som
kontrollerer medlemsskap, policy, applikasjon, behandlar/rute, ikkje-sletta
kjeldemelding og uendra meldingsinnhald. Transaksjonen lagrar saka, pinna
policyrevisjon, kjeldesnapshot og ei ventande Heart-startkvittering. Same
brukar/request-ID/innhald får same sak attende; endra innhald er konflikt.
Tilgang blir kontrollert også ved retry. Ingen GitHub-eksport skjer.

Bakgrunnsarbeidaren registrerer den faste v2-definisjonen
[`work-item-review.yaml`](../helm/sproyt/definitions/work-item-review.yaml)
og startar Heart med sak-ID som idempotensnøkkel. Heart-feil etter akseptert
registrering lèt saka bli ståande `pending` for ny prøve. `status` på saka
og `start_status` for Heart er separate felt. Arbeidaren blir berre starta
når `SPROYT_HEART_URL` er konfigurert. Ingen eksisterande Heart-prosessar
eller pilotoppgåver blir flytta.

Neste etappe er Heart-oppgåveprojeksjon, oppgåvekanalvising, kategori,
prioritet og autorisert behandlingsavgjerd. Ei akseptert sak i dette steget
kan difor ha ein starta Heart-instans utan ei synleg behandlaroppgåve enno.
Funksjonen bør ikkje aktiverast for vanlege kanalar før neste etappe er
verifisert i canary.
