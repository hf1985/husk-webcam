//! Appens konfiguration. Port af `Config.cs`.
//!
//! Bor i `%LOCALAPPDATA%\husk-viewer\`, ALDRIG i projektmappen. Mappenavnet er arvet fra
//! produktets foerste navn og maa IKKE aendres: en omdoebning ville tabe brugerens opsaetning
//! og token ved naeste opgradering.
//!
//! Skrivningen er ATOMISK (temp + erstat), saa en afbrudt skrivning ikke efterlader en halv
//! JSON-fil. En config der ikke kan laeses, er en app der ikke kan starte.
//!
//! ⛔ FELTNAVNENE ER EN KONTRAKT MED C#-UDGAVEN. De to udgaver skal kunne laese hinandens
//! `config.json`, for ellers mister brugeren sin opsaetning og sit token den dag han skifter.
//! Navnene er derfor ordret dem `System.Text.Json` skrev: PascalCase, undtagen
//! `tokenBeskyttet` som C#-siden gav et eksplicit `JsonPropertyName`.

use std::path::PathBuf;

use crate::compositor::Layout;
use crate::json::{self, Json};
use crate::secret;

/// Én telefon, som brugeren har sat den op.
#[derive(Clone, Debug, PartialEq)]
pub struct TelefonConfig {
    /// Navnet brugeren gav den. Vises paa skaermen.
    pub navn: String,
    /// Adresse, evt. med port. Husk viser den i sin egen statusvisning.
    pub vaert: String,
    /// DPAPI-beskyttet token, base64. ALDRIG klartekst i filen.
    pub token_beskyttet: Option<String>,
    /// Manuel GRUNDROTATION for netop denne telefon og kameraside.
    ///
    /// Indtil en generisk metadata-mapping er MAALT, er tyngde-sporingen en
    /// Note10+-front-fikstur, saa hver telefon faar sin egen vaerdi frem for at arve en andens.
    pub grundrotation: i32,
    /// Foelg tyngdesensoren, eller bliv paa grundrotationen.
    pub foelg_tyngde: bool,
    /// Foretrukken kameraside. Kraever Husk 1.1 paa telefonen.
    pub front: bool,
}

impl Default for TelefonConfig {
    fn default() -> Self {
        TelefonConfig {
            navn: "Telefon".to_string(),
            vaert: String::new(),
            token_beskyttet: None,
            grundrotation: 0,
            foelg_tyngde: false,
            front: false,
        }
    }
}

impl TelefonConfig {
    /// Tokenet i klartekst, eller `None`. Laeses kun naar det skal bruges.
    pub fn token(&self) -> Option<String> {
        secret::afbeskyt(self.token_beskyttet.as_deref())
    }

    /// Saet tokenet. En tom vaerdi fjerner det helt.
    pub fn saet_token(&mut self, klartekst: Option<&str>) {
        self.token_beskyttet = match klartekst {
            Some(k) if !k.is_empty() => secret::beskyt(k),
            _ => None,
        };
    }

    fn fra_json(j: &Json) -> TelefonConfig {
        let mut t = TelefonConfig::default();
        if let Some(v) = j.get("Navn").and_then(|v| v.som_str()) {
            t.navn = v.to_string();
        }
        if let Some(v) = j.get("Vaert").and_then(|v| v.som_str()) {
            t.vaert = v.to_string();
        }
        t.token_beskyttet = j.get("tokenBeskyttet").and_then(|v| v.som_str()).map(str::to_string);
        if let Some(v) = j.get("Grundrotation").and_then(|v| v.som_i32()) {
            t.grundrotation = v;
        }
        if let Some(v) = j.get("FoelgTyngde").and_then(|v| v.som_bool()) {
            t.foelg_tyngde = v;
        }
        if let Some(v) = j.get("Front").and_then(|v| v.som_bool()) {
            t.front = v;
        }
        t
    }

