//! Én levende MJPEG-kilde: forbinder, laeser, genforbinder, og holder PRAECIS den nyeste
//! ventende frame. Port af `MjpegSource.cs`.
//!
//! Aldrig en koe - en koe ville vise fortid, og en overvaagningsvisning der haenger bagud er
//! vaerre end en der dropper.
//!
//! ⛔ TRAADMODELLEN ER EN ANDEN END C#'s, og forskellen er bevidst. C# brugte `async`/`await`
//! paa en delt scheduler; Rust-udgaven bruger ÉN almindelig traad pr. kilde. Der er hoejst en
//! haandfuld telefoner, og hver traad bruger det meste af sin tid blokeret i WinHTTP - en
//! async-runtime ville vaere et rammevaerk i pakken for at spare traade vi ikke mangler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::mjpeg_parser::MjpegParser;
use crate::source_status::{Backoff, SourceStatus, StaleClock};
use crate::winhttp::Session;

/// En hel JPEG-frame med det oejeblik den ankom, maalt monotont.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JpegFrame {
    pub data: Vec<u8>,
    pub sekvens_nr: u64,
}

/// Den delte tilstand mellem laesetraaden og den der viser.
struct Indre {
    nyeste: Option<JpegFrame>,
    sekvens: u64,
    frame_taeller: u64,
    sidste_status: Option<u32>,
    forbindelses_nr: u64,
    auth_spaerret: bool,
    sidste_fejl: Option<String>,
    stale: StaleClock,
}

pub struct MjpegSource {
    navn: String,
    url: String,
    forbindelsesfrist: Duration,
    indre: Arc<Mutex<Indre>>,
    stop: Arc<AtomicBool>,
    koerer: bool,
}

impl MjpegSource {
    pub fn ny(navn: &str, url: &str) -> MjpegSource {
        Self::med_frister(navn, url, None, None)
    }

    /// `forbindelsesfrist` er fristen for at faa SVAR-HEADERNE.
    ///
    /// Adskilt fra stale-graensen med vilje: en server der svarer men tier, og en server der
    /// slet ikke svarer, er to forskellige fejl.
    pub fn med_frister(
        navn: &str,
        url: &str,
        forbindelsesfrist: Option<Duration>,
        stale_graense: Option<Duration>,
    ) -> MjpegSource {
        MjpegSource {
            navn: navn.to_string(),
            url: url.to_string(),
            forbindelsesfrist: forbindelsesfrist.unwrap_or(Duration::from_secs(5)),
            indre: Arc::new(Mutex::new(Indre {
                nyeste: None,
                sekvens: 0,
                frame_taeller: 0,
                sidste_status: None,
                forbindelses_nr: 0,
                auth_spaerret: false,
                sidste_fejl: None,
                stale: StaleClock::new(stale_graense),
            })),
            stop: Arc::new(AtomicBool::new(false)),
            koerer: false,
        }
    }

