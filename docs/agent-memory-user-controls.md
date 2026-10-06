# M2: innsyn og styring av eige agentminne

Vanlege kretsmedlemmer finn **Mitt agentminne** i kretsen sin `…`-meny,
eller via **Meny og innstillingar → Kretsadministrasjon og invitasjonskode**.
Vel agenten i dialogen. Denne flata viser berre minnet om innlogga brukar;
moderatorar og eigarar får ikkje velje andre sine profilar.

`GET /api/v1/me/circles/{circle_id}/chat-agents` gir ei avgrensa liste med
agent-ID og visingsnamn. Kretsmedlemskap og menneskeleg identitet er påkravd.
Konfigurasjon, prompt og andre sine minne blir ikkje eksponerte. Alle svar
har `Cache-Control: no-store`.

## Kontrollane

- **Tillat minne om meg** lagrar brukaren sitt val. Læring er framleis av
  i M2, sjølv om valet er på. Dialogen seier dette uttrykkeleg.
- **Rett** opnar eit tekstutkast med grense på 1024 UTF-8-byte. Kjeldeband
  og kanalgrense blir bevarte; brukaren vel ikkje kategori eller deltakarar.
- **Stadfest** markerer eit eksisterande notat som stadfesta av brukaren.
- **Gløym** krev stadfesting og fjernar også eigne notat med felles kjelder.
- **Nullstill** krev stadfesting, fjernar også utilgjengelege notat og
  flyttar grensa for framtidig innsamling. Minnevalet blir bevart.
- Notat viser kanal, kategori, dato, stadfesting og eventuelt utløp.
  Kjeldelenkjer bruker den eksisterande meldingsnavigasjonen. Utilgjengelege
  eller utgåtte notat blir berre talde, utan innhald eller kjelde-ID.

Gløyming slettar ikkje originalmeldingar eller allereie publiserte svar.
Brukaren sin ordinære kontoeksport inneheld det same autoriserte minneutvalet.
Kretsmoderator kan lagre det separate valet **Tillat minne for agenten** i
agentoppsettet. Dette aktiverer ikkje dei tre serverflaggane.

## Feil, samtidige endringar og privatliv

Alle endringar bruker profilrevisjonen frå siste henting. Konflikt blokkerer
nye endringar til brukaren hentar minnet på nytt, og rettingsutkastet blir
bevart dersom notatet framleis er synleg. Eit sletta eller utilgjengeleg
notat kan ikkje få eit gamalt utkast lagra på seg.

Dialogen blir eigd av innlogga identitet og krets, og notatflata av agenten.
Kontoskifte, lukking og agentbyte forkastar seine svar. Lesingar kan avbrytast;
ein avbroten eller lukka dialog garanterer ikkje at ein alt sendt mutasjon
vart avbroten. Ingen minnedata blir lagra i localStorage eller delt cache.
Tilgangsavslag ved mutasjon fjernar også synleg innhald og utkast.

Mobilflata bruker Sprøyt-dialog, temafargar, flytande handlingar og minimum
44px-knappar. Escape og lukking gir tilbake fokus. Chromium og iPhone WebKit
testar ordinært medlemskap, konflikt, UTF-8-grense, kjelder, gløyming,
nullstilling, tema og seine svar ved agent- og kontoskifte.

M3–M5 må framleis implementere innsamling, bygging og trygg minnebruk før
piloten i M6. M2 publiserer ikkje image og aktiverer ingen worker.
