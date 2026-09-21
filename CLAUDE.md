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

- Versionen bor kun i `src/husk-webcam-rs/Cargo.toml`; hardkod den ikke andre steder.
  `scripts/byg-installer.ps1` læser den derfra.
- Bevar Unity Captures ophavsretsnotitser og MIT-licens i `src/HuskFilter/`.
- `.ps1` er ren ASCII: skriv `ae`/`oe`/`aa`, da Windows-konsollen kan fejlvise æ/ø/å.
  Undtagelse: `src/HuskFilter/byg.ps1` har
  UTF-8-BOM og må bruge æ/ø/å. `installer/husk-webcam.nsi` **skal bevare sin UTF-8-BOM**.
- Byg aldrig i kildetræet på en sky-disk. Følg [byggevejledningen](docs/byg-fra-kilde.md),
  også dens artefaktkontrol: en exitkode på 0 er ikke nok.
- Tests skal køre uden telefon; crate'ens `testfikstur` leverer en falsk telefon i processen.
- Identifikatorer er ASCII; kommentarer og brugerstrenge er dansk med æ/ø/å.
  Ingen em-dash (U+2014); brug en-dash (U+2013) eller omformulér.
