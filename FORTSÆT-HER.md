# FORTSÆT HER – Husk Webcam

**2026-09-21:** repoet er nyt og offentligt, og `v0.1.0` er udgivet med installeren som asset
(438.066 bytes, SHA256 `3fe2c463…b27c`, efterprøvet ved at hente den ned igen fra dens
offentlige URL). Offentlig byggekæde og signatur mangler. »Produktet virker« er arvet fra en
produktprøve på det FORRIGE build – se afsnittet om hvad der ikke er målt på dette træ.
⚠️ Binæren i `v0.1.0` er bygget af commit `c473338`, mens taggen peger på `94a2551`;
forskellen står i udgivelsens tekst.
Udviklingen foregår her. Det tidligere private repo er kun historisk arkiv: dets commits
indeholder interne noter, maskinnavne og private netværksadresser og må ikke følge med.
Her er kun produkt, tests og offentlig dokumentation.

## Næste arbejde, i rækkefølge

1. **Offentlig byggekæde:** automatisér [de tre manuelle byggetrin](docs/byg-fra-kilde.md),
   så binærens forbindelse til kildekoden kan dokumenteres.
2. **Kodesignering:** indsendelse til signeringstjeneste er et menneskes opgave og kræver
   byggekæden først. Installeren er usigneret; [README](README.md) forklarer SmartScreen.

## Målte fund der endnu ikke er rettet

En adversarisk gennemgang af hele træet 2026-09-21 fandt fire ting i koden som er efterprøvet
på disken, men ikke rettet her. Grunden er at en rettelse ville ændre adfærd, og produktet
kunne ikke bygges på den maskine der fandt dem, fordi Smart App Control afviste enhver
nybygget build-script-binær.

⚠️ **Den grund er ikke helt konsekvent, og det skal stå her frem for at blive glattet ud:**
commit `94a2551` ændrer SELV to brugerrettede strenge ubygget – statuslinjens
» · preview tæller med« er gjort betinget, og beskeden når kamera-filteret ikke svarer peger
nu på produktsiden. Begge er inspiceret (`preview_aabent` er `pub bool`), ingen af dem er
kompileret eller kørt. **Første offentlige build skal køre hele suiten før næste tag**, og de
to strenge er det første der skal ses efter.

- **`gui.rs` melder succes på en verifikation der ikke kunne køre.** Doc-kommentaren ved
  kameraside-skiftet lover at sige det hvis `/flags` ikke kunne læses, men koden returnerer
  en tom streng når `Forbindelsestjek::ny()` fejler, og UI'et læser tom streng som »intet at
  melde«. Et uverificeret 200 meldes altså som succes. Fail-open i præcis den vagt
  kommentaren beskriver.
- **`demand.rs` henviser til sig selv for et fratræk den ikke laver.** Fratrækket af appens
  eget handle sker i `gui.rs::opdater_efterspoergsel`, ikke i `DemandMonitor`.
- **`json.rs` siger »under 300 linjer« om en fil på omkring 329 kodelinjer.**
- **`unity_sender.rs` skriver »de 17 tegn«** om et præfiks-loft der kun gælder
  `UnityCapture`-præfikset; projektets eget giver 15.

## Uafklaret testdækning

⛔ **Tre ting er IKKE målt på dette træ, og det skal stå her frem for at blive antaget:**

1. **Rust-suiten er ikke kørt på den udgivne kode.** Smart App Control afviste enhver
   testbinær på byggemaskinen. `v0.1.0`-binæren har bestået `byg-rust.ps1`s kør-prøve og
   intet andet.
2. **Produktprøven i en ren VM er ARVET**, ikke gentaget: den blev kørt på det private
   438.069-byte-build, ikke på det udgivne 438.066.
3. **Sky-disk-vagtens afvis-retning er ukørt.** Her stod at den var »arvet, uændret« – det er
   upræcist: selve dommen er arvet, men den er nu gated bag en NY funktion (`paa_sky_drev`),
   hvis sand-gren aldrig har kørt. Logikken er inspiceret og markøren ligger målt i
   `G:\.shortcut-targets-by-id`, altså i drev-roden, så løkken skal helt derop. Det er
   sandsynligt korrekt og det er ikke bevist.

`src/husk-webcam-rs/build.rs` har derfor ingen automatisk test af sky-disk-vagten. Et rigtigt
build har bekræftet at en lokal klon tillades. Et forsøg med separat vagtfil og hermetiske tests
blev rullet tilbage, fordi Smart App Control blokerede testbinærerne. Prøv på en maskine uden
håndhævelse; slå ikke beskyttelsen fra. [Byggevejledningen](docs/byg-fra-kilde.md) beskriver
blokeringen og scriptets fallback.
