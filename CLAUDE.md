# Husk Webcam – agent-kontekst

Læs efter opgave: [produkt og brug](README.md), [kodekort og designvalg](ARKITEKTUR.md),
[byg og test](docs/byg-fra-kilde.md), [status og næste arbejde](FORTSÆT-HER.md).

## Offentligt repo

En offentliggjort commit kan ikke kaldes tilbage. Commit aldrig hemmeligheder (tokens,
nøgler, adgangskoder), rigtige netværksadresser, maskinnavne, brugernavne, konto-id'er eller
interne henvisninger til planer, spor og andre projekter. Det gælder også tests og kommentarer.
Eksempel-IP'er: `192.0.2.0/24` eller `10.0.0.0/8`.

Før commit: gennemgå adressefund (også lovlige eksempler), og søg efter eget maskin- og
brugernavn. PowerShell:

```powershell
git grep -nE '([0-9]{1,3}\.){3}[0-9]{1,3}'
git grep -nFi "$env:COMPUTERNAME"
git grep -nFi "$env:USERNAME"
```

## Regler

- Versionen bor i `src/husk-webcam-rs/Cargo.toml`; `scripts/byg-installer.ps1` og
  `installer/husk-webcam.nsi` læser den derfra. ⚠️ **Der er ÉN kilde mere, og den er manuel:**
  `src/husk-webcam-rs/res/husk.rc` hardkoder `FILEVERSION`/`PRODUCTVERSION` og de to
  `"0.1.0.0"`-strenge, og `build.rs` kalder bare `rc.exe` på filen uden at substituere noget.
  Bumper du `Cargo.toml` uden at rette `.rc`-filen, hedder installeren det nye nummer mens
  exe'ens egenskabsdialog viser det gamle. Ret begge, eller lad `build.rs` generere blokken
  af `CARGO_PKG_VERSION`.
- Bevar ophavsretsnotitserne i `src/HuskFilter/`. To tredjeparter bor der: Unity Capture (MIT)
  og Microsofts DirectShow-baseklasser i `streams.h`/`streams.cpp`, som IKKE er dækket af
  projektets MIT-licens. Se [LICENSE](LICENSE).
- `.ps1` er ren ASCII: skriv `ae`/`oe`/`aa`, da Windows-konsollen kan fejlvise æ/ø/å.
  Undtagelse: `src/HuskFilter/byg.ps1` har
  UTF-8-BOM og må bruge æ/ø/å. `installer/husk-webcam.nsi` **skal bevare sin UTF-8-BOM**.
- Byg aldrig i kildetræet på en sky-disk. Følg [byggevejledningen](docs/byg-fra-kilde.md),
  også dens artefaktkontrol: en exitkode på 0 er ikke nok.
- Tests skal køre uden telefon; crate'ens `testfikstur` leverer en falsk telefon i processen.
- Identifikatorer er ASCII; kommentarer og brugerstrenge er dansk med æ/ø/å.
  Ingen em-dash (U+2014); brug en-dash (U+2013) eller omformulér.
