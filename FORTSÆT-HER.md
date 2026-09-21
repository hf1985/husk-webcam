# FORTSÆT HER – Husk Webcam

**2026-09-21, anden runde:** den offentlige byggekæde er live og grøn.
`.github/workflows/byg.yml` kører de tre byggetrin fra [byggevejledningen](docs/byg-fra-kilde.md) på hvert push, og den lægger en herkomst-attest på artefakterne.
Kæden er beskrevet i vejledningen; her står kun hvad den har MÅLT, og hvad der stadig mangler.

⚠️ **`v0.1.0` er stadig en håndbygget udgivelse.**
Dens binær er bygget af commit `c473338`, mens taggen peger på `94a2551`, og den bærer ingen attest.
Den næste udgivelse skal skæres af kæden: et tag `vX.Y.Z` kræver at taggen, `Cargo.toml` og `res/husk.rc` er enige, og kørslen lægger så installeren op med SHA256 og attest-vejledning.

## Det kæden fandt på sin første kørsel

`src/unity_sender.rs` manglede et komma i `concat!`-blokken bag `INGEN_MODTAGER_FORKLARING`, så **crate'en har ikke kunnet kompilere siden `94a2551`**.
Det er præcis den fejl forrige handoff advarede om: den commit ændrede to brugerrettede strenge ubygget, og »de to strenge er det første der skal ses efter«.
Den ene var i stykker. Den er rettet, og begge er nu kompileret og med i en grøn suite.

Værd at holde fast i som begrundelse for kæden: målt 2026-09-21 står Smart App Control i håndhævelse (`VerifiedAndReputablePolicyState = 1`) på udviklermaskinen, og cargo er slet ikke installeret på den.
**Ingen af de to maskiner projektet udvikles på kan i dag bygge produktet.**
Uden en byggekæde uden for huset ville fejlen først være dukket op ved næste forsøg på at udgive.

## Målt på commit `b191820`, kørsel grøn i begge job

| hvad | målt |
|---|---|
| Rust-suiten | 235 bestået, 0 fejlet (207 + 13 + 15 i tre mål) |
| `HuskWebcam.exe` | 451.072 bytes, ingen `VCRUNTIME140`-import |
| `HuskWebcamFilter64.dll` / `32` | 207.360 / 185.344 bytes |
| Installer | `HuskWebcam-0.1.0-setup.exe`, 438.100 bytes |
| Vinduets røgtjek | tegnet og gemt, 39.921 bytes PNG |
| Byggemiljø | `win25-vs2026`, rustc og cargo 1.98.1, makensis v3.12 |
| Sky-drev-vagten | begge retninger som beskrevet: afviser samme drev (exit 101, med vagtens egen begrundelse), tillader et andet |

Herkomst-attesten er efterprøvet fra en udviklermaskine i begge retninger: den hentede installer giver `gh attestation verify` exit 0, og en kopi med ét byte vendt giver exit 1 med `HTTP 404` på sin sum.
SHA256 for alle fire artefakter overlevede turen gennem GitHubs artefakt-lager uændret.

⚠️ **Buildet er ikke bit-reproducerbart, og det er målt frem for antaget.**
To kørsler hvor den eneste forskel var `.md`-filer gav fire FORSKELLIGE SHA256, og installeren skiftede endda størrelse (438.100 mod 438.214 bytes), mens exe og DLL'er beholdt deres.
Det betyder at »samme kilde« ikke kan efterprøves ved at bygge igen og sammenligne summer.
Det er netop derfor forbindelsen binær til kilde hviler på **attesten**, ikke på en sum.

## Produktprøve på en rigtig maskine, 2026-09-21

Installeren fra kæden er kørt på en udviklermaskine med Windows 11, og produktet er prøvet ende til ende.
Det er **ikke** den rene VM-prøve punkt 2 nedenfor efterlyser – maskinen havde i forvejen en umodificeret Unity Capture installeret – men alt andet end »ren maskine« er nu målt frem for arvet.

