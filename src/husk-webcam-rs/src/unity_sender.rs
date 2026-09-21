//! Skriver frames til et Unity Capture-DirectShow-filter over dets delte hukommelse.
//! Port af `UnityCaptureSender.cs`.
//!
//! ⛔ PROTOKOLLEN ER LAEST I KILDEN, IKKE I EN BLOGTEKST. Den staar i `Source/shared.inl` i
//! `schellingb/UnityCapture` (filteret er MIT), og hvert felt nedenfor er maalt der 2026-09-19.
//!
//! De fire ting det er nemt at tage fejl af, og som er maalt frem for antaget:
//!
//! 1. **Pixelformatet er RGBA, ikke BGRA.** Filteret konverterer med
//!    `(x&0xFF00FF00)|((x&0x00FF0000)>>16)|((x&0x000000FF)<<16)` paa en little-endian uint32,
//!    altsaa byte-raekkefoelgen R,G,B,A -> B,G,R,A. Husets [`Bitmap32`] er BGRA, saa senderen
//!    BYTTER R og B. Et graat testbillede kunne aldrig afsloere en forbyttet kanal
//!    (maaleregel 8) - derfor det asymmetriske moenster i testene.
//! 2. **`stride` taelles i PIXELS, ikke i bytes.** Unity-pluginnet sender `RowPitch / 4` for
//!    UINT8, og filteret indekserer med `(uint32*)buf + raekke*stride`.
//! 3. **`DataSize` taelles i BYTES** (`RowPitch * Height`).
//! 4. **Senderen OPRETTER `Want`-eventet, men AABNER mutex, `Sent`-event og mappingen.**
//!    Modtageren ejer dem. Kan de ikke aabnes, er der INGEN aktiv modtager - og det er en
//!    normal tilstand, ikke en fejl.
//!
//! ⚠️ Navnene og handle-taellingen er egenskaber ved DEN INSTALLEREDE Unity Capture-udgave,
//! ikke ved et Windows-API. En ny driverudgave kan skifte dem, og det skal kunne opdages som
//! en FEJL frem for som tavshed - derfor [`SendResultat::IngenModtager`] som eksplicit udfald.

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Memory::{
    FILE_MAP, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, OpenMutexW, ReleaseMutex, SYNCHRONIZATION_ACCESS_RIGHTS, SetEvent,
    WaitForSingleObject,
};
use windows::core::PCWSTR;

use crate::bitmap::Bitmap32;

/// Udfaldet af en [`UnityCaptureSender::send`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendResultat {
    /// Frame skrevet, og modtageren havde bedt om den.
    Ok,
    /// Frame skrevet, men modtageren naaede ikke at bede om den forrige.
    Frameskip,
    /// Frame for stor til det delte omraade.
    ForStor,
    /// Ingen modtager. Filteret har ikke oprettet det delte omraade.
    IngenModtager,
}

/// FORMAT_UINT8 i filterets enum.
pub const FORMAT_UINT8: u32 = 0;

/// RESIZEMODE_DISABLED / RESIZEMODE_LINEAR.
pub const RESIZE_DISABLED: u32 = 0;
pub const RESIZE_LINEAR: u32 = 1;

/// MIRRORMODE_DISABLED / MIRRORMODE_HORIZONTALLY.
pub const MIRROR_DISABLED: u32 = 0;
pub const MIRROR_HORIZONTAL: u32 = 1;

/// 3840 * 2160 * 4 * sizeof(short) - filterets eget loft.
pub const MAX_SHARED_IMAGE_SIZE: u64 = 3840 * 2160 * 4 * 2;

/// Header: maxSize, width, height, stride, format, resizemode, mirrormode, timeout.
/// Otte 4-byte felter, saa nyttelasten begynder paa 32.
const DATA_OFFSET: usize = 32;

/// Navne-praefikset for husets EGET, brandede filter.
pub const HUSK_PRAEFIKS: &str = "HuskWebcam";

/// Navne-praefikset for et umodificeret Unity Capture.
pub const UNITY_PRAEFIKS: &str = "UnityCapture";

/// `'z' - '0'`, filterets eget loft.
pub const MAX_CAP_NUM: i32 = (b'z' - b'0') as i32;

