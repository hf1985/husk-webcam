//! Et raat billede i hukommelsen: BGRA32, taetpakket (stride = bredde * 4), oeverste raekke
//! foerst. Formatet er valgt fordi det er baade WIC's og DirectShows naturlige, saa hverken
//! afkodningen eller afsendelsen skal bytte kanaler pr. frame.
//!
//! Port af `Bitmap32.cs`. Forskellen fra C#-udgaven er at JPEG-afkodningen er flyttet til
//! [`crate::wic`]: dér er den COM, og alt COM er samlet ét sted frem for at ligge spredt i
//! datatyperne.

/// BGRA32-billede, taetpakket, oeverste raekke foerst.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap32 {
    bredde: usize,
    hoejde: usize,
    pixels: Vec<u8>,
}

impl Bitmap32 {
    /// Nyt, sort-gennemsigtigt billede.
    ///
    /// # Panics
    /// Hvis bredde eller hoejde er nul. Et billede uden areal er ikke en gyldig tilstand
    /// nogen steder i kaeden, og en tavs 0x0 ville foerst vise sig som et tomt felt hos
    /// modtageren.
    pub fn new(bredde: usize, hoejde: usize) -> Self {
        assert!(bredde > 0, "bredde skal vaere > 0");
        assert!(hoejde > 0, "hoejde skal vaere > 0");
        Bitmap32 { bredde, hoejde, pixels: vec![0u8; bredde * hoejde * 4] }
    }

    /// Byg af raa BGRA-bytes. Returnerer `None` naar laengden ikke passer til dimensionerne -
    /// en forkert laengde er en fejl i kalderen, ikke data der skal fortolkes.
    pub fn fra_bgra(bredde: usize, hoejde: usize, pixels: Vec<u8>) -> Option<Self> {
        if bredde == 0 || hoejde == 0 || pixels.len() != bredde * hoejde * 4 {
            return None;
        }
        Some(Bitmap32 { bredde, hoejde, pixels })
    }

    pub fn bredde(&self) -> usize {
        self.bredde
    }

    pub fn hoejde(&self) -> usize {
        self.hoejde
    }

    pub fn stride(&self) -> usize {
        self.bredde * 4
    }

