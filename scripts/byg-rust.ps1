# Byg Rust-udgaven af Husk Webcam, og PROEV artefaktet bagefter.
#
# !! BYGGETRAEET HOERER UDEN FOR EN SKY-MAPPE. Ligger kilden i en Google Drive-montering,
#    korrumperer syncen hoej-churn-skrivninger, og en .gitignore stopper git - ikke syncen.
#    Kuren er CARGO_TARGET_DIR, og
#    crate'ens build.rs NAEGTER at bygge uden den. Vagten er fail-closed med vilje.
#
# !! ET GROENT BUILD ER IKKE ET KOERBART PROGRAM. Staar Smart App Control i HAANDHAEVELSE
#    (VerifiedAndReputablePolicyState = 1), blokerer den nogle nybyggede, usignerede
#    binaerer med "An Application Control policy has blocked this file".
#    Dommen er INDHOLDS-afhaengig og uforudsigelig: samme kode bygget to gange gav to
#    forskellige SHA256'er, og kun den ene blev blokeret (maalt 2026-09-20).
#    Derfor PROEVER scriptet exe'en frem for at slaa den op (maaleregel 317).
#
# !! SLAA ALDRIG SAC FRA for at komme videre. Det er en envejsdoer der kun kan fortrydes ved
#    at geninstallere Windows, og det er ejerens valg - ikke en rundes.
#
# !! CARGO SKAL KALDES FRA CRATE-MAPPEN. Crate'ens .cargo\config.toml saetter
#    -C target-feature=+crt-static, og cargo finder den fil ud fra ARBEJDSMAPPEN - ikke ud
#    fra --manifest-path. Kaldt fra en anden mappe ville flaget vaere en tavs no-op, og
#    binaeren ville igen importere VCRUNTIME140.dll. Scriptet skifter derfor mappe OG maaler
#    bagefter at importen er vaek (maaleregel 134: en erklaering er ikke et bevis).
#
# Kraever rustup med stable-x86_64-pc-windows-msvc samt Visual Studio Build Tools med
# C++-vaerktoejskaeden og Windows SDK (rc.exe). Se docs/byg-fra-kilde.md.
#
# Filen holdes i ren ASCII (husregel for .ps1).

param(
    [ValidateSet('release', 'debug')]
    [string]$Profil = 'release',
    [switch]$Test,
    [switch]$Ren,
    [int]$Forsoeg = 3
)

$ErrorActionPreference = 'Stop'

$projekt = Split-Path -Parent $PSScriptRoot
$crate = Join-Path $projekt 'src\husk-webcam-rs'
$manifest = Join-Path $crate 'Cargo.toml'
if (-not (Test-Path $manifest)) { throw "Cargo.toml findes ikke: $manifest" }

# Cargo ligger i USER scope naar rustup er installeret uden administrator.
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) {
    $cmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cmd) { $cargo = $cmd.Source } else { throw "cargo.exe blev ikke fundet - er rustup installeret?" }
}

$byggerod = Join-Path $env:LOCALAPPDATA 'husk-webcam-build'
$kanonisk = Join-Path $byggerod ('husk-rust\' + $Profil)

# !! IKKE 'Stop' omkring de native kald: PS 5.1 KASTER paa et nativt programs OMDIRIGEREDE
#    stderr under 'Stop' - ogsaa ved exit 0 - og cargo skriver rutinemaessigt advarsler dertil.
$ErrorActionPreference = 'Continue'

function Koerer([string]$exe) {
    # Proev artefaktet. $LASTEXITCODE saettes IKKE hvis programmet aldrig blev STARTET - den
    # beholder da FORRIGE native kalds vaerdi - saa vi nulstiller til $null foerst, og den
    # TOMME vaerdi er svaret "kunne ikke koere" (maaleregel 481).
    $global:LASTEXITCODE = $null
    & $exe --hjaelp 2>&1 | Out-Null
    return ($null -ne $LASTEXITCODE)
}

# Maal at +crt-static faktisk slog igennem. Slaar flaget fejl (fx fordi cargo blev kaldt fra
# en anden mappe end crate'en), importerer binaeren VCRUNTIME140.dll - og saa er loeftet om
# "ingen runtime at installere" falsk. Importtabellen laeses raat; vi har ikke brug for en
# PE-parser til at afgoere om et DLL-navn staar i filen.
function ImporterVcRuntime([string]$exe) {
    $tekst = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($exe))
    return ($tekst -match 'VCRUNTIME140' -or $tekst -match 'api-ms-win-crt-')
}

