//! Tegner de felter der IKKE er et kamerabillede: OFFLINE og AUTH-FEJL, plus etiket-bjaelken i
//! et gitter-felt. Port af `Placeholder.cs`.
//!
//! De to tilstande skal kunne SKELNES paa skaermen. I Python-vejen saa de ens ud indtil det
//! blev rettet, og brugeren fejlsoegte netvaerk mens problemet var et token. De har derfor hver
//! sin tekst og hver sin farve.
//!
//! ⛔ TEKSTEN TEGNES MED GDI, ikke med WPF's tekststak som C#-udgaven brugte. Begge er
//! Windows' egne; forskellen er at GDI ikke kraever en runtime i pakken. GDI kan ae, oe og aa
//! (det var en ren cv2-begraensning i Python-vejen at etiketten skulle translittereres), saa
//! kontrakten er uaendret: et telefonnavn vises som brugeren skrev det.
//!
//! ⚠️ GDI SKRIVER IKKE ALFA. Et 32-bit DIB faar derfor A=0 i hver pixel GDI har roert, og
//! alfaen saettes bagefter af os selv. Gjorde vi det ikke, ville hele feltet vaere
//! gennemsigtigt - og det ville se ud som et sort billede frem for som en manglende alfa.

use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, FF_DONTCARE,
    FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FW_BOLD, FillRect, FrameRect,
    GdiFlush, GetTextExtentPoint32W, HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT, TextOutW, VARIABLE_PITCH,
};

use crate::bitmap::Bitmap32;
use crate::source_status::SourceStatus;

/// Et felt for en kilde uden billede.
pub fn byg(navn: &str, bredde: usize, hoejde: usize, status: SourceStatus) -> Bitmap32 {
    let auth = status == SourceStatus::AuthError;
    let linje1 = match status {
        SourceStatus::AuthError => "AUTH-FEJL",
        SourceStatus::Connecting => "FORBINDER",
        _ => "OFFLINE",
    };
    let linje2 = if auth {
        format!("{navn} (token eller adgang)")
    } else {
        navn.to_string()
    };

    let skala = f64::max(0.5, usize::min(bredde, hoejde) as f64 / 400.0);

    let mut d = Daek::ny(bredde, hoejde);
    // Naesten sort, ikke signal-sort: et felt der er praecis sort kan forveksles med et sort
    // KAMERABILLEDE, og de to betyder noget helt forskelligt.
    d.fyld_rekt(0, 0, bredde as i32, hoejde as i32, rgb(20, 20, 20));
    d.ramme(1, 1, bredde as i32 - 1, hoejde as i32 - 1, rgb(60, 60, 60));

    let f1 = (26.0 * skala).max(8.0) as i32;
    let f2 = (15.0 * skala).max(8.0) as i32;
    let (b1, h1) = d.maal(linje1, f1, true);
    let (b2, _h2) = d.maal(&linje2, f2, true);

    let cy = hoejde as i32 / 2;
    d.tekst(
        linje1,
        f1,
        (bredde as i32 - b1) / 2,
        cy - h1 - 4,
        if auth { rgb(230, 170, 40) } else { rgb(200, 60, 60) },
    );
    d.tekst(&linje2, f2, (bredde as i32 - b2) / 2, cy + 8, rgb(180, 180, 180));

    d.til_bitmap(|_| 255)
}