    /// BGRA, 4 bytes pr. pixel, raekke for raekke.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// (B, G, R, A) paa (x, y).
    pub fn pixel(&self, x: usize, y: usize) -> (u8, u8, u8, u8) {
        let i = y * self.stride() + x * 4;
        (self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3])
    }

    pub fn saet_pixel(&mut self, x: usize, y: usize, b: u8, g: u8, r: u8, a: u8) {
        let s = self.stride();
        let i = y * s + x * 4;
        self.pixels[i] = b;
        self.pixels[i + 1] = g;
        self.pixels[i + 2] = r;
        self.pixels[i + 3] = a;
    }

    /// Fyld hele billedet med én farve.
    pub fn fyld(&mut self, b: u8, g: u8, r: u8, a: u8) {
        for p in self.pixels.chunks_exact_mut(4) {
            p[0] = b;
            p[1] = g;
            p[2] = r;
            p[3] = a;
        }
    }

    /// Uigennemsigtigt sort - default i C#-udgavens `Fyld()`.
    pub fn fyld_sort(&mut self) {
        self.fyld(0, 0, 0, 255);
    }

    /// Rotation med uret i skridt af 90 grader. 0 giver en kopi af billedet selv.
    ///
    /// Returnerer `None` naar graderne ikke er et multiplum af 90. En vaerdi der ikke er det,
    /// AFVISES frem for at blive rundet: et tavst rundet input ville skjule en fejl i kalderen.
    pub fn roteret(&self, grader: i32) -> Option<Bitmap32> {
        let g = ((grader % 360) + 360) % 360;
        if g % 90 != 0 {
            return None;
        }
        if g == 0 {
            return Some(self.clone());
        }

        let byt = g == 90 || g == 270;
        let mut ud = Bitmap32::new(
            if byt { self.hoejde } else { self.bredde },
            if byt { self.bredde } else { self.hoejde },
        );
        let ud_stride = ud.stride();
        let stride = self.stride();

        for y in 0..self.hoejde {
            for x in 0..self.bredde {
                let (nx, ny) = match g {
                    90 => (self.hoejde - 1 - y, x),
                    180 => (self.bredde - 1 - x, self.hoejde - 1 - y),
                    _ => (y, self.bredde - 1 - x),
                };
                let fra = y * stride + x * 4;
                let til = ny * ud_stride + nx * 4;
                ud.pixels[til..til + 4].copy_from_slice(&self.pixels[fra..fra + 4]);
            }
        }

        Some(ud)
    }

    /// Skalér ind i (`dest_b`, `dest_h`) med BEVARET aspekt og SORTE SIDEFELTER.
    ///
    /// Aldrig straekning: et strakt ansigt i et moede er vaerre end sorte bjaelker, og det er
    /// den fejl letterboxing findes for at undgaa.
    pub fn letterbox(&self, dest_b: usize, dest_h: usize) -> Bitmap32 {
        let mut laerred = Bitmap32::new(dest_b, dest_h);
        laerred.fyld_sort();

        let skala = f64::min(dest_b as f64 / self.bredde as f64, dest_h as f64 / self.hoejde as f64);
        let ny_b = usize::max(1, (self.bredde as f64 * skala) as usize);
        let ny_h = usize::max(1, (self.hoejde as f64 * skala) as usize);
        let ox = (dest_b - ny_b) / 2;
        let oy = (dest_h - ny_h) / 2;

        let l_stride = laerred.stride();
        let stride = self.stride();

        for y in 0..ny_h {
            let sy = usize::min(self.hoejde - 1, y * self.hoejde / ny_h);
            let dest_raekke = (oy + y) * l_stride + ox * 4;
            let kilde_raekke = sy * stride;
            for x in 0..ny_b {
                let sx = usize::min(self.bredde - 1, x * self.bredde / ny_b);
                let fra = kilde_raekke + sx * 4;
                let til = dest_raekke + x * 4;
                laerred.pixels[til] = self.pixels[fra];
                laerred.pixels[til + 1] = self.pixels[fra + 1];
                laerred.pixels[til + 2] = self.pixels[fra + 2];
                laerred.pixels[til + 3] = 255;
            }
        }

        laerred
    }

    /// Kopiér hele dette billede ind paa (x, y) i et andet. Pixels uden for maalet springes over.
    pub fn blit_til(&self, maal: &mut Bitmap32, x: i64, y: i64) {
        let stride = self.stride();
        let m_stride = maal.stride();
        for r in 0..self.hoejde {
            let my = y + r as i64;
            if my < 0 || my >= maal.hoejde as i64 {
                continue;
            }
            for c in 0..self.bredde {
                let mx = x + c as i64;
                if mx < 0 || mx >= maal.bredde as i64 {
                    continue;
                }
                let fra = r * stride + c * 4;
                let til = my as usize * m_stride + mx as usize * 4;
                maal.pixels[til..til + 4].copy_from_slice(&self.pixels[fra..fra + 4]);
            }
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn nyt_billede_har_rigtig_stoerrelse_og_stride() {
        let b = Bitmap32::new(3, 2);
        assert_eq!(b.bredde(), 3);
        assert_eq!(b.hoejde(), 2);
        assert_eq!(b.stride(), 12);
        assert_eq!(b.pixels().len(), 24);
    }

    #[test]
    fn saet_og_laes_pixel() {
        let mut b = Bitmap32::new(2, 2);
        b.saet_pixel(1, 1, 10, 20, 30, 40);
        assert_eq!(b.pixel(1, 1), (10, 20, 30, 40));
        assert_eq!(b.pixel(0, 0), (0, 0, 0, 0));
    }

    #[test]
    fn fyld_sort_giver_uigennemsigtig_sort() {
        let mut b = Bitmap32::new(2, 2);
        b.fyld_sort();
        for y in 0..2 {
            for x in 0..2 {
                assert_eq!(b.pixel(x, y), (0, 0, 0, 255));
            }
        }
    }

    #[test]
    fn rotation_der_ikke_er_multiplum_af_90_afvises() {
        let b = Bitmap32::new(2, 2);
        assert!(b.roteret(45).is_none());
        assert!(b.roteret(-45).is_none());
    }

    #[test]
    fn rotation_0_giver_samme_billede() {
        let mut b = Bitmap32::new(2, 1);
        b.saet_pixel(0, 0, 1, 2, 3, 255);
        let r = b.roteret(0).unwrap();
        assert_eq!(r.pixel(0, 0), (1, 2, 3, 255));
        assert_eq!(r.bredde(), 2);
    }

    #[test]
    fn rotation_90_bytter_dimensioner_og_flytter_hjoernet() {
        // 2x1: venstre pixel maerket. Efter 90 grader med uret staar den OEVERST.
        let mut b = Bitmap32::new(2, 1);
        b.saet_pixel(0, 0, 9, 9, 9, 255);
        let r = b.roteret(90).unwrap();
        assert_eq!((r.bredde(), r.hoejde()), (1, 2));
        assert_eq!(r.pixel(0, 0), (9, 9, 9, 255));
    }

    #[test]
    fn rotation_180_og_270_er_konsistente() {
        let mut b = Bitmap32::new(3, 2);
        b.saet_pixel(0, 0, 1, 1, 1, 255);
        let r180 = b.roteret(180).unwrap();
        assert_eq!(r180.pixel(2, 1), (1, 1, 1, 255));
        let r270 = b.roteret(270).unwrap();
        assert_eq!((r270.bredde(), r270.hoejde()), (2, 3));
        assert_eq!(r270.pixel(0, 2), (1, 1, 1, 255));
    }

    #[test]
    fn rotation_negativ_normaliseres() {
        let mut b = Bitmap32::new(2, 1);
        b.saet_pixel(0, 0, 5, 5, 5, 255);
        assert_eq!(b.roteret(-90).unwrap().pixels(), b.roteret(270).unwrap().pixels());
    }

    #[test]
    fn letterbox_bevarer_aspekt_og_giver_sorte_sidefelter() {
        // 4x1 kilde i en 4x4 ramme: 3 raekker skal vaere sorte sidefelter.
        let mut b = Bitmap32::new(4, 1);
        for x in 0..4 {
            b.saet_pixel(x, 0, 200, 200, 200, 255);
        }
        let l = b.letterbox(4, 4);
        assert_eq!((l.bredde(), l.hoejde()), (4, 4));
        // Den skalerede stribe er 4x1 og centreres paa raekke 1.
        assert_eq!(l.pixel(0, 1), (200, 200, 200, 255));
        assert_eq!(l.pixel(0, 0), (0, 0, 0, 255));
        assert_eq!(l.pixel(0, 3), (0, 0, 0, 255));
    }

    #[test]
    fn letterbox_straekker_aldrig() {
        // Et kvadrat i en bred ramme skal forblive kvadratisk.
        let mut b = Bitmap32::new(10, 10);
        b.fyld(255, 255, 255, 255);
        let l = b.letterbox(40, 10);
        // Kun 10 kolonner i midten maa vaere hvide.
        assert_eq!(l.pixel(0, 5), (0, 0, 0, 255));
        assert_eq!(l.pixel(20, 5), (255, 255, 255, 255));
        assert_eq!(l.pixel(39, 5), (0, 0, 0, 255));
    }

    #[test]
    fn blit_klipper_mod_maalets_kanter() {
        let mut kilde = Bitmap32::new(2, 2);
        kilde.fyld(7, 7, 7, 255);
        let mut maal = Bitmap32::new(2, 2);
        maal.fyld_sort();
        kilde.blit_til(&mut maal, 1, 1);
        assert_eq!(maal.pixel(1, 1), (7, 7, 7, 255));
        assert_eq!(maal.pixel(0, 0), (0, 0, 0, 255));
        // Negativ offset maa heller ikke panice.
        kilde.blit_til(&mut maal, -1, -1);
        assert_eq!(maal.pixel(0, 0), (7, 7, 7, 255));
    }

    /// Hvid blok i OEVERSTE HOEJRE hjoerne, resten sort.
    ///
    /// Asymmetrisk med vilje: et symmetrisk moenster kan ikke afsloere en forkert rotation
    /// eller et spejlvendt output.
    fn markoer(b: usize, h: usize) -> Bitmap32 {
        let mut bm = Bitmap32::new(b, h);
        bm.fyld_sort();
        for y in 0..h / 2 {
            for x in b / 2..b {
                bm.saet_pixel(x, y, 255, 255, 255, 255);
            }
        }
        bm
    }

    #[test]
    fn rotation_270_flytter_oeverst_HOEJRE_til_oeverst_VENSTRE() {
        let r = markoer(40, 20).roteret(270).unwrap();
        assert_eq!((r.bredde(), r.hoejde()), (20, 40));

        // Hele den oeverste venstre kvadrant skal nu vaere hvid.
        for y in 0..20 {
            for x in 0..10 {
                let (b, g, rr, _) = r.pixel(x, y);
                assert!(
                    b == 255 && g == 255 && rr == 255,
                    "pixel ({x},{y}) var ikke hvid efter 270 graders rotation"
                );
            }
        }
        // Og den nederste halvdel skal vaere sort.
        for y in 20..40 {
            assert_eq!(r.pixel(5, y), (0, 0, 0, 255), "pixel (5,{y}) var ikke sort");
        }
    }

    /// Fire (eller to) rotationer i samme retning skal bringe billedet TILBAGE.
    ///
    /// ⛔ Modvidnet mod en rotation der er "tæt paa": en transform der taber en raekke eller
    /// en kolonne, kommer aldrig hjem igen.
    macro_rules! ben_rundtur {
        ($navn:ident, $skridt:expr) => {
            #[test]
            fn $navn() {
                let start = markoer(16, 8);
                let mut nu = start.clone();
                let mut sum = 0;
                loop {
                    nu = nu.roteret($skridt).unwrap();
                    sum += $skridt;
                    if sum % 360 == 0 {
                        break;
                    }
                }
                assert_eq!((nu.bredde(), nu.hoejde()), (start.bredde(), start.hoejde()));
                assert_eq!(nu.pixels(), start.pixels());
            }
        };
    }

    ben_rundtur!(fire_rotationer_bringer_billedet_tilbage_90, 90);
    ben_rundtur!(fire_rotationer_bringer_billedet_tilbage_180, 180);
    ben_rundtur!(fire_rotationer_bringer_billedet_tilbage_270, 270);

    #[test]
    fn KANALERNE_byttes_IKKE() {
        // Et asymmetrisk farvemoenster afsloerer en BGR/RGB-ombytning, som et graat
        // testbillede aldrig ville kunne (maaleregel 8).
        let mut bm = Bitmap32::new(4, 4);
        bm.fyld_sort();
        bm.saet_pixel(0, 0, 10, 20, 30, 255);

        let ud = bm.roteret(180).unwrap().roteret(180).unwrap();
        assert_eq!(ud.pixel(0, 0), (10, 20, 30, 255));
    }

    #[test]
    fn TOP_og_BUND_byttes_IKKE() {
        let mut bm = Bitmap32::new(4, 4);
        bm.fyld_sort();
        bm.saet_pixel(1, 0, 255, 255, 255, 255); // oeverst

        let ud = bm.letterbox(8, 8);
        // Efter letterbox til kvadrat er billedet skaleret 2x og ligger uden sidefelter.
        assert_eq!(ud.pixel(2, 0).0, 255, "den hvide markoer skal blive OEVERST");
        assert_eq!(ud.pixel(2, 7).0, 0, "bunden skal vaere sort");
    }

    #[test]
    fn portraet_kilde_giver_SORTE_sidefelter_frem_for_et_strakt_billede() {
        // 20x40 (portraet) ind i 40x40: bredden skal letterboxes, ikke straekkes.
        let mut portraet = Bitmap32::new(20, 40);
        portraet.fyld(0, 255, 0, 255); // groen

        let ud = portraet.letterbox(40, 40);
        assert_eq!((ud.bredde(), ud.hoejde()), (40, 40));
        assert_eq!(ud.pixel(0, 20), (0, 0, 0, 255), "venstre sidefelt var ikke sort");
        assert_eq!(ud.pixel(39, 20), (0, 0, 0, 255), "hoejre sidefelt var ikke sort");
        assert_eq!(ud.pixel(20, 20), (0, 255, 0, 255), "midten baerer ikke kildens farve");
    }

    #[test]
    fn fra_bgra_afviser_forkert_laengde() {
        assert!(Bitmap32::fra_bgra(2, 2, vec![0; 15]).is_none());
        assert!(Bitmap32::fra_bgra(2, 2, vec![0; 16]).is_some());
        assert!(Bitmap32::fra_bgra(0, 2, vec![]).is_none());
    }
}
