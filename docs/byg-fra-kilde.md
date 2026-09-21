# Byg Husk Webcam fra kilden

Kør de tre trin i rækkefølge fra repo-roden i PowerShell. Hvert trin leverer til det næste.

## Værktøjer

- [rustup](https://rustup.rs), `stable-x86_64-pc-windows-msvc`.
- Visual Studio Build Tools: »Desktop development with C++« og Windows SDK til filteret og
  appens ikon/manifest. `rc.exe` findes i nyeste SDK; alternativt sæt `HUSKWEBCAM_RC` til dens sti.
- [NSIS](https://nsis.sourceforge.io) til installeren.

## 1. Appen

```powershell
powershell -File scripts\byg-rust.ps1 -Profil release
```

Scriptet kontrollerer at binæren ikke importerer `VCRUNTIME140.dll` og prøver at køre den.
Først derefter kopieres den til
`%LOCALAPPDATA%\husk-webcam-build\husk-rust\release\HuskWebcam.exe`.
Brug `-Test` for tests uden telefon.

### Byggetræet skal ligge på en lokal disk

Google Drive-sync kan korrumpere builds; `.gitignore` stopper ikke sync.
`build.rs` afviser derfor byggetræer på samme drev som sky-synket kilde. Drive genkendes på
`.shortcut-targets-by-id`. En almindelig lokal klon må bygge på samme drev som kilden.

`byg-rust.ps1` placerer byggetræet korrekt. Ved direkte Cargo-kald: sæt `CARGO_TARGET_DIR`
uden for sky-drevet, eller brug en lokal klon.

### Smart App Control kan blokere en nybygget binær

Fejlen `An Application Control policy has blocked this file (os error 4551)` kan ramme
både usignerede app-/testbinærer og Cargos build-scripts.

⚠️ **Det er ikke kun indholdet.** »Prøv igen med et frisk byggetræ« hjælper nogle gange, men
det er ikke en pålidelig udvej: målt 2026-09-21 blev ni forsøg i træk afvist, heriblandt et
`cargo build --release` på en kilde der var verificeret **byte-identisk** med den der byggede
grønt en time før. Tilstanden kan altså stramme, og så kan maskinen ikke bygge produktet
overhovedet.

`byg-rust.ps1` forsøger op til tre byggetræer. Det er en omgåelse; kuren er signering, og
indtil da: byg på en maskine uden Smart App Control i håndhævelse.
**Slå ikke Smart App Control fra:** det kan kun fortrydes ved at geninstallere Windows.

## 2. Kamera-filteret

```powershell
powershell -File src\HuskFilter\byg.ps1 -Registrer
```

Bygger 32/64-bit-DLL'er, kopierer dem til `%USERPROFILE%\Tools\HuskWebcam\` og registrerer
dem derfra. `-Registrer` kræver administrator, fordi `regsvr32` skriver i HKCR.
Udelad flaget for kun at bygge; installeren registrerer hos slutbrugeren.

**Registrér aldrig fra byggemappen:** COM gemmer filstien, og en ryddet byggecache efterlader
et kamera som stadig listes, men fejler ved brug.

## 3. Installeren

```powershell
powershell -File scripts\byg-installer.ps1
```

Udelod du `-Registrer` i trin 2, ligger DLL'erne kun i byggemappen. Peg på dem eksplicit:

```powershell
powershell -File scripts\byg-installer.ps1 -FilterDir "$env:LOCALAPPDATA\husk-webcam-build\filter"
```

Scriptet henter den verificerede app fra trin 1, kræver begge filter-DLL'er, genererer
[afinstallationslisten](../ARKITEKTUR.md) fra de pakkede filer og kører `makensis`.
Versionen læses alene fra `src/husk-webcam-rs/Cargo.toml`.
Resultatet kontrolleres og størrelsen i bytes udskrives:
`%LOCALAPPDATA%\husk-webcam-build\installer\HuskWebcam-<version>-setup.exe`.

**Bevar UTF-8-BOM i `installer\husk-webcam.nsi`.** Uden den læser `makensis` ANSI,
ødelægger danske bogstaver og melder alligevel exit 0. Scriptet afviser en compilerlog
der ikke bekræfter UTF-8.

## Den offentlige byggekæde

De samme tre trin kører automatisk i `.github/workflows/byg.yml` ved hvert push til `main`, ved hver pull request, ved et tag på formen `v*` og på et manuelt kald.
Kæden kalder præcis de scripts der står ovenfor, i den rækkefølge.
Der er med vilje ingen parallel byggeopskrift i den: en kæde der byggede anderledes end denne vejledning, ville dokumentere noget andet end det du selv kan bygge.

Kæden er grunden til at du ikke behøver tro på os.
Den kører i et miljø hvis log alle kan læse, og den lægger en **herkomst-attest** (build provenance) på installeren, appens exe og begge filter-DLL'er.
Attesten binder hver fil til den commit og den workflow-kørsel der lavede den, og den kan efterprøves fra en klon:

```powershell
gh attestation verify .\HuskWebcam-<version>-setup.exe --repo (gh repo view --json nameWithOwner -q .nameWithOwner)
```

Ud over de tre byggetrin kører kæden fire ting der ikke kan køres meningsfuldt i hånden hver gang:

- **Scrub-gaten** (`scripts/scrub-tjek.ps1`) på hvert eneste push, så et offentligt repo ikke kan komme til at bære noget privat.
- **Versions-gaten** (`scripts/tjek-version.ps1`), fordi versionen har to kilder og kun den ene er automatisk. Se [CLAUDE.md](../CLAUDE.md) og gatens egen hovedkommentar.
- **Rust-suiten**, som en udviklermaskine med Smart App Control i håndhævelse ikke kan køre: testbinærerne blokeres.
- **Sky-drev-vagten i `build.rs`** (`scripts/proev-skyvagt.ps1`), i begge retninger, mod en falsk Drive-montering. Vagten er betinget, så en almindelig lokal klon aktiverer den slet ikke.

⚠️ **Kæden falder ikke tilbage til et frisk byggetræ.** Den kalder `byg-rust.ps1` med `-Forsoeg 1`.
Fallback'en findes for Smart App Control, og en runner har ikke SAC i håndhævelse; dér ville et fald tilbage skjule en ægte fejl frem for at afsløre den.

Skubber du et tag `vX.Y.Z`, kræver versions-gaten at taggen, `Cargo.toml` og `res/husk.rc` er enige, og kørslen lægger artefakterne op som en udgivelse med SHA256 og attest-vejledning i noterne.
Bump derfor versionen **før** du sætter taggen.

## Kodesignering

Installeren er usigneret og udløser SmartScreen.
Byggekæden ovenfor er på plads, så forbindelsen fra binær til kilde nu er dokumenteret og efterprøvelig.
Selve indsendelsen til en signeringstjeneste er et menneskes opgave; se [næste arbejde](../FORTSÆT-HER.md).