# !! ALT en PowerShell-funktion skriver til OUTPUT-stroemmen ER dens returvaerdi
#    (maaleregel 238). `cargo test` skriver sine resultater til STDOUT, saa et bart kald ville
#    goere hele testrapporten til en del af $kode - og sammenligningen `$kode -eq 0` ville da
#    aldrig holde. Maalt her 2026-09-20: byg-rust.ps1 -Test meldte "bestod ikke" om en suite
#    der var helt groen. `Out-Host` sender linjerne til skaermen UDEN om output-stroemmen.
#    (`cargo build` slap igennem, fordi den skriver sin fremdrift til STDERR.)
function Byg([string]$maal) {
    $env:CARGO_TARGET_DIR = $maal
    New-Item -ItemType Directory -Force -Path $maal | Out-Null
    Push-Location $crate
    try {
        if ($Ren) { & $cargo clean --manifest-path $manifest | Out-Null }

        if ($Test) {
            & $cargo test --manifest-path $manifest --no-fail-fast | Out-Host
            return $LASTEXITCODE
        }
        if ($Profil -eq 'release') {
            & $cargo build --release --manifest-path $manifest | Out-Host
        } else {
            & $cargo build --manifest-path $manifest | Out-Host
        }
        return $LASTEXITCODE
    }
    finally { Pop-Location }
}

if ($Test) {
    # Testene koerer i debug-profilen. Release saetter panic = "abort", og en abortet proces
    # kan ikke rapportere HVILKET ben der faldt.
    $traeer = @((Join-Path $byggerod 'rust'), (Join-Path $byggerod 'rust-2'), (Join-Path $byggerod 'rust-3'))
    for ($i = 0; $i -lt [Math]::Min($Forsoeg, $traeer.Count); $i++) {
        Write-Output "byggetrae: $($traeer[$i])"
        $kode = Byg $traeer[$i]
        if ($kode -eq 0) { exit 0 }
        # Blev en TESTBINAER blokeret, er det ikke koden der er forkert.
        Write-Output "cargo test svarede $kode."
        if ($i -lt ($Forsoeg - 1)) {
            Write-Output "FALDER TILBAGE til et FRISK byggetrae: en blokeret testbinaer kan ofte slippe igennem naar indholdet skifter. Det er en OMGAAELSE, ikke en kur - se docs/byg-fra-kilde.md -> 'Smart App Control'."
        }
    }
    throw "cargo test bestod ikke efter $Forsoeg forsoeg"
}

$traeer = @((Join-Path $byggerod 'rust'), (Join-Path $byggerod 'rust-2'), (Join-Path $byggerod 'rust-3'))
for ($i = 0; $i -lt [Math]::Min($Forsoeg, $traeer.Count); $i++) {
    $maal = $traeer[$i]
    Write-Output "byggetrae: $maal"
    $kode = Byg $maal
    if ($kode -ne 0) { throw "cargo build fejlede med $kode" }

    # Doem paa ARTEFAKTET, ikke paa exitkoden alene (maaleregel 3).
    $exe = Join-Path $maal "$Profil\HuskWebcam.exe"
    if (-not (Test-Path $exe)) { throw "buildet meldte OK, men $exe findes ikke" }

    if (ImporterVcRuntime $exe) {
        throw ("$exe importerer VCRUNTIME140/api-ms-win-crt - altsaa slog +crt-static IKKE igennem. " +
               "Kontrollen er, at cargo blev kaldt fra $crate, saa crate'ens .cargo\config.toml blev laest.")
    }

    if (Koerer $exe) {
        New-Item -ItemType Directory -Force -Path $kanonisk | Out-Null
        $kanoniskExe = Join-Path $kanonisk 'HuskWebcam.exe'
        Copy-Item $exe $kanoniskExe -Force
        if (-not (Koerer $kanoniskExe)) {
            throw "den verificerede binaer kunne ikke koere fra $kanoniskExe - kopien blev doemt anderledes"
        }
        Write-Output ("OK {0}: {1} bytes" -f $kanoniskExe, (Get-Item $kanoniskExe).Length)
        Write-Output ("kilde: {0}" -f $exe)
        exit 0
    }

    Write-Output "BLOKERET: $exe kunne ikke startes (Smart App Control staar i haandhaevelse)."
    if ($i -lt ($Forsoeg - 1)) {
        Write-Output "FALDER TILBAGE til et FRISK byggetrae. Det er en OMGAAELSE, ikke en kur: SAC's dom er indholds-afhaengig, og et andet build kan slippe igennem. Den rigtige kur er en SIGNERET binaer - se docs/byg-fra-kilde.md -> 'Smart App Control'."
    }
}

throw "binaeren kunne ikke startes efter $Forsoeg forsoeg - Smart App Control blokerer den. Slaa IKKE SAC fra; det er en envejsdoer og ejerens valg."