/// Lille etiket-bjaelke i feltets oeverste venstre hjoerne. Muterer feltet.
pub fn tegn_etiket(felt: &mut Bitmap32, navn: &str) {
    let mut maaler = Daek::ny(1, 1);
    let (tb, th) = maaler.maal(navn, 13, true);
    drop(maaler);

    let b = usize::min(felt.bredde(), (tb + 12).max(1) as usize);
    let h = usize::min(felt.hoejde(), (th + 8).max(1) as usize);

    let mut d = Daek::ny(b, h);
    d.fyld_rekt(0, 0, b as i32, h as i32, rgb(0, 0, 0));
    d.tekst(navn, 13, 6, 4, rgb(255, 255, 255));

    // C#-udgaven tegnede bjaelken med alfa 200 og teksten UOVEN paa med alfa 255. Her udledes
    // alfaen af hvor lys pixelen blev: sort bjaelke -> 200, hvid tekst -> 255. Samme to
    // yderpunkter, og mellemtonerne i kantudjaevningen faar noget derimellem.
    let bjaelke = d.til_bitmap(|luma| 200u8.max(luma));
    bjaelke.blit_til(felt, 0, 0);
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

/// Et GDI-daek: en DC med et top-down 32-bit DIB valgt ind.
///
/// Alt GDI-arbejde sker gennem den, saa der er ÉT sted der rydder op. Ressourcelaek i en
/// funktion der kaldes pr. frame ville fylde handle-tabellen paa minutter.
struct Daek {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    bredde: usize,
    hoejde: usize,
    font: Option<HFONT>,
    font_stoerrelse: i32,
}

impl Daek {
    fn ny(bredde: usize, hoejde: usize) -> Daek {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bmi = BITMAPINFO::default();
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bredde as i32,
                // Negativ hoejde = TOP-DOWN. Med positiv hoejde ville billedet staa paa
                // hovedet, og netop den fejl er usynlig paa et symmetrisk testbillede.
                biHeight: -(hoejde as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .expect("CreateDIBSection");
            SelectObject(dc, HGDIOBJ(bitmap.0));
            SetBkMode(dc, TRANSPARENT);
            Daek { dc, bitmap, bits: bits as *mut u8, bredde, hoejde, font: None, font_stoerrelse: 0 }
        }
    }

    fn vaelg_font(&mut self, stoerrelse: i32, fed: bool) {
        if self.font_stoerrelse == stoerrelse && self.font.is_some() {
            return;
        }
        unsafe {
            if let Some(g) = self.font.take() {
                let _ = DeleteObject(HGDIOBJ(g.0));
            }
            let navn: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let f = CreateFontW(
                -stoerrelse,
                0,
                0,
                0,
                if fed { FW_BOLD.0 as i32 } else { 400 },
                0,
                0,
                0,
                FONT_CHARSET(0),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                CLEARTYPE_QUALITY,
                (VARIABLE_PITCH.0 | FF_DONTCARE.0) as u32,
                windows::core::PCWSTR(navn.as_ptr()),
            );
            SelectObject(self.dc, HGDIOBJ(f.0));
            self.font = Some(f);
            self.font_stoerrelse = stoerrelse;
        }
    }

    fn maal(&mut self, s: &str, stoerrelse: i32, fed: bool) -> (i32, i32) {
        self.vaelg_font(stoerrelse, fed);
        let w: Vec<u16> = s.encode_utf16().collect();
        let mut sz = SIZE::default();
        unsafe {
            let _ = GetTextExtentPoint32W(self.dc, &w, &mut sz);
        }
        (sz.cx, sz.cy)
    }

    fn tekst(&mut self, s: &str, stoerrelse: i32, x: i32, y: i32, farve: COLORREF) {
        self.vaelg_font(stoerrelse, true);
        let w: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            SetTextColor(self.dc, farve);
            let _ = TextOutW(self.dc, x, y, &w);
        }
    }

    fn fyld_rekt(&self, x: i32, y: i32, hoejre: i32, bund: i32, farve: COLORREF) {
        unsafe {
            let b: HBRUSH = CreateSolidBrush(farve);
            let r = RECT { left: x, top: y, right: hoejre, bottom: bund };
            FillRect(self.dc, &r, b);
            let _ = DeleteObject(HGDIOBJ(b.0));
        }
    }

    fn ramme(&self, x: i32, y: i32, hoejre: i32, bund: i32, farve: COLORREF) {
        unsafe {
            let b: HBRUSH = CreateSolidBrush(farve);
            let r = RECT { left: x, top: y, right: hoejre, bottom: bund };
            FrameRect(self.dc, &r, b);
            let _ = DeleteObject(HGDIOBJ(b.0));
        }
    }

    /// Kopiér DIB'ens pixels ud, og saet alfaen med `alfa(luma)`.
    fn til_bitmap(&self, alfa: impl Fn(u8) -> u8) -> Bitmap32 {
        unsafe {
            // GdiFlush FOER laesningen: GDI-kald er koeet, og en laesning uden flush kan se en
            // MELLEMTILSTAND. Symptomet ville vaere et felt uden tekst i ny og nae.
            let _ = GdiFlush();
            let n = self.bredde * self.hoejde * 4;
            let raa = std::slice::from_raw_parts(self.bits, n);
            let mut pixels = raa.to_vec();
            for p in pixels.chunks_exact_mut(4) {
                // Luma efter BT.601 paa BGRA.
                let luma = ((p[2] as u32 * 299 + p[1] as u32 * 587 + p[0] as u32 * 114) / 1000) as u8;
                p[3] = alfa(luma);
            }
            Bitmap32::fra_bgra(self.bredde, self.hoejde, pixels).expect("DIB-stoerrelse")
        }
    }
}

