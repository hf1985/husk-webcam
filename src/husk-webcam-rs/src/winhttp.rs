//! HTTP over WinHTTP - Windows' egen HTTP-klient.
//!
//! ⛔ DETTE VALG FLYTTER MEST PAA STOERRELSEN AF DEN UDGIVNE BINAER.
//!
//! Appens netvaerksbehov er ÉN chunked HTTP GET mod telefonen, plus fire smaa GET'er til
//! `/healthz`, `/flags`, `/sensor` og `/set`. En fuld HTTP-stak i pakken (TLS, HTTP/2,
//! forbindelses-pulje, certifikat-roedder) ville koste flere MB for den ene ting - og Windows
//! har allerede stakken i `winhttp.dll`.
//!
//! Modulet er bevidst SMALT: GET, statuskode, Content-Type, og en laeser der leverer bytes som
//! de kommer. Ingen genbrugspulje, ingen omdirigerings-politik ud over WinHTTP's egen, ingen
//! cookies. Alt det ville vaere kode uden en kalder.

use std::ffi::c_void;

use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE, WINHTTP_INTERNET_SCHEME_HTTPS,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_QUERY_CONTENT_TYPE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect,
    WinHttpCrackUrl, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::PCWSTR;

/// Fejl fra HTTP-laget. Tre klasser, ikke én: kalderen skal kunne skelne dem.
#[derive(Debug)]
pub enum HttpFejl {
    /// URL'en kunne ikke laeses. En fejl i KALDEREN, ikke i netvaerket.
    UgyldigUrl(String),
    /// Vi naaede ikke frem, eller forbindelsen faldt. Windows' fejlkode foelger med.
    Netvaerk(String),
}

impl std::fmt::Display for HttpFejl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpFejl::UgyldigUrl(s) => write!(f, "ugyldig URL: {s}"),
            HttpFejl::Netvaerk(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for HttpFejl {}

/// Et WinHTTP-handle der lukker sig selv.
///
/// ⛔ Uden Drop laekker hvert genforbindelses-forsoeg et handle. En kilde der genforbinder
/// hvert sekund i et doegn ville da bruge 86.400 handles - og symptomet ville vaere at appen
/// holdt op med at kunne forbinde, ikke at noget laekkede.
struct Handle(*mut c_void);

// SAFETY: WinHTTP-handles er traad-agile; dokumentationen tillader brug fra flere traade saa
// laenge kalderen selv serialiserer kald paa SAMME handle. Det goer vi: hvert handle ejes af
// én struktur ad gangen.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn sidste_fejl(hvad: &str) -> HttpFejl {
    let kode = unsafe { windows::Win32::Foundation::GetLastError() };
    HttpFejl::Netvaerk(format!("{hvad} fejlede (Windows-fejl {})", kode.0))
}