/// Hvad kalderen skal vise brugeren naar der ikke er nogen modtager.
///
/// Skriver IKKE "ukendt fejl": teksten navngiver baade den sandsynlige aarsag og den fil der
/// loeser den.
pub const INGEN_MODTAGER_FORKLARING: &str = concat!(
    "Kamera-filteret svarer ikke. Enten er 'Husk Webcam' ikke registreret paa denne PC, ",
    "eller ogsaa er der ingen app der bruger kameraet lige nu (filteret opretter foerst sin ",
    "delte hukommelse naar en modtager aabner enheden). Registrering: se docs/byg-fra-kilde.md."
);

// Adgangsrettigheder, skrevet ud frem for hentet fra et feature-gated modul.
const SYNCHRONIZE: u32 = 0x0010_0000;
const MUTEX_MODIFY_STATE: u32 = 0x0001;
const EVENT_MODIFY_STATE: u32 = 0x0002;
const FILE_MAP_ALL_ACCESS: FILE_MAP = FILE_MAP(0x000F_001F);

pub struct UnityCaptureSender {
    cap_num: i32,
    praefiks: String,
    mutex: Option<HANDLE>,
    want: Option<HANDLE>,
    sent: Option<HANDLE>,
    map: Option<HANDLE>,
    view: Option<MEMORY_MAPPED_VIEW_ADDRESS>,
    rgba: Vec<u8>,
}

// SAFETY: alle handles og viewet bruges KUN gennem &mut self, saa der er hoejst én bruger ad
// gangen. Windows tillader brug fra en anden traad end den der aabnede dem.
unsafe impl Send for UnityCaptureSender {}

impl UnityCaptureSender {
    /// `cap_num` 0 er standardenheden.
    ///
    /// Unity Capture kan installeres med flere enheder, og de faar hver sit suffiks paa alle
    /// fire navne.
    ///
    /// ⛔ Navne-praefikset er den ANDEN halvdel af brandingen, og den halvdel der faktisk
    /// isolerer. En egen CLSID skiller kun COM-REGISTRERINGEN; deler to filtre stadig mutex,
    /// events og mapping, sender de til hinandens klienter - og den fejl ligner et flimrende
    /// billede frem for en navnekollision.
    pub fn ny(cap_num: i32, praefiks: &str) -> Option<UnityCaptureSender> {
        if !(0..=MAX_CAP_NUM).contains(&cap_num) || praefiks.trim().is_empty() {
            return None;
        }
        Some(UnityCaptureSender {
            cap_num,
            praefiks: praefiks.to_string(),
            mutex: None,
            want: None,
            sent: None,
            map: None,
            view: None,
            rgba: Vec::new(),
        })
    }

    /// Standardenheden med husets eget filter.
    pub fn standard() -> UnityCaptureSender {
        Self::ny(0, HUSK_PRAEFIKS).expect("0 og husets praefiks er altid gyldige")
    }

    /// Det praefiks denne sender faktisk bruger. Til fejlbeskeder og maalinger.
    pub fn praefiks(&self) -> &str {
        &self.praefiks
    }

    fn navn(&self, basis: &str) -> String {
        // CapNum 0 bruger NUL-terminatoren, saa navnet er de 17 tegn uden ciffer - praecis som
        // den gamle filter-DLL foer multi-cap. Et hardkodet "0" ville ramme forbi.
        if self.cap_num == 0 {
            basis.to_string()
        } else {
            format!("{basis}{}", (b'0' + self.cap_num as u8) as char)
        }
    }

    pub fn mutex_navn(&self) -> String {
        self.navn(&format!("{}_Mutx", self.praefiks))
    }

    pub fn want_event_navn(&self) -> String {
        self.navn(&format!("{}_Want", self.praefiks))
    }

    pub fn sent_event_navn(&self) -> String {
        self.navn(&format!("{}_Sent", self.praefiks))
    }

    pub fn data_navn(&self) -> String {
        self.navn(&format!("{}_Data", self.praefiks))
    }

    /// Er der en aktiv modtager lige nu?
    pub fn forbundet(&self) -> bool {
        self.view.is_some()
    }