    pub fn navn(&self) -> &str {
        &self.navn
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// Den tilstand en visning skal vise lige nu.
    pub fn status(&self) -> SourceStatus {
        let i = self.laas();
        i.stale.status(i.auth_spaerret)
    }

    /// Den nyeste hele frame, eller `None`. Aeldre frames kastes vaek, aldrig koeet.
    pub fn nyeste(&self) -> Option<JpegFrame> {
        self.laas().nyeste.clone()
    }

    /// Antal hele frames leveret siden start.
    pub fn frame_taeller(&self) -> u64 {
        self.laas().frame_taeller
    }

    /// Seneste HTTP-statuskode, naar en saadan blev modtaget.
    pub fn sidste_status(&self) -> Option<u32> {
        self.laas().sidste_status
    }

    /// Seneste netvaerksfejl, redigeret for hemmeligheder.
    pub fn sidste_fejl(&self) -> Option<String> {
        self.laas().sidste_fejl.clone()
    }

    /// Taeller én op hver gang en NY MJPEG-stroem er etableret (foerste 2xx paa forbindelsen).
    ///
    /// Findes fordi telefonens kameraside IKKE er persistent: Husk holder `Rig.useFront` i en
    /// almindelig statisk variabel, saa en genstart af app eller enhed falder tavst tilbage til
    /// bagkameraet. En klient kan derfor ikke noejes med at saette siden én gang ved Start -
    /// den skal kunne se at forbindelsen er NY. Maalt 2026-09-19 i Husk 1.1 / 52.
    pub fn forbindelses_nr(&self) -> u64 {
        self.laas().forbindelses_nr
    }

    /// Saettes af 401/403 og ryddes foerst naar [`MjpegSource::nulstil_auth`] kaldes.
    ///
    /// Automatisk genforsoeg med det SAMME token er meningsloest: serveren har allerede svaret
    /// at det ikke duer, og et genforsoeg ville kun hamre den.
    pub fn auth_spaerret(&self) -> bool {
        self.laas().auth_spaerret
    }

    /// Konfigurationen er aendret (nyt token) - proev igen.
    pub fn nulstil_auth(&self) {
        self.laas().auth_spaerret = false;
    }

    /// Kun til test: er laesetraaden markeret som koerende?
    ///
    /// Soemmet findes fordi `start`/`stop`-kontrakten ellers ikke kan maales udefra, og et ben
    /// uden en maalbar egenskab bliver et ben uden assertion (maaleregel 135).
    #[cfg(test)]
    pub(crate) fn koerer_nu(&self) -> bool {
        self.koerer
    }

    /// Kun til test: det stop-flag den NUVAERENDE traad fik med.
    #[cfg(test)]
    pub(crate) fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    fn laas(&self) -> std::sync::MutexGuard<'_, Indre> {
        // En forgiftet laas betyder at en laesetraad panicede. Tilstanden er stadig
        // sammenhaengende (vi holder kun laasen om smaa tildelinger), saa vi laeser videre
        // frem for at rive hele appen ned.
        self.indre.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start laesetraaden. Idempotent.
    ///
    /// ⛔ STOP-FLAGET UDSKIFTES VED HVER START, det nulstilles ikke.
    ///
    /// `stop()` venter med vilje IKKE paa traaden (se dens egen note), saa en traad fra en
    /// tidligere start kan stadig staa i en blokerende WinHTTP-laesning i op til
    /// laese-fristen. Nulstillede vi det DELTE flag til `false`, ville netop den traad se
    /// "koer videre" og udgive frames igen - side om side med den nye traad, paa den samme
    /// `Indre`. Symptomet ville vaere en dobbelt `forbindelses_nr` og `frame_taeller`, altsaa
    /// data der ser plausible ud.
    ///
    /// Med en FRISK `Arc` pr. start beholder den gamle traad sit eget flag, som stadig er
    /// `true`, og den afslutter roligt. Den nye traad faar sit eget.
    pub fn start(&mut self) {
        if self.koerer {
            return;
        }
        self.koerer = true;
        self.stop = Arc::new(AtomicBool::new(false));

        let url = self.url.clone();
        let frist = self.forbindelsesfrist;
        let indre = Arc::clone(&self.indre);
        let stop = Arc::clone(&self.stop);

        // Traaden navngives, saa en haengende traad kan findes i en fejlsoegning.
        let _ = std::thread::Builder::new()
            .name(format!("husk-mjpeg-{}", self.navn))
            .spawn(move || loeb(url, frist, indre, stop));
    }

    /// Bed laesetraaden om at stoppe.
    ///
    /// ⛔ Der ventes IKKE paa den. Traaden kan staa i en blokerende WinHTTP-laesning i op til
    /// laese-fristen, og en GUI der ventede paa det ville fryse i netop saa lang tid. Traaden
    /// holder sin egen `Arc`, saa den kan afslutte roligt bagefter, og stop-flaget forhindrer
    /// at den udgiver noget imens.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.koerer = false;
    }
}

