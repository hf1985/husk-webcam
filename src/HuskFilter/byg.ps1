# Byg det brandede DirectShow-filter "Husk Webcam" i BAADE 32- og 64-bit.
#
# Hvorfor et script og ikke .sln-filen fra upstream: der er ingen Visual Studio-IDE i denne
# kaede, kun Build Tools. Scriptet kalder cl.exe direkte med praecis de flag upstreams
# projektfil saetter, saa resultatet er det samme uden at vi skal vedligeholde en .vcxproj.
#
# ⛔ Byggetraeet ligger UDEN FOR Drive. Drive-syncen korrumperer hoej-churn-skrivninger, og
#    .gitignore stopper git - ikke syncen.
#
# Kraever Visual Studio Build Tools med C++-vaerktoejskaeden og Windows SDK.

param(
    [string]$Ud = "$env:LOCALAPPDATA\husk-webcam-build\filter",
    [string]$InstallerTil = "$env:USERPROFILE\Tools\HuskWebcam",
    [switch]$Registrer
)

$ErrorActionPreference = 'Stop'
$kilde = $PSScriptRoot

$installer = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer"
$vswhere = Join-Path $installer 'vswhere.exe'
if (-not (Test-Path $vswhere)) { throw "vswhere.exe findes ikke - er Build Tools installeret?" }

$vs = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
if (-not $vs) { throw "ingen MSVC-vaerktoejskaede fundet" }
$vcvarsall = Join-Path $vs 'VC\Auxiliary\Build\vcvarsall.bat'
if (-not (Test-Path $vcvarsall)) { throw "vcvarsall.bat findes ikke i $vs" }

New-Item -ItemType Directory -Force -Path $Ud | Out-Null

function Byg([string]$Arch, [string]$Suffiks) {
    $obj = Join-Path $Ud "obj-$Suffiks"
    New-Item -ItemType Directory -Force -Path $obj | Out-Null
    $dll = Join-Path $Ud "HuskWebcamFilter$Suffiks.dll"

    # Flagene er upstreams Release-konfiguration: ingen undtagelser, ingen RTTI, statisk
    # runtime (saa DLL'en ikke kraever en VC-redist paa brugerens maskine), og .def-filen
    # som modul-definition.
    #
    # Bat-filen skrives LINJE FOR LINJE frem for som én && -kaede. En kaede med indlejrede
    # anfoerselstegn krydser to fortolkende lag (PowerShell og cmd), og det er to uafhaengige
    # escape-kontrakter - maalt her 2026-09-19, hvor netop den form braekkede scriptet.
    $linjer = @(
        '@echo off',
        # vcvarsall.bat kalder SELV vswhere.exe og forventer at den er paa PATH. Uden denne
        # linje fejler den med "'vswhere.exe' is not recognized" - og fejlen peger paa
        # vcvarsall frem for paa PATH.
        ('set "PATH=' + $installer + ';%PATH%"'),
        ('call "' + $vcvarsall + '" ' + $Arch + ' >nul'),
        'if errorlevel 1 exit /b 1',
        ('cd /d "' + $obj + '"'),
        ('cl /nologo /c /O2 /GL /Gw /GS- /EHs-c- /GR- /MT /DNDEBUG /D_WINDOWS /D_USRDLL /I"' +
         $kilde + '" "' + $kilde + '\streams.cpp" "' + $kilde + '\HuskFilter.cpp"'),
        'if errorlevel 1 exit /b 1',
        ('link /nologo /DLL /LTCG /SUBSYSTEM:WINDOWS /SAFESEH:NO /DEF:"' + $kilde +
         '\HuskFilter.def" /OUT:"' + $dll + '" streams.obj HuskFilter.obj ' +
         'strmiids.lib winmm.lib ole32.lib oleaut32.lib uuid.lib advapi32.lib user32.lib gdi32.lib'),
        'if errorlevel 1 exit /b 1'
    )

    $bat = Join-Path $obj 'byg.bat'
    Set-Content -Path $bat -Value $linjer -Encoding ascii
    & cmd.exe /c $bat 2>&1 | ForEach-Object { "    $_" }
    if ($LASTEXITCODE -ne 0) { throw "$Suffiks-buildet fejlede med $LASTEXITCODE" }

    # Doem paa ARTEFAKTET, ikke paa exitkoden alene (maaleregel 3).
    if (-not (Test-Path $dll)) { throw "$Suffiks-buildet meldte OK, men $dll findes ikke" }

    $i = Get-Item $dll
    Write-Output ("OK {0}: {1} bytes" -f $dll, $i.Length)
}

Byg -Arch 'x64' -Suffiks '64'
Byg -Arch 'x86' -Suffiks '32'

if ($Registrer) {
    # ⛔ REGISTRER ALDRIG DIREKTE FRA $Ud. Byggemappen er en CACHE - den ligger i
    #    %LOCALAPPDATA% netop for at kunne ryddes - og en COM-registrering er en PEGER til en
    #    filsti. Ryddes cachen, staar enheden stadig paa listen i enhver moedeapp og fejler
    #    foerst ved instantiering, hvilket ligner en fejl i APPEN frem for en manglende fil.
    #    Maalt 2026-09-19: begge brandede CLSID'er pegede ind i cachen efter foerste byg.
    #    Derfor kopieres DLL'erne til en STABIL sti foerst, og registreringen sker derfra.
    New-Item -ItemType Directory -Force -Path $InstallerTil | Out-Null
    foreach ($s in @('32', '64')) {
        $kildeDll = Join-Path $Ud "HuskWebcamFilter$s.dll"
        $dll = Join-Path $InstallerTil "HuskWebcamFilter$s.dll"
        Copy-Item $kildeDll $dll -Force
        # Fra PowerShell, ALDRIG fra Git Bash: MSYS sti-mangler slash-flaget, og et regsvr32
        # uden /s viser en MODAL dialog der haenger et ikke-interaktivt kald for evigt.
        $p = Start-Process regsvr32.exe -ArgumentList @('/s', ('"' + $dll + '"')) -PassThru -Wait -WindowStyle Hidden
        Write-Output ("regsvr32 {0} -> exit {1}" -f $dll, $p.ExitCode)
    }
}
