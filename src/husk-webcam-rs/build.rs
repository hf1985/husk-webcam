//! Byggeskridt: ikon, manifest og versionsblok ind i exe'en, plus vagten mod at bygge paa Drive.
//!
//! Begge dele er FAIL-CLOSED med vilje. En manglende ressource-compiler ville ellers give en
//! exe uden ikon og uden manifest, hvor manifestet baerer DPI-bevidstheden og
//! `asInvoker`-niveauet - og en tavs nedgradering af begge dele ligner en app der bare ser
//! lidt forkert ud (maaleregel 128).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());

    vagt_mod_byg_i_kildetraeet(&manifest, &out);

    let res_dir = manifest.join("res");
    let rc = res_dir.join("husk.rc");
    println!("cargo:rerun-if-changed={}", rc.display());
    println!("cargo:rerun-if-changed={}", res_dir.join("husk.ico").display());
    println!("cargo:rerun-if-changed={}", res_dir.join("app.manifest").display());

    let res = out.join("husk.res");
    let rc_exe = find_rc().unwrap_or_else(|| {
        panic!(
            "rc.exe (Windows SDK's ressource-compiler) blev ikke fundet.\n\
             Den foelger med Visual Studio Build Tools + Windows SDK, som MSVC-toolchainen\n\
             allerede kraever. Installér 'Desktop development with C++' med Windows SDK,\n\
             eller peg paa den med HUSKWEBCAM_RC=<sti til rc.exe>."
        )
    });

    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg(&rc)
        .current_dir(&res_dir)
        .status()
        .unwrap_or_else(|e| panic!("kunne ikke koere {}: {e}", rc_exe.display()));

    if !status.success() {
        panic!("rc.exe fejlede med {status}");
    }

    // Doem paa ARTEFAKTET, ikke paa exitkoden alene (maaleregel 3).
    if !res.is_file() {
        panic!("rc.exe meldte OK, men {} findes ikke", res.display());
    }

    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// ⛔ Ligger KILDEN paa et sky-synket drev, maa BYGGET aldrig goere det.
///
/// En sky-sync korrumperer de hoej-churn-skrivninger et build laver, og en `.gitignore`
/// stopper git - ikke syncen. Kuren er `CARGO_TARGET_DIR`, og vagten er at vi naegter at
/// bygge uden den.
///
/// ⛔ VAGTEN ER BETINGET, og det er et krav frem for en venlighed: en almindelig klon paa
/// `C:` har kilde og byggetrae paa SAMME rod, og en ubetinget vagt ville naegte at bygge for
/// enhver der ikke har kilden liggende i en sky-mappe. Betingelsen maales paa Google Drives
/// egen markoer `.shortcut-targets-by-id`; drev-typen kan ikke bruges, for Drive melder sig
/// som en almindelig fast disk.
///
/// ⛔ OG VAGTEN MAALER DREVET, IKKE STI-INDEHOLDELSE. Her stod engang alene
/// `out.starts_with(manifest)`, altsaa »ligger byggetraeet inde i crate-mappen?«. Den fangede
/// det almindelige `target/`, men slap et `CARGO_TARGET_DIR` der peger et ANDET sted paa
/// samme sky-drev igennem - fx repo-roden. Det er lige praecis den tilstand vagten findes
/// for at hindre, og den saa gyldig ud. Nu er kravet at byggetraeet ligger paa et ANDET
/// drev/rod end kilden.
fn vagt_mod_byg_i_kildetraeet(manifest: &Path, out: &Path) {
    if !paa_sky_drev(manifest) {
        return;
    }
    let i_kildetraeet = out.starts_with(manifest);
    let samme_rod = match (rod_af(manifest), rod_af(out)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        // Kan roden ikke udledes, doemmer vi ikke paa den - kun paa indeholdelsen.
        _ => false,
    };
    if !i_kildetraeet && !samme_rod {
        return;
    }

    let grund = if i_kildetraeet {
        "byggetraeet ligger INDE i kildetraeet"
    } else {
        "byggetraeet ligger paa SAMME drev som kilden (sky-drevet)"
    };
    panic!(
        "{} ({}).\n\
         Kildetraeet ligger i en Google Drive-montering, og syncen korrumperer de hoej-\n\
         churn-skrivninger et build laver. Saet CARGO_TARGET_DIR til en sti paa lokal disk:\n\
        \n\
             $env:CARGO_TARGET_DIR = \"$env:LOCALAPPDATA\\husk-webcam-build\\rust\"\n\
        \n\
         eller brug scripts\\byg-rust.ps1, som goer det for dig.\n\
         Klon i stedet repoet til en lokal disk, saa er vagten slet ikke i spil.",
        grund,
        out.display()
    );
}

