//! Efterspoergsels-maalingen: ER der nogen der ser med?
//!
//! Handle-taellingen, 8-sekunders-henstanden og 15-sekunders-genforsoeget laa i den
//! tidligere udgave spredt paa tre steder, og en laeser maatte vide hvilket der bar hvad.
//! Her er de ÉT sted med én kilde til sandheden om tilstanden.
//!
//! ⚠️ Baade MUTEXNAVNET og handle-taellingen er egenskaber ved DEN INSTALLEREDE Unity
//! Capture-udgave, ikke ved et Windows-API. En ny driverudgave kan aendre begge. Derfor
//! returnerer [`unity_demand::tael`] en EKSPLICIT [`HandleTaelling::utilgaengelig`] frem for 0,
//! saa "driveren svarer ikke som vi tror" kan opdages som en fejl i stedet for at ligne "ingen
//! ser med" (maaleregel 116).

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenMutexW, ReleaseMutex, SYNCHRONIZATION_ACCESS_RIGHTS,
};
use windows::core::PCWSTR;

/// Resultatet af en taelling. Tre udfald, ikke to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleTaelling {
    pub antal: i32,
    pub maalt: bool,
    pub fejl: Option<String>,
}

impl HandleTaelling {
    /// Filteret findes ikke i hukommelsen: der er ingen modtager. En MAALING.
    pub fn nul() -> Self {
        HandleTaelling { antal: 0, maalt: true, fejl: None }
    }

    pub fn taelt(antal: i32) -> Self {
        HandleTaelling { antal, maalt: true, fejl: None }
    }

    /// Vi kunne ikke maale. IKKE det samme som nul.
    pub fn utilgaengelig(grund: impl Into<String>) -> Self {
        HandleTaelling { antal: -1, maalt: false, fejl: Some(grund.into()) }
    }
}

pub mod unity_demand {
    use super::*;

    const SYNCHRONIZE: SYNCHRONIZATION_ACCESS_RIGHTS = SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0000);

    // NtQueryObject er ikke i den offentlige `windows`-flade. Erklaeringen staar her frem for
    // at vi skal tage et helt Wdk-modul med for ét kald - praecis som C#-udgavens DllImport.
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryObject(
            handle: HANDLE,
            info_class: i32,
            info: *mut core::ffi::c_void,
            info_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    /// Antal handles paa filterets mutex, fratrukket vores eget maale-handle.
    ///
    /// Senderen holder selv ét handle under streaming; kalderen skal traekke det fra hvis den
    /// selv sender (se [`super::DemandMonitor`]).
    pub fn tael(mutex_navn: &str) -> HandleTaelling {
        let navn: Vec<u16> = mutex_navn.encode_utf16().chain(std::iter::once(0)).collect();
        let h = unsafe { OpenMutexW(SYNCHRONIZE, false, PCWSTR(navn.as_ptr())) };

        let h = match h {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                let fejl = unsafe { GetLastError() };
                return if fejl == ERROR_FILE_NOT_FOUND {
                    HandleTaelling::nul()
                } else {
                    HandleTaelling::utilgaengelig(format!("OpenMutexW fejlede med {}", fejl.0))
                };
            }
        };

        // OBJECT_BASIC_INFORMATION: [0]=Attributes, [1]=GrantedAccess, [2]=HandleCount.
        let mut buf = [0u32; 14];
        let status = unsafe {
            NtQueryObject(
                h,
                0,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                std::mem::size_of_val(&buf) as u32,
                std::ptr::null_mut(),
            )
        };
        unsafe {
            let _ = CloseHandle(h);
        }

        if status != 0 {
            return HandleTaelling::utilgaengelig(format!("NtQueryObject fejlede med 0x{status:08X}"));
        }

        HandleTaelling::taelt((buf[2] as i32 - 1).max(0))
    }

    /// Standardnavnet for et umodificeret Unity Capture.
    pub const UNITY_MUTEX: &str = "UnityCapture_Mutx";
}

