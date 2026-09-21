; Husk Webcam - NSIS-installer
;
; Bygges med: scripts\byg-installer.ps1
;
; ⚠️ Filen SKAL gemmes med UTF-8-BOM. makensis læser en BOM-løs kilde som ANSI og gør hvert
;    dansk bogstav i installerens tekster til mojibake - og den melder exit 0 om resultatet.
;    Vagten mod det står i byg-installer.ps1, hvor den kan fejle lukket.
;
; ⚠️ »Samme afinstallation som installationen lagde« er et KRAV der måtte erobres, ikke en
;    gratis egenskab. Den første udgave brugte `RMDir /r "$INSTDIR"` og slettede derved
;    brugerens EGNE filer i app-mappen - målt, i begge retninger. Se noten i
;    Uninstall-sektionen.
;
; ===========================================================================
; DE TRE TING EN INSTALLER FOR DETTE PRODUKT SKAL GØRE, OG HVORFOR
; ===========================================================================
;
; 1. REGISTRERER ET DIRECTSHOW-FILTER i både 32 og 64 bit. Det er derfor den beder om
;    administrator. Et nyt kamera i Windows kan ikke leveres uden én forhøjelse af
;    rettigheder. UAC kræves KUN her, ikke ved almindelig kamerabrug.
;
; 2. LÆGGER AUTOSTARTEN I BRUGERENS Startup-mappe, ikke i Alle brugeres, og ikke som en
;    HKCU\...\Run-værdi.
;
; 3. AFREGISTRERER FILTRENE FØR filerne slettes. Omvendt rækkefølge ville efterlade en
;    COM-registrering der peger på en fil der ikke findes - enheden ville stadig stå på
;    listen i enhver app og fejle ved instantiering.
;
; ===========================================================================
; FÆLDE A: `regsvr32` RAMMER DET FORKERTE `System32`, TAVST
; ===========================================================================
;
; En NSIS-installer er ALTID en 32-bit proces. For en 32-bit proces omskriver Windows
; stien `C:\Windows\System32` til `C:\Windows\SysWOW64` - WOW64's filsystem-omdirigering.
; Et `"$SYSDIR\regsvr32.exe" /s HuskWebcamFilter64.dll` ville derfor kalde den 32-BIT
; regsvr32 med den 64-BIT DLL. Den skriver i den 32-bit registerview, og det 64-bit
; filter ville aldrig blive synligt for en 64-bit app.
;
; Kuren er den virtuelle sti `$WINDIR\Sysnative`, som KUN findes for en 32-bit proces og
; peger på det ægte System32. Det 32-bit filter registreres med den almindelige sti, som
; netop derfor rammer SysWOW64 og dermed den rigtige regsvr32.
;
; Bytter man de to om, svarer regsvr32 med en modul-fejl - men `/s` skjuler den, og
; installeren ville se ud til at lykkes. Registreringen måles derfor som en TILSTAND,
; aldrig på en exitkode.
;
; ===========================================================================
; FÆLDE B: AUTOSTART-GENVEJEN KAN LANDE HOS DEN FORKERTE BRUGER
; ===========================================================================
;
; `$SMSTARTUP` med `SetShellVarContext current` opløses for den bruger PROCESSENS TOKEN
; tilhører. Når installeren er forhøjet, er det den konto forhøjelsen skete til. Startede
; en almindelig bruger installationen og indtastede en ANDEN konto i UAC-dialogen, lander
; genvejen i den kontos Startup-mappe, og appen ville aldrig starte for den bruger der
; installerede - uden at noget fejler synligt.
;
; ⛔ DET ER IKKE EN NSIS-ULEMPE. Enhver installer der beder om administrator og samtidig
;    skriver i et per-bruger-område har præcis samme eksponering; Inno Setup advarer
;    ordret om den ved kompilering. Hvad der ER målt i en ren prøve-VM: genvejen lander i
;    den INSTALLERENDE brugers Startup-mappe, og `HKCU\...\Run` er tom. Det dækker den
;    almindelige vej (en administrator dobbeltklikker og siger ja til UAC), ikke
;    kryds-bruger-vejen.
;
; ===========================================================================
; FÆLDE C: MANIFESTET BESTEMMER HVEM DER KAN STARTE INSTALLEREN
; ===========================================================================
;
; `RequestExecutionLevel admin` lægger `requireAdministrator` i manifestet. En sådan exe
; kan ikke startes med `CreateProcess` fra en ikke-forhøjet proces - kaldet fejler med
; ERROR_ELEVATION_REQUIRED (740). Kun `ShellExecute` (altså et dobbeltklik i Stifinder,
; eller `Start-Process`) går gennem UAC-tjenesten og forhøjer.
;
; Forskellen er ikke synlig for en slutbruger, men den er synlig for enhver automatiseret
; prøve der starter programmer med `CreateProcessAsUser` - dér skal installeren startes
; gennem `Start-Process`, ikke kaldes direkte.