impl Drop for Daek {
    fn drop(&mut self) {
        unsafe {
            if let Some(f) = self.font.take() {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    /// Hvor mange FORSKELLIGE farver er der i billedet? En tegning der ikke skete, giver 1.
    fn farver(bm: &Bitmap32) -> usize {
        let mut set = std::collections::HashSet::new();
        for p in bm.pixels().chunks_exact(4) {
            set.insert(u32::from_le_bytes([p[0], p[1], p[2], p[3]]));
        }
        set.len()
    }

    #[test]
    fn offline_felt_bliver_faktisk_TEGNET() {
        // Et groent build beviser ikke at der blev tegnet noget. Et ensfarvet felt er praecis
        // den fejl benet findes for (maaleregel 37).
        let b = byg("Note10+", 320, 240, SourceStatus::Offline);
        assert_eq!((b.bredde(), b.hoejde()), (320, 240));
        assert!(farver(&b) >= 4, "kun {} farver - blev der tegnet?", farver(&b));
    }

    #[test]
    fn auth_fejl_og_offline_kan_SKELNES() {
        // Det var netop den fejl Python-vejen havde: de to saa ens ud, og brugeren fejlsoegte
        // netvaerk mens problemet var et token.
        let a = byg("Note10+", 320, 240, SourceStatus::AuthError);
        let o = byg("Note10+", 320, 240, SourceStatus::Offline);
        assert_ne!(a.pixels(), o.pixels(), "de to tilstande skal se forskellige ud");
    }

    #[test]
    fn forbinder_er_sin_egen_tilstand() {
        let c = byg("Note10+", 320, 240, SourceStatus::Connecting);
        let o = byg("Note10+", 320, 240, SourceStatus::Offline);
        assert_ne!(c.pixels(), o.pixels());
    }

    #[test]
    fn feltet_er_uigennemsigtigt() {
        let b = byg("Note10+", 64, 64, SourceStatus::Offline);
        assert!(b.pixels().chunks_exact(4).all(|p| p[3] == 255), "alfa skal vaere 255");
    }

    #[test]
    fn baggrunden_er_naesten_sort_men_ikke_sort() {
        // Et praecis sort felt kan forveksles med et sort KAMERABILLEDE.
        let b = byg("x", 64, 64, SourceStatus::Offline);
        assert_eq!(b.pixel(32, 5), (20, 20, 20, 255));
    }

    #[test]
    fn etiket_tegnes_i_oeverste_venstre_hjoerne() {
        let mut felt = Bitmap32::new(320, 240);
        felt.fyld(90, 90, 90, 255);
        tegn_etiket(&mut felt, "Note10+");
        // Bjaelken er moerk og halvgennemsigtig; resten af feltet er uroert.
        assert_eq!(felt.pixel(1, 1).3, 200, "bjaelkens alfa");
        assert_eq!(felt.pixel(300, 200), (90, 90, 90, 255), "resten skal vaere uroert");
    }

    #[test]
    fn etiket_med_aeoeaa_tegnes_uden_translitteration() {
        let mut felt = Bitmap32::new(320, 240);
        felt.fyld_sort();
        tegn_etiket(&mut felt, "K\u{00f8}kken\u{00e6}bler\u{00e5}");
        assert!(farver(&felt) >= 3, "der skal vaere tegnet tekst");
    }

    #[test]
    fn etiket_klippes_til_feltets_bredde() {
        let mut felt = Bitmap32::new(40, 30);
        felt.fyld_sort();
        // Et meget langt navn maa ikke skrive uden for feltet eller panice.
        tegn_etiket(&mut felt, &"meget langt telefonnavn ".repeat(8));
        assert_eq!((felt.bredde(), felt.hoejde()), (40, 30));
    }
}
