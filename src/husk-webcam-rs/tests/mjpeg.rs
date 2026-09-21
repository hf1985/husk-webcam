#![allow(non_snake_case)]

//! Integrationsben for MJPEG-vejen: WinHTTP + parser + [`MjpegSource`], maalt mod en LEVENDE
//! HTTP-server frem for en attrap.
//!
//! ⛔ En attrap af [`MjpegSource`] ville dele nul kode med den vej der faktisk koerer, og den
//! ville vaere groen den dag den rigtige gaar i stykker. Fiksturet er en rigtig TCP-server, og
//! hele kaeden - socket, chunked HTTP, multipart-parser, stale-ur og genforbindelse - er den
//! samme som mod en telefon.
//!
//! ⛔ Benene bruger IKKE en telefon, og det er hele pointen: kaeden skal kunne
//! maales paa en maskine uden hardware, og en maaling der kraever et menneske med en telefon
//! kan ikke koere automatisk.

use std::time::{Duration, Instant};

use husk_webcam::mjpeg_parser::MjpegParser;
use husk_webcam::mjpeg_source::MjpegSource;
use husk_webcam::source_status::SourceStatus;
use husk_webcam::testfikstur::{Opsaetning, TestTelefon, standard_frame};
use husk_webcam::winhttp::Session;

/// Vent til betingelsen holder, eller til fristen loeber ud. Returnerer betingelsens sidste vaerdi.
fn vent_paa(mut betingelse: impl FnMut() -> bool, frist: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < frist {
        if betingelse() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    betingelse()
}

/// Laes ÉN hel /stream-forbindelse til EOF og levér de frames parseren fandt.
fn laes_en_stroem(url: &str) -> (u32, Vec<Vec<u8>>) {
    let session = Session::ny("husk-test").expect("WinHttpOpen");
    let mut stroem = session.aabn(url, 5000, 5000).expect("stroemmen kunne ikke aabnes");
    let status = stroem.status;
    let mut parser = MjpegParser::new(stroem.content_type.as_deref());
    let mut frames = Vec::new();
    let mut buf = vec![0u8; 4096];
    loop {
        match stroem.laes(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => frames.extend(parser.feed(&buf[..n])),
        }
    }
    (status, frames)
}

// ---- hele kaeden mod en rigtig HTTP-server ------------------------------------------

#[test]
fn fiksturet_leverer_et_KENDT_antal_rammer_og_parseren_finder_PRAECIS_det_antal() {
    const ANTAL: u32 = 25;
    let frame = standard_frame(7);
    let laengde = frame.len();

    let telefon = TestTelefon::start(Opsaetning {
        fps: 200,
        luk_efter_frames: ANTAL,
        frame: frame.clone(),
        ..Default::default()
    })
    .expect("fikstur-serveren kunne ikke startes");

    let (status, frames) = laes_en_stroem(&telefon.stream_url());

    assert_eq!(status, 200);
    assert_eq!(frames.len(), ANTAL as usize, "forventede praecis {ANTAL} rammer");
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f.len(), laengde, "ramme {i} har forkert laengde");
        assert_eq!(f, &frame, "ramme {i} er ikke den der blev sendt");
        assert!(MjpegParser::er_helt_jpeg(f), "ramme {i} er ikke et helt JPEG");
    }
}

#[test]
fn samme_KENDTE_antal_ogsaa_UDEN_content_length() {
    // Boundary-vejen skal baere sig selv, ikke kun Content-Length-vejen.
    //
    // ⚠️ Uden Content-Length slutter den SIDSTE del foerst ved den naeste boundary, og
    // serveren lukker i stedet. Den sidste ramme kan derfor ikke leveres - det er KORREKT
    // opfoersel, og benet forventer praecis ANTAL-1.
    const ANTAL: u32 = 25;
    let frame = standard_frame(3);

    let telefon = TestTelefon::start(Opsaetning {
        fps: 200,
        luk_efter_frames: ANTAL,
        uden_content_length: true,
        frame: frame.clone(),
        ..Default::default()
    })
    .expect("fikstur-serveren kunne ikke startes");

    let (_, frames) = laes_en_stroem(&telefon.stream_url());

    assert_eq!(
        frames.len(),
        ANTAL as usize - 1,
        "boundary-vejen kan ikke afslutte den sidste del uden en efterfoelgende boundary"
    );
    assert!(frames.iter().all(|f| f == &frame));
}