Unicode true
ManifestDPIAware true

!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"
; FileFunc-makroerne skal ERKLÆRES før ${GetSize} kan bruges; uden denne linje fejler
; kompileringen på et navn der ellers ser ud til at være med i include-filen.
!insertmacro GetSize

; --------------------------------------------------------------------------
; Produkt-konstanter. VERSIONEN BOR I `src\husk-webcam-rs\Cargo.toml`, ikke her: den er
; den ene kilde, og scripts\byg-installer.ps1 læser den derfra og sender den ind med /D.
; Defaults nedenfor er kun til en håndkørsel.
; --------------------------------------------------------------------------
!define NAVN     "Husk Webcam"
!define UDGIVER  "Hans Frederik Brobjerg"
!define EXENAVN  "HuskWebcam.exe"
!define WEB      "https://xplat.co/husk"

!ifndef Version
  !define Version "0.0.0"
!endif
!ifndef PublishDir
  !define PublishDir "$%LOCALAPPDATA%\husk-webcam-build\publish-rust"
!endif
!ifndef FilterDir
  !define FilterDir "$%USERPROFILE%\Tools\HuskWebcam"
!endif
!ifndef UdDir
  !define UdDir "$%LOCALAPPDATA%\husk-webcam-build\installer"
!endif
; Afinstallations-listen GENERERES af byg-installer.ps1 ud af de filer der faktisk pakkes.
; Uden den ville afinstallationen skulle gætte, og det gæt er `RMDir /r` - se noten i
; Uninstall-sektionen for hvad den kostede.
!ifndef AfinstallerListe
  !error "AfinstallerListe mangler. Byg med scripts\byg-installer.ps1, som genererer den."
!endif

; Produktets stabile id. Den må ALDRIG ændres: den er nøglen en opgradering finder den
; forrige installation på, og et nyt id ville efterlade den gamle post i Programmer og
; funktioner med en afinstaller ingen rydder op efter.
!define APPID         "{7A3E1C64-9D2B-4E51-8F07-2C6B5D44A981}"
!define AFREG_NOEGLE  "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}"

Name "${NAVN}"
OutFile "${UdDir}\HuskWebcam-${Version}-setup.exe"
InstallDir "$PROGRAMFILES64\${NAVN}"
; !! IKKE InstallDirRegKey: den læses FØR .onInit og dermed i 32-bit registerview, hvor
;    vores post aldrig står. Den ville altså tavst altid svare "ikke fundet" og se ud til
;    at virke. Opslaget sker i .onInit efter SetRegView 64.
RequestExecutionLevel admin
ShowInstDetails show
ShowUnInstDetails show

; Solid LZMA på maksimum. Det brugeren HENTER er derfor væsentligt mindre end det
; installationen fylder.
SetCompressor /SOLID lzma
SetCompressorDictSize 32
SetDatablockOptimize on

VIProductVersion "${Version}.0"
VIAddVersionKey /LANG=0 "ProductName"      "${NAVN}"
VIAddVersionKey /LANG=0 "ProductVersion"   "${Version}"
VIAddVersionKey /LANG=0 "FileVersion"      "${Version}.0"
VIAddVersionKey /LANG=0 "CompanyName"      "${UDGIVER}"
VIAddVersionKey /LANG=0 "LegalCopyright"   "${UDGIVER}"
VIAddVersionKey /LANG=0 "FileDescription"  "${NAVN} - installation"