    /// Proev at forbinde til filterets delte hukommelse.
    ///
    /// Returnerer `false` naar der ikke er en modtager - en HELT normal tilstand, ikke en fejl,
    /// fordi filteret foerst opretter omraadet naar en DirectShow-klient aabner enheden.
    pub fn forbind(&mut self) -> bool {
        if self.view.is_some() {
            return true;
        }

        if self.mutex.is_none() {
            let navn = bred(&self.mutex_navn());
            let h = unsafe {
                OpenMutexW(
                    SYNCHRONIZATION_ACCESS_RIGHTS(SYNCHRONIZE | MUTEX_MODIFY_STATE),
                    false,
                    PCWSTR(navn.as_ptr()),
                )
            };
            match h {
                Ok(h) if !h.is_invalid() => self.mutex = Some(h),
                _ => return false,
            }
        }

        if self.want.is_none() {
            // Senderen OPRETTER Want. Findes det allerede, aabnes det - to sendere paa samme
            // enhed er en fejl vi fanger et andet sted (singleton-laasen).
            let navn = bred(&self.want_event_navn());
            let h = unsafe { CreateEventW(None, false, false, PCWSTR(navn.as_ptr())) };
            match h {
                Ok(h) if !h.is_invalid() => self.want = Some(h),
                _ => {
                    self.luk();
                    return false;
                }
            }
        }

        if self.sent.is_none() {
            let navn = bred(&self.sent_event_navn());
            let h = unsafe {
                OpenEventW(
                    SYNCHRONIZATION_ACCESS_RIGHTS(EVENT_MODIFY_STATE | SYNCHRONIZE),
                    false,
                    PCWSTR(navn.as_ptr()),
                )
            };
            match h {
                Ok(h) if !h.is_invalid() => self.sent = Some(h),
                _ => {
                    self.luk();
                    return false;
                }
            }
        }

        let navn = bred(&self.data_navn());
        let map =
            unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(navn.as_ptr())) };
        let map = match map {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                self.luk();
                return false;
            }
        };

        let view = unsafe { MapViewOfFile(map, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view.Value.is_null() {
            unsafe {
                let _ = CloseHandle(map);
            }
            self.luk();
            return false;
        }

        self.map = Some(map);
        self.view = Some(view);
        true
    }

    /// Send et billede.
    ///
    /// `billede` er BGRA (husets format); kanalerne byttes her, fordi filteret vil have RGBA.
    ///
    /// `spejlvend` overlades til FILTERET frem for at vende pixels selv.
    /// ⚠️ Brug den ikke som kompensation for at en moedeapp spejler sin EGENVISNING - det goer
    /// den kun for dig, og de andre deltagere ville da se teksten omvendt.
    pub fn send(&mut self, billede: &Bitmap32, spejlvend: bool, timeout_ms: i32) -> SendResultat {
        if self.view.is_none() && !self.forbind() {
            return SendResultat::IngenModtager;
        }

        let view = self.view.expect("forbind() satte viewet").Value as *mut u8;
        let bytes = billede.pixels().len();

        let max_size = unsafe { std::ptr::read_unaligned(view as *const u32) };
        if (max_size as usize) < bytes {
            // maxSize = 0 betyder at modtageren aldrig har sat den, altsaa at filteret er der
            // men ikke aktivt. Det er IKKE det samme som et for stort billede, og de to maa
            // ikke smelte sammen i ét udfald.
            return if max_size == 0 { SendResultat::IngenModtager } else { SendResultat::ForStor };
        }

        if self.rgba.len() != bytes {
            self.rgba = vec![0u8; bytes];
        }
        bgra_til_rgba_bund_op(billede.pixels(), &mut self.rgba, billede.bredde(), billede.hoejde());

        let mutex = self.mutex.expect("forbind() aabnede mutexen");
        let vent = unsafe { WaitForSingleObject(mutex, 2000) };
        let taget = vent == WAIT_OBJECT_0 || vent == WAIT_ABANDONED;
        if !taget {
            return SendResultat::IngenModtager;
        }

        if vent == WAIT_ABANDONED {
            // Modtageren doede mens den holdt mutexen. Vi HAR den nu, saa den skal stadig
            // frigives - og tilstanden er "ingen modtager", ikke "ukendt fejl".
            unsafe {
                let _ = ReleaseMutex(mutex);
            }
            return SendResultat::IngenModtager;
        }

        unsafe {
            let felter = [
                billede.bredde() as u32,
                billede.hoejde() as u32,
                billede.bredde() as u32, // stride i PIXELS, ikke bytes
                FORMAT_UINT8,
                RESIZE_LINEAR,
                if spejlvend { MIRROR_HORIZONTAL } else { MIRROR_DISABLED },
                timeout_ms as u32,
            ];
            for (i, v) in felter.iter().enumerate() {
                std::ptr::write_unaligned(view.add(4 + i * 4) as *mut u32, *v);
            }
            std::ptr::copy_nonoverlapping(self.rgba.as_ptr(), view.add(DATA_OFFSET), bytes);
            let _ = ReleaseMutex(mutex);
        }

        unsafe {
            let _ = SetEvent(self.sent.expect("forbind() aabnede Sent"));
        }

        // Frameskip: modtageren havde ikke bedt om en ny frame endnu. Ikke en fejl, men det tal
        // er det eneste der viser at vi producerer hurtigere end nogen forbruger.
        let bad = unsafe { WaitForSingleObject(self.want.expect("forbind() oprettede Want"), 0) };
        if bad == WAIT_OBJECT_0 {
            SendResultat::Ok
        } else {
            SendResultat::Frameskip
        }
    }

    fn luk(&mut self) {
        unsafe {
            if let Some(v) = self.view.take() {
                let _ = UnmapViewOfFile(v);
            }
            if let Some(h) = self.map.take() {
                let _ = CloseHandle(h);
            }
            if let Some(h) = self.sent.take() {
                let _ = CloseHandle(h);
            }
        }
    }
}