- **Installation:** `/S`, exit 0. Fire filer i `C:\Program Files\Husk Webcam`, afinstallations-nøgle med korrekt version og `EstimatedSize`, start-menu-gruppe og autostart-genvej. Exe'ens egenskabsdialog viser `0.1.0.0`, altså det versions-gaten måler.
- **Registrering:** `Husk Webcam` står som DirectShow-kamera i **både** 64- og 32-bit-viewet, og begge CLSID'er peger ind i app-mappen. Den eksisterende Unity Capture er urørt; de to filtre deler ingen CLSID.
- **Ende til ende:** en fremmed modtager åbnede kameraet, `--maal-efterspoergsel` gik til 1, og `--send-testmoenster` leverede `ok=238 frameskip=1 ingen-modtager=1 for-stor=0`. Billederne kom ud af kameraet.
- **Uden modtager** svarer senderen `ingen-modtager` for hver frame og exit 5 med hele `INGEN_MODTAGER_FORKLARING` – altså netop den streng der ikke kunne kompilere for en time siden.
- **Afinstallation, målt med kanariefiler:** to fremmede filer blev lagt i app-mappen før `Uninstall.exe /S`. Bagefter var produktets egne fire filer, begge CLSID-registreringer, afinstallations-nøglen og begge genveje væk, mens **begge kanarier og mappen selv stod tilbage**. `.nsi`-filens begrundelse for at afvise `RMDir /r` holder altså på den færdige installer, ikke kun på proben. Maskinen er geninstalleret bagefter.
- **Smart App Control blokerede hverken installeren eller appen**, selv om den står i håndhævelse på maskinen og afviser nybyggede binærer dér. Maskinen kan altså køre produktet fra kæden, men ikke bygge det selv.
- **Med en rigtig telefon:** `--maal-stroem` mod en Husk-telefon på et privat net gav `frames=10 status=Live afkodet=1280x720`, og efter ét `Start kamera` leverede kameraet levende 1920x1080-billeder til en fremmed modtager. Opsætningen blev skrevet som `config.json` med tokenet DPAPI-beskyttet under entropien i `secret.rs`, og appen læste den. Hele vejen fra telefon til kameraenhed er altså målt, ikke arvet.

⚠️ **VLC viser billedet forkert, og det er VLC's fejl – ikke produktets.**
I VLC 3.0.23 kommer billedet lodret vendt og med rød og blå byttet. `ffmpeg` på samme kamera og samme frames viser det **rigtigt**: rød, grøn foroven og blå, hvid forneden, præcis som `main.rs::testmoenster` skriver det.
Tre ting peger samme vej: filteret melder `MEDIASUBTYPE_ARGB32` med positiv `biHeight`, altså bund-op, og konverterer selv RGBA til BGRA; filterets **eget** indbyggede »ingen frames«-billede kommer også vendt ud i VLC, og det billede rører senderen aldrig; og den **umodificerede** Unity Capture giver nøjagtig samme forkerte resultat i VLC.
Det er værd at have i baghovedet, fordi VLC er det første en nysgerrig bruger griber efter.

## Næste arbejde, i rækkefølge

1. **Skær en udgivelse med kæden.** Bump versionen i `Cargo.toml` **og** `res/husk.rc` (versions-gaten måler begge), sæt taggen, og lad kørslen udgive. Det giver den første installer hvis forbindelse til kilden kan efterprøves udefra.
2. **Kodesignering.** Et menneskes opgave: indsendelse til en signeringstjeneste. Byggekæden, som var forudsætningen, er nu på plads. Installeren er indtil da usigneret, og [README](README.md) forklarer SmartScreen.
3. **De fire målte fund nedenfor.** Grunden til at de stod urørte er væk.

## ⛔ Appen begynder ikke at sende af sig selv (målt 2026-09-21)

