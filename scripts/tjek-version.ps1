# Versions-gate: staar det SAMME nummer alle de steder versionen er skrevet ned?
#
# !! VERSIONEN HAR TO KILDER, OG KUN DEN ENE ER AUTOMATISK.
#    src\husk-webcam-rs\Cargo.toml er kilden: scripts\byg-installer.ps1 laeser den derfra og
#    sender den ind i installer\husk-webcam.nsi med /D. MEN
#    src\husk-webcam-rs\res\husk.rc HARDKODER FILEVERSION, PRODUCTVERSION og de to
#    versionsstrenge, og build.rs kalder bare rc.exe paa filen uden at substituere noget.
#
#    Bumper man Cargo.toml alene, hedder installeren altsaa det NYE nummer mens exe'ens
#    egenskabsdialog viser det GAMLE - og INGEN af de to melder fejl. Derfor denne gate.
#
# !! DEN HAR EN POSITIV KONTROL. Et moenster der ikke kan matche - fordi .rc-filen er skrevet
#    om, eller fordi regexen er forkert - svarer "ingen uenighed" praecis som et rent traee.
#    De to udfald skal kunne skelnes, saa scriptet KRAEVER at alle fire felter blev FUNDET,
#    og fejler LUKKET hvis et af dem mangler.
#
# Brug:  powershell -File scripts\tjek-version.ps1 [-ForventetTag v0.1.0]
# Exit:  0 = enige, 1 = uenige, 2 = instrumentet kunne ikke maale
#
# Filen holdes i ren ASCII (husregel for .ps1).
param(
    [string]$ForventetTag = ''
)
$ErrorActionPreference = 'Stop'

$rod = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $rod 'src\husk-webcam-rs\Cargo.toml'
$rcFil     = Join-Path $rod 'src\husk-webcam-rs\res\husk.rc'
foreach ($f in @($cargoToml, $rcFil)) {
    if (-not (Test-Path $f)) { Write-Output "FEJL: finder ikke $f"; exit 2 }
}

# --- kilden: [package] version i Cargo.toml -----------------------------------------------
# !! Ankeret er BUNDET til [package]-sektionen. Et bart '^version = ' kunne ogsaa ramme en
#    afhaengigheds-sektion laengere nede, og saa maalte vi et fremmed biblioteks nummer.
$tomlTekst = Get-Content $cargoToml -Raw
$pakkeBlok = [regex]::Match($tomlTekst, '(?ms)^\[package\]\s*(.*?)(?=^\[|\z)')
if (-not $pakkeBlok.Success) { Write-Output "FEJL: fandt ingen [package]-sektion i $cargoToml"; exit 2 }
$m = [regex]::Match($pakkeBlok.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
if (-not $m.Success) { Write-Output "FEJL: kunne ikke laese version ud af [package] i $cargoToml"; exit 2 }
$version = $m.Groups[1].Value

if ($version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Output ("FEJL: Cargo.tomls version '{0}' er ikke paa formen x.y.z, og saa kan .rc-filens" -f $version)
    Write-Output '      fire-leddede form ikke udledes af den. Ret enten versionen eller denne gate.'
    exit 2
}
$dele = $version.Split('.')
$kommaForm  = ('{0},{1},{2},0' -f $dele[0], $dele[1], $dele[2])
$strengForm = ('{0}.{1}.{2}.0' -f $dele[0], $dele[1], $dele[2])
Write-Output ("kilde: {0} -> version {1} (forventer '{2}' og '{3}' i husk.rc)" -f
              (Split-Path -Leaf $cargoToml), $version, $kommaForm, $strengForm)

# --- den manuelle kopi: res\husk.rc --------------------------------------------------------
$rcTekst = Get-Content $rcFil -Raw
$felter = @(
    @{ navn = 'FILEVERSION';               moenster = '(?m)^\s*FILEVERSION\s+([0-9]+\s*,\s*[0-9]+\s*,\s*[0-9]+\s*,\s*[0-9]+)\s*$'; ventet = $kommaForm  },
    @{ navn = 'PRODUCTVERSION';            moenster = '(?m)^\s*PRODUCTVERSION\s+([0-9]+\s*,\s*[0-9]+\s*,\s*[0-9]+\s*,\s*[0-9]+)\s*$'; ventet = $kommaForm  },
    @{ navn = 'VALUE "FileVersion"';       moenster = '(?m)VALUE\s+"FileVersion"\s*,\s*"([^"]+)"';    ventet = $strengForm },
    @{ navn = 'VALUE "ProductVersion"';    moenster = '(?m)VALUE\s+"ProductVersion"\s*,\s*"([^"]+)"'; ventet = $strengForm }
)

$uenige = 0
$rcUenige = 0
foreach ($felt in $felter) {
    $t = [regex]::Match($rcTekst, $felt.moenster)
    # POSITIV KONTROL pr. felt: fandt vi det slet ikke, har vi ikke MAALT noget.
    if (-not $t.Success) {
        Write-Output ("FEJL: fandt ikke feltet {0} i {1}." -f $felt.navn, (Split-Path -Leaf $rcFil))
        Write-Output '      Gaten kan da ikke svare, og et tavst "enige" ville vaere forkert.'
        exit 2
    }
    $fundet = ($t.Groups[1].Value -replace '\s', '')
    if ($fundet -ne $felt.ventet) {
        Write-Output ("UENIGE {0}: husk.rc siger '{1}', Cargo.toml siger '{2}'" -f $felt.navn, $fundet, $felt.ventet)
        $uenige++
        $rcUenige++
    } else {
        Write-Output ("  OK {0} = {1}" -f $felt.navn, $fundet)
    }
}

# --- taggen, naar der er en ----------------------------------------------------------------
if ($ForventetTag) {
    $tagVersion = $ForventetTag -replace '^v', ''
    if ($tagVersion -ne $version) {
        Write-Output ("UENIGE tag: taggen hedder '{0}' (altsaa {1}), Cargo.toml siger {2}" -f
                      $ForventetTag, $tagVersion, $version)
        $uenige++
    } else {
        Write-Output ("  OK tag {0} svarer til version {1}" -f $ForventetTag, $version)
    }
}

if ($uenige -gt 0) {
    Write-Output ''
    Write-Output ("SAMLET: {0} uenighed(er)." -f $uenige)
    # Kuren afhaenger af HVEM der er uenig, saa raadet maa ikke pege paa .rc-filen naar det
    # er taggen der staar forkert.
    if ($rcUenige -gt 0) {
        Write-Output '  husk.rc: ret src\husk-webcam-rs\res\husk.rc, saa den foelger Cargo.toml - ELLER lad'
        Write-Output '           build.rs generere versionsblokken af CARGO_PKG_VERSION, saa der kun er'
        Write-Output '           een kilde tilbage.'
    }
    if ($uenige -gt $rcUenige) {
        Write-Output '  tag:     bump versionen i Cargo.toml (og husk.rc) FOER du saetter taggen, ellers'
        Write-Output '           baerer udgivelsen et andet nummer end det binaeren siger.'
    }
    exit 1
}
Write-Output ("ENIGE: version {0} staar ens alle steder" -f $version)
exit 0
