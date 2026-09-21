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

## Næste arbejde, i rækkefølge

1. **Skær en udgivelse med kæden.** Bump versionen i `Cargo.toml` **og** `res/husk.rc` (versions-gaten måler begge), sæt taggen, og lad kørslen udgive. Det giver den første installer hvis forbindelse til kilden kan efterprøves udefra.
2. **Kodesignering.** Et menneskes opgave: indsendelse til en signeringstjeneste. Byggekæden, som var forudsætningen, er nu på plads. Installeren er indtil da usigneret, og [README](README.md) forklarer SmartScreen.
3. **De fire målte fund nedenfor.** Grunden til at de stod urørte er væk.

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
2. **Produktprøven i en ren VM er stadig ARVET**, ikke gentaget: den blev kørt på et privat 438.069-byte-build. Kæden tegner vinduet i et rent miljø på hvert push, men den **installerer ikke** installeren, registrerer ikke filteret og åbner ikke kameraet i et mødeprogram. Den prøve kræver et menneske og en VM.
3. ~~Sky-disk-vagtens afvis-retning er ukørt.~~ **Lukket:** `scripts/proev-skyvagt.ps1` kører den mod en falsk Drive-montering, og begge retninger er grønne. Prøven dømmer på vagtens egen begrundelse frem for på exitkoden, så et build der falder af en anden grund ikke kan ligne en vagt der virker.
