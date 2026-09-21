#![allow(non_snake_case)]

//! Ben for [`Forbindelsestjek`] mod en LEVENDE HTTP-server.
//!
//! De tre fejlklasser findes netop fordi »virker ikke« er ubrugeligt for brugeren: tjek
//! netvaerket, ret tokenet, opgradér appen er tre forskellige handlinger. Benene skal derfor
//! kunne fremprovokere hver enkelt tilstand - ellers maaler de kun det lykkelige udfald.

use std::time::Duration;

use husk_webcam::forbindelsestjek::{Forbindelsesdom, Forbindelsestjek};
use husk_webcam::testfikstur::{Opsaetning, TavsVaert, TestTelefon, ledig_port};

fn tjek() -> Forbindelsestjek {
    Forbindelsestjek::med_frist(4000).expect("WinHttpOpen")
}

fn telefon(flags: &str) -> TestTelefon {
    TestTelefon::start(Opsaetning { flags_krop: flags.to_string(), ..Default::default() })
        .expect("fikstur")
}

#[test]
fn en_HUSK11_giver_ok_og_SIGER_at_kameravalg_kan_styres() {
    let t = telefon(r#"{"a11y":true,"camera":false,"front":false,"screen":false}"#);
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::Ok, "{}", s.tekst);
    assert_eq!(s.front, Some(false));
    assert!(s.tekst.contains("1.1"), "{}", s.tekst);
}

#[test]
fn en_AELDRE_husk_giver_ok_men_SIGER_at_front_ikke_kan_styres() {
    // Før 1.1 findes feltet ikke. Det er ikke en fejl - men guiden maa ikke love en knap der
    // ikke kan virke.
    let t = telefon(r#"{"a11y":true,"camera":false}"#);
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::Ok, "{}", s.tekst);
    assert_eq!(s.front, None);
    assert!(s.tekst.contains("ÆLDRE"), "{}", s.tekst);
}

#[test]
fn camera_false_er_en_GYLDIG_hviletilstand_og_IKKE_en_inkompatibel_app() {
    // Kameraet er dovent. En guide der doemte paa det felt, ville afvise en sund telefon.
    let t = telefon(r#"{"a11y":true,"camera":false,"front":true}"#);
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::Ok, "{}", s.tekst);
    assert_eq!(s.front, Some(true));
    assert_eq!(s.kamera_koerer, Some(false));
}

#[test]
fn forkert_token_giver_ADGANG_NAEGTET_og_LOVER_ikke_at_et_nyt_loeser_den() {
    // En 401/403 kan ogsaa vaere Husks kilde-IP-spaerre. At love at et nyt token hjaelper,
    // ville sende brugeren ud at lede det forkerte sted.
    let t = TestTelefon::start(Opsaetning {
        kraevet_token: Some("rigtigt".into()),
        ..Default::default()
    })
    .expect("fikstur");
    let s = tjek().tjek(&t.vaert(), Some("forkert"));
    assert_eq!(s.dom, Forbindelsesdom::AdgangNaegtet, "{}", s.tekst);
    assert!(s.tekst.contains("kilde-IP"), "{}", s.tekst);
}

#[test]
fn RIGTIGT_token_giver_ok() {
    // Modvidne: en dom der ALTID siger "adgang nægtet", maaler ingenting.
    let t = TestTelefon::start(Opsaetning {
        kraevet_token: Some("rigtigt".into()),
        ..Default::default()
    })
    .expect("fikstur");
    let s = tjek().tjek(&t.vaert(), Some("rigtigt"));
    assert_eq!(s.dom, Forbindelsesdom::Ok, "{}", s.tekst);
}

#[test]
fn en_AFVIST_forbindelse_giver_INTET_SVAR_og_IKKE_adgang_naegtet() {
    let s = tjek().tjek(&format!("127.0.0.1:{}", ledig_port()), None);
    assert_eq!(s.dom, Forbindelsesdom::IntetSvar, "{}", s.tekst);
    assert!(s.tekst.contains("netværket"), "{}", s.tekst);
}

#[test]
fn en_TAVS_vaert_giver_OGSAA_INTET_SVAR() {
    // ⚠️ Den ANDEN vej til samme dom (maaleregel 69). Benet pinner DOMMEN, ikke teksten:
    // en afvist forbindelse og en tavs vaert giver hver sin besked, og et ben der pinnede
    // den ene ville gaa roedt af noget der ikke er en fejl.
    let v = TavsVaert::start().expect("tavs vaert");
    let t = Forbindelsestjek::med_frist(1500).expect("WinHttpOpen");
    let s = t.tjek(v.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::IntetSvar, "{}", s.tekst);
    assert!(!s.tekst.is_empty());
}

#[test]
fn en_FREMMED_tjeneste_paa_porten_giver_FORKERT_ELLER_FOR_GAMMEL() {
    let t = TestTelefon::start(Opsaetning { healthz_status: 404, ..Default::default() })
        .expect("fikstur");
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::ForkertEllerForGammel, "{}", s.tekst);
}

#[test]
fn et_svar_der_ikke_er_json_giver_FORKERT_ELLER_FOR_GAMMEL() {
    let t = telefon("det her er ikke json");
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::ForkertEllerForGammel, "{}", s.tekst);
}

#[test]
fn et_json_svar_uden_camera_felt_giver_FORKERT_ELLER_FOR_GAMMEL() {
    let t = telefon(r#"{"noget":1}"#);
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::ForkertEllerForGammel, "{}", s.tekst);
    assert!(s.tekst.contains("camera"), "{}", s.tekst);
}

#[test]
fn TOM_adresse_siger_hvor_man_finder_den() {
    let s = tjek().tjek("   ", None);
    assert_eq!(s.dom, Forbindelsesdom::IntetSvar);
    assert!(s.tekst.contains("statusvisning"), "{}", s.tekst);
}

#[test]
fn healthz_kaldes_UDEN_token_saa_de_to_fejl_kan_skelnes() {
    // /healthz kraever ALDRIG et token hos Husk. Kaldte tjekket den MED tokenet, ville en
    // spaerret telefon svare 401 allerede dér, og dommen ville blive »forkert app« i stedet
    // for »adgang nægtet«. Fiksturet kraever token paa ALT undtagen /healthz, praecis som
    // Husk - saa benet ville gaa roedt hvis rækkefølgen blev vendt.
    let t = TestTelefon::start(Opsaetning {
        kraevet_token: Some("rigtigt".into()),
        ..Default::default()
    })
    .expect("fikstur");
    let s = tjek().tjek(&t.vaert(), None);
    assert_eq!(s.dom, Forbindelsesdom::AdgangNaegtet, "{}", s.tekst);
}

#[test]
fn en_doed_vaert_maa_ikke_give_en_falsk_HURTIG_dom() {
    // Fristen skal faktisk gaelde: et tjek der svarede med det samme uden at have proevet,
    // ville se ud som en maaling.
    let start = std::time::Instant::now();
    let v = TavsVaert::start().expect("tavs vaert");
    let t = Forbindelsestjek::med_frist(1200).expect("WinHttpOpen");
    let s = t.tjek(v.vaert(), None);
    let gik = start.elapsed();
    assert_eq!(s.dom, Forbindelsesdom::IntetSvar);
    assert!(
        gik >= Duration::from_millis(900),
        "dommen faldt efter {gik:?} - fristen blev ikke brugt"
    );
}