; --------------------------------------------------------------------------
; Udseende. `${__FILEDIR__}` frem for en relativ sti.
; ⚠️ Her stod først at makensis opløser relative stier mod sin EGEN arbejdsmappe. Det er
; MÅLT FALSK: `makensis /HELP` siger »/NOCD disables the current directory change to that
; of the .nsi file«, altså skifter den som default TIL scriptets mappe, og en relativ sti
; ville have virket. `${__FILEDIR__}` er beholdt fordi den er entydig uanset `/NOCD` og
; uanset hvor kalderen står - men begrundelsen var opdigtet, og den slags står som fakta
; indtil nogen måler den.
; --------------------------------------------------------------------------
!define MUI_ICON   "${__FILEDIR__}\..\src\husk-webcam-rs\res\husk.ico"
!define MUI_UNICON "${__FILEDIR__}\..\src\husk-webcam-rs\res\husk.ico"
!define MUI_ABORTWARNING

; Ingen velkomstside og ingen programgruppe-side: installationen har præcis ét valg, og
; det står på komponent-siden.
!define MUI_COMPONENTSPAGE_TEXT_TOP "Vælg hvad der skal ske efter installationen. Klik Næste for at fortsætte."
!define MUI_COMPONENTSPAGE_TEXT_COMPLIST "Efter installation:"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Start ${NAVN} nu"
!define MUI_FINISHPAGE_RUN_FUNCTION StartSomAlmindeligBruger
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Danish"

; --------------------------------------------------------------------------
; Hjælpere
; --------------------------------------------------------------------------

; Stop en kørende kopi FØR filerne røres - ellers er DLL'en låst og kan hverken erstattes
; eller afregistreres.
; En fejl her må ikke stoppe noget: er processen der ikke, er målet allerede nået.
!macro StopAppen
  DetailPrint "Stopper en eventuelt kørende ${NAVN} ..."
  nsExec::Exec '"$SYSDIR\taskkill.exe" /f /im ${EXENAVN}'
  Pop $0
!macroend

; Registrér eller afregistrér begge filtre. Handling er "" (registrér) eller "/u ".
; Se FÆLDE A i hovedet for hvorfor de to kald bruger HVER SIN sti til regsvr32.
!macro RoerFiltre Handling
  ${If} ${RunningX64}
    DetailPrint "regsvr32 ${Handling} (64-bit) via Sysnative ..."
    nsExec::Exec '"$WINDIR\Sysnative\regsvr32.exe" ${Handling}/s "$INSTDIR\HuskWebcamFilter64.dll"'
    Pop $0
    DetailPrint "  64-bit regsvr32 svarede $0"
  ${Else}
    ; Kan ikke nås: .onInit afviser en 32-bit Windows, fordi appen er x64.
    DetailPrint "  SPRINGER 64-bit over: ikke et 64-bit Windows"
  ${EndIf}
  DetailPrint "regsvr32 ${Handling} (32-bit) ..."
  nsExec::Exec '"$SYSDIR\regsvr32.exe" ${Handling}/s "$INSTDIR\HuskWebcamFilter32.dll"'
  Pop $0
  DetailPrint "  32-bit regsvr32 svarede $0"
!macroend

Function un.onInit
  SetRegView 64
FunctionEnd

