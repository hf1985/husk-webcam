//! Kontrollerer en telefon foer den tages i brug. Port af `Forbindelsestjek.cs`.
//!
//! Tre fejlklasser, ikke én: »virker ikke« er ubrugeligt naar de tre kraever hver sin handling
//! (tjek netvaerket, ret tokenet, opgradér appen).
//!
//! ⛔ `camera:false` er en GYLDIG hviletilstand, ikke en inkompatibel app. Kameraet er dovent
//! og lukkes faa sekunder efter sidste forbruger. En guide der doemte paa det felt, ville
//! afvise en helt sund telefon.

use crate::husk_urls;
use crate::json;
use crate::url_redactor;
use crate::winhttp::{BRUGERAGENT, HttpFejl, Session};

/// Hvad et forbindelsestjek fandt. Hver vaerdi kraever sin EGEN handling af brugeren.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forbindelsesdom {
    /// Telefonen svarer, og den er en Husk der kan det vi skal bruge.
    Ok,
    /// Intet svar. Netvaerk, forkert adresse, eller telefonen er slukket.
    IntetSvar,
    /// 401/403. Tokenet mangler eller er forkert - ELLER kilde-IP'en er uden for Husks egen
    /// ACL. Lov derfor ALDRIG at et nyt token loeser den; sig hvad der blev maalt.
    AdgangNaegtet,
    /// Svarer, men ikke som en Husk. Forkert app, eller en for gammel udgave.
    ForkertEllerForGammel,
}

/// Resultatet, med en tekst brugeren kan handle paa.
#[derive(Clone, Debug, PartialEq)]
pub struct Forbindelsessvar {
    pub dom: Forbindelsesdom,
    pub tekst: String,
    pub front: Option<bool>,
    pub kamera_koerer: Option<bool>,
}

impl Forbindelsessvar {
    fn ny(dom: Forbindelsesdom, tekst: impl Into<String>) -> Self {
        Forbindelsessvar { dom, tekst: tekst.into(), front: None, kamera_koerer: None }
    }
}

pub struct Forbindelsestjek {
    session: Session,
    frist_ms: u32,
}

impl Forbindelsestjek {
    pub fn ny() -> Result<Forbindelsestjek, HttpFejl> {
        Ok(Forbindelsestjek { session: Session::ny(BRUGERAGENT)?, frist_ms: 6000 })
    }

    pub fn med_frist(frist_ms: u32) -> Result<Forbindelsestjek, HttpFejl> {
        Ok(Forbindelsestjek { session: Session::ny(BRUGERAGENT)?, frist_ms })
    }

    pub fn tjek(&self, vaert: &str, token: Option<&str>) -> Forbindelsessvar {
        if vaert.trim().is_empty() {
            return Forbindelsessvar::ny(
                Forbindelsesdom::IntetSvar,
                "Indtast telefonens adresse. Husk viser den i appens egen statusvisning.",
            );
        }

        // /healthz kraever ALDRIG et token. Svarer den, er telefonen naabar, og en 401 paa
        // /flags bagefter er saa entydigt et ADGANGS-problem frem for et netvaerks-problem.
        // Uden det foerste kald kunne de to ikke skelnes.
        match self.session.get(&husk_urls::healthz(vaert), self.frist_ms) {
            Err(_) => {
                return Forbindelsessvar::ny(
                    Forbindelsesdom::IntetSvar,
                    "Ingen forbindelse. Telefon og PC skal kunne nå hinanden på netværket - \
                     en parring skaber ikke i sig selv netværksadgang.",
                );
            }
            Ok(h) if !h.er_ok() => {
                return Forbindelsessvar::ny(
                    Forbindelsesdom::ForkertEllerForGammel,
                    format!(
                        "Adressen svarer, men ikke som en Husk-telefon (/healthz gav {}). \
                         Er det den rigtige adresse og port?",
                        h.status
                    ),
                );
            }
            Ok(_) => {}
        }

        let url = husk_urls::flags(vaert, token);
        let svar = match self.session.get(&url, self.frist_ms) {
            Ok(s) => s,
            Err(e) => {
                return Forbindelsessvar::ny(
                    Forbindelsesdom::IntetSvar,
                    url_redactor::hide_secrets(Some(&e.to_string()), Some(&url)),
                );
            }
        };

        if svar.status == 401 || svar.status == 403 {
            return Forbindelsessvar::ny(Forbindelsesdom::AdgangNaegtet, tekst_adgang_naegtet(svar.status));
        }

        if !svar.er_ok() {
            return Forbindelsessvar::ny(
                Forbindelsesdom::ForkertEllerForGammel,
                format!(
                    "/flags gav {}. Forkert app, eller en for gammel Husk.",
                    svar.status
                ),
            );
        }

        doem_flags(&svar.krop_som_tekst())
    }
}

/// Teksten til [`Forbindelsesdom::AdgangNaegtet`].
///
/// ⛔ Den bor i en FUNKTION frem for som en literal paa kaldestedet, og det er ikke kosmetik.
/// Benet der vogter loeftet (»lov aldrig at et nyt token loeser det«) byggede indtil
/// 2026-09-20 sin EGEN kopi af sætningen og asserterede paa den. Det ben kunne per
/// konstruktion ikke gaa roedt af en aendring i produktteksten - det maalte sin egen literal
/// (maaleregel 135). Nu kalder baade produktet og benet denne funktion.
pub fn tekst_adgang_naegtet(status: u32) -> String {
    format!(
        "Telefonen svarer, men afviser adgang ({}). Det kan være et manglende \
         eller forkert token - men det kan også være Husks egen kilde-IP-spærre. \
         Et nyt token løser ikke nødvendigvis en spærret adresse.",
        status
    )
}