Autostart-genvejen starter appen med `--bakke`, og dér **bliver den stående uden at sende**, også når et program har kameraet åbent.
`start_stop()` kaldes kun fra tre steder, og alle tre er en brugerhandling: knappen **Start kamera**, bakkens **Test kamera** og bakkens **Fortsæt**.
Intet kalder den ved opstart, og `koerer` gemmes ikke i `config.json`, så den er falsk igen ved hvert login.

Målt mod en rigtig telefon med en fremmed DirectShow-modtager åben hele vejen igennem:

| | billeder | forskellige størrelser |
|---|---:|---:|
| appen urørt i bakken | 8 | 1 (filterets »ingen frames«-plakat) |
| efter ét `Start kamera` | 16 | 16 (levende billede, 1920x1080) |

Det betyder at brugeren skal ind i bakken **én gang pr. login** før kameraet leverer noget.
Det er værd at kende, fordi forgængeren gjorde det modsatte: den gamle Python-vagt ventede selv på at et program åbnede kameraet, og startede først da telefonens stream.
Om det skal laves om er et produktvalg – enten start ved opstart når der er en telefon opsat, eller lad efterspørgsels-vagten arme sig selv – men som det står nu, vil et møde uden det ene klik vise plakaten.

## Målte fund der endnu ikke er rettet

En adversarisk gennemgang af hele træet 2026-09-21 fandt fire ting, efterprøvet på disken.
De stod urørte fordi en rettelse ville ændre adfærd, og fordi maskinen der fandt dem ikke kunne bygge produktet.
**Den grund gælder ikke længere:** kæden bygger og kører suiten på hvert push, så en adfærdsændring kan nu måles frem for at blive gættet.

- **`gui.rs` melder succes på en verifikation der ikke kunne køre.** Doc-kommentaren ved kameraside-skiftet lover at sige det hvis `/flags` ikke kunne læses, men koden returnerer en tom streng når `Forbindelsestjek::ny()` fejler, og UI'et læser tom streng som »intet at melde«. Et uverificeret 200 meldes altså som succes. Fail-open i præcis den vagt kommentaren beskriver.
- **`demand.rs` henviser til sig selv for et fratræk den ikke laver.** Fratrækket af appens eget handle sker i `gui.rs::opdater_efterspoergsel`, ikke i `DemandMonitor`.
- **`json.rs` siger »under 300 linjer« om en fil på omkring 329 kodelinjer.**
- **`unity_sender.rs` skriver »de 17 tegn«** om et præfiks-loft der kun gælder `UnityCapture`-præfikset; projektets eget giver 15.

Den anden ubyggede streng fra `94a2551` – statuslinjens » · preview tæller med«, gjort betinget – kompilerer nu og er med i den grønne suite.
Det siger at den oversættes, ikke at teksten står rigtigt i alle tilstande; ingen test dømmer den.

## Uafklaret testdækning

Her stod tre ting. To af dem er nu målt, og det tredje er stadig arvet.

1. ~~Rust-suiten er ikke kørt på den udgivne kode.~~ **Lukket:** suiten kører på hvert push, 235 bestået.
2. **Produktprøven er nu kørt på kædens egen installer** (se afsnittet ovenfor): installation, registrering i begge views, frames gennem kameraet til en fremmed modtager, og afinstallation målt med kanariefiler. **Det der stadig mangler, er en REN maskine**: prøven kørte på en udviklermaskine der i forvejen havde Unity Capture installeret, så den kan ikke svare på om en maskine uden noget Unity Capture-slægtskab opfører sig ens. Kæden selv installerer ikke noget; den tegner kun vinduet.
3. ~~Sky-disk-vagtens afvis-retning er ukørt.~~ **Lukket:** `scripts/proev-skyvagt.ps1` kører den mod en falsk Drive-montering, og begge retninger er grønne. Prøven dømmer på vagtens egen begrundelse frem for på exitkoden, så et build der falder af en anden grund ikke kan ligne en vagt der virker.