impl Drop for UnityCaptureSender {
    fn drop(&mut self) {
        self.luk();
        unsafe {
            if let Some(h) = self.want.take() {
                let _ = CloseHandle(h);
            }
            if let Some(h) = self.mutex.take() {
                let _ = CloseHandle(h);
            }
        }
    }
}

fn bred(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// BGRA -> RGBA, og raekkerne VENDT (sidste raekke foerst).
///
/// ⛔ Vendingen er MAALT, ikke antaget. Maalt 2026-09-19 paa Windows 11 med et asymmetrisk
/// firefelts-moenster (roed oeverst venstre, groen oeverst hoejre, blaa nederst venstre, hvid
/// nederst hoejre) sendt til det registrerede filter og laest af en SEPARAT DirectShow-klient
/// (pygrabber):
///
///   uden vending: modtageren saa hvid oeverst HOEJRE og groen nederst HOEJRE, altsaa billedet
///                 spejlet om den vandrette akse. Farverne var rigtige.
///
/// Det er DirectShows egen konvention for RGB-buffere med positiv `biHeight`: de er BUND-OP.
/// Filteret vender ikke selv, saa det skal senderen.
///
/// Kanalerne er derimod rigtige UDEN bytte ud over BGRA->RGBA: filteret konverterer med
/// `(x&0xFF00FF00)|((x&0x00FF0000)>>16)|((x&0x000000FF)<<16)`, altsaa RGBA->BGRA. Havde vi
/// gaettet paa BGRA ind, ville roed og blaa vaere byttet - og det kunne et graat testbillede
/// aldrig have vist (maaleregel 8).
pub fn bgra_til_rgba_bund_op(bgra: &[u8], rgba: &mut [u8], bredde: usize, hoejde: usize) {
    let stride = bredde * 4;
    for y in 0..hoejde {
        let fra = y * stride;
        let til = (hoejde - 1 - y) * stride;
        for x in (0..stride).step_by(4) {
            rgba[til + x] = bgra[fra + x + 2];
            rgba[til + x + 1] = bgra[fra + x + 1];
            rgba[til + x + 2] = bgra[fra + x];
            rgba[til + x + 3] = bgra[fra + x + 3];
        }
    }
}

/// Læs et felt ud af en RGBA-buffer der er skrevet BUND-OP. Til testene.
pub fn rgba_pixel(rgba: &[u8], bredde: usize, hoejde: usize, x: usize, y: usize) -> (u8, u8, u8, u8) {
    // y taelles fra BUNDEN i bufferen.
    let raekke = hoejde - 1 - y;
    let i = raekke * bredde * 4 + x * 4;
    (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3])
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    fn moenster(b: usize, h: usize) -> Bitmap32 {
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

    #[test]
    fn kanalerne_byttes_fra_bgra_til_rgba() {
        let mut bm = Bitmap32::new(1, 1);
        bm.saet_pixel(0, 0, 10, 20, 30, 40); // B=10 G=20 R=30 A=40
        let mut ud = vec![0u8; 4];
        bgra_til_rgba_bund_op(bm.pixels(), &mut ud, 1, 1);
        assert_eq!(ud, vec![30, 20, 10, 40], "R,G,B,A");
    }

    #[test]
    fn raekkerne_vendes_bund_op() {
        // ⛔ Vendingen er MAALT. Uden den saa modtageren billedet spejlet om den vandrette akse.
        let mut bm = Bitmap32::new(1, 2);
        bm.saet_pixel(0, 0, 0, 0, 255, 255); // oeverste raekke: roed
        bm.saet_pixel(0, 1, 255, 0, 0, 255); // nederste raekke: blaa
        let mut ud = vec![0u8; 8];
        bgra_til_rgba_bund_op(bm.pixels(), &mut ud, 1, 2);
        // Foerste raekke i bufferen skal vaere billedets NEDERSTE (blaa -> R=0,G=0,B=255).
        assert_eq!(&ud[0..4], &[0, 0, 255, 255]);
        assert_eq!(&ud[4..8], &[255, 0, 0, 255]);
    }

    #[test]
    fn det_asymmetriske_moenster_overlever_transformen() {
        // Fire entydige svar i ét billede: et graat eller symmetrisk billede ville bestaa
        // uanset hvad (maaleregel 8).
        let bm = moenster(4, 4);
        let mut ud = vec![0u8; 4 * 4 * 4];
        bgra_til_rgba_bund_op(bm.pixels(), &mut ud, 4, 4);
        assert_eq!(rgba_pixel(&ud, 4, 4, 0, 0), (255, 0, 0, 255), "oeverst venstre: roed");
        assert_eq!(rgba_pixel(&ud, 4, 4, 3, 0), (0, 255, 0, 255), "oeverst hoejre: groen");
        assert_eq!(rgba_pixel(&ud, 4, 4, 0, 3), (0, 0, 255, 255), "nederst venstre: blaa");
        assert_eq!(rgba_pixel(&ud, 4, 4, 3, 3), (255, 255, 255, 255), "nederst hoejre: hvid");
    }

    #[test]
    fn navnene_er_dem_filteret_lytter_paa() {
        let s = UnityCaptureSender::standard();
        assert_eq!(s.mutex_navn(), "HuskWebcam_Mutx");
        assert_eq!(s.want_event_navn(), "HuskWebcam_Want");
        assert_eq!(s.sent_event_navn(), "HuskWebcam_Sent");
        assert_eq!(s.data_navn(), "HuskWebcam_Data");
    }

    #[test]
    fn bgra_bliver_rgba_og_raekkerne_VENDES() {
        // 2x2, fire entydige pixels. Et graat eller symmetrisk billede kunne hverken afsloere
        // en byttet kanal eller en vendt raekkefoelge (maaleregel 8).
        let mut bm = Bitmap32::new(2, 2);
        bm.saet_pixel(0, 0, 0, 0, 255, 255); // roed  (BGRA 0,0,255)
        bm.saet_pixel(1, 0, 0, 255, 0, 255); // groen
        bm.saet_pixel(0, 1, 255, 0, 0, 255); // blaa
        bm.saet_pixel(1, 1, 255, 255, 255, 255); // hvid

        let mut ud = vec![0u8; bm.pixels().len()];
        bgra_til_rgba_bund_op(bm.pixels(), &mut ud, 2, 2);

        // Raekke 0 i UD skal vaere kildens SIDSTE raekke (blaa, hvid), i RGBA.
        assert_eq!(&ud[0..4], &[0, 0, 255, 255]);
        assert_eq!(&ud[4..8], &[255, 255, 255, 255]);
        // Raekke 1 i UD skal vaere kildens FOERSTE raekke (roed, groen).
        assert_eq!(&ud[8..12], &[255, 0, 0, 255]);
        assert_eq!(&ud[12..16], &[0, 255, 0, 255]);
    }

    #[test]
    fn capnum_NUL_bruger_navne_UDEN_ciffer() {
        // CapNum 0 bruger NUL-terminatoren; et hardkodet "0" ville ramme forbi.
        let s = UnityCaptureSender::ny(0, UNITY_PRAEFIKS).unwrap();
        assert_eq!(s.mutex_navn(), "UnityCapture_Mutx");
        assert_eq!(s.data_navn(), "UnityCapture_Data");
    }

    #[test]
    fn capnum_OVER_NUL_faar_sit_ciffer() {
        let s = UnityCaptureSender::ny(3, UNITY_PRAEFIKS).unwrap();
        assert_eq!(s.mutex_navn(), "UnityCapture_Mutx3");
        assert_eq!(s.data_navn(), "UnityCapture_Data3");
        assert_eq!(UnityCaptureSender::ny(3, HUSK_PRAEFIKS).unwrap().mutex_navn(), "HuskWebcam_Mutx3");
    }

    #[test]
    fn DEFAULT_er_husets_EGET_filter_ikke_unity_capture() {
        let s = UnityCaptureSender::standard();
        assert_eq!(s.praefiks(), HUSK_PRAEFIKS);
        assert_ne!(s.praefiks(), UNITY_PRAEFIKS);
    }

    #[test]
    fn de_to_FILTRE_SKAL_have_FORSKELLIGE_delte_navne() {
        // ⛔ Det er den halvdel af brandingen der faktisk ISOLERER. En egen CLSID skiller kun
        // COM-registreringen; deler to filtre stadig mutex, events og mapping, sender de til
        // hinandens klienter - og den fejl ligner et flimrende billede frem for en kollision.
        let husk = UnityCaptureSender::ny(0, HUSK_PRAEFIKS).unwrap();
        let unity = UnityCaptureSender::ny(0, UNITY_PRAEFIKS).unwrap();
        assert_ne!(husk.mutex_navn(), unity.mutex_navn());
        assert_ne!(husk.want_event_navn(), unity.want_event_navn());
        assert_ne!(husk.sent_event_navn(), unity.sent_event_navn());
        assert_ne!(husk.data_navn(), unity.data_navn());
    }

    #[test]
    fn et_TOMT_praefiks_AFVISES() {
        assert!(UnityCaptureSender::ny(0, "   ").is_none());
        assert!(UnityCaptureSender::ny(0, "").is_none());
    }

    #[test]
    fn ugyldigt_capnum_AFVISES_negativt() {
        assert!(UnityCaptureSender::ny(-1, "X").is_none());
    }

    #[test]
    fn ugyldigt_capnum_AFVISES_over_loftet() {
        assert!(UnityCaptureSender::ny(MAX_CAP_NUM + 1, "X").is_none());
    }

    #[test]
    fn ALFA_kanalen_bevares() {
        let mut bm = Bitmap32::new(1, 1);
        bm.saet_pixel(0, 0, 10, 20, 30, 77);
        let mut ud = vec![0u8; 4];
        bgra_til_rgba_bund_op(bm.pixels(), &mut ud, 1, 1);
        assert_eq!(ud[3], 77);
    }

    #[test]
    fn uden_et_registreret_filter_er_svaret_INGEN_MODTAGER() {
        // ⛔ En NORMAL tilstand, ikke en fejl: filteret opretter foerst sin delte hukommelse
        // naar en DirectShow-klient aabner enheden. Benet bruger et praefiks ingen bruger, saa
        // det er deterministisk ogsaa paa en maskine hvor filteret ER installeret.
        let mut s = UnityCaptureSender::ny(0, "HuskWebcamTestIngenModtager").unwrap();
        assert!(!s.forbind());
        assert!(!s.forbundet());
        let bm = moenster(8, 8);
        assert_eq!(s.send(&bm, false, 1000), SendResultat::IngenModtager);
    }

    #[test]
    fn forklaringen_navngiver_baade_aarsag_og_kur() {
        // Skriver IKKE "ukendt fejl".
        assert!(INGEN_MODTAGER_FORKLARING.contains("Husk Webcam"));
        assert!(INGEN_MODTAGER_FORKLARING.contains("docs/byg-fra-kilde.md"));
    }
}
