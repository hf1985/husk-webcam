//! Henter adgangstokenet fra telefonen, med brugerens godkendelse paa telefonen.
//!
//! Husk 1.4 og nyere har to ruter uden token-krav: `/token/request?client=<navn>` viser en
//! notifikation paa telefonen med Godkend/Afvis, og `/token/status?id=<id>` svarer `pending`,
//! `denied`, `expired` eller `approved` med tokenet. Tokenet udleveres praecis én gang.
//!
//! ⛔ Klienten sender ALDRIG `new=`. Et forslag fra PC'en ville sætte tokenet paa en telefon
//!    der intet har, og det valg hører til den der står med telefonen, ikke til denne app.
//!
//! ⛔ Tokenet maa ikke naa en log eller en statustekst. Ingen tekst herfra baerer det, fejltekster
//!    gaar gennem [`url_redactor::hide_secrets`], og [`Hentningsdom`]s `Debug` skjuler vaerdien.

use std::time::{Duration, Instant};

use crate::husk_urls;
use crate::json;
use crate::url_redactor;
use crate::winhttp::{BRUGERAGENT, HttpFejl, Session};

/// Hvor ofte `/token/status` spoerges.
pub const POLL_MS: u64 = 2000;
/// Hvor laenge der ventes paa at brugeren trykker. Samme udloeb som telefonens egen anmodning.
pub const FRIST_MS: u64 = 120_000;

/// Udfaldet af en hentning. Hver vaerdi kraever sin egen tekst til brugeren.
#[derive(Clone, PartialEq)]
pub enum Hentningsdom {
    /// Brugeren trykkede Godkend. Vaerdien er tokenet.
    Godkendt(String),
    /// Brugeren trykkede Afvis.
    Afvist,
    /// Ingen trykkede inden fristen, eller telefonen glemte anmodningen.
    Udloebet,
    /// Telefonen kender ikke ruten: Husk foer 1.4.
    ForGammel,
    /// En anden anmodning venter allerede paa telefonen.
    Optaget,
    /// Telefonen kan ikke vise notifikationen, saa ingen kan godkende.
    NotifikationerFra,
    /// Alt andet: netvaerk, et uventet svar.
    Fejl(String),
}

impl std::fmt::Debug for Hentningsdom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Tokenet skrives aldrig ud, heller ikke i en testfejl.
            Hentningsdom::Godkendt(_) => f.write_str("Godkendt(***)"),
            Hentningsdom::Afvist => f.write_str("Afvist"),
            Hentningsdom::Udloebet => f.write_str("Udloebet"),
            Hentningsdom::ForGammel => f.write_str("ForGammel"),
            Hentningsdom::Optaget => f.write_str("Optaget"),
            Hentningsdom::NotifikationerFra => f.write_str("NotifikationerFra"),
            Hentningsdom::Fejl(t) => write!(f, "Fejl({t:?})"),
        }
    }
}

impl Hentningsdom {
    /// Teksten til brugeren. Baerer ALDRIG tokenet.
    pub fn tekst(&self) -> String {
        match self {
            Hentningsdom::Godkendt(_) => {
                "Godkendt på telefonen. Tokenet er gemt beskyttet på denne PC.".to_string()
            }
            Hentningsdom::Afvist => "Afvist på telefonen. Intet er gemt.".to_string(),
            Hentningsdom::Udloebet => {
                "Ingen godkendte på telefonen inden for to minutter. Intet er gemt - prøv igen."
                    .to_string()
            }
            Hentningsdom::ForGammel => {
                "Telefonen skal opdateres til Husk 1.4 eller nyere. Indtil da kan tokenet skrives \
                 i feltet her."
                    .to_string()
            }
            Hentningsdom::Optaget => {
                "Telefonen venter allerede på svar på en anden anmodning. Tryk Godkend eller Afvis \
                 dér, eller vent to minutter."
                    .to_string()
            }
            Hentningsdom::NotifikationerFra => {
                "Telefonen kan ikke vise anmodningen, fordi notifikationer er slået fra for Husk. \
                 Slå dem til, eller skriv tokenet i feltet."
                    .to_string()
            }
            Hentningsdom::Fejl(t) => format!("Kunne ikke hente tokenet: {t}"),
        }
    }
}

/// Et svar fra `/token/status`.
#[derive(Debug, PartialEq)]
pub enum Statusdom {
    Venter,
    Faerdig(Hentningsdom),
}

