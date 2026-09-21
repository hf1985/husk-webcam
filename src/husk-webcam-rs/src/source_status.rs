//! En kildes tilstand, stale-uret og backoff-kurven. Port af `SourceStatus.cs`.

use std::time::{Duration, Instant};

/// En kildes tilstand.
///
/// AUTH-FEJL er sin EGEN tilstand og ikke en slags OFFLINE: de to kraever modsatte handlinger
/// af brugeren (ret tokenet mod tjek netvaerket), og i Python-vejen saa de ens ud paa skaermen
/// indtil det blev rettet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceStatus {
    /// Forbinder for foerste gang, eller igen efter et tab.
    Connecting,
    /// Der er kommet en hel, gyldig frame inden for stale-graensen.
    Live,
    /// Ingen gyldig frame i stale-graensen. Netvaerk, telefon eller kamera.
    Offline,
    /// 401/403. Tokenet mangler eller er forkert - ELLER kilde-IP'en er uden for Husks egen
    /// ACL. Lov derfor aldrig at et nyt token loeser den; sig hvad der blev maalt.
    AuthError,
}

/// Afgoer OFFLINE ud fra hvornaar den seneste GYLDIGE frame kom, maalt med et MONOTONT ur.
///
/// Et vaegur kan springe (sommertid, NTP), og et spring ville enten vise en levende kilde som
/// doed eller omvendt. `Instant` er monotont pr. kontrakt.
///
/// Graensen er arvet uaendret fra den tidligere udgave: 5,0 sekunder uden frame -> OFFLINE.
pub struct StaleClock {
    start: Instant,
    graense: Duration,
    sidste_frame: Option<Duration>,
}

impl StaleClock {
    pub const STANDARD_GRAENSE: Duration = Duration::from_secs(5);

    pub fn new(graense: Option<Duration>) -> Self {
        StaleClock {
            start: Instant::now(),
            graense: graense.unwrap_or(Self::STANDARD_GRAENSE),
            sidste_frame: None,
        }
    }

    pub fn graense(&self) -> Duration {
        self.graense
    }

    /// Har vi nogensinde set en gyldig frame?
    pub fn har_set_frame(&self) -> bool {
        self.sidste_frame.is_some()
    }

    /// Marker at en FULDT AFKODET, gyldig frame netop ankom.
    pub fn frame_modtaget(&mut self) {
        self.sidste_frame = Some(self.start.elapsed());
    }

    /// Tid siden sidste gyldige frame, eller `None` hvis der aldrig kom en.
    pub fn siden_sidste_frame(&self) -> Option<Duration> {
        self.sidste_frame.map(|t| self.start.elapsed().saturating_sub(t))
    }

    /// Er kilden stale lige nu?
    pub fn er_stale(&self) -> bool {
        match self.siden_sidste_frame() {
            None => true,
            Some(d) => d >= self.graense,
        }
    }

    /// Glem den seneste frame - fx naar en forbindelse falder.
    pub fn nulstil(&mut self) {
        self.sidste_frame = None;
    }

    /// Tilstanden en visning skal vise lige nu, givet om auth er spaerret.
    pub fn status(&self, auth_spaerret: bool) -> SourceStatus {
        if auth_spaerret {
            return SourceStatus::AuthError;
        }
        if !self.har_set_frame() {
            return SourceStatus::Connecting;
        }
        if self.er_stale() {
            SourceStatus::Offline
        } else {
            SourceStatus::Live
        }
    }
}

/// Begraenset backoff ved genforbindelse.
///
/// Samme kurve som Python-vejen: start 1,5 s, fordobl, loft 30 s, og NULSTIL efter en session
/// der faktisk leverede frames - saa et netvaerks-blip heler hurtigt uden at hamre telefonen.
pub struct Backoff {
    start: Duration,
    loft: Duration,
    nu: Duration,
}

impl Backoff {
    pub fn new(start: Option<Duration>, loft: Option<Duration>) -> Self {
        let start = start.unwrap_or(Duration::from_millis(1500));
        Backoff { start, loft: loft.unwrap_or(Duration::from_secs(30)), nu: start }
    }

    pub fn naeste(&self) -> Duration {
        self.nu
    }

    pub fn fejlede(&mut self) {
        let dobbelt = self.nu * 2;
        self.nu = if dobbelt > self.loft { self.loft } else { dobbelt };
    }

    pub fn lykkedes(&mut self) {
        self.nu = self.start;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(None, None)
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn staleclock_uden_frame_er_stale_og_connecting() {
        let u = StaleClock::new(None);
        assert!(!u.har_set_frame());
        assert!(u.er_stale());
        assert_eq!(u.status(false), SourceStatus::Connecting);
        assert!(u.siden_sidste_frame().is_none());
    }

    #[test]
    fn staleclock_efter_frame_er_live() {
        let mut u = StaleClock::new(None);
        u.frame_modtaget();
        assert!(u.har_set_frame());
        assert!(!u.er_stale());
        assert_eq!(u.status(false), SourceStatus::Live);
    }

    #[test]
    fn staleclock_bliver_offline_naar_graensen_passeres() {
        let mut u = StaleClock::new(Some(Duration::from_millis(1)));
        u.frame_modtaget();
        std::thread::sleep(Duration::from_millis(12));
        assert!(u.er_stale());
        assert_eq!(u.status(false), SourceStatus::Offline);
    }

    #[test]
    fn auth_spaerret_slaar_alt_andet() {
        let mut u = StaleClock::new(None);
        u.frame_modtaget();
        // Selv en frisk frame maa ikke skjule en auth-fejl: de to kraever modsatte handlinger.
        assert_eq!(u.status(true), SourceStatus::AuthError);
    }

    #[test]
    fn nulstil_glemmer_framen() {
        let mut u = StaleClock::new(None);
        u.frame_modtaget();
        u.nulstil();
        assert!(!u.har_set_frame());
        assert_eq!(u.status(false), SourceStatus::Connecting);
    }

    #[test]
    fn backoff_fordobler_og_loftes() {
        let mut b = Backoff::new(Some(Duration::from_secs(1)), Some(Duration::from_secs(4)));
        assert_eq!(b.naeste(), Duration::from_secs(1));
        b.fejlede();
        assert_eq!(b.naeste(), Duration::from_secs(2));
        b.fejlede();
        assert_eq!(b.naeste(), Duration::from_secs(4));
        b.fejlede();
        assert_eq!(b.naeste(), Duration::from_secs(4), "loftet skal holde");
    }

    #[test]
    fn backoff_nulstilles_af_en_session_der_leverede() {
        let mut b = Backoff::new(Some(Duration::from_secs(1)), Some(Duration::from_secs(8)));
        b.fejlede();
        b.fejlede();
        b.lykkedes();
        assert_eq!(b.naeste(), Duration::from_secs(1));
    }

    #[test]
    fn backoff_default_er_python_vejens_kurve() {
        let b = Backoff::default();
        assert_eq!(b.naeste(), Duration::from_millis(1500));
    }
}
