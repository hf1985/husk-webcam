# Hermetisk proeve af build.rs' vagt mod at bygge paa et sky-synket drev - BEGGE RETNINGER.
#
# !! HVORFOR DEN IKKE KAN KOERE PAA EN UDVIKLERMASKINE.
#    Vagten i src\husk-webcam-rs\build.rs er betinget: den taender kun naar KILDEN ligger
#    under en Google Drive-montering, genkendt paa markoeren .shortcut-targets-by-id. Paa en
#    almindelig lokal klon er den altsaa slet ikke i spil, og paa den maskine hvor kilden
#    LIGGER paa Drive, blokerede Smart App Control enhver nybygget testbinaer. Afvis-retningen
#    har derfor aldrig koert. Dette script koerer den paa et byggemiljoe uden haandhaevelse.
#
# !! DEN MAALER BEGGE RETNINGER, og det er ikke en venlighed. En vagt der afviser ALT er
#    lige saa oedelagt som en der afviser intet: den ville naegte at bygge for enhver med en
#    almindelig lokal klon. Proeven kraever derfor baade et AFVIST og et TILLADT build.
#
# !! OG DEN FEJLER LUKKET NAAR DEN IKKE KAN MAALE. Proeven har kun mening hvis de to
#    byggetraeer ligger paa FORSKELLIGE drev-roedder; ligger de paa samme, maaler den ikke
#    det den tror, og et groent svar ville vaere vaerdiloest.
#
# Brug:  powershell -File scripts\proev-skyvagt.ps1
# Exit:  0 = vagten opfoerer sig som beskrevet, 1 = den goer ikke, 2 = kunne ikke maale
#
# Filen holdes i ren ASCII (husregel for .ps1).
param(
    [string]$SkyRod = '',
    [string]$LokalRod = ''
)
$ErrorActionPreference = 'Stop'

$rod = Split-Path -Parent $PSScriptRoot
$crate = Join-Path $rod 'src\husk-webcam-rs'
if (-not (Test-Path (Join-Path $crate 'build.rs'))) { Write-Output "FEJL: finder ikke $crate\build.rs"; exit 2 }

if (-not $SkyRod)   { $SkyRod   = Join-Path $env:RUNNER_TEMP 'skyvagt' }
if (-not $SkyRod)   { $SkyRod   = Join-Path $env:TEMP 'skyvagt' }
if (-not $LokalRod) { $LokalRod = Join-Path $env:LOCALAPPDATA 'skyvagt-lokal' }

function DrevAf([string]$sti) { return (Split-Path -Qualifier $sti).ToUpperInvariant() }

New-Item -ItemType Directory -Force -Path $SkyRod, $LokalRod | Out-Null
$skyDrev = DrevAf (Resolve-Path $SkyRod).Path
$lokDrev = DrevAf (Resolve-Path $LokalRod).Path
Write-Output ("sky-side:   {0} (drev {1})" -f $SkyRod, $skyDrev)
Write-Output ("lokal side: {0} (drev {1})" -f $LokalRod, $lokDrev)
if ($skyDrev -eq $lokDrev) {
    Write-Output 'FEJL: de to byggetraeer ligger paa SAMME drev-rod. Vagten doemmer netop paa'
    Write-Output '      drev-roden, saa proeven kan ikke skelne de to retninger her. Angiv'
    Write-Output '      -SkyRod og -LokalRod paa hver sit drev.'
    exit 2
}

# Cargo ligger i USER scope naar rustup er installeret uden administrator.
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
if (-not (Test-Path $cargo)) {
    $cmd = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cmd) { $cargo = $cmd.Source } else { Write-Output 'FEJL: cargo.exe blev ikke fundet'; exit 2 }
}