/// Dommen over svaret paa `/token/request`: anmodningens id, eller hvorfor der ikke kom et.
pub fn doem_anmodning(status: u32, krop: &str) -> Result<String, Hentningsdom> {
    match status {
        200 => {}
        404 => return Err(Hentningsdom::ForGammel),
        429 => return Err(Hentningsdom::Optaget),
        503 => return Err(Hentningsdom::NotifikationerFra),
        s => return Err(Hentningsdom::Fejl(format!("telefonen svarede {s}"))),
    }
    let Ok(j) = json::parse(krop) else {
        return Err(Hentningsdom::Fejl("svaret var ikke JSON".into()));
    };
    let id = j.get("id").and_then(|v| v.som_str()).unwrap_or("");
    // Id'et kommer tilbage i en URL, saa kun den form Husk udsteder accepteres: 32 hex-tegn.
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Hentningsdom::Fejl("svaret bar intet gyldigt id".into()));
    }
    Ok(id.to_string())
}

/// Dommen over svaret paa `/token/status`.
pub fn doem_status(status: u32, krop: &str) -> Statusdom {
    if status == 404 {
        return Statusdom::Faerdig(Hentningsdom::ForGammel);
    }
    if status != 200 {
        return Statusdom::Faerdig(Hentningsdom::Fejl(format!("telefonen svarede {status}")));
    }
    let Ok(j) = json::parse(krop) else {
        return Statusdom::Faerdig(Hentningsdom::Fejl("svaret var ikke JSON".into()));
    };
    match j.get("status").and_then(|v| v.som_str()) {
        Some("pending") => Statusdom::Venter,
        Some("denied") => Statusdom::Faerdig(Hentningsdom::Afvist),
        Some("expired") => Statusdom::Faerdig(Hentningsdom::Udloebet),
        Some("approved") => match j.get("token").and_then(|v| v.som_str()) {
            Some(t) if !t.is_empty() => Statusdom::Faerdig(Hentningsdom::Godkendt(t.to_string())),
            _ => Statusdom::Faerdig(Hentningsdom::Fejl("godkendt, men uden token".into())),
        },
        _ => Statusdom::Faerdig(Hentningsdom::Fejl("ukendt status".into())),
    }
}

/// Computerens navn som det vises i telefonens notifikation. Laeses ved koersel.
///
/// Renses til det Husk selv accepterer (`[A-Za-z0-9 ._-]`, hoejst 32 tegn), saa brugeren ser
/// det samme navn her og paa telefonen.
pub fn computernavn() -> String {
    rens_klientnavn(&std::env::var("COMPUTERNAME").unwrap_or_default())
}

pub fn rens_klientnavn(raa: &str) -> String {
    let r: String = raa
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
        .take(32)
        .collect();
    let r = r.trim().to_string();
    if r.is_empty() { "PC".to_string() } else { r }
}

pub struct TokenHenter {
    session: Session,
    poll_ms: u64,
    frist_ms: u64,
    http_frist_ms: u32,
}

impl TokenHenter {
    pub fn ny() -> Result<TokenHenter, HttpFejl> {
        Self::med_tider(POLL_MS, FRIST_MS)
    }

    /// Kortere tider til testene.
    pub fn med_tider(poll_ms: u64, frist_ms: u64) -> Result<TokenHenter, HttpFejl> {
        Ok(TokenHenter { session: Session::ny(BRUGERAGENT)?, poll_ms, frist_ms, http_frist_ms: 6000 })
    }

