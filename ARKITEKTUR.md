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

**Windows leverer funktionerne.** Eneste DIREKTE afhængighed er Microsofts `windows`-bindinger:
WIC afkoder JPEG, WinHTTP henter HTTP, GDI tegner og DPAPI krypterer. JSON og MJPEG parses i
projektet selv. Det holder appen under en halv megabyte uden medpakket runtime; begrundelsen
står også i `Cargo.toml`. (`Cargo.lock` har **16** poster: crate'en selv plus 15
afhængigheder, som alle er `windows`-familien eller de proc-makroer den selv trækker ind.
Målt med `grep -c '^\[\[package\]\]'`.)

**Filteret er ændret tre steder i forhold til Unity Capture**, alle tre navngivning:
enhedens navn og de fire CLSID'er i `HuskFilter.cpp`, og de fire delte kerneobjekter i
`shared.inl`. En egen CLSID isolerer kun COM-registreringen; delte de to filtre stadig mutex,
events og mapping, ville de sende frames til hinandens klienter og ligne billedflimmer.
Ophavsret og licenser bevares uændret. ⚠️ **Mappen har TO tredjeparter, ikke én:** ud over
Unity Capture er `streams.h` og `streams.cpp` Microsofts DirectShow-baseklasser, og de er
92,5 % af kildelinjerne dér. Begge er MIT; se
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

**Afinstallér efter pakkelisten.** `byg-installer.ps1` genererer én `Delete` pr. pakket fil
og én `RMDir` pr. mappe i omvendt dybde-orden fra samme filer som `File /r` pakker.
`RMDir /r "$INSTDIR"` har målbart slettet brugerens egne filer og må ikke erstatte listen.
⚠️ De tre navne der IKKE kommer fra pakkelisten – begge filter-DLL'er og `Uninstall.exe` –
står hardkodet både i `byg-installer.ps1` og i `husk-webcam.nsi`. Netop de tre KAN drive fra
hinanden; ændrer du et af dem, så ret begge steder.

**Bevar konfigurationsmappen `husk-viewer`.** Navnet er arvet; omdøbning taber eksisterende
brugeres opsætning og token. Versionskilden og øvrige redigeringsregler står i [CLAUDE.md](CLAUDE.md).

## Ældre kildekommentarer

Tre slags henvisninger i koden peger på ting der ikke er i repoet, og det er med vilje:

- **»C#-udgaven«** er den tidligere .NET-implementering. Henvisninger til den forklarer
  hvorfor et feltnavn, en værdi eller en adfærd er bevaret frem for forbedret.
- **»Python-vejen«, »Python-udgaven«** er implementeringen før den igen. Samme rolle.
- **»måleregel N«** er et internt arkivnummer. Sætningen foran nummeret bærer selve reglen,
  så ingen ekstern kilde er nødvendig for at forstå den.

Status for CI og signering: [FORTSÆT-HER.md](FORTSÆT-HER.md).
