//! Inkrementel parser for `multipart/x-mixed-replace`. Rent CPU-arbejde, ingen I/O:
//! den faar bytes som de kommer ind af netvaerket og leverer hele JPEG-frames.
//!
//! Ordret port af `MjpegParser.cs`, inklusive dens graenser og dens afvisnings-taelling.
//!
//! Boundaryen tages fra det serveren ANNONCERER i Content-Type. Husk bruger `rigframe`, men
//! den er ikke hardkodet: en anden Husk-udgave, en proxy eller et testfikstur maa gerne vaelge
//! en anden, og en hardkodet vaerdi ville da vaere en tavs no-op. Er der ingen boundary,
//! scannes der efter JPEG'ens egne SOI/EOI-markoerer, som Python-vejen altid har gjort.
//!
//! ⚠️ Fallbacken ophaever IKKE den invariant den primaere vej findes for (maaleregel 211):
//! BEGGE veje leverer kun bytes der begynder med FF D8 FF og slutter med FF D9. En del som
//! boundaryen udpeger, men som ikke er et JPEG, kasseres og TAELLES - den bliver aldrig til en
//! frame bare fordi en header sagde at den var én.
//!
//! Alle graenser maales FOER allokering, saa en fejlbehaeftet eller fjendtlig modpart ikke kan
//! faa parseren til at vokse ubegraenset.

/// Hvorfor en del blev kasseret.
///
/// Et loft der rammes, skal kunne SES - ellers kan "der kom ingen frame" ikke skelnes fra
/// "vi smed den vaek" (maaleregel 116).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParserAfvisning {
    /// Delens header-blok naaede loftet uden at slutte.
    HeaderForStor,
    /// Delens krop naaede loftet uden en slutmarkoer.
    DelForStor,
    /// Delen indeholdt ikke et komplet JPEG (FF D8 FF .. FF D9).
    IkkeEtJpeg,
    /// Content-Length var ulaeselig eller stoerre end loftet.
    ForkertLaengde,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tilstand {
    SoegBoundary,
    Headere,
    Krop,
}

/// JPEG Start Of Image.
pub const SOI: &[u8] = &[0xFF, 0xD8, 0xFF];

/// JPEG End Of Image.
pub const EOI: &[u8] = &[0xFF, 0xD9];

pub struct MjpegParser {
    boundary: Option<Vec<u8>>,
    max_header_bytes: usize,
    max_part_bytes: usize,
    afvisninger: Vec<ParserAfvisning>,
    buf: Vec<u8>,
    tilstand: Tilstand,
    /// `None` = ingen Content-Length paa denne del (C#-udgavens -1).
    forventet_krop: Option<usize>,
}

impl MjpegParser {
    pub const STANDARD_MAX_HEADER: usize = 8 * 1024;
    pub const STANDARD_MAX_PART: usize = 16 * 1024 * 1024;

    pub fn new(content_type: Option<&str>) -> Self {
        Self::med_graenser(content_type, Self::STANDARD_MAX_HEADER, Self::STANDARD_MAX_PART)
    }

    pub fn med_graenser(
        content_type: Option<&str>,
        max_header_bytes: usize,
        max_part_bytes: usize,
    ) -> Self {
        let boundary = Self::extract_boundary(content_type)
            .map(|b| { let mut v = b"--".to_vec(); v.extend_from_slice(b.as_bytes()); v });
        MjpegParser {
            boundary,
            max_header_bytes,
            max_part_bytes,
            afvisninger: Vec::new(),
            buf: Vec::with_capacity(64 * 1024),
            tilstand: Tilstand::SoegBoundary,
            forventet_krop: None,
        }
    }

    /// Den boundary parseren bruger, eller `None` naar den scanner SOI/EOI.
    pub fn boundary(&self) -> Option<String> {
        self.boundary
            .as_ref()
            .map(|b| String::from_utf8_lossy(&b[2..]).into_owned())
    }

    /// Afviste dele, i raekkefoelge.
    pub fn afvisninger(&self) -> &[ParserAfvisning] {
        &self.afvisninger
    }