/// Afgoer om kameraet skal levere lige nu, og holder 8-sekunders-henstanden.
///
/// Reglerne er Python-vejens, samlet:
/// - poll hvert 0,5 s,
/// - stop foerst efter 8 sekunder UDEN en modtager (henstanden),
/// - appens EGET aabne preview taeller ogsaa som en modtager,
/// - Pause stopper optagelsen UANSET modtagere, og Afslut stopper alt.
///
/// Henstanden maa hente frames - nul-trafik-kravet gaelder foerst EFTER stop. Det er ikke en
/// detalje: en test der kraever nul frames fra det oejeblik den sidste modtager lukker, ville
/// maale henstanden som en fejl.
pub struct DemandMonitor {
    ur: Instant,
    sidste_modtager: Option<Duration>,
    henstand: Duration,
    poll_interval: Duration,
    /// Appens eget preview er aabent. Taeller som en modtager.
    pub preview_aabent: bool,
    /// Brugeren har trykket Pause. Stopper UANSET modtagere.
    pub pauseret: bool,
    sidste_modtagere: i32,
    sidste_maalefejl: Option<String>,
    leverer: bool,
}

impl DemandMonitor {
    pub fn ny(henstand: Option<Duration>, poll_interval: Option<Duration>) -> DemandMonitor {
        DemandMonitor {
            ur: Instant::now(),
            sidste_modtager: None,
            henstand: henstand.unwrap_or(Duration::from_secs(8)),
            poll_interval: poll_interval.unwrap_or(Duration::from_millis(500)),
            preview_aabent: false,
            pauseret: false,
            sidste_modtagere: -1,
            sidste_maalefejl: None,
            leverer: false,
        }
    }

    /// 8 sekunder. Vaerdien er arvet fra den tidligere udgaves on-demand-loekke.
    pub fn henstand(&self) -> Duration {
        self.henstand
    }

    /// 0,5 sekund, som i Python-vejen.
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// Sidste maalte antal DirectShow-modtagere, eller -1 hvis det ikke kunne maales.
    pub fn sidste_modtagere(&self) -> i32 {
        self.sidste_modtagere
    }

    /// Hvorfor maalingen sidst ikke kunne tages. `None` naar den kunne.
    pub fn sidste_maalefejl(&self) -> Option<&str> {
        self.sidste_maalefejl.as_deref()
    }

    /// Leverer vi lige nu?
    pub fn leverer(&self) -> bool {
        self.leverer
    }

    /// Ét gennemloeb.
    ///
    /// `taelling` injiceres, saa benene kan fremprovokere enhver tilstand - ogsaa den
    /// umaalelige - uden en rigtig driver.
    pub fn tick(&mut self, taelling: &HandleTaelling) -> bool {
        self.sidste_maalefejl = taelling.fejl.clone();

        if self.pauseret {
            self.sidste_modtagere = if taelling.maalt { taelling.antal } else { -1 };
            self.sidste_modtager = None;
            self.leverer = false;
            return false;
        }

        let mut har_modtager;
        if taelling.maalt {
            self.sidste_modtagere = taelling.antal;
            har_modtager = taelling.antal > 0;
        } else {
            // Kan tilstanden ikke MAALES, er svaret "behandl den som optaget" (maaleregel 83).
            // Et gaet paa "ingen ser med" ville slukke kameraet midt i et moede.
            self.sidste_modtagere = -1;
            har_modtager = true;
        }

        if self.preview_aabent {
            har_modtager = true;
        }

        if har_modtager {
            self.sidste_modtager = Some(self.ur.elapsed());
            self.leverer = true;
            return true;
        }

        let Some(sidst) = self.sidste_modtager else {
            self.leverer = false;
            return false;
        };

        let inden_for = self.ur.elapsed().saturating_sub(sidst) < self.henstand;
        self.leverer = inden_for;
        if !inden_for {
            self.sidste_modtager = None;
        }
        self.leverer
    }

    /// Tid tilbage af henstanden, eller `None` naar den ikke loeber.
    pub fn henstand_tilbage(&self) -> Option<Duration> {
        let s = self.sidste_modtager?;
        if self.leverer && self.sidste_modtagere == 0 && !self.preview_aabent {
            Some(self.henstand.saturating_sub(self.ur.elapsed().saturating_sub(s)))
        } else {
            None
        }
    }
}

impl Default for DemandMonitor {
    fn default() -> Self {
        Self::ny(None, None)
    }
}

/// Praecis ÉN sender pr. Windows-session. Port af `singleton()` i `unity_demand.py`.
///
/// To sendere paa samme enhed ville skiftes til at overskrive hinandens frames, og billedet
/// ville se ud som om det flimrede mellem to kilder.
pub struct SingletonLaas {
    mutex: Option<HANDLE>,
}

