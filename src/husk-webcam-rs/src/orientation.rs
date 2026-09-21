//! Fysisk orientering fra Husks gravity-sensor, uafhaengigt af skaermlaasen.
//!
//! Ordret arvet fra de tidligere udgaver, med samme testvaerdier.
//!
//! ⛔ Det er en NOTE10+-FRONT-FIKSTUR, ikke en universel front/bag-mapping. Den gaelder
//! "sensor 270 grader, JPEG-rotation 0". En anden telefon eller den
//! anden kameraside kan have en anden sammenhaeng, og indtil en generisk metadata-mapping er
//! MAALT, faar hver telefon og kameraside i stedet en manuel grundrotation.

use crate::json::Json;

/// Standard-grundrotation, samme som Python-vejens default.
pub const STANDARD_ROTATION: i32 = 270;

/// Rotation med uret ud fra en gravity-maaling.
///
/// Android-akser: x mod hoejre, y mod telefonens top; gravity peger op.
///
/// Sidste valg BEVARES naar telefonen ligger fladt, staar naer en diagonal, eller naar
/// maalingen er ugyldig. Et flad-liggende ur ville ellers vippe billedet frem og tilbage paa
/// stoej alene.
pub fn front_rotation(vaerdier: Option<&[f64]>, forrige: i32) -> i32 {
    let Some(v) = vaerdier else { return forrige };
    if v.len() < 3 {
        return forrige;
    }

    let (x, y, z) = (v[0], v[1], v[2]);
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return forrige;
    }

    // Fladt: for lidt tyngde i skaerm-planet til at en retning betyder noget.
    if (x * x + y * y).sqrt() < 4.0 {
        return forrige;
    }

    // Naer en diagonal: de to akser er for taet paa hinanden til et sikkert valg.
    let stoerst = x.abs().max(y.abs());
    let mindst = x.abs().min(y.abs());
    if stoerst < 1.35 * mindst {
        return forrige;
    }

    if y.abs() > x.abs() {
        return if y > 0.0 { 270 } else { 90 };
    }

    if x > 0.0 {
        0
    } else {
        180
    }
}

/// Som [`front_rotation`], men for en raa JSON-liste der kan indeholde alt.
///
/// Svarer til Python-vejens `try/except (TypeError, ValueError)`: et element der ikke er et
/// tal, bevarer den forrige rotation frem for at kaste. En streng der PARSER som et tal
/// accepteres, praecis som i Python og i C#-porten.
pub fn front_rotation_raa(raa: Option<&[Json]>, forrige: i32) -> i32 {
    let Some(r) = raa else { return forrige };
    if r.len() < 3 {
        return forrige;
    }

    let mut tal = [0.0f64; 3];
    for (i, v) in r.iter().take(3).enumerate() {
        tal[i] = match v {
            Json::Number(n) => *n,
            Json::String(s) => match s.trim().parse::<f64>() {
                Ok(n) => n,
                Err(_) => return forrige,
            },
            _ => return forrige,
        };
    }

    front_rotation(Some(&tal), forrige)
}

/// Daemper flakken: et skift kraever TO ENS maalinger i traek.
///
/// Daempningen er arvet fra den tidligere udgave. Uden den vipper billedet paa en enkelt
/// stoejmaaling under bevaegelse - og en netvaerksfejl nulstiller kandidaten, saa den naeste
/// gyldige maaling ikke faar lov at taelle som "den anden ens" sammen med en maaling fra foer
/// fejlen.
pub struct OrientationDamper {
    kandidat: Option<i32>,
    rotation: i32,
}

impl OrientationDamper {
    pub fn new(start: i32) -> Self {
        OrientationDamper { kandidat: None, rotation: start }
    }

    /// Den rotation der faktisk gaelder lige nu.
    pub fn rotation(&self) -> i32 {
        self.rotation
    }

    /// Levér en ny maaling. Returnerer den gaeldende rotation bagefter.
    pub fn maaling(&mut self, vaerdier: Option<&[f64]>) -> i32 {
        let foreslaaet = front_rotation(vaerdier, self.rotation);
        if self.kandidat == Some(foreslaaet) {
            self.rotation = foreslaaet;
        }
        self.kandidat = Some(foreslaaet);
        self.rotation
    }

    /// Maalingen kunne ikke tages (netvaerksfejl, ulaeseligt svar).
    ///
    /// Behold den sidste gyldige orientering, og glem kandidaten - ellers kunne en maaling FOER
    /// fejlen parre sig med en maaling EFTER og se ud som to ens i traek.
    pub fn fejlede(&mut self) {
        self.kandidat = None;
    }
}