/// Dommen ud fra `/flags`-kroppen alene.
///
/// Skilt fra HTTP-laget saa den kan maales uden en server: hele logikken om hvad der goer en
/// telefon til en Husk 1.1 ligger her.
pub fn doem_flags(krop: &str) -> Forbindelsessvar {
    let Ok(r) = json::parse(krop) else {
        return Forbindelsessvar::ny(
            Forbindelsesdom::ForkertEllerForGammel,
            "Svaret på /flags er ikke JSON. Det er næppe en Husk-telefon.",
        );
    };

    // "camera" har fandtes siden laenge foer 1.1 og er vores vidne om at det ER en Husk.
    // "front" kom med 1.1 og er vidnet om at kameravalget kan styres.
    let Some(kam) = r.get("camera") else {
        return Forbindelsessvar::ny(
            Forbindelsesdom::ForkertEllerForGammel,
            "Svaret ligner ikke en Husk: /flags mangler feltet \"camera\".",
        );
    };

    let front = r.get("front").and_then(|f| f.som_bool());
    let kamera_koerer = kam.som_bool() == Some(true);

    let tekst = if front.is_none() {
        "Forbundet. ⚠️ Telefonen er en ÆLDRE Husk: /flags har intet \"front\"-felt, \
         så kameraside kan ikke vælges herfra. Opgradér appen til 1.2 eller nyere."
    } else {
        "Forbundet, og telefonen er Husk 1.1 eller nyere - kameraside kan vælges herfra. \
         (Kameraet er dovent; at det står stille lige nu er normalt.)"
    };

    Forbindelsessvar {
        dom: Forbindelsesdom::Ok,
        tekst: tekst.to_string(),
        front,
        kamera_koerer: Some(kamera_koerer),
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn tom_vaert_beder_om_en_adresse_frem_for_at_ringe_ud() {
        let t = Forbindelsestjek::ny().unwrap();
        let s = t.tjek("   ", None);
        assert_eq!(s.dom, Forbindelsesdom::IntetSvar);
        assert!(s.tekst.contains("adresse"));
    }

    #[test]
    fn husk_11_genkendes_paa_front_feltet() {
        let s = doem_flags(r#"{"camera": false, "screen": false, "front": true}"#);
        assert_eq!(s.dom, Forbindelsesdom::Ok);
        assert_eq!(s.front, Some(true));
        assert_eq!(s.kamera_koerer, Some(false));
        assert!(s.tekst.contains("1.1 eller nyere"));
    }

    #[test]
    fn kamera_false_er_en_GYLDIG_hviletilstand() {
        // ⛔ En guide der doemte paa det felt, ville afvise en helt sund telefon.
        let s = doem_flags(r#"{"camera": false, "front": false}"#);
        assert_eq!(s.dom, Forbindelsesdom::Ok);
        assert_eq!(s.kamera_koerer, Some(false));
    }

    #[test]
    fn aeldre_husk_uden_front_felt_er_OK_men_siges_hoejt() {
        let s = doem_flags(r#"{"camera": true, "screen": false}"#);
        assert_eq!(s.dom, Forbindelsesdom::Ok);
        assert_eq!(s.front, None);
        assert!(s.tekst.contains("ÆLDRE"), "faktisk: {}", s.tekst);
    }

    #[test]
    fn et_svar_uden_camera_er_ikke_en_husk() {
        let s = doem_flags(r#"{"noget": "andet"}"#);
        assert_eq!(s.dom, Forbindelsesdom::ForkertEllerForGammel);
        assert!(s.tekst.contains("camera"));
    }

    #[test]
    fn et_svar_der_ikke_er_json_er_ikke_en_husk() {
        let s = doem_flags("<html>404</html>");
        assert_eq!(s.dom, Forbindelsesdom::ForkertEllerForGammel);
        assert!(s.tekst.contains("JSON"));
    }

    #[test]
    fn adgang_naegtet_lover_ALDRIG_at_et_nyt_token_loeser_det() {
        // Det kan lige saa vel vaere Husks egen kilde-IP-spaerre.
        //
        // Benet kalder PRODUKTETS egen tekstfunktion. Her byggede det indtil 2026-09-20 sin
        // egen kopi af saetningen, og kunne derfor ikke se en aendring i den rigtige tekst.
        for status in [401u32, 403u32] {
            let t = tekst_adgang_naegtet(status);
            assert!(t.contains("kilde-IP"), "status {status}: {t}");
            assert!(t.contains("ikke nødvendigvis"), "status {status}: {t}");
            assert!(t.contains(&status.to_string()), "status {status}: {t}");
            // Og den maa ALDRIG love at et nyt token er kuren.
            assert!(!t.contains("skaf et nyt token"), "status {status}: {t}");
        }
    }

    #[test]
    fn en_doed_adresse_giver_INTET_SVAR_ikke_en_forkert_app() {
        let t = Forbindelsestjek::med_frist(1500).unwrap();
        let s = t.tjek("127.0.0.1:9", None);
        assert_eq!(s.dom, Forbindelsesdom::IntetSvar, "faktisk: {}", s.tekst);
        assert!(s.tekst.contains("netværket"));
    }
}
