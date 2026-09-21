# Husk Webcam – arkitektur

```text
Husk på telefonen -- MJPEG/HTTP --> HuskWebcam.exe --> HuskWebcamFilter{32,64}.dll
                                                        |
                                                kameraet "Husk Webcam"
```

| Del | Ansvar |
|---|---|
| `src/husk-webcam-rs/` | Henter strømmen, afkoder JPEG, bygger én frame pr. tick og sender til filteret. Viser vindue/bakke-ikon og gemmer konfiguration. |
| `src/HuskFilter/` | DirectShow-kameraet, vendoreret fra Unity Capture. Windows kræver et kamerafilter; modtagerprogrammet ser denne del. |
| `installer/` | NSIS: registrerer 32/64-bit-filteret, tilbyder autostart i brugerens Startup-mappe og afinstallerer egne filer. |
| `scripts/` | App- og installerbyg; filterets byggescript ligger i `src/HuskFilter/`. Se [de tre byggetrin](docs/byg-fra-kilde.md). |

## Valg der skal bevares

**Windows leverer funktionerne.** Eneste eksterne afhængighed er Microsofts `windows`-bindinger:
WIC afkoder JPEG, WinHTTP henter HTTP, GDI tegner og DPAPI krypterer. JSON og MJPEG parses i
huset. Det holder appen under en halv megabyte uden medpakket runtime; begrundelsen står
også i `Cargo.toml`.

**Filterets eneste ændring fra Unity Capture er Husk-navne på de fire delte kerneobjekter:**
mutex, events og mapping. En egen CLSID isolerer kun COM-registreringen; fælles objektnavne
ville sende frames til hinandens klienter og ligne billedflimmer. Ophavsret og MIT-licens
bevares uændret.

**Afinstallér efter pakkelisten.** `byg-installer.ps1` genererer én `Delete` pr. pakket fil
og én `RMDir` pr. mappe i omvendt dybde-orden fra samme filer som `File /r` pakker.
`RMDir /r "$INSTDIR"` har målbart slettet brugerens egne filer og må ikke erstatte listen.

**Bevar konfigurationsmappen `husk-viewer`.** Navnet er arvet; omdøbning taber eksisterende
brugeres opsætning og token. Versionskilden og øvrige redigeringsregler står i [CLAUDE.md](CLAUDE.md).

## Ældre kildekommentarer

»C#-udgaven« er den tidligere .NET-implementering, som ikke er i repoet. Henvisninger forklarer
bevidst kompatibilitet. »måleregel N« er et internt arkivnummer; sætningen foran bærer selve
reglen, så ingen ekstern kilde er nødvendig.

Status for CI og signering: [FORTSÆT-HER.md](FORTSÆT-HER.md).
