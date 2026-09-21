# FORTSÆT HER – Husk Webcam

**2026-09-21:** repoet er nyt; produktet virker. Offentlig byggekæde og signatur mangler.
Udviklingen foregår her. Det tidligere private repo er kun historisk arkiv: dets commits
indeholder interne noter, maskinnavne og private netværksadresser og må ikke følge med.
Her er kun produkt, tests og offentlig dokumentation.

## Næste arbejde, i rækkefølge

1. **Offentlig byggekæde:** automatisér [de tre manuelle byggetrin](docs/byg-fra-kilde.md),
   så binærens forbindelse til kildekoden kan dokumenteres.
2. **Kodesignering:** indsendelse til signeringstjeneste er et menneskes opgave og kræver
   byggekæden først. Installeren er usigneret; [README](README.md) forklarer SmartScreen.

## Uafklaret testdækning

`src/husk-webcam-rs/build.rs` har ingen automatisk test af sky-disk-vagten. Et rigtigt build
har bekræftet at en lokal klon tillades; afvisning af sky-synket kilde med byggetræ på samme
drev er arvet, uændret og ikke genmålt. Et forsøg med separat vagtfil og hermetiske tests
blev rullet tilbage, fordi Smart App Control blokerede testbinærerne. Prøv på en maskine uden
håndhævelse; slå ikke beskyttelsen fra. [Byggevejledningen](docs/byg-fra-kilde.md) beskriver
blokeringen og scriptets fallback.