    /// Bed telefonen om tokenet og vent paa brugerens svar.
    ///
    /// `fremskridt` faar de tekster brugeren skal se undervejs. Ingen af dem baerer tokenet.
    pub fn hent(&self, vaert: &str, klient: &str, mut fremskridt: impl FnMut(&str)) -> Hentningsdom {
        if vaert.trim().is_empty() {
            return Hentningsdom::Fejl("indtast telefonens adresse først".into());
        }
        let url = husk_urls::token_request(vaert, klient);
        let id = match self.session.get(&url, self.http_frist_ms) {
            Ok(s) => match doem_anmodning(s.status, &s.krop_som_tekst()) {
                Ok(id) => id,
                Err(d) => return d,
            },
            Err(e) => {
                return Hentningsdom::Fejl(url_redactor::hide_secrets(Some(&e.to_string()), Some(&url)));
            }
        };

        fremskridt("Godkend på telefonen …");
        let start = Instant::now();
        let status_url = husk_urls::token_status(vaert, &id);
        loop {
            match self.session.get(&status_url, self.http_frist_ms) {
                Ok(s) => match doem_status(s.status, &s.krop_som_tekst()) {
                    Statusdom::Venter => {}
                    Statusdom::Faerdig(d) => return d,
                },
                Err(e) => {
                    return Hentningsdom::Fejl(url_redactor::hide_secrets(
                        Some(&e.to_string()),
                        Some(&status_url),
                    ));
                }
            }
            if start.elapsed() >= Duration::from_millis(self.frist_ms) {
                return Hentningsdom::Udloebet;
            }
            let tilbage = self.frist_ms.saturating_sub(start.elapsed().as_millis() as u64) / 1000;
            fremskridt(&format!("Godkend på telefonen … ({tilbage} s tilbage)"));
            std::thread::sleep(Duration::from_millis(self.poll_ms));
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use crate::testfikstur::{Opsaetning, TestTelefon, TokenUdfald};

    const TOKEN: &str = "abcDEF0123456789abcDEF0123456789";

    fn telefon(udfald: TokenUdfald) -> TestTelefon {
        TestTelefon::start(Opsaetning { token_udfald: udfald, ..Default::default() }).unwrap()
    }

    fn hent(t: &TestTelefon, log: &mut Vec<String>) -> Hentningsdom {
        let h = TokenHenter::med_tider(20, 3000).unwrap();
        h.hent(&t.vaert(), "Test-PC", |s| log.push(s.to_string()))
    }

    #[test]
    fn godkendt_giver_tokenet_og_det_naar_ALDRIG_loggen() {
        let t = telefon(TokenUdfald::Godkendt { token: TOKEN.into(), efter_poll: 2 });
        let mut log = Vec::new();
        let d = hent(&t, &mut log);
        assert_eq!(d, Hentningsdom::Godkendt(TOKEN.into()));
        assert!(!log.is_empty(), "brugeren skal se at der ventes");
        log.push(d.tekst());
        log.push(format!("{d:?}"));
        for linje in &log {
            assert!(!linje.contains(TOKEN), "tokenet stod i: {linje}");
        }
        // Og klienten sendte intet forslag.
        assert_eq!(t.sidste_token_anmodning().as_deref().map(|q| q.contains("new=")), Some(false));
        assert!(t.sidste_token_anmodning().unwrap().contains("client=Test-PC"));
    }

    #[test]
    fn afvist_gemmer_intet() {
        let t = telefon(TokenUdfald::Afvist);
        let d = hent(&t, &mut Vec::new());
        assert_eq!(d, Hentningsdom::Afvist);
        assert!(d.tekst().contains("Intet er gemt"));
    }

    #[test]
    fn udloebet_paa_telefonen_gemmer_intet() {
        let t = telefon(TokenUdfald::Udloebet);
        let d = hent(&t, &mut Vec::new());
        assert_eq!(d, Hentningsdom::Udloebet);
        assert!(d.tekst().contains("Intet er gemt"));
    }

    #[test]
    fn ingen_der_trykker_udloeber_ved_vores_egen_frist() {
        let t = telefon(TokenUdfald::Venter);
        let h = TokenHenter::med_tider(20, 300).unwrap();
        let start = Instant::now();
        let d = h.hent(&t.vaert(), "Test-PC", |_| {});
        assert_eq!(d, Hentningsdom::Udloebet);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn husk_foer_1_4_svarer_404_og_beder_om_en_opdatering() {
        let t = telefon(TokenUdfald::FindesIkke);
        let d = hent(&t, &mut Vec::new());
        assert_eq!(d, Hentningsdom::ForGammel);
        assert!(d.tekst().contains("Husk 1.4"), "{}", d.tekst());
    }

    #[test]
    fn optaget_og_notifikationer_fra_har_hver_sin_tekst() {
        assert_eq!(doem_anmodning(429, ""), Err(Hentningsdom::Optaget));
        assert_eq!(doem_anmodning(503, ""), Err(Hentningsdom::NotifikationerFra));
        assert_ne!(Hentningsdom::Optaget.tekst(), Hentningsdom::NotifikationerFra.tekst());
    }

    #[test]
    fn et_id_der_ikke_er_32_hex_afvises_foer_det_naar_en_URL() {
        assert!(doem_anmodning(200, r#"{"id":"../../x&token=1","expires_in":120}"#).is_err());
        assert!(doem_anmodning(200, r#"{"id":"abc","expires_in":120}"#).is_err());
        let id = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(doem_anmodning(200, &format!(r#"{{"id":"{id}","expires_in":120}}"#)), Ok(id.into()));
    }

    #[test]
    fn godkendt_uden_token_er_en_fejl_ikke_et_tomt_token() {
        assert!(matches!(
            doem_status(200, r#"{"status":"approved"}"#),
            Statusdom::Faerdig(Hentningsdom::Fejl(_))
        ));
    }

    #[test]
    fn debug_skjuler_tokenet() {
        assert_eq!(format!("{:?}", Hentningsdom::Godkendt(TOKEN.into())), "Godkendt(***)");
    }

    #[test]
    fn klientnavnet_renses_som_paa_telefonen() {
        assert_eq!(rens_klientnavn("KONTOR-PC"), "KONTOR-PC");
        assert_eq!(rens_klientnavn("æøå<script>"), "script");
        assert_eq!(rens_klientnavn(""), "PC");
        assert_eq!(rens_klientnavn(&"x".repeat(40)).len(), 32);
    }
}