// ---- MjpegSource mod en levende server ------------------------------------------------

#[test]
fn mindst_ti_HELE_frames_laeses_fra_stroemmen() {
    let telefon = TestTelefon::start(Opsaetning { fps: 100, ..Default::default() }).unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    let naaede = vent_paa(|| kilde.frame_taeller() >= 10, Duration::from_secs(15));
    let taeller = kilde.frame_taeller();
    let nyeste = kilde.nyeste();
    kilde.stop();

    assert!(naaede, "kun {taeller} frames");
    let f = nyeste.expect("der skal ligge en frame");
    assert!(
        MjpegParser::er_helt_jpeg(&f.data),
        "den leverede frame er ikke et helt JPEG (FF D8 FF .. FF D9)"
    );
}

#[test]
fn uden_content_length_laeses_der_OGSAA_frames() {
    let telefon = TestTelefon::start(Opsaetning {
        fps: 100,
        uden_content_length: true,
        ..Default::default()
    })
    .unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    let naaede = vent_paa(|| kilde.frame_taeller() >= 10, Duration::from_secs(15));
    let taeller = kilde.frame_taeller();
    kilde.stop();

    assert!(naaede, "kun {taeller} frames uden Content-Length");
}

#[test]
fn forkert_token_giver_AUTHFEJL_og_ikke_OFFLINE() {
    let telefon = TestTelefon::start(Opsaetning {
        kraevet_token: Some("det-rigtige".into()),
        ..Default::default()
    })
    .unwrap();
    let mut kilde = MjpegSource::ny("test", &format!("{}?token=det-forkerte", telefon.stream_url()));
    kilde.start();

    let naaede = vent_paa(
        || kilde.status() == SourceStatus::AuthError,
        Duration::from_secs(10),
    );
    let status = kilde.status();
    let sidste = kilde.sidste_status();
    kilde.stop();

    assert!(naaede, "tilstand blev {status:?}, ikke AuthError");
    assert_eq!(sidste, Some(401));
    // Og den maa IKKE hedde OFFLINE: de to kraever modsatte handlinger af brugeren.
    assert_ne!(status, SourceStatus::Offline);
}

#[test]
fn auth_fejl_stopper_automatiske_genforsoeg() {
    // Serveren har allerede svaret at tokenet ikke duer. At hamre den med det samme token
    // igen er ikke et genforsoeg, det er stoej.
    let telefon = TestTelefon::start(Opsaetning {
        kraevet_token: Some("det-rigtige".into()),
        ..Default::default()
    })
    .unwrap();
    let mut kilde = MjpegSource::ny("test", &format!("{}?token=det-forkerte", telefon.stream_url()));
    kilde.start();

    vent_paa(
        || kilde.status() == SourceStatus::AuthError,
        Duration::from_secs(10),
    );
    let efter_foerste = telefon.forbindelser();
    std::thread::sleep(Duration::from_secs(2));
    let efter_ventetid = telefon.forbindelser();
    kilde.stop();

    assert_eq!(efter_foerste, efter_ventetid);
}