fn bred(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// De dele af en URL WinHTTP skal bruge.
struct Delt {
    vaert: Vec<u16>,
    port: u16,
    sti: Vec<u16>,
    sikker: bool,
}

fn opdel(url: &str) -> Result<Delt, HttpFejl> {
    let w = bred(url);
    let mut k = URL_COMPONENTS { dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32, ..Default::default() };
    // Et laengde-felt paa -1 (dvs. u32::MAX) beder WinHTTP om at UDPEGE delen i vores egen
    // streng frem for at kopiere den ud. Det er den eneste form der ikke kraever at vi gaetter
    // en bufferstoerrelse.
    k.dwSchemeLength = u32::MAX;
    k.dwHostNameLength = u32::MAX;
    k.dwUrlPathLength = u32::MAX;
    k.dwExtraInfoLength = u32::MAX;

    unsafe {
        // ⛔ FLAGENE SKAL VAERE NUL. `ICU_ESCAPE` og `ICU_DECODE` faar WinHTTP til at OMSKRIVE
        // delene, og en omskrivning kraever at kalderen selv leverer buffere - den kan ikke
        // kombineres med laengde-feltet `u32::MAX`, som netop beder om pegere ind i vores egen
        // streng. Maalt her 2026-09-20: `ICU_ESCAPE` gav `E_INVALIDARG` paa ENHVER gyldig URL,
        // og fejlen lignede en uparsebar adresse frem for et forkert flag.
        WinHttpCrackUrl(&w[..w.len() - 1], 0, &mut k)
            .map_err(|e| HttpFejl::UgyldigUrl(format!("{url} ({e})")))?;
    }

    if k.lpszHostName.is_null() || k.dwHostNameLength == 0 {
        return Err(HttpFejl::UgyldigUrl(url.to_string()));
    }

    let vaert: Vec<u16> = unsafe {
        std::slice::from_raw_parts(k.lpszHostName.0, k.dwHostNameLength as usize)
    }
    .iter()
    .copied()
    .chain(std::iter::once(0))
    .collect();

    // Sti og query skal sendes SAMMEN som "objektet": deler man dem, taber query'en - og et
    // tabt ?token= ser ud som en adgangsfejl paa telefonen.
    let mut sti: Vec<u16> = Vec::new();
    if !k.lpszUrlPath.is_null() {
        sti.extend_from_slice(unsafe {
            std::slice::from_raw_parts(k.lpszUrlPath.0, k.dwUrlPathLength as usize)
        });
    }
    if !k.lpszExtraInfo.is_null() {
        sti.extend_from_slice(unsafe {
            std::slice::from_raw_parts(k.lpszExtraInfo.0, k.dwExtraInfoLength as usize)
        });
    }
    if sti.is_empty() {
        sti.extend_from_slice(&bred("/")[..1]);
    }
    sti.push(0);

    let sikker = k.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;
    let port = if k.nPort == 0 {
        if sikker { INTERNET_DEFAULT_HTTPS_PORT as u16 } else { 80 }
    } else {
        k.nPort
    };

    Ok(Delt { vaert, port, sti, sikker })
}

/// Et svar med hele kroppen laest ind.
#[derive(Debug)]
pub struct Svar {
    pub status: u32,
    pub content_type: Option<String>,
    pub krop: Vec<u8>,
}

impl Svar {
    pub fn er_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn krop_som_tekst(&self) -> String {
        String::from_utf8_lossy(&self.krop).into_owned()
    }
}

/// Vores User-Agent, ÉT sted og afledt af pakkeversionen.
///
/// ⛔ Her stod `"husk-webcam-rs/1.1"` som en literal seks steder, mens `Cargo.toml` stod på en
/// helt anden version. Det var en versionskilde mere end de tre versions-gaten måler, og den
/// eneste der ikke kunne fejle synligt: en forkert UA ser ud præcis som en rigtig.
/// Telefonen læser den ikke, så feltet er til logge og fejlsøgning - og dér er en version der
/// lyver værre end ingen version.
pub const BRUGERAGENT: &str = concat!("husk-webcam-rs/", env!("CARGO_PKG_VERSION"));

/// En levende WinHTTP-session. Genbruges af alle kald fra samme sted.
pub struct Session {
    session: Handle,
}

impl Session {
    pub fn ny(agent: &str) -> Result<Session, HttpFejl> {
        let a = bred(agent);
        let h = unsafe {
            WinHttpOpen(
                PCWSTR(a.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        };
        if h.is_null() {
            return Err(sidste_fejl("WinHttpOpen"));
        }
        Ok(Session { session: Handle(h) })
    }

    /// Hent en hel ressource. Til de smaa JSON-kald.
    pub fn get(&self, url: &str, frist_ms: u32) -> Result<Svar, HttpFejl> {
        let mut st = self.aabn(url, frist_ms, frist_ms)?;
        let mut krop = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = st.laes(&mut buf)?;
            if n == 0 {
                break;
            }
            krop.extend_from_slice(&buf[..n]);
            // Et loft, saa et svar der aldrig slutter ikke kan spise hukommelsen. /flags og
            // /sensor er nogle faa hundrede bytes; 8 MB er rigeligt og stadig en graense.
            if krop.len() > 8 * 1024 * 1024 {
                return Err(HttpFejl::Netvaerk("svaret var over 8 MB".into()));
            }
        }
        Ok(Svar { status: st.status, content_type: st.content_type.clone(), krop })
    }

    /// Aabn en stroem. Headerne er laest naar den returnerer; kroppen laeses med
    /// [`Stroem::laes`] indtil den giver 0.
    pub fn aabn(&self, url: &str, forbind_ms: u32, laes_ms: u32) -> Result<Stroem, HttpFejl> {
        let d = opdel(url)?;

        let forbind = unsafe {
            WinHttpConnect(self.session.0, PCWSTR(d.vaert.as_ptr()), d.port, 0)
        };
        if forbind.is_null() {
            return Err(sidste_fejl("WinHttpConnect"));
        }
        let forbind = Handle(forbind);

        let flag = if d.sikker { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let get = bred("GET");
        let anmodning = unsafe {
            WinHttpOpenRequest(
                forbind.0,
                PCWSTR(get.as_ptr()),
                PCWSTR(d.sti.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flag,
            )
        };
        if anmodning.is_null() {
            return Err(sidste_fejl("WinHttpOpenRequest"));
        }
        let anmodning = Handle(anmodning);

        unsafe {
            // Fristerne saettes PR. FASE. En samlet timeout ville draebe en SUND stroem, fordi
            // en MJPEG-stroem per definition aldrig slutter.
            let _ = WinHttpSetTimeouts(
                anmodning.0,
                forbind_ms as i32,
                forbind_ms as i32,
                forbind_ms as i32,
                laes_ms as i32,
            );
        }

        let hdr = bred(&format!("User-Agent: {}\r\n", BRUGERAGENT));
        unsafe {
            let _ = WinHttpAddRequestHeaders(
                anmodning.0,
                &hdr[..hdr.len() - 1],
                WINHTTP_ADDREQ_FLAG_ADD,
            );
        }

        unsafe {
            WinHttpSendRequest(anmodning.0, None, None, 0, 0, 0)
                .map_err(|e| HttpFejl::Netvaerk(format!("kunne ikke sende anmodningen ({e})")))?;
            WinHttpReceiveResponse(anmodning.0, std::ptr::null_mut())
                .map_err(|e| HttpFejl::Netvaerk(format!("intet svar ({e})")))?;
        }

        let status = query_tal(&anmodning, WINHTTP_QUERY_STATUS_CODE)?;
        let content_type = query_tekst(&anmodning, WINHTTP_QUERY_CONTENT_TYPE);

        Ok(Stroem { _forbind: forbind, anmodning, status, content_type })
    }
}

/// En aaben stroem: headerne er laest, kroppen laeses bid for bid.
pub struct Stroem {
    // Forbindelses-handlet SKAL leve saa laenge anmodningen goer. Uden feltet lukkes det ved
    // slutningen af aabn(), og laesningen ville fejle med et handle der ser gyldigt ud.
    _forbind: Handle,
    anmodning: Handle,
    pub status: u32,
    pub content_type: Option<String>,
}

impl Stroem {
    /// Laes den naeste bid. 0 betyder at serveren lukkede.
    pub fn laes(&mut self, buf: &mut [u8]) -> Result<usize, HttpFejl> {
        let mut n = 0u32;
        unsafe {
            WinHttpReadData(
                self.anmodning.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut n,
            )
            .map_err(|e| HttpFejl::Netvaerk(format!("laesningen faldt ({e})")))?;
        }
        Ok(n as usize)
    }
}

fn query_tal(h: &Handle, niveau: u32) -> Result<u32, HttpFejl> {
    let mut vaerdi = 0u32;
    let mut stoerrelse = std::mem::size_of::<u32>() as u32;
    let mut indeks = 0u32;
    unsafe {
        WinHttpQueryHeaders(
            h.0,
            niveau | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut vaerdi as *mut u32 as *mut c_void),
            &mut stoerrelse,
            &mut indeks,
        )
        .map_err(|e| HttpFejl::Netvaerk(format!("kunne ikke laese statuskoden ({e})")))?;
    }
    Ok(vaerdi)
}

fn query_tekst(h: &Handle, niveau: u32) -> Option<String> {
    let mut stoerrelse = 0u32;
    let mut indeks = 0u32;
    // Foerste kald fejler MED den noedvendige stoerrelse. Det er WinHTTP's egen kontrakt, og
    // en fejl her er derfor ikke en fejl.
    unsafe {
        let _ = WinHttpQueryHeaders(h.0, niveau, PCWSTR::null(), None, &mut stoerrelse, &mut indeks);
    }
    if stoerrelse == 0 {
        return None;
    }
    let mut buf = vec![0u16; (stoerrelse as usize / 2) + 1];
    unsafe {
        WinHttpQueryHeaders(
            h.0,
            niveau,
            PCWSTR::null(),
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut stoerrelse,
            &mut indeks,
        )
        .ok()?;
    }
    let laengde = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..laengde]))
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn ugyldig_url_afvises_som_KALDER_fejl_ikke_som_netvaerksfejl() {
        // De to kraever modsatte handlinger: ret adressen, mod tjek netvaerket.
        let s = Session::ny("test").unwrap();
        let f = s.get("ikke en url", 1000).unwrap_err();
        assert!(matches!(f, HttpFejl::UgyldigUrl(_)), "faktisk: {f:?}");
    }

    #[test]
    fn opdeling_tager_query_MED_i_objektet() {
        // Et tabt ?token= ser ud som en adgangsfejl paa telefonen.
        let d = opdel("http://10.0.0.5:8090/stream?token=abc").unwrap();
        let sti = String::from_utf16_lossy(&d.sti[..d.sti.len() - 1]);
        assert_eq!(sti, "/stream?token=abc");
        assert_eq!(d.port, 8090);
        assert!(!d.sikker);
    }

    #[test]
    fn opdeling_kender_https_og_dens_standardport() {
        let d = opdel("https://eksempel.dk/x").unwrap();
        assert!(d.sikker);
        assert_eq!(d.port, 443);
    }

    #[test]
    fn opdeling_giver_skraastreg_naar_stien_mangler() {
        let d = opdel("http://eksempel.dk").unwrap();
        let sti = String::from_utf16_lossy(&d.sti[..d.sti.len() - 1]);
        assert_eq!(sti, "/");
    }

    #[test]
    fn en_doed_adresse_giver_en_NETVAERKS_fejl() {
        let s = Session::ny("test").unwrap();
        // 127.0.0.1 paa en port ingen lytter paa: forbindelsen afvises med det samme.
        let f = s.get("http://127.0.0.1:9/x", 2000).unwrap_err();
        assert!(matches!(f, HttpFejl::Netvaerk(_)), "faktisk: {f:?}");
    }
}
