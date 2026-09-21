# Scrub-tjek: leder efter ting der ikke hoerer i et offentligt repo.
#
# !! DEN MAALER FILERNE PAA DISKEN, IKKE GITS INDEKS.
#    `git grep` ser kun det der er i indekset, saa en fil der lige er kopieret eller skrevet
#    er USYNLIG for den - og gaten svarer da "rent" om noget den aldrig kiggede paa. Det er en
#    maalt faelde, ikke en teoretisk: den ramte netop det spor der gjorde dette repo offentligt.
#
# !! OG DEN HAR EN POSITIV KONTROL.
#    Et moenster der ikke kan matche - fordi det er stavet forkert, eller fordi soegningen ikke
#    naaede filerne - svarer nul traef praecis som et rent traee. De to udfald skal kunne
#    skelnes, saa scriptet soeger FOERST efter noget det VED findes. Svarer kontrollen nul, er
#    instrumentet i stykker og scriptet fejler LUKKET.
#
# Brug:  powershell -File scripts\scrub-tjek.ps1
# Exit:  0 = rent, 1 = fund, 2 = instrumentet kunne ikke maale
#
# Filen holdes i ren ASCII (husregel for .ps1).
param(
    [string]$Rod = ''
)
$ErrorActionPreference = 'Stop'
if (-not $Rod) { $Rod = Split-Path -Parent $PSScriptRoot }

# Filer der maales: alt sporet OG alt usporet, minus .git og byggeartefakter.
#
# !! `.git` KAN VAERE EN FIL, ikke en mappe. Ligger gitdir'en et andet sted - som den goer
#    naar arbejdstraeet ligger paa et sky-drev og repoet paa lokal disk - er `.git` en
#    ETLINJES PEGER med en ABSOLUT lokal sti, altsaa med brugernavnet i. Den fil er gits egen
#    og bliver aldrig committet, men et filter der kun kendte mappeformen maalte den, og
#    gaten stod da ROED paa selve udviklermaskinen (maalt 2026-09-21). En gate der er roed af
#    sig selv bliver ignoreret, og saa fanger den heller ikke det den findes for.
$filer = Get-ChildItem -LiteralPath $Rod -Recurse -File |
    Where-Object { $_.FullName -notmatch '\\\.git\\' -and $_.Name -ne '.git' -and
                   $_.FullName -notmatch '\\target\\' }
Write-Output ("maaler {0} filer under {1}" -f $filer.Count, $Rod)

function Soeg([string]$Moenster) {
    # -Encoding utf8 frem for default: en fil med danske bogstaver maa ikke tabe tegn paa vej ind.
    return @(Select-String -LiteralPath $filer.FullName -Pattern $Moenster -Encoding utf8 -ErrorAction SilentlyContinue)
}

# --- POSITIV KONTROL: noget der SKAL findes ----------------------------------------------
$kontrol = Soeg 'HuskWebcamFilter64'
if ($kontrol.Count -lt 1) {
    Write-Output 'FEJL: den positive kontrol gav NUL traef. Soegningen naaede ikke filerne, saa'
    Write-Output '      et rent svar nedenfor ville vaere vaerdiloest. Retter du filtret ovenfor,'
    Write-Output '      saa ret ogsaa kontrol-moensteret.'
    exit 2
}
Write-Output ("positiv kontrol: {0} traef - instrumentet maaler" -f $kontrol.Count)

# --- DET DER IKKE MAA STAA HER ------------------------------------------------------------
# Adresser: alt der ikke er dokumentations- eller privatadresser.
# Identiteter: maskin- og brugernavn udledes ved koersel, saa listen ikke bliver maskin-bundet.
$moenstre = @{}
$moenstre['tailnet-adresse (100.64-127.x)'] = '\b100\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.'
$moenstre['e-mailadresse i en fil']         = '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}'
$moenstre['konkret brugermappe']            = 'Users[\\/][A-Za-z0-9._-]+'
$moenstre['MAC-adresse']                    = '([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}'
if ($env:COMPUTERNAME) { $moenstre['denne maskines navn'] = [regex]::Escape($env:COMPUTERNAME) }
if ($env:USERNAME)     { $moenstre['dette brugernavn']    = [regex]::Escape($env:USERNAME) }

$fund = 0
foreach ($navn in $moenstre.Keys | Sort-Object) {
    $traef = Soeg $moenstre[$navn]
    if ($traef.Count -eq 0) { continue }
    $fund += $traef.Count
    Write-Output ("FUND {0} ({1}):" -f $navn, $traef.Count)
    $traef | Select-Object -First 10 | ForEach-Object {
        Write-Output ("  {0}:{1}" -f $_.Path.Substring($Rod.Length + 1), $_.LineNumber)
    }
}

# IP-adresser meldes for sig: de lovlige eksempler SKAL laeses igennem hver gang, saa listen
# holdes kort nok til det. Den faelder ikke af sig selv.
$ip = Soeg '\b([0-9]{1,3}\.){3}[0-9]{1,3}\b'
Write-Output ("IP-lignende strenge: {0} - laes dem igennem, kun 192.0.2.x, 10.x og 127.x er lovlige" -f $ip.Count)

if ($fund -gt 0) { Write-Output ("SAMLET: {0} fund" -f $fund); exit 1 }
Write-Output 'RENT: ingen fund'
exit 0
