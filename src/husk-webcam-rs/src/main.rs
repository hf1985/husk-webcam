//! Indgangen. Port af `Program.cs`.
//!
//! Uden argumenter aabner GUI'en; med et maalings-argument koerer den ÉN maaling og skriver
//! resultatet. Maalings-tilstandene er ikke et separat vaerktoej ved siden af appen - huset har
//! en staaende regel mod to naesten ens kodesteder, og de skal maale PRAECIS den kode der koerer
//! i drift.

use std::path::Path;

use husk_webcam::bitmap::Bitmap32;
use husk_webcam::demand::{SingletonLaas, unity_demand};
use husk_webcam::gui;
use husk_webcam::unity_sender::{
    HUSK_PRAEFIKS, INGEN_MODTAGER_FORKLARING, SendResultat, UnityCaptureSender,
};
use husk_webcam::wic;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kode = if args.is_empty() {
        gui::start(false)
    } else {
        match args[0].as_str() {
            "--maal-efterspoergsel" => maal_efterspoergsel(&args),
            "--send-testmoenster" => send_testmoenster(&args),
            "--vindue-roegtjek" => gui::roegtjek(&ud_sti(&args, "husk-gui-roegtjek.png")),
            "--bakke-roegtjek" => gui::bakke_roegtjek(),
            // Autostarten ved login bruger denne: kun bakke-ikon, intet vindue.
            "--bakke" => gui::start(true),
            "--maal-stroem" => maal_stroem(&args),
            "--afkod-jpeg" => afkod_jpeg(&args),
            "--lav-testjpeg" => lav_testjpeg(&args),
            "--sammenlign-raa" => sammenlign_raa(&args),
            "--hjaelp" | "-h" | "--help" => hjaelp(),
            andet => ukendt(andet),
        }
    };
    std::process::exit(kode);
}

fn hjaelp() -> i32 {
    println!("Husk Webcam");
    println!("  (uden argumenter)         aabn vinduet + bakke-ikon");
    println!("  --bakke                   kun bakke-ikon (autostart ved login)");
    println!("  --maal-efterspoergsel     hvor mange DirectShow-modtagere ser med?");
    println!("  --send-testmoenster [n]   send n asymmetriske testframes (default 300)");
    println!("  --vindue-roegtjek <png>   byg vinduet, TEGN det, og gem et billede");
    println!("  --bakke-roegtjek          bakkens fire kommandoer + ikon-ressourcen");
    println!("  --maal-stroem <url> [n]   hent n hele frames og AFKOD den nyeste");
    println!("  --afkod-jpeg <jpg> <bin>  afkod med WIC og dump raa BGRA (afkoder-proeve)");
    println!("  --lav-testjpeg <jpg> [b h]  skriv fikstur-billedet til afkoder-proeven");
    println!("  --sammenlign-raa <a> <b>  sammenlign to raa BGRA-dumps pixel for pixel");
    0
}

fn ukendt(arg: &str) -> i32 {
    eprintln!("ukendt argument: {arg}");
    hjaelp();
    2
}

fn ud_sti(args: &[String], standard: &str) -> std::path::PathBuf {
    match args.get(1) {
        Some(s) => std::path::PathBuf::from(s),
        None => std::env::temp_dir().join(standard),
    }
}

fn praefiks_fra(args: &[String]) -> String {
    for a in args {
        if let Some(v) = a.strip_prefix("--praefiks=") {
            return v.to_string();
        }
    }
    HUSK_PRAEFIKS.to_string()
}

fn maal_efterspoergsel(args: &[String]) -> i32 {
    let navn = format!("{}_Mutx", praefiks_fra(args));
    let t = unity_demand::tael(&navn);
    if !t.maalt {
        // En umaalelig tilstand maa ALDRIG melde 0: kalderen ville vaelge den forkerte
        // fejlretning (maaleregel 116).
        eprintln!("KUNNE IKKE MAALE: {}", t.fejl.unwrap_or_default());
        return 3;
    }
    println!("modtagere={}", t.antal);
    0
}