#[test]
fn nulstil_auth_genoptager_forbindelsen() {
    // Modvidne til benet ovenfor: spaerringen skal kunne LOESNES, ellers er den en permanent
    // doed tilstand frem for et vaern.
    let telefon = TestTelefon::start(Opsaetning {
        kraevet_token: Some("det-rigtige".into()),
        ..Default::default()
    })
    .unwrap();
    let mut kilde = MjpegSource::ny("test", &format!("{}?token=det-forkerte", telefon.stream_url()));
    kilde.start();

    vent_paa(
        || kilde.status() == SourceStatus::AuthError,
        Duration::from_secs(10),
    );
    let foer = telefon.forbindelser();

    telefon.saet(|o| o.kraevet_token = Some("det-forkerte".into())); // konfigurationen er "rettet"
    kilde.nulstil_auth();

    let kom = vent_paa(|| kilde.frame_taeller() > 0, Duration::from_secs(10));
    let efter = telefon.forbindelser();
    kilde.stop();

    assert!(kom, "kilden kom aldrig i gang efter nulstil_auth");
    assert!(efter > foer);
}

#[test]
fn tavshed_giver_IKKE_live() {
    // Et 200 er ikke et bevis paa at der kommer billeder. Fiksturet svarer med headerne og
    // tier derefter - praecis den tilstand hvor statuskoden lyver.
    let telefon = TestTelefon::start(Opsaetning { tavs: true, ..Default::default() }).unwrap();
    let mut kilde = MjpegSource::med_frister(
        "test",
        &telefon.stream_url(),
        None,
        Some(Duration::from_millis(400)),
    );
    kilde.start();

    std::thread::sleep(Duration::from_millis(1200));
    let status = kilde.status();
    let taeller = kilde.frame_taeller();
    kilde.stop();

    // Der er aldrig kommet en frame, saa tilstanden er Connecting - og den er IKKE Live.
    assert_ne!(status, SourceStatus::Live, "en tavs server maa ikke se levende ud");
    assert_eq!(taeller, 0);
}

#[test]
fn en_LEVENDE_kilde_gaar_IKKE_offline() {
    // Modvidne til benet ovenfor: en stale-regel der altid siger stale, maaler intet.
    let telefon = TestTelefon::start(Opsaetning { fps: 50, ..Default::default() }).unwrap();
    let mut kilde = MjpegSource::med_frister(
        "test",
        &telefon.stream_url(),
        None,
        Some(Duration::from_secs(5)),
    );
    kilde.start();

    vent_paa(|| kilde.frame_taeller() >= 5, Duration::from_secs(10));
    std::thread::sleep(Duration::from_secs(1));
    let status = kilde.status();
    kilde.stop();

    assert_eq!(status, SourceStatus::Live);
}

#[test]
fn forbindelsen_GENOPTAGES_naar_serveren_lukker_midt_i_stroemmen() {
    let telefon = TestTelefon::start(Opsaetning {
        fps: 200,
        luk_efter_frames: 3,
        ..Default::default()
    })
    .unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    let flere = vent_paa(|| telefon.forbindelser() >= 2, Duration::from_secs(20));
    let taeller = kilde.frame_taeller();
    let forb = telefon.forbindelser();
    kilde.stop();

    assert!(flere, "kun {forb} forbindelse(r) - der blev ikke genforbundet");
    assert!(taeller >= 3, "kun {taeller} frames");
}

#[test]
fn STOP_LUKKER_faktisk_socketen() {
    // ⛔ "Stop" maa ikke bare vaere et flag: socketen skal LUKKES, ellers slipper telefonens
    // dovne kamera aldrig. Fiksturet taeller AFSLUTTEDE stroemme, saa maalingen er paa
    // serverens side - ikke paa vores eget flag.
    //
    // ⚠️ Rust-udgaven VENTER ikke paa laesetraaden som C#-udgaven gjorde; den saetter flaget
    // og lader traaden loebe ud. En GUI der ventede, ville fryse i op til laesefristen. Derfor
    // maales lukningen med en frist frem for som en oejeblikkelig tilstand.
    let telefon = TestTelefon::start(Opsaetning { fps: 100, ..Default::default() }).unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    assert!(
        vent_paa(|| kilde.frame_taeller() > 0, Duration::from_secs(10)),
        "kilden kom aldrig i gang"
    );
    assert_eq!(telefon.afsluttede(), 0, "stroemmen var lukket FOER vi bad om det");

    kilde.stop();
    assert!(
        vent_paa(|| telefon.afsluttede() >= 1, Duration::from_secs(15)),
        "socketen blev aldrig lukket - kameraet ville blive ved med at koere"
    );
}