impl Default for OrientationDamper {
    fn default() -> Self {
        Self::new(STANDARD_ROTATION)
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    /// De fire entydige stillinger. ÉT ben pr. stilling: en loekke ville stoppe ved den
    /// foerste, og et roedt ben ville ikke sige hvilken stilling der var brudt.
    macro_rules! ben_stilling {
        ($navn:ident, $x:expr, $y:expr, $z:expr, $grader:expr) => {
            #[test]
            fn $navn() {
                // Den forrige vaerdi er en der ALDRIG kan vaere svaret, saa et ben ikke kan
                // bestaa ved at funktionen bare beholdt den.
                assert_eq!(front_rotation(Some(&[$x, $y, $z]), 999), $grader);
            }
        };
    }

    ben_stilling!(stilling_staaende_giver_270, 0.0, 9.8, 0.0, 270);
    ben_stilling!(stilling_liggende_hoejre_giver_0, 9.8, 0.0, 0.0, 0);
    ben_stilling!(stilling_paa_hovedet_giver_90, 0.0, -9.8, 0.0, 90);
    ben_stilling!(stilling_liggende_venstre_giver_180, -9.8, 0.0, 0.0, 180);

    /// De fire tilstande hvor sidste valg skal BEVARES.
    macro_rules! ben_bevarer {
        ($navn:ident, $x:expr, $y:expr, $z:expr) => {
            #[test]
            fn $navn() {
                assert_eq!(front_rotation(Some(&[$x, $y, $z]), 123), 123);
            }
        };
    }

    ben_bevarer!(fladt_bevarer_forrige, 0.0, 0.0, 9.8);
    ben_bevarer!(diagonal_bevarer_forrige, 6.9, 6.9, 0.0);
    ben_bevarer!(nan_bevarer_forrige, f64::NAN, 9.0, 0.0);
    ben_bevarer!(uendelig_bevarer_forrige, 0.0, f64::INFINITY, 0.0);

    #[test]
    fn for_faa_vaerdier_bevarer_forrige() {
        assert_eq!(front_rotation(Some(&[9.8, 0.0]), 90), 90);
        assert_eq!(front_rotation(None, 90), 90);
        assert_eq!(front_rotation(Some(&[]), 90), 90);
    }

    #[test]
    fn raa_liste_med_tal_som_strenge_virker() {
        let raa = vec![
            Json::String("0".into()),
            Json::String("9.8".into()),
            Json::Number(0.0),
        ];
        assert_eq!(front_rotation_raa(Some(&raa), 0), 270);
    }

    #[test]
    fn raa_liste_med_skrald_bevarer_forrige() {
        let raa = vec![Json::Bool(true), Json::Number(9.8), Json::Number(0.0)];
        assert_eq!(front_rotation_raa(Some(&raa), 123), 123);
        assert_eq!(front_rotation_raa(None, 123), 123);
    }

    #[test]
    fn damper_kraever_to_ens_i_traek() {
        let mut d = OrientationDamper::new(270);
        // Foerste afvigende maaling maa IKKE flytte rotationen.
        assert_eq!(d.maaling(Some(&[9.8, 0.0, 0.0])), 270);
        // Anden ens maaling flytter den.
        assert_eq!(d.maaling(Some(&[9.8, 0.0, 0.0])), 0);
        assert_eq!(d.rotation(), 0);
    }

    #[test]
    fn damper_flakker_ikke_paa_en_enkelt_stoejmaaling() {
        let mut d = OrientationDamper::new(270);
        d.maaling(Some(&[9.8, 0.0, 0.0])); // stoej
        d.maaling(Some(&[0.0, 9.8, 0.0])); // tilbage til staaende
        assert_eq!(d.rotation(), 270);
    }

    #[test]
    fn fejlede_glemmer_kandidaten() {
        let mut d = OrientationDamper::new(270);
        d.maaling(Some(&[9.8, 0.0, 0.0]));
        d.fejlede();
        // Efter fejlen maa den foerste maaling ikke taelle som "den anden ens".
        assert_eq!(d.maaling(Some(&[9.8, 0.0, 0.0])), 270);
        assert_eq!(d.maaling(Some(&[9.8, 0.0, 0.0])), 0);
    }

    #[test]
    fn en_FEJL_BEVARER_den_sidste_gyldige_orientering() {
        // En netvaerksfejl maa ikke vippe billedet tilbage til udgangspunktet: det sidste
        // GYLDIGE valg staar ved magt, indtil to nye ens maalinger flytter det.
        let mut d = OrientationDamper::new(270);
        d.maaling(Some(&[9.8, 0.0, 0.0]));
        d.maaling(Some(&[9.8, 0.0, 0.0]));
        assert_eq!(d.rotation(), 0);
        d.fejlede();
        assert_eq!(d.rotation(), 0);
    }

    #[test]
    fn damper_default_starter_paa_standardrotationen() {
        assert_eq!(OrientationDamper::default().rotation(), STANDARD_ROTATION);
    }
}
