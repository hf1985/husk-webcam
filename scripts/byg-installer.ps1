# Bygger Husk Webcam-installeren (NSIS).
#
# Kaeden er tre led, og de tre skal have koert FOER dette script:
#
#   1. scripts\byg-rust.ps1            -> HuskWebcam.exe (den verificerede binaer)
#   2. src\HuskFilter\byg.ps1 -Registrer -> HuskWebcamFilter32/64.dll i en STABIL mappe
#   3. NSIS installeret                -> makensis.exe
#
# !! DER ER KUN EN RAMME OG EN VARIANT. Scriptet havde tidligere et -Ramme-valg (Inno
#    Setup) og et -Variant-valg (en .NET-udgave). Ingen af de to findes i dette repo, og en
#    valgmulighed der peger paa noget fravaerende er en faelde, ikke en frihed: den ser ud
#    til at virke indtil nogen vaelger den.
#
# !! VERSIONEN BOR I src\husk-webcam-rs\Cargo.toml, IKKE HER. Scriptet LAESER den derfra og
#    sender den ind i .nsi-filen med /D. Hardkodede vi navnet her, ville et versionsbump
#    give en fil med det GAMLE navn og en ny produktversion - og tilbagelaesningen nederst
#    ville finde den gamle fil og melde groent (maaleregel 17 og 21).
#
# Komprimeringen er solid LZMA paa maksimum, saa det brugeren HENTER er vaesentligt mindre
# end det installationen fylder. Begge tal staar i outputtet, saa de kan foelges over tid.
#
# Filen holdes i ren ASCII (husregel for .ps1).
param(
    [string]$Konfiguration = 'Release',
    [string]$PublishDir = '',
    [string]$FilterDir  = '',
    [string]$UdDir      = '',
    [switch]$SpringUdgivelseOver
)
$ErrorActionPreference = 'Stop'

$rod = Split-Path -Parent $PSScriptRoot
$nsi = Join-Path $rod 'installer\husk-webcam.nsi'
$cargoToml = Join-Path $rod 'src\husk-webcam-rs\Cargo.toml'
if (-not (Test-Path $nsi)) { throw "Finder ikke $nsi" }
if (-not (Test-Path $cargoToml)) { throw "Finder ikke $cargoToml" }

$byggerod = Join-Path $env:LOCALAPPDATA 'husk-webcam-build'
if (-not $PublishDir) { $PublishDir = Join-Path $byggerod 'publish-rust' }
if (-not $FilterDir)  { $FilterDir  = Join-Path $env:USERPROFILE 'Tools\HuskWebcam' }
if (-not $UdDir)      { $UdDir      = Join-Path $byggerod 'installer' }

