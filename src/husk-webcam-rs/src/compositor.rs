//! Bygger ÉN output-frame pr. tick.
//!
//! Solo = den valgte feed letterboxet ind i hele rammen (ren feed, ingen etiket - den gaar
//! direkte til moedeappen). Grid = alle kilder med auto-kolonner og etiket pr. celle.
//!
//! En kilde uden billede giver et SORT felt med tekst, aldrig en frossen gammel frame. En
//! frossen frame er den vaerste af de tre udfald: den ser levende ud.

use crate::bitmap::Bitmap32;
use crate::placeholder;
use crate::source_status::SourceStatus;

/// Hvad kompositoren bygger: én stor feed, eller alle i et gitter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Solo,
    Grid,
}

/// Det kompositoren skal vide om en kilde.
///
/// En trait frem for en konkret type, saa et testben kan levere en fast frame og en fast
/// tilstand UDEN at rejse en HTTP-server - og saa [`crate::mjpeg_source::MjpegSource`] ikke
/// behoever at kende noget til tegning.
pub trait FrameKilde {
    fn navn(&self) -> &str;

    fn status(&self) -> SourceStatus;

    /// Grundrotation med uret, multiplum af 90.
    fn rotation(&self) -> i32;

    /// Den nyeste AFKODEDE frame, eller `None`.
    fn billede(&self) -> Option<Bitmap32>;
}

pub struct Compositor<'a> {
    pub kilder: &'a [&'a dyn FrameKilde],
    pub bredde: usize,
    pub hoejde: usize,
    pub layout: Layout,
    pub solo_indeks: usize,
}