/// Sender et ASYMMETRISK moenster, saa en modtager kan afsloere baade en ombyttet farvekanal og
/// et vendt billede. Et graat eller symmetrisk billede ville bestaa uanset hvad (maaleregel 8).
///
/// Moensteret: RENT ROEDT felt oeverst til VENSTRE, rent groent oeverst til hoejre, rent blaat
/// nederst til venstre, hvidt nederst til hoejre. Fire entydige svar i ét billede.
fn send_testmoenster(args: &[String]) -> i32 {
    let mut antal: u32 = 300;
    if let Some(n) = args.get(1).and_then(|s| s.parse::<u32>().ok()) {
        if n > 0 {
            antal = n;
        }
    }

    let Some(_laas) = SingletonLaas::tag_standard() else {
        eprintln!("En anden Husk Webcam-sender koerer allerede.");
        return 4;
    };

    // Praefikset kan vaelges, saa BEGGE filtre kan maales fra samme binaer: husets eget
    // (default) og et umodificeret Unity Capture. Uden det ville en maaling af det ene ikke
    // kunne skelnes fra en maaling af det andet.
    let praefiks = praefiks_fra(args);
    let Some(mut sender) = UnityCaptureSender::ny(0, &praefiks) else {
        eprintln!("ugyldigt praefiks: {praefiks}");
        return 2;
    };
    println!("sender til: {}", sender.mutex_navn());

    let moenster = testmoenster(1280, 720);
    let (mut ok, mut frameskip, mut ingen, mut for_stor) = (0u32, 0u32, 0u32, 0u32);
    for _ in 0..antal {
        match sender.send(&moenster, false, 1000) {
            SendResultat::Ok => ok += 1,
            SendResultat::Frameskip => frameskip += 1,
            SendResultat::ForStor => for_stor += 1,
            SendResultat::IngenModtager => ingen += 1,
        }
        std::thread::sleep(std::time::Duration::from_millis(33)); // ca. 30 fps
    }

    println!("ok={ok} frameskip={frameskip} ingen-modtager={ingen} for-stor={for_stor}");
    if ok + frameskip == 0 {
        eprintln!("{INGEN_MODTAGER_FORKLARING}");
        return 5;
    }
    0
}

pub(crate) fn testmoenster(b: usize, h: usize) -> Bitmap32 {
    let mut bm = Bitmap32::new(b, h);
    for y in 0..h {
        for x in 0..b {
            let venstre = x < b / 2;
            let oeverst = y < h / 2;
            let (bb, gg, rr) = match (venstre, oeverst) {
                (true, true) => (0u8, 0u8, 255u8),   // roed, oeverst venstre
                (false, true) => (0, 255, 0),        // groen, oeverst hoejre
                (true, false) => (255, 0, 0),        // blaa, nederst venstre
                _ => (255, 255, 255),                // hvid, nederst hoejre
            };
            bm.saet_pixel(x, y, bb, gg, rr, 255);
        }
    }
    bm
}