impl Drop for MjpegSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Fristen for en enkelt laesning i en stroem.
///
/// Den er sat til stale-graensen: en stroem der tier saa laenge, er alligevel OFFLINE paa
/// skaermen, saa der er intet at vinde ved at blive haengende i den. Den er samtidig det
/// vaerste tilfaelde for hvor laenge en stoppet kilde kan have en levende traad.
const LAESEFRIST: Duration = Duration::from_secs(5);

fn loeb(url: String, forbindelsesfrist: Duration, indre: Arc<Mutex<Indre>>, stop: Arc<AtomicBool>) {
    let Ok(session) = Session::ny("husk-webcam-rs/1.1") else {
        return;
    };
    let mut backoff = Backoff::default();

    while !stop.load(Ordering::SeqCst) {
        {
            let i = indre.lock().unwrap_or_else(|e| e.into_inner());
            if i.auth_spaerret {
                drop(i);
                // Vent paa at nogen retter konfigurationen. Vi poller flaget frem for at holde
                // traaden i en lang soevn, saa nulstil_auth virker med det samme.
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
        }

        let fik_frames = laes_en_session(&session, &url, forbindelsesfrist, &indre, &stop);

        if stop.load(Ordering::SeqCst) {
            break;
        }

        if fik_frames {
            backoff.lykkedes();
        } else {
            backoff.fejlede();
        }

        // Sov i smaa bidder, saa et stop ikke skal vente en hel backoff ud.
        let mut tilbage = backoff.naeste();
        while tilbage > Duration::ZERO && !stop.load(Ordering::SeqCst) {
            let bid = tilbage.min(Duration::from_millis(100));
            std::thread::sleep(bid);
            tilbage -= bid;
        }
    }
}

fn laes_en_session(
    session: &Session,
    url: &str,
    forbindelsesfrist: Duration,
    indre: &Arc<Mutex<Indre>>,
    stop: &Arc<AtomicBool>,
) -> bool {
    let mut stroem = match session.aabn(
        url,
        forbindelsesfrist.as_millis() as u32,
        LAESEFRIST.as_millis() as u32,
    ) {
        Ok(s) => s,
        Err(e) => {
            let mut i = indre.lock().unwrap_or_else(|e| e.into_inner());
            // Fejlteksten kan citere hele URL'en, altsaa ogsaa ?token=.
            i.sidste_fejl = Some(crate::url_redactor::hide_secrets(
                Some(&e.to_string()),
                Some(url),
            ));
            return false;
        }
    };

    {
        let mut i = indre.lock().unwrap_or_else(|e| e.into_inner());
        i.sidste_status = Some(stroem.status);
        if stroem.status == 401 || stroem.status == 403 {
            i.auth_spaerret = true;
            return false;
        }
        if !(200..300).contains(&stroem.status) {
            return false;
        }
        // Ny stroem etableret - se feltets doc for hvorfor det taelles.
        i.forbindelses_nr += 1;
        i.sidste_fejl = None;
    }

    let mut parser = MjpegParser::new(stroem.content_type.as_deref());
    let mut buf = vec![0u8; 16 * 1024];
    let mut fik = false;

    while !stop.load(Ordering::SeqCst) {
        let n = match stroem.laes(&mut buf) {
            Ok(0) => break, // serveren lukkede
            Ok(n) => n,
            Err(_) => break, // forbindelsen faldt eller tav; tilstanden gaar OFFLINE af sig selv
        };

        for jpeg in parser.feed(&buf[..n]) {
            if stop.load(Ordering::SeqCst) {
                return fik;
            }
            let mut i = indre.lock().unwrap_or_else(|e| e.into_inner());
            i.sekvens += 1;
            let nr = i.sekvens;
            i.nyeste = Some(JpegFrame { data: jpeg, sekvens_nr: nr });
            i.frame_taeller += 1;
            i.stale.frame_modtaget();
            fik = true;
        }
    }

    fik
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    // ⛔ De LEVENDE egenskaber maales i `tests/mjpeg.rs` mod en rigtig HTTP-server. Her staar
    // kun det der kan afgoeres uden et netvaerk - men modulet skal have sine egne ben, saa en
    // fejl i konstruktionen ikke foerst viser sig som en mystisk timeout i et integrationsben.

    #[test]
    fn en_frisk_kilde_er_CONNECTING_og_har_intet_at_vise() {
        let k = MjpegSource::ny("test", "http://127.0.0.1:9/stream");
        assert_eq!(k.status(), SourceStatus::Connecting);
        assert!(k.nyeste().is_none());
        assert_eq!(k.frame_taeller(), 0);
        assert_eq!(k.forbindelses_nr(), 0);
        assert!(!k.auth_spaerret());
        assert!(k.sidste_status().is_none());
    }

    #[test]
    fn navn_og_url_bevares_ordret() {
        // Navnet vises paa skaermen, og URL'en baerer tokenet. Et trimmet eller normaliseret
        // felt ville vaere en tavs aendring af det brugeren skrev.
        let k = MjpegSource::ny("Møderum Øst", "http://10.0.0.5:8090/stream?token=abc");
        assert_eq!(k.navn(), "Møderum Øst");
        assert_eq!(k.url(), "http://10.0.0.5:8090/stream?token=abc");
    }

    #[test]
    fn nulstil_auth_kan_kaldes_paa_en_kilde_der_aldrig_har_koert() {
        // UI'et kalder den naar brugeren retter tokenet, ogsaa foer Start. Den maa ikke panice.
        let k = MjpegSource::ny("test", "http://127.0.0.1:9/stream");
        k.nulstil_auth();
        assert!(!k.auth_spaerret());
    }

    #[test]
    fn stop_paa_en_kilde_der_aldrig_er_startet_er_en_NO_OP() {
        let mut k = MjpegSource::ny("test", "http://127.0.0.1:9/stream");
        assert!(!k.koerer_nu());
        k.stop();
        k.stop();
        // Det benet HEDDER, er at stop er en no-op - ikke at en kilde der aldrig koerte har
        // nul frames. Det sidste er sandt uanset hvad stop() goer.
        assert!(!k.koerer_nu());
        assert_eq!(k.frame_taeller(), 0);
    }

    #[test]
    fn start_er_IDEMPOTENT() {
        // To traade paa samme kilde ville skiftes til at overskrive den nyeste frame, og
        // forbindelses-taelleren ville taelle dobbelt - saa kameravalget blev sat to gange.
        //
        // Benet havde INGEN assertion indtil 2026-09-20: det kaldte start() to gange og
        // sluttede. Det ville have bestaaet uanset hvor mange traade der blev spawnet.
        let mut k = MjpegSource::ny("test", "http://127.0.0.1:9/stream");
        k.start();
        assert!(k.koerer_nu());
        let foerste = k.stop_flag();
        k.start();
        // Anden start maa hverken skifte flaget ud eller give traaden et nyt: gjorde den det,
        // ville der vaere to traade paa den samme `Indre`.
        assert!(Arc::ptr_eq(&foerste, &k.stop_flag()));
        assert!(k.koerer_nu());
        k.stop();
        assert!(!k.koerer_nu());
    }

    #[test]
    fn genstart_giver_den_GAMLE_traad_lov_til_at_doe() {
        // stop() venter ikke paa traaden. Nulstillede start() det DELTE flag til false, ville
        // en traad der stadig stod i en blokerende laesning se "koer videre" og udgive frames
        // side om side med den nye traad. Invarianten er derfor: efter en genstart er det
        // GAMLE flag stadig sat.
        let mut k = MjpegSource::ny("test", "http://127.0.0.1:9/stream");
        k.start();
        let gammelt = k.stop_flag();
        k.stop();
        assert!(gammelt.load(Ordering::SeqCst), "stop() skal saette det gamle flag");
        k.start();
        assert!(
            gammelt.load(Ordering::SeqCst),
            "start() maa IKKE nulstille det flag den gamle traad holder"
        );
        assert!(!Arc::ptr_eq(&gammelt, &k.stop_flag()), "den nye traad skal have sit EGET flag");
        k.stop();
    }
}