    /// Bytes der venter i bufferen lige nu.
    pub fn buffered_bytes(&self) -> usize {
        self.buf.len()
    }

    /// Boundaryen ud af en Content-Type.
    ///
    /// Taaler citationstegn, mellemrum og andre parametre:
    /// `multipart/x-mixed-replace; boundary="rigframe"; charset=x`.
    pub fn extract_boundary(content_type: Option<&str>) -> Option<String> {
        let ct = content_type?;
        if ct.trim().is_empty() {
            return None;
        }

        for del in ct.split(';') {
            let t = del.trim();
            if t.len() < "boundary".len()
                || !t[.."boundary".len()].eq_ignore_ascii_case("boundary")
            {
                continue;
            }
            let Some(lig) = t.find('=') else { continue };
            let v = t[lig + 1..].trim().trim_matches('"');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }

        None
    }

    /// Er dette et komplet JPEG?
    ///
    /// Samme dom for enhver kilde, saa et `/snapshot`-svar og en stroem-frame ikke kan blive
    /// bedoemt forskelligt.
    pub fn er_helt_jpeg(data: &[u8]) -> bool {
        data.len() >= 5
            && data[0] == 0xFF
            && data[1] == 0xD8
            && data[2] == 0xFF
            && data[data.len() - 2] == 0xFF
            && data[data.len() - 1] == 0xD9
    }

