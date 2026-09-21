//! En minimal JSON-laeser og -skriver.
//!
//! ⛔ HVORFOR DEN ER SKREVET I HUSET og ikke hentet som en pakke.
//!
//! C#-udgaven fik `System.Text.Json` gratis, fordi den laa i den runtime vi nu smider ud.
//! Rust har ingen runtime, saa enhver afhaengighed er noget vi selv laegger i pakken. Behovet
//! er lille og fast: en konfigurationsfil vi selv skriver, og to smaa svar fra telefonen
//! (`/flags` og `/sensor`). Det er under 300 linjer at daekke - og en pakke ville koste det i
//! bytes hver eneste gang, plus en forsyningskaede vi ikke kontrollerer.
//!
//! Den er bevidst SNAEVER: ingen streaming, intet skema, ingen afledte traits. Den laeser det
//! Husk sender, og skriver det vi selv har skrevet.
//!
//! Den fejler LUKKET: en ulaeselig fil giver `Err`, aldrig en halv struktur.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Vec frem for en map: raekkefoelgen bevares, saa en gemt config-fil ser ens ud hver
    /// gang. En fil der skifter raekkefoelge ved hver skrivning kan ikke diffes.
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, noegle: &str) -> Option<&Json> {
        match self {
            Json::Object(felter) => felter.iter().find(|(k, _)| k == noegle).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn som_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn som_tal(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn som_i32(&self) -> Option<i32> {
        self.som_tal().map(|n| n as i32)
    }

    pub fn som_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn som_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Skriv med indrykning, som C#-udgavens `WriteIndented = true`.
    pub fn til_tekst(&self) -> String {
        let mut s = String::new();
        self.skriv(&mut s, 0);
        s
    }

    fn skriv(&self, ud: &mut String, dybde: usize) {
        match self {
            Json::Null => ud.push_str("null"),
            Json::Bool(true) => ud.push_str("true"),
            Json::Bool(false) => ud.push_str("false"),
            Json::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(ud, "{}", *n as i64);
                } else {
                    let _ = write!(ud, "{n}");
                }
            }
            Json::String(s) => skriv_streng(ud, s),
            Json::Array(a) => {
                if a.is_empty() {
                    ud.push_str("[]");
                    return;
                }
                ud.push_str("[\n");
                for (i, v) in a.iter().enumerate() {
                    indryk(ud, dybde + 1);
                    v.skriv(ud, dybde + 1);
                    if i + 1 < a.len() {
                        ud.push(',');
                    }
                    ud.push('\n');
                }
                indryk(ud, dybde);
                ud.push(']');
            }
            Json::Object(felter) => {
                if felter.is_empty() {
                    ud.push_str("{}");
                    return;
                }
                ud.push_str("{\n");
                for (i, (k, v)) in felter.iter().enumerate() {
                    indryk(ud, dybde + 1);
                    skriv_streng(ud, k);
                    ud.push_str(": ");
                    v.skriv(ud, dybde + 1);
                    if i + 1 < felter.len() {
                        ud.push(',');
                    }
                    ud.push('\n');
                }
                indryk(ud, dybde);
                ud.push('}');
            }
        }
    }
}

fn indryk(ud: &mut String, dybde: usize) {
    for _ in 0..dybde {
        ud.push_str("  ");
    }
}

fn skriv_streng(ud: &mut String, s: &str) {
    ud.push('"');
    for c in s.chars() {
        match c {
            '"' => ud.push_str("\\\""),
            '\\' => ud.push_str("\\\\"),
            '\n' => ud.push_str("\\n"),
            '\r' => ud.push_str("\\r"),
            '\t' => ud.push_str("\\t"),
            // Kontroltegn SKAL escapes; alt andet - ogsaa ae/oe/aa - skrives som UTF-8.
            c if (c as u32) < 0x20 => {
                let _ = write!(ud, "\\u{:04x}", c as u32);
            }
            c => ud.push(c),
        }
    }
    ud.push('"');
}