/// Hent N hele frames fra en MJPEG-kilde, og AFKOD den nyeste.
///
/// ⛔ Begge halvdele er noedvendige. En frame der kun LIGNER et JPEG er ikke en frame, og en
/// taelling alene kan ikke skelne de to. Tilstanden bruges baade mod en rigtig telefon og mod
/// en UAFHAENGIG fikstur-server - altsaa en anden end crate'ens egen `testfikstur`, saa et
/// groent ben ikke kan skyldes at begge sider deler den samme fejl.
fn maal_stroem(args: &[String]) -> i32 {
    let Some(url) = args.get(1) else {
        eprintln!("brug: --maal-stroem <url> [antal] [sekunder]");
        return 2;
    };
    let antal: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let frist = std::time::Duration::from_secs(
        args.get(3).and_then(|s| s.parse().ok()).unwrap_or(25),
    );

    let mut kilde = husk_webcam::mjpeg_source::MjpegSource::ny("maaling", url);
    kilde.start();

    let start = std::time::Instant::now();
    while start.elapsed() < frist && kilde.frame_taeller() < antal {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let taeller = kilde.frame_taeller();
    let status = kilde.status();
    let nyeste = kilde.nyeste();
    let fejl = kilde.sidste_fejl();
    kilde.stop();

    let Some(f) = nyeste else {
        println!("frames={taeller} status={status:?} afkodet=INGEN");
        if let Some(e) = fejl {
            eprintln!("sidste fejl: {e}");
        }
        // Redigeret: URL'en kan baere ?token=. Den staar ganske vist allerede i argv i netop
        // denne tilstand, men secret.rs lover at hemmeligheden aldrig naar en log, og et
        // loefte der kun holder de fleste steder, er ikke et loefte.
        let vist = husk_webcam::url_redactor::hide_secrets(Some(url), Some(url));
        eprintln!("der kom ingen frame fra {vist}");
        return 6;
    };

    if !husk_webcam::mjpeg_parser::MjpegParser::er_helt_jpeg(&f.data) {
        println!("frames={taeller} status={status:?} afkodet=IKKE-ET-JPEG");
        eprintln!("den leverede frame er ikke et helt JPEG (FF D8 FF .. FF D9)");
        return 6;
    }

    let Some(bm) = wic::afkod_jpeg(&f.data) else {
        println!("frames={taeller} status={status:?} afkodet=KUNNE-IKKE-AFKODES");
        eprintln!("framen ligner et JPEG, men WIC kunne ikke afkode den");
        return 6;
    };

    println!(
        "frames={taeller} status={status:?} afkodet={}x{} bytes={}",
        bm.bredde(),
        bm.hoejde(),
        f.data.len()
    );
    if taeller < antal {
        eprintln!("kun {taeller} af {antal} frames inden for fristen");
        return 6;
    }
    0
}

/// Afkod et JPEG med WIC og dump den raa BGRA. Halvdelen af afkoder-proeven.
fn afkod_jpeg(args: &[String]) -> i32 {
    let (Some(ind), Some(ud)) = (args.get(1), args.get(2)) else {
        eprintln!("brug: --afkod-jpeg <ind.jpg> <ud.bin>");
        return 2;
    };
    let Some(bm) = wic::afkod_jpeg_fil(Path::new(ind)) else {
        eprintln!("kunne ikke afkode {ind}");
        return 6;
    };
    if let Err(e) = wic::skriv_raa_bgra(&bm, Path::new(ud)) {
        eprintln!("kunne ikke skrive {ud}: {e}");
        return 7;
    }
    println!("afkodet={}x{} bytes={} fil={ud}", bm.bredde(), bm.hoejde(), bm.pixels().len());
    0
}

/// Skriv fikstur-billedet til afkoder-proeven.
///
/// ⛔ Produktet koder ALDRIG JPEG i drift - det laeser dem. Tilstanden findes udelukkende for
/// at kunne lave et FAST testbillede med Windows' egen encoder, saa proeven ikke afhaenger af
/// et billede ingen kan genskabe.
fn lav_testjpeg(args: &[String]) -> i32 {
    let Some(ud) = args.get(1) else {
        eprintln!("brug: --lav-testjpeg <ud.jpg> [bredde hoejde]");
        return 2;
    };
    let b = args.get(2).and_then(|s| s.parse::<usize>().ok()).unwrap_or(640);
    let h = args.get(3).and_then(|s| s.parse::<usize>().ok()).unwrap_or(480);

    let bm = fikstur_moenster(b, h);
    match wic::kod_jpeg(&bm) {
        Ok(data) => match std::fs::write(ud, &data) {
            Ok(()) => {
                println!("skrevet={ud} bytes={} stoerrelse={b}x{h}", data.len());
                0
            }
            Err(e) => {
                eprintln!("kunne ikke skrive {ud}: {e}");
                7
            }
        },
        Err(e) => {
            eprintln!("WIC kunne ikke kode: {e}");
            6
        }
    }
}

/// Fikstur-moensteret: de fire rene felter PLUS en gradient og en finere skakternet stribe.
///
/// ⛔ De fire felter alene ville vaere et DAARLIGT proeve-billede: store ensfarvede flader er
/// netop dem to JPEG-afkodere er mest enige om. Gradienten og det fine moenster tvinger
/// afkoderne ud i chroma-opsampling og DCT-afrunding, hvor en forskel FAKTISK ville vise sig.
fn fikstur_moenster(b: usize, h: usize) -> Bitmap32 {
    let mut bm = testmoenster(b, h);
    for y in 0..h {
        for x in 0..b {
            // Nederste tredjedel: en vandret gradient med et fint skakternet moenster i.
            if y >= h * 2 / 3 {
                let g = ((x * 255) / b.max(1)) as u8;
                let ternet = if (x / 3 + y / 3) % 2 == 0 { 40 } else { 0 };
                bm.saet_pixel(x, y, g.saturating_add(ternet), g, 255 - g, 255);
            } else if y >= h / 3 && y < h * 2 / 3 {
                // Midterbaandet: lodrette striber paa én pixel - det haardeste for chroma.
                let v = if x % 2 == 0 { 255 } else { 0 };
                bm.saet_pixel(x, y, v, 255 - v, v / 2, 255);
            }
        }
    }
    bm
}

/// Sammenlign to raa BGRA-dumps pixel for pixel.
///
/// ⛔ DOMMEN LIGGER I KODEN, ikke i et script eller i et menneskes aflaesning af to tal
/// (maaleregel 59). Exitkoden er 0 KUN ved nul afvigende pixels.
fn sammenlign_raa(args: &[String]) -> i32 {
    let (Some(a), Some(b)) = (args.get(1), args.get(2)) else {
        eprintln!("brug: --sammenlign-raa <a.bin> <b.bin>");
        return 2;
    };

    let (Ok(da), Ok(db)) = (std::fs::read(a), std::fs::read(b)) else {
        eprintln!("kunne ikke laese begge filer");
        return 7;
    };

    match wic::sammenlign_raa_bgra(&da, &db) {
        wic::Sammenligning::Ulaeselig(grund) => {
            println!("ULAESELIG: {grund}");
            7
        }
        wic::Sammenligning::ForskelligStoerrelse { a, b } => {
            println!("FORSKELLIG STOERRELSE a={}x{} b={}x{}", a.0, a.1, b.0, b.1);
            8
        }
        wic::Sammenligning::Maalt { bredde, hoejde, afvigende, stoerste_kanalforskel } => {
            println!(
                "stoerrelse={bredde}x{hoejde} samlet={} afvigende-pixels={afvigende} \
                 stoerste-kanalforskel={stoerste_kanalforskel}",
                bredde as u64 * hoejde as u64
            );
            if afvigende == 0 { 0 } else { 9 }
        }
    }
}
