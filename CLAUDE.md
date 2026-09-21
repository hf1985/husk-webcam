# Husk Webcam – agent-kontekst

Læs efter opgave: [produkt og brug](README.md), [kodekort og designvalg](ARKITEKTUR.md),
[byg og test](docs/byg-fra-kilde.md), [status og næste arbejde](FORTSÆT-HER.md).

## Offentligt repo

En offentliggjort commit kan ikke kaldes tilbage. Commit aldrig hemmeligheder (tokens,
nøgler, adgangskoder), rigtige netværksadresser, maskinnavne, brugernavne, konto-id'er eller
interne henvisninger til planer, spor og andre projekter. Det gælder også tests og kommentarer.
Eksempel-IP'er: `192.0.2.0/24` eller `10.0.0.0/8`.

Før commit:

```powershell
powershell -File scripts\scrub-tjek.ps1
```

⛔ **Brug scriptet, ikke et `git grep` i hånden.** `git grep` ser kun gits INDEKS, så en fil der
lige er kopieret eller skrevet er usynlig for den – og gaten svarer »rent« om noget den aldrig
kiggede på. Scriptet måler filerne på disken, og det har en **positiv kontrol**: det søger først
efter noget det ved findes, så et rent svar ikke kan forveksles med en søgning der ikke nåede
filerne. Begge retninger er målt: en indsat kanariefil med en tailnet-adresse giver `rc=1`.

Scriptet fælder ikke på IP-lignende strenge; det tæller dem og beder dig læse listen. Det er
med vilje – de lovlige eksempler skal ses igennem hver gang.

## Regler

- Versionen bor i `src/husk-webcam-rs/Cargo.toml`; `scripts/byg-installer.ps1` og
  `installer/husk-webcam.nsi` læser den derfra. ⚠️ **Der er ÉN kilde mere, og den er manuel:**
  `src/husk-webcam-rs/res/husk.rc` hardkoder `FILEVERSION`/`PRODUCTVERSION` og de to
  `"0.1.0.0"`-strenge, og `build.rs` kalder bare `rc.exe` på filen uden at substituere noget.
  Bumper du `Cargo.toml` uden at rette `.rc`-filen, hedder installeren det nye nummer mens
  exe'ens egenskabsdialog viser det gamle. Ret begge, eller lad `build.rs` generere blokken
  af `CARGO_PKG_VERSION`.
- Bevar ophavsretsnotitserne i `src/HuskFilter/`. To tredjeparter bor der: Unity Capture og
  Microsofts DirectShow-baseklasser i `streams.h`/`streams.cpp`. Begge er MIT, men
  ophavsretten er ikke projektets. Se [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
  ⛔ **`LICENSE` skal blive ved med KUN at være MIT-teksten.** Lægger man tredjeparts-afsnit
  ind i den, holder GitHub op med at genkende licensen (målt: feltet gik til `Other`), og
  SignPaths OSI-krav læses maskinelt fra netop det felt.
- Commit-metadata bærer ejerens e-mailadresse, og **det er etableret praksis frem for en
  forglemmelse – målt 2026-09-21:** begge hans øvrige offentlige repoer bærer den samme
  adresse i deres commits. Dette repo er altså ikke det der eksponerer den. At profilens
  `email`-felt står tomt, siger intet om commit-headere; det er et andet felt. **Skift derfor
  ikke `user.email` her alene** – det ville gøre ét af tre repoer uens uden at skjule noget.
  Skal praksis laves om, er det alle tre på én gang, og det er ejerens valg.
  Bemærk at scrub-tjekket måler FILINDHOLD, ikke commit-headere.
- ⛔ **Skriv ikke ejerens kontonavn eller en fuld GitHub-URL ind i en fil her.** Det er ikke
  fordi navnet er hemmeligt – repoet ligger under det – men fordi kontonavnet INDEHOLDER hans
  Windows-brugernavn som delstreng, og scrub-tjekket kan ikke skelne de to. Hold træet fri af
  formen, så tjekket kan blive ved med at være skarpt. README linker derfor til udgivelserne
  med den repo-relative form `../../releases/latest`, som GitHub selv opløser.
- `.ps1` er ren ASCII: skriv `ae`/`oe`/`aa`, da Windows-konsollen kan fejlvise æ/ø/å.
  Undtagelse: `src/HuskFilter/byg.ps1` har
  UTF-8-BOM og må bruge æ/ø/å. `installer/husk-webcam.nsi` **skal bevare sin UTF-8-BOM**.
- Byg aldrig i kildetræet på en sky-disk. Følg [byggevejledningen](docs/byg-fra-kilde.md),
  også dens artefaktkontrol: en exitkode på 0 er ikke nok.
- Tests skal køre uden telefon; crate'ens `testfikstur` leverer en falsk telefon i processen.
- Identifikatorer er ASCII; kommentarer og brugerstrenge er dansk med æ/ø/å.
  Ingen em-dash (U+2014); brug en-dash (U+2013) eller omformulér.