; Starter appen efter installationen som den ALMINDELIGE bruger, ikke som administrator.
;
; Installeren er forhøjet, så et bart `Exec` ville give appen et forhøjet token, og den
; ville skrive sin config i administratorens %LOCALAPPDATA% frem for i brugerens.
;
; Kuren uden plugin er at bede den KØRENDE Stifinder om at starte genvejen: explorer.exe
; videregiver anmodningen til den allerede kørende instans, som er brugerens egen og
; kører på medium integritetsniveau. Argumenterne følger med, fordi de står i genvejen.
;
; ⛔ DETTE ER DEN ENE RÆKKE EN SILENT PRØVE IKKE KAN MÅLE: siden springes over ved en
;    silent installation. Kører Stifinder ikke, sker der ingenting - samme udfald som hvis
;    brugeren havde fjernet fluebenet.
;
; ⛔ OG DEN SKAL STARTE MED `--bakke`. Her stod først et fald tilbage til
;    Start-menu-genvejen, som IKKE bærer argumentet: brugeren ville da få et VINDUE frem
;    for et bakke-ikon (`--bakke` = `gui::start(true)`). Kuren er en midlertidig genvej
;    der bærer argumentet; explorer.exe kan ikke selv videregive parametre.
Function StartSomAlmindeligBruger
  SetShellVarContext current
  ${If} ${FileExists} "$SMSTARTUP\${NAVN}.lnk"
    Exec '"$WINDIR\explorer.exe" "$SMSTARTUP\${NAVN}.lnk"'
  ${Else}
    CreateShortCut "$TEMP\${NAVN} start.lnk" "$INSTDIR\${EXENAVN}" "--bakke"
    Exec '"$WINDIR\explorer.exe" "$TEMP\${NAVN} start.lnk"'
  ${EndIf}
FunctionEnd

; --------------------------------------------------------------------------
; Installation
; --------------------------------------------------------------------------

Section "${NAVN}" SEK_APP
  SectionIn RO

  !insertmacro StopAppen

  SetOutPath "$INSTDIR"
  ; Appen. For Rust-udgaven er det én fil, men mappen pakkes rekursivt, så et senere
  ; ekstra artefakt ikke tavst falder ud.
  ; `\*` og ikke `\*.*`: sidstnævnte udelader tavst enhver fil UDEN et punktum i navnet.
  ClearErrors
  File /r "${PublishDir}\*"
  ; Filtrene. De ligger i app-mappen, IKKE i en tempmappe og ALDRIG på et netværks- eller
  ; sky-synket drev: regsvr32 skriver DLL'ens STI ind i registreringen, så en mappe der
  ; forsvinder giver et filter der peger på ingenting.
  File "${FilterDir}\HuskWebcamFilter64.dll"
  File "${FilterDir}\HuskWebcamFilter32.dll"
  ; ⛔ EN FIL DER IKKE KUNNE SKRIVES, SPRINGES OVER I TAVSHED - og installationen melder
  ; succes med den GAMLE fil på pladsen. `AllowSkipFiles` er default `on` (NSIS.chm
  ; 4.8.2.1): kan `File` ikke åbne målet, sættes kun fejlflaget, og uden dette `IfErrors`
  ; læser ingen det. Den realistiske vej er en opgradering mens en app har
  ; `HuskWebcamFilter64.dll` indlæst - altså præcis det `StopAppen` ikke kan nå, fordi
  ; DLL'en holdes af en FREMMED proces. Vi kan ikke lukke den for brugeren, men vi kan
  ; nægte at melde succes.
  ${If} ${Errors}
    Abort "Mindst én fil kunne ikke skrives til $INSTDIR. Luk de programmer der bruger Husk Webcam-kameraet, og prøv igen."
  ${EndIf}

  !insertmacro RoerFiltre ""

  ; Start-menu-gruppen hører til ALLE brugere: installationen er i forvejen forhøjet, og
  ; app-mappen ligger under Program Files.
  SetShellVarContext all
  CreateDirectory "$SMPROGRAMS\${NAVN}"
  CreateShortCut "$SMPROGRAMS\${NAVN}\${NAVN}.lnk" "$INSTDIR\${EXENAVN}"
  CreateShortCut "$SMPROGRAMS\${NAVN}\Afinstallér ${NAVN}.lnk" "$INSTDIR\Uninstall.exe"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "DisplayName"      "${NAVN}"
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "DisplayVersion"   "${Version}"
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "Publisher"        "${UDGIVER}"
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "URLInfoAbout"     "${WEB}"
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "DisplayIcon"      "$INSTDIR\${EXENAVN}"
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "InstallLocation"  "$INSTDIR"
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "UninstallString"  '"$INSTDIR\Uninstall.exe"'
  WriteRegStr   HKLM "${AFREG_NOEGLE}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${AFREG_NOEGLE}" "NoModify" 1
  WriteRegDWORD HKLM "${AFREG_NOEGLE}" "NoRepair" 1
  WriteRegDWORD HKLM "${AFREG_NOEGLE}" "EstimatedSize" $0