    /// Fodr parseren med de bytes der lige kom ind, og faa de hele frames der blev faerdige.
    ///
    /// Et kald kan give nul, én eller flere frames, og en frame maa vaere delt vilkaarligt over
    /// mange kald.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(chunk);
        let mut ud = Vec::new();
        loop {
            let frame = if self.boundary.is_some() {
                self.tag_via_boundary()
            } else {
                self.tag_via_soi_eoi()
            };
            match frame {
                Some(f) => ud.push(f),
                None => break,
            }
        }
        ud
    }

    // ---- buffer -----------------------------------------------------------------

    fn forbrug(&mut self, antal: usize) {
        if antal == 0 {
            return;
        }
        if antal >= self.buf.len() {
            self.buf.clear();
            return;
        }
        self.buf.drain(..antal);
    }

    fn find(&self, moenster: &[u8], fra: usize) -> Option<usize> {
        if fra >= self.buf.len() || moenster.is_empty() {
            return None;
        }
        let hale = &self.buf[fra..];
        hale.windows(moenster.len()).position(|w| w == moenster).map(|i| fra + i)
    }

    // ---- vej 1: boundary + valgfri Content-Length --------------------------------

    fn tag_via_boundary(&mut self) -> Option<Vec<u8>> {
        let boundary = self.boundary.clone().expect("kaldt uden boundary");

        if self.tilstand == Tilstand::SoegBoundary {
            match self.find(&boundary, 0) {
                None => {
                    // Behold lige praecis nok til at en boundary delt over to laesninger
                    // stadig kan findes naar resten kommer.
                    if self.buf.len() > boundary.len() - 1 {
                        self.forbrug(self.buf.len() - (boundary.len() - 1));
                    }
                    return None;
                }
                Some(b) => {
                    self.forbrug(b + boundary.len());
                    self.tilstand = Tilstand::Headere;
                    self.forventet_krop = None;
                }
            }
        }

        if self.tilstand == Tilstand::Headere {
            match self.find_header_slut() {
                None => {
                    if self.buf.len() > self.max_header_bytes {
                        self.afvisninger.push(ParserAfvisning::HeaderForStor);
                        let n = self.buf.len();
                        self.forbrug(n);
                        self.tilstand = Tilstand::SoegBoundary;
                    }
                    return None;
                }
                Some((slut, skip)) => {
                    let headere = String::from_utf8_lossy(&self.buf[..slut]).into_owned();
                    self.forventet_krop = self.laes_content_length(&headere);
                    if self.forventet_krop.is_some_and(|n| n > self.max_part_bytes) {
                        self.afvisninger.push(ParserAfvisning::ForkertLaengde);
                        self.forventet_krop = None;
                    }
                    self.forbrug(slut + skip);
                    self.tilstand = Tilstand::Krop;
                }
            }
        }

        // Tilstand::Krop
        if let Some(n) = self.forventet_krop {
            if self.buf.len() < n {
                return None;
            }
            let krop = self.buf[..n].to_vec();
            self.forbrug(n);
            self.tilstand = Tilstand::SoegBoundary;
            return self.doem_krop(krop);
        }

        // Ingen Content-Length: kroppen slutter hvor den naeste boundary begynder.
        match self.find(&boundary, 0) {
            None => {
                if self.buf.len() > self.max_part_bytes {
                    self.afvisninger.push(ParserAfvisning::DelForStor);
                    let n = self.buf.len();
                    self.forbrug(n);
                    self.tilstand = Tilstand::SoegBoundary;
                }
                None
            }
            Some(naeste) => {
                let krop = self.buf[..naeste].to_vec();
                self.forbrug(naeste); // boundaryen selv bliver liggende til naeste runde
                self.tilstand = Tilstand::SoegBoundary;
                self.doem_krop(krop)
            }
        }
    }

    /// Kroppen er kun en frame hvis den ER et JPEG.
    ///
    /// Afsluttende CRLF/LF trimmes foerst - de hoerer til multipart-rammen, ikke til billedet.
    fn doem_krop(&mut self, krop: Vec<u8>) -> Option<Vec<u8>> {
        let mut slut = krop.len();
        while slut > 0 && (krop[slut - 1] == b'\n' || krop[slut - 1] == b'\r') {
            slut -= 1;
        }

        if !Self::er_helt_jpeg(&krop[..slut]) {
            self.afvisninger.push(ParserAfvisning::IkkeEtJpeg);
            return None;
        }

        Some(if slut == krop.len() { krop } else { krop[..slut].to_vec() })
    }

    /// Returnerer (offset til header-slut, antal bytes der skal springes over).
    ///
    /// Efter boundaryen staar der CRLF, saa header-linjer, saa en TOM linje. Vi accepterer
    /// baade CRLF og bart LF, fordi en haandskrevet server kan bruge begge - og en parser der
    /// kun kender den ene form, maaler ikke, den gaetter (maaleregel 221).
    fn find_header_slut(&self) -> Option<(usize, usize)> {
        let n = self.buf.len();
        let mut i = 0usize;
        while i + 1 < n {
            if self.buf[i] == b'\n' && self.buf[i + 1] == b'\n' {
                return Some((i, 2));
            }
            if i + 3 < n
                && self.buf[i] == b'\r'
                && self.buf[i + 1] == b'\n'
                && self.buf[i + 2] == b'\r'
                && self.buf[i + 3] == b'\n'
            {
                return Some((i, 4));
            }
            i += 1;
        }
        None
    }

    fn laes_content_length(&mut self, headere: &str) -> Option<usize> {
        for linje in headere.split('\n') {
            let t = linje.trim();
            if t.len() < "content-length".len()
                || !t[.."content-length".len()].eq_ignore_ascii_case("content-length")
            {
                continue;
            }
            let Some(kolon) = t.find(':') else { continue };
            let vaerdi = t[kolon + 1..].trim();
            // Som C#-udgavens NumberStyles.None: kun rene cifre, intet fortegn.
            if !vaerdi.is_empty() && vaerdi.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(n) = vaerdi.parse::<usize>() {
                    return Some(n);
                }
            }
            self.afvisninger.push(ParserAfvisning::ForkertLaengde);
            return None;
        }
        None
    }

    // ---- vej 2: ren SOI/EOI-scanning (ingen annonceret boundary) ------------------

    fn tag_via_soi_eoi(&mut self) -> Option<Vec<u8>> {
        let Some(soi) = self.find(SOI, 0) else {
            if self.buf.len() > SOI.len() - 1 {
                self.forbrug(self.buf.len() - (SOI.len() - 1));
            }
            return None;
        };

        let Some(eoi) = self.find(EOI, soi + SOI.len()) else {
            if self.buf.len() - soi > self.max_part_bytes {
                // Kaempe del uden slutmarkoer = korrupt. Drop STARTEN, ikke hele bufferen:
                // den naeste rigtige SOI kan allerede ligge i det vi har.
                self.afvisninger.push(ParserAfvisning::DelForStor);
                self.forbrug(soi + SOI.len());
                return None;
            }
            self.forbrug(soi);
            return None;
        };

        let slut = eoi + EOI.len();
        let jpeg = self.buf[soi..slut].to_vec();
        self.forbrug(slut);
        Some(jpeg)
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    const BOUNDARY: &str = "rigframe";
    const CT: &str = "multipart/x-mixed-replace; boundary=rigframe";

    fn jpeg(nyttelast: usize, fyld: u8) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8, 0xFF];
        b.extend(std::iter::repeat_n(fyld, nyttelast));
        b.extend_from_slice(&[0xFF, 0xD9]);
        b
    }

    fn del(j: &[u8], med_laengde: bool) -> Vec<u8> {
        let mut s = format!("--{BOUNDARY}\r\nContent-Type: image/jpeg\r\n");
        if med_laengde {
            s.push_str(&format!("Content-Length: {}\r\n", j.len()));
        }
        s.push_str("\r\n");
        let mut ud = s.into_bytes();
        ud.extend_from_slice(j);
        ud.extend_from_slice(b"\r\n");
        ud
    }

    /// Fodr parseren i bidder af N bytes og saml alle frames.
    ///
    /// ⛔ Opdelingen er hele pointen: en MJPEG-frame paa 200 KB ankommer ALTID i mange
    /// TCP-segmenter, og en parser der kun er proevet med hele dele ad gangen, er kun proevet
    /// paa den nemme halvdel.
    fn foder(p: &mut MjpegParser, data: &[u8], bidde: usize) -> Vec<Vec<u8>> {
        let mut alle = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let n = bidde.min(data.len() - i);
            alle.extend(p.feed(&data[i..i + n]));
            i += n;
        }
        alle
    }

    /// ⛔ ÉT BEN PR. FORM. En loekke over seks Content-Type-former stopper ved den foerste
    /// fejl, og et roedt ben ville da ikke sige HVILKEN form der var brudt.
    macro_rules! ben_boundary {
        ($navn:ident, $ct:expr, $forventet:expr) => {
            #[test]
            fn $navn() {
                assert_eq!(MjpegParser::extract_boundary($ct).as_deref(), $forventet);
            }
        };
    }

    ben_boundary!(
        boundary_bar,
        Some("multipart/x-mixed-replace; boundary=rigframe"),
        Some("rigframe")
    );
    ben_boundary!(
        boundary_i_citationstegn,
        Some("multipart/x-mixed-replace;boundary=\"rigframe\""),
        Some("rigframe")
    );
    ben_boundary!(
        boundary_med_store_bogstaver_og_flere_parametre,
        Some("multipart/x-mixed-replace; BOUNDARY = rigframe ; charset=x"),
        Some("rigframe")
    );
    ben_boundary!(boundary_mangler_i_image_jpeg, Some("image/jpeg"), None);
    ben_boundary!(boundary_af_tom_content_type, Some(""), None);
    ben_boundary!(boundary_af_manglende_content_type, None, None);

    #[test]
    fn boundary_er_ikke_hardkodet() {
        // Husk bruger "rigframe", men en anden server maa gerne vaelge noget andet. En
        // hardkodet vaerdi ville vaere en TAVS no-op mod netop den server.
        let p = MjpegParser::new(Some("multipart/x-mixed-replace; boundary=noget-helt-andet"));
        assert_eq!(p.boundary().as_deref(), Some("noget-helt-andet"));
    }

    /// Parseren skal taale at boundary, header og JPEG er delt VILKAARLIGT over
    /// netvaerkslaesninger - ét ben pr. biddestoerrelse, saa et roedt ben navngiver den.
    macro_rules! ben_opdeling_med_laengde {
        ($navn:ident, $bidde:expr) => {
            #[test]
            fn $navn() {
                let a = jpeg(500, 0x41);
                let b = jpeg(700, 0x42);
                let mut stroem = del(&a, true);
                stroem.extend(del(&b, true));

                let mut p = MjpegParser::new(Some(CT));
                let frames = foder(&mut p, &stroem, $bidde);
                assert_eq!(frames.len(), 2);
                assert_eq!(frames[0], a);
                assert_eq!(frames[1], b);
                assert!(p.afvisninger().is_empty());
            }
        };
    }

    ben_opdeling_med_laengde!(opdeling_med_laengde_1_byte, 1);
    ben_opdeling_med_laengde!(opdeling_med_laengde_3_bytes, 3);
    ben_opdeling_med_laengde!(opdeling_med_laengde_7_bytes, 7);
    ben_opdeling_med_laengde!(opdeling_med_laengde_64_bytes, 64);
    ben_opdeling_med_laengde!(opdeling_med_laengde_4096_bytes, 4096);

    macro_rules! ben_opdeling_uden_laengde {
        ($navn:ident, $bidde:expr) => {
            #[test]
            fn $navn() {
                let a = jpeg(300, 0x43);
                let b = jpeg(301, 0x44);
                let mut stroem = del(&a, false);
                stroem.extend(del(&b, false));
                // Uden Content-Length slutter den sidste del foerst ved den NAESTE boundary,
                // saa en stroem der bare stopper, efterlader den sidste del ufuldendt. Det er
                // korrekt opfoersel, og fiksturet skal derfor lukke med en boundary.
                stroem.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

                let mut p = MjpegParser::new(Some(CT));
                let frames = foder(&mut p, &stroem, $bidde);
                assert_eq!(frames.len(), 2);
                assert_eq!(frames[0], a);
                assert_eq!(frames[1], b);
            }
        };
    }

    ben_opdeling_uden_laengde!(opdeling_uden_laengde_1_byte, 1);
    ben_opdeling_uden_laengde!(opdeling_uden_laengde_5_bytes, 5);
    ben_opdeling_uden_laengde!(opdeling_uden_laengde_512_bytes, 512);

    #[test]
    fn bart_lf_i_stedet_for_crlf_virker_ogsaa() {
        // En haandskrevet server maa gerne bruge bart LF. En parser der kun kender CRLF
        // maaler ikke - den gaetter (maaleregel 221).
        let j = jpeg(100, 0x41);
        let mut s = format!("--{BOUNDARY}\nContent-Type: image/jpeg\n\n").into_bytes();
        s.extend_from_slice(&j);
        s.extend_from_slice(format!("\n--{BOUNDARY}--\n").as_bytes());

        let mut p = MjpegParser::new(Some(CT));
        let frames = foder(&mut p, &s, 3);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], j);
    }

    #[test]
    fn en_del_der_IKKE_er_et_jpeg_afvises_og_taelles() {
        // Boundaryen siger at det er en del; det goer den ikke til en frame.
        let mut s = format!(
            "--{BOUNDARY}\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhejsa\r\n"
        )
        .into_bytes();
        let god = jpeg(50, 0x41);
        s.extend(del(&god, true));

        let mut p = MjpegParser::new(Some(CT));
        let frames = foder(&mut p, &s, 8);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], god);
        assert!(p.afvisninger().contains(&ParserAfvisning::IkkeEtJpeg));
    }

    #[test]
    fn afbrudt_del_giver_ingen_frame() {
        let hel = del(&jpeg(400, 0x41), true);
        let halv = &hel[..hel.len() / 2];
        let mut p = MjpegParser::new(Some(CT));
        assert!(foder(&mut p, halv, 16).is_empty());
    }

    #[test]
    fn for_stor_del_rammer_loftet_SYNLIGT() {
        // Et loft der rammes tavst, kan ikke skelnes fra "der kom ingen frame".
        let mut p = MjpegParser::med_graenser(Some(CT), MjpegParser::STANDARD_MAX_HEADER, 256);
        let mut s = format!("--{BOUNDARY}\r\nContent-Type: image/jpeg\r\n\r\n").into_bytes();
        s.extend(jpeg(4096, 0x41));

        assert!(foder(&mut p, &s, 512).is_empty());
        assert!(p.afvisninger().contains(&ParserAfvisning::DelForStor));
    }

    #[test]
    fn for_stor_header_rammer_loftet_SYNLIGT() {
        let mut p = MjpegParser::med_graenser(Some(CT), 64, MjpegParser::STANDARD_MAX_PART);
        let mut s = format!("--{BOUNDARY}\r\n").into_bytes();
        s.extend_from_slice(format!("X-Fyld: {}\r\n", "y".repeat(500)).as_bytes());

        foder(&mut p, &s, 64);
        assert!(p.afvisninger().contains(&ParserAfvisning::HeaderForStor));
    }

    #[test]
    fn ulaeselig_content_length_afvises_frem_for_at_blive_paa_gaet() {
        let mut s =
            format!("--{BOUNDARY}\r\nContent-Length: ikke-et-tal\r\n\r\n").into_bytes();
        let j = jpeg(40, 0x41);
        s.extend_from_slice(&j);
        s.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

        let mut p = MjpegParser::new(Some(CT));
        let frames = foder(&mut p, &s, 7);

        // Den ulaeselige laengde skal SES, og delen skal stadig kunne leveres via boundary.
        assert!(p.afvisninger().contains(&ParserAfvisning::ForkertLaengde));
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], j);
    }

    #[test]
    fn uden_boundary_scannes_der_soi_eoi() {
        // Fallbacken: ingen annonceret boundary. Den maa ikke ophaeve invarianten - det den
        // leverer skal stadig vaere hele JPEG'er.
        let mut p = MjpegParser::new(Some("image/jpeg"));
        assert!(p.boundary().is_none());

        let a = jpeg(80, 0x51);
        let b = jpeg(90, 0x52);
        let mut s = b"stoej foer".to_vec();
        s.extend_from_slice(&a);
        s.extend_from_slice(b"stoej imellem");
        s.extend_from_slice(&b);

        let frames = foder(&mut p, &s, 11);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], a);
        assert_eq!(frames[1], b);
        assert!(frames.iter().all(|f| MjpegParser::er_helt_jpeg(f)));
    }

    #[test]
    fn bufferen_vokser_ikke_ubegraenset_paa_rent_SKRALD() {
        // En modpart der aldrig sender en SOI, maa ikke kunne faa parseren til at spise
        // hukommelse. Uden boundary skal bufferen holde sig til nogle faa bytes.
        let mut p = MjpegParser::new(None);
        for _ in 0..200 {
            p.feed(&[0u8; 1024]);
        }
        assert!(
            p.buffered_bytes() <= 8,
            "bufferen voksede til {} bytes paa rent skrald",
            p.buffered_bytes()
        );
    }

    macro_rules! ben_helt_jpeg {
        ($navn:ident, $data:expr, $forventet:expr) => {
            #[test]
            fn $navn() {
                let data: &[u8] = &$data;
                assert_eq!(MjpegParser::er_helt_jpeg(data), $forventet);
            }
        };
    }

    ben_helt_jpeg!(helt_jpeg_med_begge_markoerer, [0xFF, 0xD8, 0xFF, 0x00, 0xFF, 0xD9], true);
    ben_helt_jpeg!(helt_jpeg_uden_slutmarkoer, [0xFF, 0xD8, 0xFF, 0x00], false);
    ben_helt_jpeg!(helt_jpeg_uden_startmarkoer, [0x00, 0xD8, 0xFF, 0x00, 0xFF, 0xD9], false);
    ben_helt_jpeg!(helt_jpeg_for_kort, [0xFF, 0xD8], false);
}