/// Ligger stien under en Google Drive-montering?
///
/// ⛔ Maalt paa Drives egen markoer `.shortcut-targets-by-id`, som ligger i RODEN af
/// monteringen. Monteringen kan vaere et drev ELLER en mappe, og mappenavnet er
/// lokaliseret (`My Drive` paa en engelsk klient, `Mit drev` paa en dansk), saa hverken
/// drevbogstavet eller navnet kan bruges. Klienten melder desuden monteringen som en
/// almindelig fast disk, saa `GetDriveType` svarer det samme som for `C:`.
fn paa_sky_drev(sti: &Path) -> bool {
    let mut p = Some(sti);
    while let Some(d) = p {
        if d.join(".shortcut-targets-by-id").is_dir() {
            return true;
        }
        p = d.parent();
    }
    false
}

/// Drev-praefikset (`G:`, `C:`) eller UNC-servernavnet for en sti.
///
/// Returnerer `None` for en sti uden praefiks, saa kalderen kan skelne »en anden rod« fra
/// »kunne ikke maales« (maaleregel 116).
fn rod_af(p: &Path) -> Option<String> {
    use std::path::{Component, Prefix};
    match p.components().next() {
        Some(Component::Prefix(pre)) => match pre.kind() {
            Prefix::Disk(b) | Prefix::VerbatimDisk(b) => Some((b as char).to_string()),
            Prefix::UNC(s, _) | Prefix::VerbatimUNC(s, _) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        },
        _ => None,
    }
}

/// Find `rc.exe` i Windows SDK.
///
/// ⛔ Stien UDLEDES, den hardkodes ikke. SDK-versionen skifter med hver opdatering, og
/// `winget`/VS-installeren kan lande i baade `Program Files` og `Program Files (x86)` -
/// en hardkodet rod er en tavs no-op frem for en fejl.
fn find_rc() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("HUSKWEBCAM_RC") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }

    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let mut kandidater: Vec<(String, PathBuf)> = Vec::new();

    for rod in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Some(base) = std::env::var_os(rod) else { continue };
        let bin = PathBuf::from(base).join("Windows Kits").join("10").join("bin");
        let Ok(laes) = std::fs::read_dir(&bin) else { continue };
        for e in laes.flatten() {
            let ver = e.file_name().to_string_lossy().into_owned();
            // Kun versionsmapper (10.0.x.y), ikke de bare arch-mapper.
            if !ver.starts_with("10.") {
                continue;
            }
            let rc = e.path().join(arch).join("rc.exe");
            if rc.is_file() {
                kandidater.push((ver, rc));
            }
        }
    }

    // Nyeste SDK vinder. Sorteringen er numerisk pr. led, ikke leksikografisk: "10.0.9" ville
    // ellers slaa "10.0.26100".
    kandidater.sort_by_key(|(v, _)| {
        let mut n = [0u64; 4];
        for (i, d) in v.split('.').take(4).enumerate() {
            n[i] = d.parse().unwrap_or(0);
        }
        n
    });
    kandidater.pop().map(|(_, p)| p)
}