SectionEnd

Section "Start ${NAVN} automatisk ved login" SEK_AUTOSTART
  ; En genvej i brugerens Startup-mappe, IKKE en HKCU\...\Run-værdi: en genvej kan
  ; afinstallationen fjerne uden at skulle skrive i en anden brugers registerhive.
  ; Se FÆLDE B i hovedet for hvad "brugerens" betyder når installeren er forhøjet.
  SetShellVarContext current
  CreateShortCut "$SMSTARTUP\${NAVN}.lnk" "$INSTDIR\${EXENAVN}" "--bakke"
  ; ⛔ SKRIV DEN OPLØSTE STI NED, og slet SENERE præcis den.
  ; `$SMSTARTUP` opløses for det token processen har. Afinstallationen kan blive forhøjet
  ; til en ANDEN konto end installationen, og så ville et `Delete "$SMSTARTUP\..."` pege på
  ; en tredje brugers mappe og efterlade genvejen hos den der installerede.
  WriteRegStr HKLM "${AFREG_NOEGLE}" "AutostartGenvej" "$SMSTARTUP\${NAVN}.lnk"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEK_APP} "Appen og kamera-filteret. Kræves."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEK_AUTOSTART} "Lægger en genvej i din Startup-mappe, så ${NAVN} starter i bakken ved login."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ⛔ `.onInit` STÅR HER, EFTER SEKTIONERNE, OG DET ER ET KRAV - ikke en stilart.
; `${SEK_AUTOSTART}` er et compile-tids-define NSIS først skaber når den PARSER sektionen.
; Stod funktionen før, ville `!insertmacro UnselectSection ${SEK_AUTOSTART}` fælde bygget
; med et ukendt symbol.
Function .onInit
  ; Appen er udgivet x64, og filteret er 64-bit. ${RunningX64} er her det samme som
  ; ${IsWow64}, fordi installeren altid er 32-bit - og den bruger IsWow64Process2, så et
  ; 64-bit ARM-Windows også svarer ja.
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "${NAVN} kræver et 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  ; Registret læses og skrives i 64-bit view, så afinstallations-posten er synlig i
  ; Programmer og funktioner.
  SetRegView 64
  ; En eksisterende installation opgraderes dér hvor den står.
  ReadRegStr $0 HKLM "${AFREG_NOEGLE}" "InstallLocation"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
    ; ⛔ RESPEKTÉR ET TIDLIGERE FRAVALG AF AUTOSTARTEN.
    ; NSIS husker ikke selv et fravalg, så en opgradering ville ellers genindføre den
    ; autostart brugeren havde slået fra - hver gang.
    ; Målestokken er den sti installationen loggede, ikke en kontekst-opløsning.
    ReadRegStr $1 HKLM "${AFREG_NOEGLE}" "AutostartGenvej"
    ${If} $1 == ""
      !insertmacro UnselectSection ${SEK_AUTOSTART}
    ${ElseIfNot} ${FileExists} "$1"
      !insertmacro UnselectSection ${SEK_AUTOSTART}
    ${EndIf}
  ${EndIf}
FunctionEnd

; --------------------------------------------------------------------------
; Afinstallation
; --------------------------------------------------------------------------