    fn til_json(&self) -> Json {
        let mut felter: Vec<(String, Json)> = vec![
            ("Navn".into(), Json::String(self.navn.clone())),
            ("Vaert".into(), Json::String(self.vaert.clone())),
        ];
        // DefaultIgnoreCondition = WhenWritingNull: et manglende token skrives slet ikke.
        if let Some(t) = &self.token_beskyttet {
            felter.push(("tokenBeskyttet".into(), Json::String(t.clone())));
        }
        felter.push(("Grundrotation".into(), Json::Number(self.grundrotation as f64)));
        felter.push(("FoelgTyngde".into(), Json::Bool(self.foelg_tyngde)));
        felter.push(("Front".into(), Json::Bool(self.front)));
        Json::Object(felter)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub telefoner: Vec<TelefonConfig>,
    /// Solo eller gitter.
    pub layout: Layout,
    /// Hvilken telefon der vises i solo-tilstand.
    pub solo_indeks: usize,
    /// Har foerstegangsguiden vaeret igennem?
    pub guide_fuldfoert: bool,
}

impl Config {
    /// Standardmappen. Kan overstyres af `HUSKWEBCAM_CONFIGDIR` i tests.
    pub fn mappe() -> PathBuf {
        if let Some(ydre) = std::env::var_os("HUSKWEBCAM_CONFIGDIR") {
            if !ydre.is_empty() {
                return PathBuf::from(ydre);
            }
        }
        let lokal = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        lokal.join("husk-viewer")
    }

    pub fn sti() -> PathBuf {
        Self::mappe().join("config.json")
    }

    /// Laes konfigurationen.
    ///
    /// En manglende fil giver en TOM config, ikke en fejl - foerste start er en normal
    /// tilstand. En ULAESELIG fil giver ogsaa en tom config, men anden del af returvaerdien
    /// siger hvorfor, saa UI'et kan vise det frem for tavst at starte forfra og se ud som om
    /// brugerens opsaetning aldrig har eksisteret.
    pub fn laes() -> (Config, Option<String>) {
        let sti = Self::sti();
        if !sti.is_file() {
            return (Config::default(), None);
        }

        let raa = match std::fs::read_to_string(&sti) {
            Ok(s) => s,
            Err(e) => {
                return (Config::default(), Some(format!("config.json kunne ikke aabnes: {e}")));
            }
        };

        match json::parse(&raa) {
            Ok(j) => (Config::fra_json(&j), None),
            Err(e) => (Config::default(), Some(format!("config.json kunne ikke laeses: {e}"))),
        }
    }

    pub fn fra_json(j: &Json) -> Config {
        let mut c = Config::default();
        if let Some(a) = j.get("Telefoner").and_then(|v| v.som_array()) {
            c.telefoner = a.iter().map(TelefonConfig::fra_json).collect();
        }
        // System.Text.Json skriver et enum som TAL uden en converter, saa Solo=0, Grid=1.
        if let Some(n) = j.get("Layout").and_then(|v| v.som_i32()) {
            c.layout = if n == 1 { Layout::Grid } else { Layout::Solo };
        }
        if let Some(n) = j.get("SoloIndeks").and_then(|v| v.som_i32()) {
            c.solo_indeks = n.max(0) as usize;
        }
        if let Some(b) = j.get("GuideFuldfoert").and_then(|v| v.som_bool()) {
            c.guide_fuldfoert = b;
        }
        c
    }

    pub fn til_json(&self) -> Json {
        Json::Object(vec![
            (
                "Telefoner".into(),
                Json::Array(self.telefoner.iter().map(TelefonConfig::til_json).collect()),
            ),
            ("Layout".into(), Json::Number(if self.layout == Layout::Grid { 1.0 } else { 0.0 })),
            ("SoloIndeks".into(), Json::Number(self.solo_indeks as f64)),
            ("GuideFuldfoert".into(), Json::Bool(self.guide_fuldfoert)),
        ])
    }