impl<'a> Compositor<'a> {
    pub fn new(kilder: &'a [&'a dyn FrameKilde], bredde: usize, hoejde: usize) -> Self {
        Compositor { kilder, bredde, hoejde, layout: Layout::Grid, solo_indeks: 0 }
    }

    pub fn byg(&self) -> Bitmap32 {
        if self.layout == Layout::Solo || self.kilder.len() <= 1 {
            if self.kilder.is_empty() {
                return self.tomt_laerred();
            }
            let i = self.solo_indeks.min(self.kilder.len() - 1);
            return self.felt(self.kilder[i], self.bredde, self.hoejde, false);
        }

        let n = self.kilder.len();
        let kol = (n as f64).sqrt().ceil() as usize;
        let raekker = n.div_ceil(kol);
        let celle_b = (self.bredde / kol).max(1);
        let celle_h = (self.hoejde / raekker).max(1);

        let mut laerred = Bitmap32::new(self.bredde, self.hoejde);
        laerred.fyld_sort();

        for (i, k) in self.kilder.iter().enumerate() {
            let felt = self.felt(*k, celle_b, celle_h, true);
            felt.blit_til(
                &mut laerred,
                (i % kol * celle_b) as i64,
                (i / kol * celle_h) as i64,
            );
        }

        laerred
    }

    fn tomt_laerred(&self) -> Bitmap32 {
        let mut b = Bitmap32::new(self.bredde, self.hoejde);
        b.fyld_sort();
        b
    }

    fn felt(&self, kilde: &dyn FrameKilde, b: usize, h: usize, med_etiket: bool) -> Bitmap32 {
        // ⛔ KUN en LIVE kilde viser sit billede. En OFFLINE kilde der stadig havde en gammel
        // frame liggende, ville vise fortid som nutid - og det ser levende ud.
        let billede = if kilde.status() == SourceStatus::Live { kilde.billede() } else { None };

        let mut felt = match billede {
            None => placeholder::byg(kilde.navn(), b, h, kilde.status()),
            Some(bm) => {
                let roteret = if kilde.rotation() == 0 {
                    bm
                } else {
                    // En ugyldig rotation er en fejl i KALDEREN, ikke i billedet. Vi viser
                    // billedet uroteret frem for at kaste midt i en frame-loekke.
                    bm.roteret(kilde.rotation()).unwrap_or(bm)
                };
                roteret.letterbox(b, h)
            }
        };

        if med_etiket {
            placeholder::tegn_etiket(&mut felt, kilde.navn());
        }

        felt
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    struct FastKilde {
        navn: String,
        status: SourceStatus,
        rotation: i32,
        billede: Option<Bitmap32>,
    }

    impl FrameKilde for FastKilde {
        fn navn(&self) -> &str {
            &self.navn
        }
        fn status(&self) -> SourceStatus {
            self.status
        }
        fn rotation(&self) -> i32 {
            self.rotation
        }
        fn billede(&self) -> Option<Bitmap32> {
            self.billede.clone()
        }
    }

    fn ensfarvet(b: usize, h: usize, v: u8) -> Bitmap32 {
        let mut bm = Bitmap32::new(b, h);
        bm.fyld(v, v, v, 255);
        bm
    }

    fn live(navn: &str, v: u8) -> FastKilde {
        FastKilde {
            navn: navn.into(),
            status: SourceStatus::Live,
            rotation: 0,
            billede: Some(ensfarvet(64, 64, v)),
        }
    }

    #[test]
    fn solo_fylder_hele_rammen_uden_etiket() {
        let k = live("A", 200);
        let kilder: Vec<&dyn FrameKilde> = vec![&k];
        let mut c = Compositor::new(&kilder, 128, 128);
        c.layout = Layout::Solo;
        let ud = c.byg();
        assert_eq!((ud.bredde(), ud.hoejde()), (128, 128));
        // Ingen etiket i solo: hjoernet skal vaere billedet selv, ikke en moerk bjaelke.
        assert_eq!(ud.pixel(2, 2), (200, 200, 200, 255));
    }

    #[test]
    fn grid_med_fire_kilder_giver_to_gange_to() {
        let a = live("A", 10);
        let b = live("B", 60);
        let c2 = live("C", 110);
        let d = live("D", 160);
        let kilder: Vec<&dyn FrameKilde> = vec![&a, &b, &c2, &d];
        let mut c = Compositor::new(&kilder, 200, 200);
        c.layout = Layout::Grid;
        let ud = c.byg();
        // Midt i hver celle, uden for etiket-bjaelken.
        assert_eq!(ud.pixel(50, 70).0, 10);
        assert_eq!(ud.pixel(150, 70).0, 60);
        assert_eq!(ud.pixel(50, 170).0, 110);
        assert_eq!(ud.pixel(150, 170).0, 160);
    }

    #[test]
    fn EN_kilde_er_altid_solo_ogsaa_i_grid() {
        let a = live("A", 200);
        let kilder: Vec<&dyn FrameKilde> = vec![&a];
        let mut c = Compositor::new(&kilder, 128, 128);
        c.layout = Layout::Grid;
        let ud = c.byg();
        assert_eq!(ud.pixel(2, 2), (200, 200, 200, 255), "ingen etiket ved én kilde");
    }

    #[test]
    fn offline_kilde_viser_ALDRIG_sin_gamle_frame() {
        // Den vaerste af de tre udfald: en frossen frame ser LEVENDE ud.
        let k = FastKilde {
            navn: "A".into(),
            status: SourceStatus::Offline,
            rotation: 0,
            billede: Some(ensfarvet(64, 64, 200)),
        };
        let kilder: Vec<&dyn FrameKilde> = vec![&k];
        let mut c = Compositor::new(&kilder, 128, 128);
        c.layout = Layout::Solo;
        let ud = c.byg();
        assert_ne!(ud.pixel(64, 20), (200, 200, 200, 255), "den gamle frame maa ikke vises");
        assert_eq!(ud.pixel(64, 5), (20, 20, 20, 255), "placeholder-baggrunden");
    }

    #[test]
    fn ingen_kilder_giver_et_sort_laerred_frem_for_at_panice() {
        let kilder: Vec<&dyn FrameKilde> = vec![];
        let c = Compositor::new(&kilder, 32, 32);
        let ud = c.byg();
        assert_eq!(ud.pixel(16, 16), (0, 0, 0, 255));
    }

    #[test]
    fn solo_indeks_uden_for_listen_klemmes_ind() {
        let a = live("A", 10);
        let b = live("B", 200);
        let kilder: Vec<&dyn FrameKilde> = vec![&a, &b];
        let mut c = Compositor::new(&kilder, 64, 64);
        c.layout = Layout::Solo;
        c.solo_indeks = 99;
        assert_eq!(c.byg().pixel(32, 32).0, 200, "sidste kilde");
    }

    #[test]
    fn rotation_anvendes_paa_kildens_billede() {
        // Et 2:1-billede roteret 90 grader bliver 1:2, saa letterboxen faar sidefelter i
        // stedet for over/under.
        let mut bm = Bitmap32::new(64, 32);
        bm.fyld(200, 200, 200, 255);
        let k = FastKilde {
            navn: "A".into(),
            status: SourceStatus::Live,
            rotation: 90,
            billede: Some(bm),
        };
        let kilder: Vec<&dyn FrameKilde> = vec![&k];
        let mut c = Compositor::new(&kilder, 64, 64);
        c.layout = Layout::Solo;
        let ud = c.byg();
        assert_eq!(ud.pixel(2, 32), (0, 0, 0, 255), "sidefelt til venstre");
        assert_eq!(ud.pixel(32, 32), (200, 200, 200, 255), "billedet i midten");
    }

    #[test]
    fn ugyldig_rotation_viser_billedet_frem_for_at_panice() {
        let k = FastKilde {
            navn: "A".into(),
            status: SourceStatus::Live,
            rotation: 45,
            billede: Some(ensfarvet(64, 64, 200)),
        };
        let kilder: Vec<&dyn FrameKilde> = vec![&k];
        let mut c = Compositor::new(&kilder, 64, 64);
        c.layout = Layout::Solo;
        assert_eq!(c.byg().pixel(32, 32).0, 200);
    }

    #[test]
    fn grid_med_tre_kilder_bruger_to_kolonner() {
        let a = live("A", 10);
        let b = live("B", 60);
        let c2 = live("C", 110);
        let kilder: Vec<&dyn FrameKilde> = vec![&a, &b, &c2];
        let mut c = Compositor::new(&kilder, 200, 200);
        c.layout = Layout::Grid;
        let ud = c.byg();
        assert_eq!(ud.pixel(50, 70).0, 10);
        assert_eq!(ud.pixel(150, 70).0, 60);
        assert_eq!(ud.pixel(50, 170).0, 110);
        // Den fjerde celle er tom og skal vaere sort.
        assert_eq!(ud.pixel(150, 170), (0, 0, 0, 255));
    }
}