// SAFETY: handlet roeres kun af ejeren, og frigives i Drop paa den traad der holder det.
unsafe impl Send for SingletonLaas {}

impl SingletonLaas {
    pub const STANDARD_NAVN: &'static str = "HuskWebcam_Sender";

    /// Tag laasen, eller returnér `None` hvis en anden instans allerede har den.
    pub fn tag(navn: &str) -> Option<SingletonLaas> {
        let w: Vec<u16> = navn.encode_utf16().chain(std::iter::once(0)).collect();
        let h = unsafe { CreateMutexW(None, true, PCWSTR(w.as_ptr())) }.ok()?;
        if h.is_invalid() {
            return None;
        }

        // ⛔ Doem paa ERROR_ALREADY_EXISTS, IKKE paa et forsoeg paa at tage mutexen. En mutex er
        // RE-ENTRANT for den traad der ejer den, saa et forsoeg paa at tage den ville lykkes
        // for os selv - og laasen ville da ikke udelukke en anden instans i SAMME proces.
        // Maalt 2026-09-19 paa C#-siden: benet gik roedt praecis paa den sag.
        //
        // Et navngivet mutex-OBJEKT lever kun saa laenge mindst ét handle er aabent, saa
        // "fandtes den allerede" er et paalideligt vidne om en LEVENDE holder - et doedt
        // program efterlader ikke objektet.
        let fandtes = unsafe { GetLastError() } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
        if fandtes {
            unsafe {
                let _ = CloseHandle(h);
            }
            return None;
        }

        Some(SingletonLaas { mutex: Some(h) })
    }

    /// Standardnavnet.
    pub fn tag_standard() -> Option<SingletonLaas> {
        Self::tag(Self::STANDARD_NAVN)
    }
}