# --- byg en FALSK sky-montering: en kopi af crate'en under en .shortcut-targets-by-id-markoer ---
# Markoeren er praecis den Drive selv laegger i RODEN af monteringen, og den er det eneste
# vagten kigger efter. Monteringen behoever altsaa ikke vaere en rigtig Drive-mappe for at
# vagten taender - den skal bare se ud som en.
$kopi = Join-Path $SkyRod 'crate'
if (Test-Path $kopi) { Remove-Item -LiteralPath $kopi -Recurse -Force }
Copy-Item -LiteralPath $crate -Destination $kopi -Recurse -Force
if (Test-Path (Join-Path $kopi 'target')) { Remove-Item -LiteralPath (Join-Path $kopi 'target') -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $SkyRod '.shortcut-targets-by-id') | Out-Null
Write-Output ("falsk sky-montering: {0} (markoer .shortcut-targets-by-id lagt i roden)" -f $SkyRod)

# !! `cargo check` og ikke `cargo build`: build.rs KOERER ogsaa under check, og det er
#    build.rs vi maaler. En fuld release-binaer ville koste minutter uden at maale mere.
function Koer([string]$maal) {
    $env:CARGO_TARGET_DIR = $maal
    Push-Location $kopi
    try {
        $gammel = $ErrorActionPreference
        # !! IKKE 'Stop' omkring det native kald: PS 5.1 KASTER paa et nativt programs
        #    OMDIRIGEREDE stderr - ogsaa ved exit 0 - og cargo skriver rutinemaessigt dertil.
        $ErrorActionPreference = 'Continue'
        $log = & $cargo check --quiet 2>&1 | ForEach-Object { [string]$_ }
        $kode = $LASTEXITCODE
        $ErrorActionPreference = $gammel
        return [pscustomobject]@{ Kode = $kode; Log = ($log -join "`n") }
    }
    finally { Pop-Location }
}

$fejl = 0

# --- RETNING 1: byggetrae paa SAMME rod som den (falske) sky-kilde -> skal AFVISES ---------
Write-Output ''
Write-Output '--- retning 1: byggetrae paa SAMME drev som sky-kilden (forventet AFVIST) ---'
$afvist = Koer (Join-Path $SkyRod 'target-samme-drev')
$vagtTekst = 'SAMME drev som kilden'
if ($afvist.Kode -eq 0) {
    Write-Output 'FUND: vagten TILLOD et build paa samme drev som den sky-synkede kilde.'
    Write-Output '      Det er praecis den tilstand den findes for at hindre.'
    $fejl++
} elseif ($afvist.Log -notmatch [regex]::Escape($vagtTekst)) {
    # !! Exitkoden alene er ikke svaret. Et build kan fejle af hundrede grunde, og en
    #    manglende rc.exe ville her ligne en vagt der virker (maaleregel 3).
    Write-Output ("FUND: buildet fejlede ({0}), men IKKE med vagtens egen tekst '{1}'." -f $afvist.Kode, $vagtTekst)
    Write-Output '      Saa er det noget andet der faldt, og vagten er ikke maalt.'
    Write-Output '--- sidste linjer ---'
    ($afvist.Log -split "`n" | Select-Object -Last 15) | ForEach-Object { Write-Output "  $_" }
    $fejl++
} else {
    Write-Output ("OK: afvist med exit {0}, og med vagtens egen begrundelse." -f $afvist.Kode)
}

# --- RETNING 2: byggetrae paa en ANDEN rod -> skal TILLADES --------------------------------
Write-Output ''
Write-Output '--- retning 2: byggetrae paa et ANDET drev (forventet TILLADT) ---'
$tilladt = Koer (Join-Path $LokalRod 'target-andet-drev')
if ($tilladt.Kode -ne 0) {
    Write-Output ("FUND: vagten AFVISTE et lovligt byggetrae (exit {0})." -f $tilladt.Kode)
    Write-Output '      Kilden ligger paa sky-drevet, men byggetraeet gjorde ikke - det er tilladt.'
    Write-Output '--- sidste linjer ---'
    ($tilladt.Log -split "`n" | Select-Object -Last 15) | ForEach-Object { Write-Output "  $_" }
    $fejl++
} else {
    Write-Output 'OK: tilladt.'
}

Write-Output ''
if ($fejl -gt 0) { Write-Output ("SAMLET: {0} fund" -f $fejl); exit 1 }
Write-Output 'BEGGE RETNINGER SOM BESKREVET: afviser samme drev, tillader et andet.'
exit 0
