# Husk Webcam

Gør din telefon til et helt almindeligt webcam i Windows.

Husk Webcam melder telefonens kamera til Windows som en kameraenhed ved navn **Husk Webcam**.
Ethvert program der lader dig vælge et webcam, kan vælge den – du skal ikke installere noget
plugin i programmet, og du skal ikke ændre noget i det ud over kameravalget.

Telefonsiden er appen [Husk](https://xplat.co/husk), som streamer kameraet over dit eget
netværk. Denne PC-side henter strømmen og skriver den ind i kameraenheden.

## Krav

- Windows 10 eller 11, 64-bit.
- Husk installeret på telefonen.
- Telefon og PC på samme netværk – dit eget LAN, eller en krypteret tunnel som Tailscale.
  Strømmen er almindelig HTTP og hører ikke hjemme på et netværk du ikke selv styrer.

## Installation

1. Hent `HuskWebcam-<version>-setup.exe` fra [Releases](../../releases/latest).
2. Dobbeltklik den, og sig ja til Windows' rettighedsdialog.
   **Den ene forhøjelse er ikke til at spare væk:** et nyt kamera i Windows er et
   DirectShow-filter, og et filter skal registreres som administrator. Selve kamerabrugen
   bagefter kræver ingen særlige rettigheder.
3. Sæt flueben ved autostart hvis du vil have Husk Webcam i bakken ved login.

### ⚠️ Installeren er usigneret, og det ser Windows

Der er ingen kodesignatur på filen endnu, så **SmartScreen stopper den første gang**:

> Windows beskyttede din pc

Klik **Flere oplysninger** og derefter **Kør alligevel**. Advarslen betyder ikke at der er
fundet noget galt – den betyder at filen ikke er set før og ikke er signeret. Uden den
besked ville mange tro at produktet er i stykker; det er det ikke.

Vil du selv kontrollere filen, står dens SHA256 på udgivelsen, og hele kildekoden ligger her
i repoet.

## Sådan bruges den

1. Start **Husk Webcam**.
2. Klik **Tilføj**, og udfyld:
   - **Navn** – hvad telefonen skal hedde i billedet.
   - **Telefonens adresse** – Husk viser den i appens egen statusvisning.
   - **Token** – kun hvis du har slået et til på telefonen. Det gemmes beskyttet med Windows'
     egen brugerkryptering, men det beskytter ikke selve forbindelsen.
   - **Grundrotation** og tyngdesensor efter behov. **Forsidekameraet kræver Husk 1.1** på
     telefonen.
3. Vælg **Husk Webcam** som kamera i det program du skal bruge.

Appen kan køre som et vindue eller som et bakke-ikon. Autostart-genvejen starter den i bakken.

Kamera-filteret laver først sin delte hukommelse når en modtager åbner enheden. Siger appen
at filteret ikke svarer, er den almindelige årsag derfor at intet program har kameraet åbent
lige nu.

## Afinstallation

**Indstillinger → Apps → Installerede apps → Husk Webcam → Afinstaller**, eller genvejen
**Afinstallér Husk Webcam** i Start-menuen.

Afinstallationen afregistrerer kamera-filteret, fjerner autostart-genvejen og sletter
**præcis de filer installationen selv lagde**. Har du lagt egne filer i programmappen, bliver
de stående. Din opsætning i `%LOCALAPPDATA%\husk-viewer\` rører den ikke, så en geninstallation
husker dine telefoner.

## Byg fra kilden

Se [docs/byg-fra-kilde.md](docs/byg-fra-kilde.md).

## Licens

MIT – se [LICENSE](LICENSE).

Kamera-filteret i `src/HuskFilter/` bærer to vendorerede tredjeparter, begge MIT:
[Unity Capture](https://github.com/schellingb/UnityCapture) og Microsofts DirectShow-baseklasser.
Deres ophavsret og notitser står i [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