Section "Uninstall"
  ; ⛔ HER STOD ET `SetOutPath "$TEMP"`, OG DET BLEV FJERNET IGEN FORDI PRÆMISSEN BLEV
  ;    MÅLT OG FALDT.
  ;    Frygten var rimelig: afinstallerens genvej i Start-menuen har »Start i« = app-mappen,
  ;    og Windows nægter at fjerne en mappe der er en levende proces' arbejdsmappe. Var
  ;    arbejdsmappen arvet, ville app-mappen blive stående efter en afinstallation.
  ;    MÅLT med en probe der skrev sin EGEN arbejdsmappe ud af en kørende afinstaller,
  ;    startet med arbejdsmappen sat til installationsmappen: NSIS' temp-kopi svarede
  ;    `C:\...\Temp\~nsu1.tmp`, altså sin egen mappe, og installationsmappen blev fjernet
  ;    både med og uden linjen. Positiv kontrol i samme kørsel: en mappe der ER en levende
  ;    proces' arbejdsmappe, kunne IKKE fjernes - så proben kunne gå rød.
  ;    NSIS håndterer det altså selv, og en ekstra linje ville være umålt kode med en
  ;    kommentar der påstod noget forkert.

  ; VÆRN FØRST: rør intet hvis mappen ikke ER vores installation.
  ; Filter-DLL'en er beviset - ingen andre lægger den.
  StrCpy $R9 "nej"
  ${If} ${FileExists} "$INSTDIR\HuskWebcamFilter64.dll"
  ${AndIf} ${FileExists} "$INSTDIR\${EXENAVN}"
    StrCpy $R9 "ja"
  ${EndIf}

  !insertmacro StopAppen

  ; AFREGISTRÉR FØR FILERNE SLETTES. Omvendt rækkefølge efterlader en COM-registrering
  ; der peger på en fil der ikke findes.
  !insertmacro RoerFiltre "/u "

  ; Autostart-genvejen: den STI installationen loggede, og som et net den kontekst-opløste.
  ; Se noten ved SEK_AUTOSTART: de to er kun den samme når afinstallationen forhøjes til
  ; samme konto som installationen.
  ReadRegStr $R8 HKLM "${AFREG_NOEGLE}" "AutostartGenvej"
  ${If} $R8 != ""
    Delete "$R8"
  ${EndIf}
  SetShellVarContext current
  Delete "$SMSTARTUP\${NAVN}.lnk"
  ; Den gamle HKCU\...\Run-form findes ikke i nogen udgivet udgave af dette produkt, men
  ; en maskine kan bære den fra en tidlig prøve. At fjerne den koster intet.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "HuskWebcam"

  SetShellVarContext all
  Delete "$SMPROGRAMS\${NAVN}\${NAVN}.lnk"
  Delete "$SMPROGRAMS\${NAVN}\Afinstallér ${NAVN}.lnk"
  RMDir "$SMPROGRAMS\${NAVN}"

  ; ⛔ HER STOD `RMDir /r "$INSTDIR"`, OG DEN SLETTEDE BRUGERENS EGNE FILER.
  ;
  ; MÅLT med en probe der bar en ORDRET kopi af denne blok: en app-mappe der FØR
  ; installationen indeholdt `KANARIE.txt` og `Dokumenter\vigtig.txt` fik BEGGE filer
  ; slettet af afinstallationen. Negativ kontrol (filter-DLL'en fjernet, så værnet faldt):
  ; begge overlevede, så proben kunne gå grøn.
  ;
  ; Værnet ovenfor beviser at VORES filer er der. Det beviser IKKE at andres ikke er, og
  ; `$INSTDIR` er ikke bundet: Directory-siden tager en indtastet sti, `/D=` accepteres,
  ; og `.onInit` overtager en gammel `InstallLocation`. NSIS' egen manual (NSIS.chm 4.9)
  ; advarer ordret mod formen.
  ;
  ; KUREN: `byg-installer.ps1` genererer listen - én `Delete` pr. fil installeren faktisk
  ; pakker, og én `RMDir` pr. mappe i omvendt dybde-orden, så en mappe kun forsvinder når
  ; den er tom. Listen udledes af PRÆCIS de filer `File /r` pakker.
  ${If} $R9 == "ja"
    !include "${AfinstallerListe}"
  ${Else}
    DetailPrint "Springer sletningen over: $INSTDIR ligner ikke en ${NAVN}-installation."
  ${EndIf}

  DeleteRegKey HKLM "${AFREG_NOEGLE}"
SectionEnd
