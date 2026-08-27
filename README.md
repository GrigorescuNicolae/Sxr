# sxr

O reimplementare fidelă a **editorului clasic de imagini din ShareX**, pentru Linux,
scrisă în Rust.

Nu încearcă să fie ShareX întreg. Acoperă fluxul folosit zilnic: apeși scurtătura,
alegi o regiune de pe ecran, se deschide editorul, imaginea e deja în clipboard.

## Ce face

- **Captură de regiune**, cu copiere automată în clipboard imediat după selecție —
  dacă închizi fără să editezi, captura e deja acolo.
- **Editorul clasic**, cu bara de unelte în ordinea din ShareX și aceleași iconițe:
  dreptunghi, elipsă, mână liberă, mână liberă cu vârf de săgeată, linie, săgeată,
  text cu contur, text cu fundal, balon de dialog, numărător de pași, lupă,
  imagine din fișier, imagine din ecran, sticker, cursor, gumă inteligentă,
  blur, pixelare, evidențiere, reflector, decupare, tăiere.
- **Fereastră de introducere a textului** ca în ShareX: familie de font din sistem,
  mărime, culoare, culoare secundară, bold / italic / subliniat, aliniere pe ambele axe.
  `Enter` = OK, `Ctrl+Enter` = rând nou, `Esc` = renunță.
- **Linie și săgeată curbabile**: forma primește `2 + N` noduri; nodul din mijloc
  o îndoaie pe o curbă cardinală, exact ca `LineDrawingShape`.
- **Meniul Imagine**: dimensiune imagine, dimensiune pânză, decupare, decupare
  automată, rotire la stânga și la dreapta.
- Anulare / refacere, ordonarea straturilor, umbră, duplicare.

Nu include: încărcare pe servicii, OCR, fluxuri de lucru, istoric, tipărire.

## Compilare

```sh
cargo build --release
install -m755 target/release/sxr ~/.local/bin/sxr
```

Cerințe: Rust (ediția 2024) și, pentru captura de regiune, un mediu Wayland cu
portalul de captură de ecran. Lista de fonturi din fereastra de text vine din
`fc-list` (fontconfig); dacă lipsește, se folosește DejaVu Sans.

## Utilizare

```sh
sxr              # selectează o regiune de pe ecran, apoi deschide editorul
sxr <fișier>     # deschide direct o imagine existentă
```

Practic se leagă de o scurtătură globală (de exemplu `Ctrl+Print`).

Stickerele se citesc din `~/.local/share/sxr/stickers/` — pui acolo fișiere PNG
și apar în unealta de stickere.

## Relația cu ShareX

Proiectul e inspirat din [ShareX](https://github.com/ShareX/ShareX) și urmărește
îndeaproape comportamentul editorului clasic, dar e scris de la zero în Rust.
**Niciun rând de cod nu e copiat din ShareX.** Comportamentul, ordinea uneltelor
și valorile implicite au fost reproduse pe baza observării aplicației și a
documentației publice.

Nu e afiliat cu proiectul ShareX și nu e susținut de acesta. ShareX este
© ShareX Team, licențiat GPL-3.0; acea licență nu se aplică aici.

## Licență

Codul e sub licența [MIT](LICENSE).

Iconițele, fonturile și stickerele au licențele lor — vezi
[ATTRIBUTION.md](ATTRIBUTION.md).