#[test]
fn KUN_den_NYESTE_frame_holdes_tilbage() {
    // En koe ville vise fortid. Den frame man kan hente, er altid den sidst modtagne.
    let telefon = TestTelefon::start(Opsaetning { fps: 100, ..Default::default() }).unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    vent_paa(|| kilde.frame_taeller() >= 5, Duration::from_secs(15));
    let a = kilde.nyeste().expect("frame").sekvens_nr;
    vent_paa(
        || kilde.nyeste().map(|f| f.sekvens_nr).unwrap_or(0) > a,
        Duration::from_secs(5),
    );
    let b = kilde.nyeste().expect("frame").sekvens_nr;
    let taeller = kilde.frame_taeller();
    kilde.stop();

    assert!(b > a, "den nyeste frame blev ikke erstattet");
    assert_eq!(taeller, b, "sekvensnummeret skal foelge antallet af leverede frames");
}

#[test]
fn en_DOED_VAERT_giver_IKKE_authfejl() {
    // Fejlklasserne maa ikke smelte sammen: et netvaerksdoedt maal er OFFLINE, ikke AUTH-FEJL.
    // Port 9 paa loopback har ingen lytter.
    let mut kilde = MjpegSource::med_frister(
        "test",
        "http://127.0.0.1:9/stream",
        Some(Duration::from_secs(1)),
        None,
    );
    kilde.start();
    std::thread::sleep(Duration::from_secs(2));
    let status = kilde.status();
    let auth = kilde.auth_spaerret();
    kilde.stop();

    assert!(!auth);
    assert_ne!(status, SourceStatus::AuthError);
}

#[test]
fn forbindelses_nr_TAELLER_hver_GENFORBINDELSE() {
    // Telefonen gemmer IKKE kameravalget (Rig.useFront er en almindelig statisk variabel i
    // Husk 1.1), saa vieweren skal kunne SE at forbindelsen er ny og saette siden igen.
    // Benet maaler netop den evne: en vaert der lukker efter 3 frames tvinger genforbindelser.
    let telefon = TestTelefon::start(Opsaetning {
        fps: 200,
        luk_efter_frames: 3,
        ..Default::default()
    })
    .unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    let naaede = vent_paa(|| kilde.forbindelses_nr() >= 3, Duration::from_secs(25));
    let gen = kilde.forbindelses_nr();
    let forbindelser = telefon.forbindelser();
    kilde.stop();

    assert!(naaede, "kun {gen} forbindelser talt");
    // Taelleren skal foelge VAERTENS eget tal. Den maa ikke vaere hoejere: saa talte vi FORSOEG
    // frem for etablerede stroemme, og vieweren ville saette kameraside ved hvert mislykket
    // forsoeg. Et enkelt forsoeg kan vaere i flugt naar vi laeser, deraf slacken.
    assert!(
        (1..=forbindelser as u64).contains(&gen),
        "gen={gen} mod {forbindelser} forbindelser"
    );
}

#[test]
fn forbindelses_nr_STIGER_IKKE_paa_en_STABIL_stroem() {
    // Kontrollen til benet ovenfor: uden den kunne taelleren taelle FRAMES og bestaa alligevel.
    // Her leveres mange frames over ÉN stroem, og tallet skal blive staaende paa 1 - ellers
    // ville vieweren saette kameraside igen og igen paa et sundt link.
    let telefon = TestTelefon::start(Opsaetning { fps: 200, ..Default::default() }).unwrap();
    let mut kilde = MjpegSource::ny("test", &telefon.stream_url());
    kilde.start();

    let naaede = vent_paa(|| kilde.frame_taeller() >= 20, Duration::from_secs(15));
    let gen = kilde.forbindelses_nr();
    let frames = kilde.frame_taeller();
    kilde.stop();

    assert!(naaede, "kun {frames} frames");
    assert_eq!(gen, 1);
}