/// Laes en JSON-tekst. Fejler LUKKET: enten et helt dokument, eller en fejl med position.
pub fn parse(tekst: &str) -> Result<Json, String> {
    let b = tekst.as_bytes();
    let mut p = Parser { b, i: 0, dybde: 0 };
    p.spring_hvidt();
    let v = p.vaerdi()?;
    p.spring_hvidt();
    if p.i != b.len() {
        return Err(format!("overskydende tegn ved position {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    dybde: usize,
}

/// Et loft paa indlejringsdybden. En fjendtlig eller fejlbehaeftet modpart maa ikke kunne
/// sprænge stakken med en dyb struktur - graensen maales FOER rekursionen, ikke efter.
const MAX_DYBDE: usize = 64;

impl<'a> Parser<'a> {
    fn spring_hvidt(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn vaerdi(&mut self) -> Result<Json, String> {
        if self.dybde > MAX_DYBDE {
            return Err("for dyb indlejring".to_string());
        }
        let Some(&c) = self.b.get(self.i) else {
            return Err("uventet slutning".to_string());
        };
        match c {
            b'{' => self.objekt(),
            b'[' => self.array(),
            b'"' => self.streng().map(Json::String),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'n' => self.literal("null", Json::Null),
            _ => self.tal(),
        }
    }

    fn literal(&mut self, ord: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(ord.as_bytes()) {
            self.i += ord.len();
            Ok(v)
        } else {
            Err(format!("ukendt literal ved position {}", self.i))
        }
    }

    fn tal(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.i < self.b.len() && (self.b[self.i] == b'-' || self.b[self.i] == b'+') {
            self.i += 1;
        }
        while self.i < self.b.len()
            && matches!(self.b[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
        {
            self.i += 1;
        }
        if start == self.i {
            return Err(format!("forventede en vaerdi ved position {start}"));
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
        s.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("ulaeseligt tal '{s}' ved position {start}"))
    }

    fn streng(&mut self) -> Result<String, String> {
        // Kaldes kun naar b[i] == '"'.
        self.i += 1;
        let mut ud = String::new();
        loop {
            let Some(&c) = self.b.get(self.i) else {
                return Err("uafsluttet streng".to_string());
            };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(ud);
                }
                b'\\' => {
                    self.i += 1;
                    let Some(&e) = self.b.get(self.i) else {
                        return Err("uafsluttet escape".to_string());
                    };
                    self.i += 1;
                    match e {
                        b'"' => ud.push('"'),
                        b'\\' => ud.push('\\'),
                        b'/' => ud.push('/'),
                        b'b' => ud.push('\u{8}'),
                        b'f' => ud.push('\u{c}'),
                        b'n' => ud.push('\n'),
                        b'r' => ud.push('\r'),
                        b't' => ud.push('\t'),
                        b'u' => ud.push(self.unicode_escape()?),
                        _ => return Err(format!("ukendt escape '\\{}'", e as char)),
                    }
                }
                _ => {
                    // UTF-8 laeses som UTF-8: vi finder slutningen af sekvensen og kopierer den
                    // hel. En byte-for-byte-kopi ville braekke ae/oe/aa i et telefonnavn.
                    let laengde = utf8_laengde(c);
                    let Some(stykke) = self.b.get(self.i..self.i + laengde) else {
                        return Err("afkortet UTF-8".to_string());
                    };
                    let s = std::str::from_utf8(stykke).map_err(|_| "ugyldig UTF-8".to_string())?;
                    ud.push_str(s);
                    self.i += laengde;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let hex = self
            .b
            .get(self.i..self.i + 4)
            .ok_or_else(|| "afkortet \\u".to_string())?;
        let s = std::str::from_utf8(hex).map_err(|_| "ugyldig \\u".to_string())?;
        let n = u32::from_str_radix(s, 16).map_err(|_| format!("ugyldig \\u{s}"))?;
        self.i += 4;

        // Surrogat-par: hoej surrogat skal efterfoelges af en lav.
        if (0xD800..0xDC00).contains(&n) {
            if self.b.get(self.i) == Some(&b'\\') && self.b.get(self.i + 1) == Some(&b'u') {
                self.i += 2;
                let hex2 = self
                    .b
                    .get(self.i..self.i + 4)
                    .ok_or_else(|| "afkortet lav surrogat".to_string())?;
                let s2 = std::str::from_utf8(hex2).map_err(|_| "ugyldig \\u".to_string())?;
                let lav = u32::from_str_radix(s2, 16).map_err(|_| format!("ugyldig \\u{s2}"))?;
                self.i += 4;
                if (0xDC00..0xE000).contains(&lav) {
                    let kode = 0x10000 + ((n - 0xD800) << 10) + (lav - 0xDC00);
                    return char::from_u32(kode).ok_or_else(|| "ugyldigt surrogat-par".to_string());
                }
            }
            return Err("hoej surrogat uden lav".to_string());
        }

        char::from_u32(n).ok_or_else(|| format!("ugyldigt kodepunkt U+{n:04X}"))
    }

    fn array(&mut self) -> Result<Json, String> {
        self.i += 1;
        self.dybde += 1;
        let mut ud = Vec::new();
        self.spring_hvidt();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            self.dybde -= 1;
            return Ok(Json::Array(ud));
        }
        loop {
            self.spring_hvidt();
            ud.push(self.vaerdi()?);
            self.spring_hvidt();
            match self.b.get(self.i) {
                Some(&b',') => self.i += 1,
                Some(&b']') => {
                    self.i += 1;
                    self.dybde -= 1;
                    return Ok(Json::Array(ud));
                }
                _ => return Err(format!("forventede ',' eller ']' ved position {}", self.i)),
            }
        }
    }

    fn objekt(&mut self) -> Result<Json, String> {
        self.i += 1;
        self.dybde += 1;
        let mut ud: Vec<(String, Json)> = Vec::new();
        self.spring_hvidt();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            self.dybde -= 1;
            return Ok(Json::Object(ud));
        }
        loop {
            self.spring_hvidt();
            if self.b.get(self.i) != Some(&b'"') {
                return Err(format!("forventede en noegle ved position {}", self.i));
            }
            let k = self.streng()?;
            self.spring_hvidt();
            if self.b.get(self.i) != Some(&b':') {
                return Err(format!("forventede ':' ved position {}", self.i));
            }
            self.i += 1;
            self.spring_hvidt();
            let v = self.vaerdi()?;
            ud.push((k, v));
            self.spring_hvidt();
            match self.b.get(self.i) {
                Some(&b',') => self.i += 1,
                Some(&b'}') => {
                    self.i += 1;
                    self.dybde -= 1;
                    return Ok(Json::Object(ud));
                }
                _ => return Err(format!("forventede ',' eller '}}' ved position {}", self.i)),
            }
        }
    }
}

fn utf8_laengde(foerste: u8) -> usize {
    match foerste {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn laeser_et_fladt_objekt() {
        let j = parse(r#"{"a": 1, "b": "to", "c": true, "d": null}"#).unwrap();
        assert_eq!(j.get("a").unwrap().som_i32(), Some(1));
        assert_eq!(j.get("b").unwrap().som_str(), Some("to"));
        assert_eq!(j.get("c").unwrap().som_bool(), Some(true));
        assert_eq!(j.get("d"), Some(&Json::Null));
        assert!(j.get("findes-ikke").is_none());
    }

    #[test]
    fn laeser_et_array_af_tal() {
        let j = parse("[1, -2.5, 3e2]").unwrap();
        let a = j.som_array().unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].som_tal(), Some(1.0));
        assert_eq!(a[1].som_tal(), Some(-2.5));
        assert_eq!(a[2].som_tal(), Some(300.0));
    }

    #[test]
    fn laeser_aeoeaa_i_en_streng() {
        // Et telefonnavn maa gerne hedde noget dansk. En byte-for-byte-kopi ville braekke det.
        let j = parse("{\"navn\": \"Tel\u{00e6}fon \u{00f8}st \u{00e5}bn\"}").unwrap();
        assert_eq!(j.get("navn").unwrap().som_str(), Some("Tel\u{00e6}fon \u{00f8}st \u{00e5}bn"));
    }

    #[test]
    fn laeser_escapes_og_surrogatpar() {
        let j = parse(r#"{"s": "a\"b\\c\ndæ😀"}"#).unwrap();
        assert_eq!(
            j.get("s").unwrap().som_str(),
            Some("a\"b\\c\nd\u{00e6}\u{1F600}")
        );
    }

    #[test]
    fn fejler_lukket_paa_skrald() {
        assert!(parse("{").is_err());
        assert!(parse("{\"a\": }").is_err());
        assert!(parse("").is_err());
        assert!(parse("{} overskydende").is_err());
        assert!(parse("\"uafsluttet").is_err());
    }

    #[test]
    fn dyb_indlejring_afvises_frem_for_at_spraenge_stakken() {
        let dyb = "[".repeat(500) + &"]".repeat(500);
        assert!(parse(&dyb).is_err());
    }

    #[test]
    fn skriver_og_laeser_igen_giver_samme_struktur() {
        let ind = r#"{"tal": 42, "flag": false, "liste": [1, 2], "tekst": "å"}"#;
        let j = parse(ind).unwrap();
        let ud = j.til_tekst();
        assert_eq!(parse(&ud).unwrap(), j);
    }

    #[test]
    fn heltal_skrives_uden_decimaler() {
        assert_eq!(Json::Number(270.0).til_tekst(), "270");
        assert_eq!(Json::Number(0.0).til_tekst(), "0");
    }

    #[test]
    fn tomme_beholdere_skrives_kompakt() {
        assert_eq!(Json::Array(vec![]).til_tekst(), "[]");
        assert_eq!(Json::Object(vec![]).til_tekst(), "{}");
    }

    #[test]
    fn kontroltegn_escapes_i_output() {
        let s = Json::String("a\u{1}b".to_string()).til_tekst();
        assert_eq!(s, "\"a\\u0001b\"");
    }
}