    /// Gem ATOMISK: skriv en temp-fil, og erstat foerst derefter den rigtige.
    pub fn gem(&self) -> std::io::Result<()> {
        let mappe = Self::mappe();
        std::fs::create_dir_all(&mappe)?;
        let sti = Self::sti();
        let temp = sti.with_extension("json.ny");

        // Kod FOER filen aabnes: en skrivning der truncerer destinationen foer nyttelasten er
        // kodet, taber filen naar kodningen fejler (maaleregel 256).
        let bytes = self.til_json().til_tekst().into_bytes();
        std::fs::write(&temp, &bytes)?;
        std::fs::rename(&temp, &sti)?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    /// ⛔ ÉN laas om `HUSKWEBCAM_CONFIGDIR`, ikke én pr. ben.
    ///
    /// Variablen er PROCES-GLOBAL, og cargo koerer ben i parallelle traade i SAMME proces.
    /// To forskellige laase om den samme globale udelukker ikke hinanden - det er en laas der
    /// ser ud som et vaern og ikke er et (maaleregel 230). Symptomet ville vaere et ben der
    /// gaar roedt i ny og nae, afhaengigt af hvilke andre ben der koerte samtidig.
    static ENV_LAAS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// En config-mappe der kun hoerer til dette ben. Rydder op efter sig selv.
    ///
    /// ⛔ Hvert ben faar sin EGEN mappe, og navnet baerer en taeller: `HUSKWEBCAM_CONFIGDIR` er
    /// proces-global, og cargo koerer ben i PARALLELLE traade i samme proces. To ben der delte
    /// mappe ville se hinandens filer, og et groent resultat ville afhaenge af raekkefoelgen.
    struct MidlertidigConfig {
        mappe: PathBuf,
    }

    impl MidlertidigConfig {
        fn ny(navn: &str) -> Self {
            let mappe = std::env::temp_dir().join(format!("husk-rs-test-{navn}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&mappe);
            std::fs::create_dir_all(&mappe).unwrap();
            MidlertidigConfig { mappe }
        }

        /// Kald ind i koden med netop denne mappe som config-mappe.
        ///
        /// Laasen serialiserer benene, saa den proces-globale env-variabel ikke laekker mellem
        /// dem. Serialisering er ikke et vaern (maaleregel 230), men her ER det kravet: to
        /// samtidige skrivere af én env-variabel kan ikke give et entydigt svar.
        fn kaldt<T>(&self, f: impl FnOnce() -> T) -> T {
            let _g = ENV_LAAS.lock().unwrap_or_else(|e| e.into_inner());
            std::env::set_var("HUSKWEBCAM_CONFIGDIR", &self.mappe);
            let r = f();
            std::env::remove_var("HUSKWEBCAM_CONFIGDIR");
            r
        }
    }

    impl Drop for MidlertidigConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.mappe);
        }
    }

    #[test]
    fn manglende_fil_giver_tom_config_uden_fejl() {
        let m = MidlertidigConfig::ny("manglende");
        let (c, fejl) = m.kaldt(Config::laes);
        assert!(fejl.is_none(), "foerste start er en NORMAL tilstand");
        assert!(c.telefoner.is_empty());
        assert_eq!(c.layout, Layout::Solo);
    }

    #[test]
    fn ulaeselig_fil_giver_tom_config_MED_fejl() {
        let m = MidlertidigConfig::ny("ulaeselig");
        std::fs::write(m.mappe.join("config.json"), "{ dette er ikke json").unwrap();
        let (c, fejl) = m.kaldt(Config::laes);
        assert!(c.telefoner.is_empty());
        let f = fejl.expect("en ulaeselig fil maa ALDRIG se ud som en foerste start");
        assert!(f.contains("config.json"), "faktisk: {f}");
    }

    #[test]
    fn rundtur_bevarer_alle_felter() {
        let m = MidlertidigConfig::ny("rundtur");
        let mut c = Config {
            telefoner: vec![TelefonConfig {
                navn: "Note10+".into(),
                vaert: "192.0.2.10".into(),
                token_beskyttet: None,
                grundrotation: 270,
                foelg_tyngde: true,
                front: true,
            }],
            layout: Layout::Grid,
            solo_indeks: 0,
            guide_fuldfoert: true,
        };
        c.telefoner[0].saet_token(Some("hemmeligt"));

        let (laest, fejl) = m.kaldt(|| {
            c.gem().unwrap();
            Config::laes()
        });
        assert!(fejl.is_none());
        assert_eq!(laest, c);
        assert_eq!(laest.telefoner[0].token().as_deref(), Some("hemmeligt"));
    }

    #[test]
    fn tokenet_staar_ALDRIG_i_klartekst_i_filen() {
        let m = MidlertidigConfig::ny("klartekst");
        let mut c = Config::default();
        let mut t = TelefonConfig::default();
        t.saet_token(Some("s3kr3t-VAERDI"));
        c.telefoner.push(t);

        let raa = m.kaldt(|| {
            c.gem().unwrap();
            std::fs::read_to_string(Config::sti()).unwrap()
        });
        assert!(!raa.contains("s3kr3t-VAERDI"), "klartekst i config.json: {raa}");
        assert!(raa.contains("tokenBeskyttet"));
    }

    #[test]
    fn tomt_token_fjerner_feltet_helt() {
        let mut t = TelefonConfig::default();
        t.saet_token(Some("noget"));
        assert!(t.token_beskyttet.is_some());
        t.saet_token(Some(""));
        assert!(t.token_beskyttet.is_none());
        t.saet_token(Some("noget"));
        t.saet_token(None);
        assert!(t.token_beskyttet.is_none());
        assert!(!t.til_json().til_tekst().contains("tokenBeskyttet"));
    }

