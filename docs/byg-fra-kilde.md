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
både usignerede app-/testbinærer og Cargos build-scripts. Dommen er indholdsafhængig;
samme kode kan bygge grønt én gang og afvises næste gang.

`byg-rust.ps1` forsøger op til tre byggetræer. Det er en omgåelse; kuren er signering.
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

## Kodesignering

Installeren er usigneret og udløser SmartScreen. Signering skal ske i en offentlig byggekæde
med dokumenteret forbindelse til kilden; se [næste arbejde](../FORTSÆT-HER.md).