impl Drop for SingletonLaas {
    fn drop(&mut self) {
        if let Some(h) = self.mutex.take() {
            unsafe {
                let _ = ReleaseMutex(h);
                let _ = CloseHandle(h);
            }
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn et_filter_der_ikke_findes_taelles_som_NUL_ikke_som_umaaleligt() {
        // Forskellen er hele pointen: nul er en MAALING, utilgaengelig er fravaeret af en.
        let t = unity_demand::tael("HuskWebcamFindesHelSikkertIkke_Mutx");
        assert!(t.maalt, "et fravaerende filter ER en maaling");
        assert_eq!(t.antal, 0);
        assert!(t.fejl.is_none());
    }

    #[test]
    fn en_mutex_vi_selv_holder_kan_taelles() {
        // Positiv kontrol: uden den kan "nul" ikke skelnes fra "taelleren virker ikke"
        // (maaleregel 7c).
        let navn = format!("HuskWebcamTestTael_{}", std::process::id());
        let laas = SingletonLaas::tag(&navn).expect("laasen skal kunne tages");
        let t = unity_demand::tael(&navn);
        assert!(t.maalt);
        assert_eq!(t.antal, 1, "vores eget handle, minus maale-handlet");
        drop(laas);
    }

    #[test]
    fn singleton_udelukker_en_ANDEN_instans_i_samme_proces() {
        let navn = format!("HuskWebcamTestSingleton_{}", std::process::id());
        let foerste = SingletonLaas::tag(&navn).expect("foerste skal lykkes");
        assert!(SingletonLaas::tag(&navn).is_none(), "anden maa IKKE lykkes");
        drop(foerste);
    }

    #[test]
    fn singleton_laasen_kan_tages_igen_efter_frigivelse() {
        // Modvidne: en laas der ALDRIG kan tages igen, er ikke et vaern men en permanent
        // spaerre - og den ville goere en genstart af appen umulig.
        let navn = format!("HuskWebcamTestGenTag_{}", std::process::id());
        let a = SingletonLaas::tag(&navn).expect("foerste skal lykkes");
        drop(a);
        assert!(SingletonLaas::tag(&navn).is_some(), "laasen kunne ikke tages igen");
    }

    #[test]
    fn en_modtager_faar_os_til_at_levere() {
        let mut d = DemandMonitor::default();
        assert!(d.tick(&HandleTaelling::taelt(1)));
        assert!(d.leverer());
        assert_eq!(d.sidste_modtagere(), 1);
    }

    #[test]
    fn nul_modtagere_holder_HENSTANDEN_foer_der_stoppes() {
        let mut d = DemandMonitor::ny(Some(Duration::from_millis(60)), None);
        d.tick(&HandleTaelling::taelt(1));
        assert!(d.tick(&HandleTaelling::nul()), "henstanden loeber stadig");
        std::thread::sleep(Duration::from_millis(80));
        assert!(!d.tick(&HandleTaelling::nul()), "henstanden er udloebet");
        assert!(!d.leverer());
    }

    #[test]
    fn en_UMAALELIG_tilstand_behandles_som_OPTAGET() {
        // Et gaet paa "ingen ser med" ville slukke kameraet midt i et moede (maaleregel 83).
        let mut d = DemandMonitor::default();
        assert!(d.tick(&HandleTaelling::utilgaengelig("driveren svarer ikke")));
        assert!(d.leverer());
        assert_eq!(d.sidste_modtagere(), -1, "-1 er IKKE nul");
        assert_eq!(d.sidste_maalefejl(), Some("driveren svarer ikke"));
    }

    #[test]
    fn en_MAALT_NUL_er_IKKE_det_samme_som_en_UMAALELIG_tilstand() {
        // De to maa aldrig smelte sammen: den ene betyder "ingen ser med", den anden "vi ved
        // det ikke" (maaleregel 116).
        let mut d = DemandMonitor::ny(Some(Duration::ZERO), None);
        d.tick(&HandleTaelling::taelt(1));
        assert!(!d.tick(&HandleTaelling::nul()));
        assert_eq!(d.sidste_modtagere(), 0);
        assert!(d.sidste_maalefejl().is_none());
    }

    #[test]
    fn to_modtagere_og_lukning_af_den_ENE_stopper_IKKE_leveringen() {
        let mut d = DemandMonitor::ny(Some(Duration::from_millis(50)), None);
        assert!(d.tick(&HandleTaelling::taelt(2)));
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            d.tick(&HandleTaelling::taelt(1)),
            "leveringen stoppede da den ene af to modtagere lukkede"
        );
    }

    #[test]
    fn henstanden_NULSTILLES_af_en_ny_modtager() {
        let mut d = DemandMonitor::ny(Some(Duration::from_millis(250)), None);
        d.tick(&HandleTaelling::taelt(1));
        d.tick(&HandleTaelling::nul());
        std::thread::sleep(Duration::from_millis(180));
        d.tick(&HandleTaelling::taelt(1)); // ny modtager midt i henstanden
        std::thread::sleep(Duration::from_millis(180));
        assert!(
            d.tick(&HandleTaelling::nul()),
            "henstanden blev ikke nulstillet af den nye modtager"
        );
    }

    #[test]
    fn preview_taeller_som_en_modtager() {
        let mut d = DemandMonitor::default();
        d.preview_aabent = true;
        assert!(d.tick(&HandleTaelling::nul()));
        assert!(d.leverer());
    }

    #[test]
    fn pause_stopper_UANSET_modtagere() {
        let mut d = DemandMonitor::default();
        d.pauseret = true;
        assert!(!d.tick(&HandleTaelling::taelt(3)));
        assert!(!d.leverer());
    }

    #[test]
    fn pause_ophaever_OGSAA_previewet() {
        // Appens eget preview taeller normalt som en modtager. Pause skal slaa DEN med;
        // ellers ville "Pause" ikke stoppe noget saa laenge vinduet stod aabent.
        let mut d = DemandMonitor::default();
        d.preview_aabent = true;
        d.pauseret = true;
        assert!(!d.tick(&HandleTaelling::nul()));
    }

    #[test]
    fn henstand_tilbage_loeber_kun_naar_den_faktisk_loeber() {
        let mut d = DemandMonitor::ny(Some(Duration::from_secs(8)), None);
        d.tick(&HandleTaelling::taelt(1));
        assert!(d.henstand_tilbage().is_none(), "der ER en modtager");
        d.tick(&HandleTaelling::nul());
        let t = d.henstand_tilbage().expect("henstanden loeber nu");
        assert!(t <= Duration::from_secs(8) && t > Duration::from_secs(7), "faktisk: {t:?}");
    }

    #[test]
    fn standardvaerdierne_er_python_vejens() {
        let d = DemandMonitor::default();
        assert_eq!(d.henstand(), Duration::from_secs(8));
        assert_eq!(d.poll_interval(), Duration::from_millis(500));
    }
}