    #[test]
    fn feltnavnene_er_de_samme_som_c_sharp_skrev() {
        // Kontrakten med C#-udgaven: de to skal kunne laese hinandens config.json.
        let mut c = Config::default();
        c.telefoner.push(TelefonConfig::default());
        let s = c.til_json().til_tekst();
        for navn in ["Telefoner", "Layout", "SoloIndeks", "GuideFuldfoert",
                     "Navn", "Vaert", "Grundrotation", "FoelgTyngde", "Front"] {
            assert!(s.contains(&format!("\"{navn}\"")), "manglede {navn} i: {s}");
        }
    }

    #[test]
    fn laeser_en_config_i_den_gamle_pascalcase_form() {
        // Ordret den form den tidligere udgave skrev: PascalCase, enum som tal.
        let raa = r#"{
  "Telefoner": [
    {
      "Navn": "Note10+",
      "Vaert": "192.0.2.10",
      "Grundrotation": 270,
      "FoelgTyngde": true,
      "Front": true
    }
  ],
  "Layout": 1,
  "SoloIndeks": 0,
  "GuideFuldfoert": true
}"#;
        let c = Config::fra_json(&json::parse(raa).unwrap());
        assert_eq!(c.telefoner.len(), 1);
        assert_eq!(c.telefoner[0].navn, "Note10+");
        assert_eq!(c.telefoner[0].grundrotation, 270);
        assert!(c.telefoner[0].front);
        assert_eq!(c.layout, Layout::Grid);
        assert!(c.guide_fuldfoert);
    }

    #[test]
    fn ukendte_felter_ignoreres_frem_for_at_faelde_laesningen() {
        // En nyere udgave maa gerne tilfoeje felter uden at braekke en aeldre.
        let c = Config::fra_json(&json::parse(r#"{"Layout": 1, "NoegetNyt": 42}"#).unwrap());
        assert_eq!(c.layout, Layout::Grid);
    }

    #[test]
    fn en_UDSKIFTNING_af_tokenet_slaar_igennem() {
        let mut t = TelefonConfig::default();
        t.saet_token(Some("foerste"));
        let foerste_blob = t.token_beskyttet.clone();
        t.saet_token(Some("andet"));
        assert_eq!(t.token().as_deref(), Some("andet"));
        assert_ne!(t.token_beskyttet, foerste_blob, "blobben skal vaere skiftet");
    }

    #[test]
    fn FLERE_end_en_telefon_kan_gemmes() {
        // Multikamera-kravet kan ellers kun naas ved at redigere filer i haanden.
        let m = MidlertidigConfig::ny("flere");
        let mut c = Config::default();
        for i in 1..=3 {
            c.telefoner.push(TelefonConfig {
                navn: format!("Telefon {i}"),
                vaert: format!("10.0.0.{i}"),
                ..Default::default()
            });
        }
        let (laest, fejl) = m.kaldt(|| {
            c.gem().unwrap();
            Config::laes()
        });
        assert!(fejl.is_none());
        assert_eq!(laest.telefoner.len(), 3);
        assert_eq!(laest.telefoner[2].vaert, "10.0.0.3");
    }

    #[test]
    fn config_ligger_IKKE_i_projektmappen() {
        // ⛔ Ligger kildetraeet i en sky-synket mappe, ville en config dér laegge en -
        // ganske vist DPAPI-beskyttet - hemmelighed i skyen.
        //
        // Benet maaler den RIGTIGE sti, altsaa UDEN miljoevariablen: det er den vej driften
        // bruger, og en maaling af testens egen overstyring ville bevise ingenting.
        let _g = ENV_LAAS.lock().unwrap_or_else(|e| e.into_inner());
        let gammel = std::env::var_os("HUSKWEBCAM_CONFIGDIR");
        std::env::remove_var("HUSKWEBCAM_CONFIGDIR");
        let sti = Config::sti();
        if let Some(v) = gammel {
            std::env::set_var("HUSKWEBCAM_CONFIGDIR", v);
        }

        let tekst = sti.to_string_lossy().to_ascii_lowercase();
        assert!(
            tekst.contains("husk-viewer"),
            "config-stien skal ligge i sin egen mappe: {tekst}"
        );
        let projekt = env!("CARGO_MANIFEST_DIR").to_ascii_lowercase();
        assert!(
            !tekst.starts_with(&projekt),
            "config-stien laa I KILDETRAEET: {tekst}"
        );
    }

    #[test]
    fn gem_er_atomisk_og_efterlader_ingen_temp_fil() {
        let m = MidlertidigConfig::ny("atomisk");
        let c = Config::default();
        m.kaldt(|| c.gem().unwrap());
        let filer: Vec<String> = std::fs::read_dir(&m.mappe)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        assert_eq!(filer, vec!["config.json".to_string()], "faktisk: {filer:?}");
    }
}