# Versionen har EEN kilde: [package] version = "..." i Cargo.toml.
# !! Ankeret er BUNDET til [package]-sektionen. Et bart '^version = ' ville ogsaa kunne
#    ramme en afhaengigheds-sektion laengere nede i filen, og saa ville installeren baere
#    et fremmed biblioteks versionsnummer.
$tomlTekst = Get-Content $cargoToml -Raw
$pakkeBlok = [regex]::Match($tomlTekst, '(?ms)^\[package\]\s*(.*?)(?=^\[|\z)')
if (-not $pakkeBlok.Success) { throw "Fandt ingen [package]-sektion i $cargoToml" }
$m = [regex]::Match($pakkeBlok.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
if (-not $m.Success) { throw "Kunne ikke laese version ud af [package] i $cargoToml" }
$version = $m.Groups[1].Value
$udbase = "HuskWebcam-$version-setup"
Write-Output ("--- Husk Webcam {0} (NSIS) ---" -f $version)

# --- 1) laeg den verificerede binaer i udgivelsesmappen ---
if (-not $SpringUdgivelseOver) {
    Write-Output '--- henter den VERIFICEREDE Rust-binaer ---'
    # !! Den KANONISKE sti, ikke byggetraeets: byg-rust.ps1 laegger kun en binaer der naar
    #    den har PROEVET at koere den. Smart App Control kan blokere nybyggede, usignerede
    #    binaerer, og en installer pakket om en binaer der ikke kan startes, ville se helt
    #    normal ud (maaleregel 317).
    $profil = if ($Konfiguration -eq 'Debug') { 'debug' } else { 'release' }
    $rustExe = Join-Path $byggerod ("husk-rust\$profil\HuskWebcam.exe")
    if (-not (Test-Path $rustExe)) {
        throw "MANGLER: $rustExe. Koer scripts\byg-rust.ps1 -Profil $profil foerst."
    }
    # !! SLET KUN DEN ENE FIL VI SELV LAEGGER - ikke hele mappen.
    #    -PublishDir er en fri parameter, og et `Remove-Item -Recurse -Force` paa en
    #    kalder-leveret sti sletter hvad der end ligger der.
    New-Item -ItemType Directory -Force -Path $PublishDir | Out-Null
    $maalExe = Join-Path $PublishDir 'HuskWebcam.exe'
    if (Test-Path $maalExe) { Remove-Item $maalExe -Force }
    Copy-Item $rustExe $maalExe -Force
}

# --- 2) kontroller at det installeren skal pakke, FAKTISK findes ---
# Uden det her ville makensis fejle med en sti-fejl langt inde i en log, og en tom
# installer ville ligne en bygget installer.
$exe = Join-Path $PublishDir 'HuskWebcam.exe'
$f64 = Join-Path $FilterDir 'HuskWebcamFilter64.dll'
$f32 = Join-Path $FilterDir 'HuskWebcamFilter32.dll'
foreach ($k in @($exe, $f64, $f32)) {
    if (-not (Test-Path $k)) { throw "MANGLER: $k" }
}
$antal = (Get-ChildItem $PublishDir -Recurse -File).Count
$mb = [math]::Round(((Get-ChildItem $PublishDir -Recurse -File | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Output ("  udgivelse: {0} filer, {1} MB (ukomprimeret)" -f $antal, $mb)

# --- 2b) GENERER AFINSTALLATIONS-LISTEN ---
#
# !! `RMDir /r "$INSTDIR"` SLETTER BRUGERENS EGNE FILER, og det er MAALT frem for frygtet.
#    En probe med en ORDRET kopi af .nsi'ens afinstallations-blok mod en mappe der FOER
#    installationen indeholdt `KANARIE.txt` og `Dokumenter\vigtig.txt`: installation rc=0,
#    afinstallation rc=0, og BEGGE fremmede filer var vaek. NSIS' egen manual siger det
#    samme (NSIS.chm 4.9): "Using RMDir /r $INSTDIR in the uninstaller is not safe ... the
#    user might select to install to the root of the Program Files folder and this command
#    would wipe out the entire Program Files folder".
#    $INSTDIR er IKKE bundet: Directory-siden tager en indtastet sti, `/D=` accepteres, og
#    .onInit overtager en gammel `InstallLocation`.
#
# KUREN: hver fil der pakkes faar sin egen `Delete`, hver mappe sin egen `RMDir` i OMVENDT
# dybde-orden, saa en mappe kun fjernes naar den er tom. Listen udledes af PRAECIS de samme
# filer `File /r` pakker, saa de to kan ikke drive fra hinanden.
New-Item -ItemType Directory -Force -Path $UdDir | Out-Null
$nsh = Join-Path $UdDir 'afinstaller.nsh'
$linjer = New-Object System.Collections.Generic.List[string]
$linjer.Add('; GENERERET af scripts\byg-installer.ps1 - ret ALDRIG i haanden.')
$linjer.Add('; Een Delete pr. fil installeren pakker, og een RMDir pr. mappe i omvendt')
$linjer.Add('; dybde-orden. Se begrundelsen i byg-installer.ps1 (afsnit 2b).')

$publishRod = (Resolve-Path $PublishDir).Path.TrimEnd('\')
$filer = @(Get-ChildItem -LiteralPath $publishRod -Recurse -File)
foreach ($f in $filer) {
    $rel = $f.FullName.Substring($publishRod.Length).TrimStart('\')
    $linjer.Add('  Delete "$INSTDIR\' + $rel + '"')
}
# Filtrene og afinstalleren ligger i app-mappens rod og staar ikke i PublishDir.
$linjer.Add('  Delete "$INSTDIR\HuskWebcamFilter64.dll"')
$linjer.Add('  Delete "$INSTDIR\HuskWebcamFilter32.dll"')
$linjer.Add('  Delete "$INSTDIR\Uninstall.exe"')

$mapper = @(Get-ChildItem -LiteralPath $publishRod -Recurse -Directory |
            ForEach-Object { $_.FullName.Substring($publishRod.Length).TrimStart('\') } |
            Sort-Object -Property { ($_ -split '\\').Count } -Descending)
foreach ($m in $mapper) { $linjer.Add('  RMDir "$INSTDIR\' + $m + '"') }
$linjer.Add('  RMDir "$INSTDIR"')

# UTF-8 UDEN BOM: filen `!include`'es af en .nsi der ALLEREDE er UTF-8, og en BOM midt
# i et include ville lande som tegn i kildeteksten. Listen er i forvejen ren ASCII.
[System.IO.File]::WriteAllLines($nsh, $linjer, (New-Object System.Text.UTF8Encoding($false)))
Write-Output ("  afinstallations-liste: {0} filer, {1} mappe(r) -> {2}" -f
              ($filer.Count + 3), ($mapper.Count + 1), $nsh)
# TILBAGELAESNING, og her et korrekthedskrav: listen SKAL baere mindst een Delete pr.
# pakket fil, ellers ville afinstallationen efterlade dem.
$skrevet = @(Get-Content -LiteralPath $nsh)
$antalDelete = @($skrevet | Where-Object { $_ -match '^\s*Delete ' }).Count
if ($antalDelete -ne ($filer.Count + 3)) {
    throw ("Afinstallations-listen baerer {0} Delete-linjer, forventet {1}" -f $antalDelete, ($filer.Count + 3))
}

# --- 3) find makensis ---
# winget UDEN administrator installerer i USER scope, ikke i Program Files, mens en
# forhoejet winget lander i Program Files. BEGGE roedder skal derfor staa paa listen.
$kandidater = @(
    (Join-Path $env:LOCALAPPDATA 'Programs\NSIS\makensis.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'),
    (Join-Path $env:ProgramFiles 'NSIS\makensis.exe')
)
$navn = 'makensis.exe'
$compiler = $kandidater | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $compiler) { throw ("$navn findes ikke. Proevede:`n  " + ($kandidater -join "`n  ")) }
Write-Output "  compiler: $compiler"

# --- 4) byg ---
Write-Output '--- bygger installeren ---'
$gammel = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
# !! IKKE 'Stop' omkring det native kald: PS 5.1 KASTER paa et nativt programs OMDIRIGEREDE
#    stderr under 'Stop' - ogsaa ved exit 0.
$log = & $compiler "/DVersion=$version" "/DPublishDir=$PublishDir" "/DFilterDir=$FilterDir" `
    "/DUdDir=$UdDir" "/DAfinstallerListe=$nsh" $nsi 2>&1
$kode = $LASTEXITCODE
$ErrorActionPreference = $gammel
$log | Select-Object -Last 12 | ForEach-Object { Write-Output "  $_" }
if ($kode -ne 0) { throw "$navn gav exit $kode" }

# !! makensis LAESER KILDEFILEN SOM ANSI HVIS DEN MANGLER SIN UTF-8-BOM, og den siger
#    exit 0 om resultatet. MAALT med en probe der skrev sin egen danske streng ud af en
#    koerende installer: MED BOM kom "Afinstaller/Kraever/Vaelg" korrekt ud, UDEN BOM kom de
#    ud som mojibake - i begge tilfaelde rc=0. Vagten er derfor her, hvor den kan fejle
#    LUKKET, og ikke en note i .nsi-filen.
#    !! Vagten spoerger om der FINDES en linje der siger UTF8 - ikke om der findes en linje
#       der lader vaere. `$log -notmatch 'X'` er en FILTRERING der svarer alle de linjer der
#       ikke matcher, og den er sand for enhver log paa mere end een linje (maaleregel 46).
$utf8linjer = @($log | ForEach-Object { [string]$_ } |
                Where-Object { $_ -match 'Processing script file.+\(UTF8\)' })
if ($utf8linjer.Count -eq 0) {
    throw ("makensis laeste ikke $nsi som UTF-8. Filen skal gemmes med UTF-8-BOM, " +
           "ellers bliver hvert dansk bogstav i installerens tekster til mojibake - tavst.")
}

# --- 5) TILBAGELAESNING: findes filen, og hvor stor blev den? ---
# !! Filen slaas op paa DET navn versionen giver, ikke paa det nyeste
#    'HuskWebcam-*-setup.exe'. Navnet er udledt af Cargo.tomls egen version, saa de to kan
#    ikke drive fra hinanden.
$ud = Get-Item (Join-Path $UdDir ($udbase + '.exe')) -ErrorAction SilentlyContinue
if (-not $ud) { throw ("{0} meldte 0, men {1}.exe findes ikke i {2}" -f $navn, $udbase, $UdDir) }
Write-Output ''
Write-Output ("INSTALLER: {0}" -f $ud.FullName)
Write-Output ("BYTES: {0}" -f $ud.Length)
Write-Output ("STOERRELSE: {0} MB (mod {1} MB ukomprimeret)" -f [math]::Round($ud.Length / 1MB, 1), $mb)
