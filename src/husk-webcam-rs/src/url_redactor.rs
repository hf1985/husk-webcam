//! Fjerner hemmeligheder fra en URL, saa den kan skrives i en log.
//!
//! Kontrakten er arvet ordret fra de tidligere udgaver, inklusive deres testkorpus, og de
//! tre kanter der kostede maalinger dengang er bevaret med vilje:
//!
//! 1. ALLE query-vaerdier redigeres, ikke kun `token`. Et
//!    `http://bruger:HEMMELIGHED@vaert/stream` slap uredigeret igennem den gamle
//!    inline-redaktion, som kun saa efter `token=`.
//! 2. Et query-felt UDEN `=` har ingen noegle - dér ER navnet vaerdien, saa hele feltet
//!    erstattes. En tidlig udgave svarede `?<token>=***` paa netop den form.
//! 3. STIEN redigeres IKKE. Det er en smallere garanti end navnet lover, og den er bevidst:
//!    stien er det man fejlsoeger paa, og en heuristik der gaettede paa "ligner dette et
//!    token?" ville baade ramme forbi og goere loggen ubrugelig.
//!
//! Og den fejler LUKKET: kan URL'en ikke parses, returneres en fast tekst frem for inputtet -
//! ellers ville filteret vaere en tavs no-op praecis for det input der braekkede parseren
//! (maaleregel 128).

use crate::husk_urls::escape_data_string;

pub const UPARSEBAR_URL: &str = "<uparsebar URL - redigeret>";
pub const UPARSEBAR_TEKST: &str = "<undtagelse skjult - URL kunne ikke parses>";

/// En absolut URL, delt op i de dele redaktionen skal kende.
///
/// ⛔ Det er en SNAEVER parser med vilje: den kan http/https-formen og afviser resten.
/// .NET's `Uri` afviser ogsaa, hvor Pythons `urlsplit` naesten aldrig goer, og den forskel er
/// loest i den SIKRE retning - det der ikke kan parses, redigeres helt vaek.
pub(crate) struct Url {
    pub scheme: String,
    pub user_info: String,
    pub host: String,
    pub er_ipv6: bool,
    pub port: Option<u16>,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

impl Url {
    pub fn parse(url: &str) -> Option<Url> {
        let skil = url.find("://")?;
        let scheme = &url[..skil];
        if scheme.is_empty()
            || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
            || !scheme.as_bytes()[0].is_ascii_alphabetic()
        {
            return None;
        }

        let rest = &url[skil + 3..];
        let auth_slut = rest
            .find(['/', '?', '#'])
            .unwrap_or(rest.len());
        let authority = &rest[..auth_slut];
        let hale = &rest[auth_slut..];

        // Sidste '@' skiller userinfo fra vaerten: et kodeord maa gerne indeholde '@'.
        let (user_info, vaert_del) = match authority.rfind('@') {
            Some(i) => (&authority[..i], &authority[i + 1..]),
            None => ("", authority),
        };

        let (host, er_ipv6, port_tekst) = if let Some(rest_v6) = vaert_del.strip_prefix('[') {
            // IPv6 SKAL have sin lukkende klamme. Uden den afviser .NET's Uri ogsaa, og
            // det er netop det ben der beviser at filteret fejler LUKKET.
            let luk = rest_v6.find(']')?;
            let h = &rest_v6[..luk];
            let efter = &rest_v6[luk + 1..];
            let p = efter.strip_prefix(':').unwrap_or("");
            if !efter.is_empty() && !efter.starts_with(':') {
                return None;
            }
            (h.to_string(), true, p.to_string())
        } else {
            match vaert_del.rfind(':') {
                Some(i) => (vaert_del[..i].to_string(), false, vaert_del[i + 1..].to_string()),
                None => (vaert_del.to_string(), false, String::new()),
            }
        };

        if host.is_empty() {
            return None;
        }
        if !er_ipv6 && host.contains(':') {
            return None; // bar IPv6 uden klammer - .NET afviser ogsaa
        }

        let port = if port_tekst.is_empty() {
            None
        } else {
            Some(port_tekst.parse::<u16>().ok()?)
        };

        // Del halen i sti, query og fragment.
        let (uden_frag, fragment) = match hale.find('#') {
            Some(i) => (&hale[..i], &hale[i + 1..]),
            None => (hale, ""),
        };
        let (path, query) = match uden_frag.find('?') {
            Some(i) => (&uden_frag[..i], &uden_frag[i + 1..]),
            None => (uden_frag, ""),
        };

        Some(Url {
            scheme: scheme.to_ascii_lowercase(),
            user_info: user_info.to_string(),
            host,
            er_ipv6,
            port,
            // Som .NET's AbsolutePath: en tom sti er "/".
            path: if path.is_empty() { "/".to_string() } else { path.to_string() },
            query: query.to_string(),
            fragment: fragment.to_string(),
        })
    }

    /// Er porten skemaets standard (og skal dermed ikke skrives)?
    fn er_standardport(&self) -> bool {
        match self.port {
            None => true,
            Some(p) => match self.scheme.as_str() {
                "http" | "ws" => p == 80,
                "https" | "wss" => p == 443,
                _ => false,
            },
        }
    }

    fn vaert_med_port(&self) -> String {
        let mut h = if self.er_ipv6 { format!("[{}]", self.host) } else { self.host.clone() };
        if !self.er_standardport() {
            if let Some(p) = self.port {
                h.push(':');
                h.push_str(&p.to_string());
            }
        }
        h
    }
}

/// Redigér en URL til logning. Skema, vaert, port og sti bevares.
pub fn redact(url: Option<&str>) -> String {
    let Some(url) = url else { return UPARSEBAR_URL.to_string() };
    if url.is_empty() {
        return UPARSEBAR_URL.to_string();
    }
    let Some(u) = Url::parse(url) else { return UPARSEBAR_URL.to_string() };

    let mut vaert = u.vaert_med_port();
    if !u.user_info.is_empty() {
        vaert = format!("***@{vaert}");
    }

    let mut s = format!("{}://{}{}", u.scheme, vaert, u.path);

    if !u.query.is_empty() {
        let felter: Vec<&str> = u.query.split('&').filter(|f| !f.is_empty()).collect();
        s.push('?');
        if felter.is_empty() {
            s.push_str("***");
        } else {
            for (i, f) in felter.iter().enumerate() {
                if i > 0 {
                    s.push('&');
                }
                match f.find('=') {
                    Some(lig) => {
                        s.push_str(&f[..lig]);
                        s.push_str("=***");
                    }
                    None => s.push_str("***"),
                }
            }
        }
    }

    if !u.fragment.is_empty() {
        s.push_str("#***");
    }

    s
}

/// Fjern URL'ens egne hemmelige VAERDIER fra en vilkaarlig tekst - foerst og fremmest en
/// fejlbeskeds egen tekst, som kan citere hele stien inklusive `?token=`.
///
/// Baade den raa og den URL-kodede form erstattes, fordi stream-URL'en sender tokenet gennem
/// procent-kodning.
pub fn hide_secrets(tekst: Option<&str>, url: Option<&str>) -> String {
    let t = tekst.unwrap_or("");
    if t.is_empty() {
        return String::new();
    }
    let Some(url) = url.filter(|u| !u.is_empty()) else { return t.to_string() };
    let Some(u) = Url::parse(url) else { return UPARSEBAR_TEKST.to_string() };

    let mut ud = t.to_string();
    for hemmelighed in secret_parts(&u) {
        if hemmelighed.is_empty() {
            continue;
        }
        ud = ud.replace(&hemmelighed, "***");
        let kodet = escape_data_string(&hemmelighed);
        if kodet != hemmelighed {
            ud = ud.replace(&kodet, "***");
        }
    }

    ud
}

/// De dele af en URL der behandles som hemmelige - ÉT sted.
///
/// [`redact`] og [`hide_secrets`] skal vaere enige om hvad en hemmelighed ER. I Python var de
/// det ikke, fordi listen fandtes to steder, og et bart query-felt, et brugernavn og et
/// fragment slap derfor igennem i en undtagelsestekst. Én funktion frem for to lister der skal
/// holdes i sync.
fn secret_parts(u: &Url) -> Vec<String> {
    let mut ud = Vec::new();

    if !u.user_info.is_empty() {
        match u.user_info.find(':') {
            Some(k) => {
                let bruger = &u.user_info[..k];
                let kode = &u.user_info[k + 1..];
                if !bruger.is_empty() {
                    ud.push(bruger.to_string());
                }
                if !kode.is_empty() {
                    ud.push(kode.to_string());
                }
            }
            None => ud.push(u.user_info.clone()),
        }
    }

    for felt in u.query.split('&').filter(|f| !f.is_empty()) {
        let vaerdi = match felt.find('=') {
            Some(lig) => &felt[lig + 1..],
            None => felt,
        };
        if !vaerdi.is_empty() {
            ud.push(vaerdi.to_string());
        }
    }

    if !u.fragment.is_empty() {
        ud.push(u.fragment.clone());
    }

    ud
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use crate::husk_urls;

    const HEMMELIGHED: &str = "s3kr3t-token-VAERDI";

    #[test]
    fn token_i_query_redigeres() {
        let r = redact(Some(&format!("http://10.0.0.5:8080/stream?token={HEMMELIGHED}")));
        assert!(!r.contains(HEMMELIGHED));
        assert_eq!(r, "http://10.0.0.5:8080/stream?token=***");
    }

    #[test]
    fn brugerinfo_redigeres() {
        // Netop den form den gamle inline-redaktion slap igennem: hemmeligheden staar FOER
        // '?', saa en split paa '?' redigerede ingenting.
        let r = redact(Some(&format!("http://bruger:{HEMMELIGHED}@10.0.0.5:8080/stream")));
        assert!(!r.contains(HEMMELIGHED));
        assert!(!r.contains("bruger"));
        assert_eq!(r, "http://***@10.0.0.5:8080/stream");
    }

    #[test]
    fn anden_parameter_end_token_redigeres() {
        let r = redact(Some(&format!("http://10.0.0.5:8080/stream?key={HEMMELIGHED}")));
        assert_eq!(r, "http://10.0.0.5:8080/stream?key=***");
    }

    #[test]
    fn flere_parametre_beholder_alle_navne() {
        let r = redact(Some(&format!(
            "http://10.0.0.5:8080/stream?fps=10&token={HEMMELIGHED}&rot=90"
        )));
        assert_eq!(r, "http://10.0.0.5:8080/stream?fps=***&token=***&rot=***");
    }

    #[test]
    fn fragment_redigeres() {
        let r = redact(Some(&format!("http://10.0.0.5:8080/stream#{HEMMELIGHED}")));
        assert!(!r.contains(HEMMELIGHED));
        assert!(r.ends_with("#***"));
    }

    #[test]
    fn url_uden_hemmelighed_beholder_sin_information() {
        // Modvidne: filteret maa ikke goere en harmloes URL ubrugelig at fejlsoege paa.
        let u = "http://10.0.0.5:8080/stream";
        assert_eq!(redact(Some(u)), u);
    }

    #[test]
    fn ipv6_beholder_sine_klammer() {
        let r = redact(Some(&format!("http://[fd7a::1]:8080/stream?token={HEMMELIGHED}")));
        assert_eq!(r, "http://[fd7a::1]:8080/stream?token=***");
    }

    #[test]
    fn uparsebar_url_fejler_lukket() {
        // Et visningsfilter der returnerer uparsebart input UAENDRET, er en tavs no-op
        // praecis for det input der braekkede parseren.
        let r = redact(Some(&format!("http://[ugyldig-ipv6/stream?token={HEMMELIGHED}")));
        assert!(!r.contains(HEMMELIGHED));
        assert_eq!(r, UPARSEBAR_URL);
    }

    #[test]
    fn query_uden_lighedstegn_forsvinder_ikke_tavst() {
        let r = redact(Some(&format!("http://10.0.0.5:8080/stream?{HEMMELIGHED}")));
        assert!(!r.contains(HEMMELIGHED));
        assert!(r.contains('?'));
        assert_eq!(r, "http://10.0.0.5:8080/stream?***");
    }

    #[test]
    fn tom_eller_manglende_url_fejler_lukket() {
        assert_eq!(redact(None), UPARSEBAR_URL);
        assert_eq!(redact(Some("")), UPARSEBAR_URL);
        assert_eq!(redact(Some("ikke-en-url")), UPARSEBAR_URL);
    }

    #[test]
    fn standardport_skrives_ikke() {
        assert_eq!(redact(Some("http://vaert:80/x")), "http://vaert/x");
        assert_eq!(redact(Some("https://vaert:443/x")), "https://vaert/x");
        assert_eq!(redact(Some("http://vaert:8090/x")), "http://vaert:8090/x");
    }

    #[test]
    fn tom_sti_bliver_skraastreg() {
        assert_eq!(redact(Some("http://vaert")), "http://vaert/");
    }

    // ---- hide_secrets ---------------------------------------------------------------

    #[test]
    fn undtagelsestekst_renses() {
        // Den MAALTE laekagevej: en URL-fejl citerer hele stien, altsaa ogsaa ?token=.
        let url = format!("http://10.0.0.5:8080/stream?token={HEMMELIGHED}");
        let tekst = format!("URL can't contain control characters. '/stream?token={HEMMELIGHED}'");
        let r = hide_secrets(Some(&tekst), Some(&url));
        assert!(!r.contains(HEMMELIGHED));
        assert!(r.contains("control characters"));
    }

    #[test]
    fn url_kodet_form_renses_ogsaa() {
        let raa = "tegn+med specialer";
        let url = husk_urls::stream("10.0.0.5", Some(raa));
        assert!(url.contains("%2B"));
        assert!(!url.contains(raa));

        let r = hide_secrets(Some(&format!("fejl i {url}")), Some(&url));
        assert!(!r.contains("%2B"));
        assert!(!r.contains(raa));
    }

    #[test]
    fn tekst_uden_hemmelighed_er_uroert() {
        let url = format!("http://10.0.0.5:8080/stream?token={HEMMELIGHED}");
        let tekst = "<urlopen error timed out>";
        assert_eq!(hide_secrets(Some(tekst), Some(&url)), tekst);
    }

    #[test]
    fn uparsebar_url_skjuler_hele_teksten() {
        let r = hide_secrets(
            Some(&format!("fejl med {HEMMELIGHED}")),
            Some(&format!("http://[ugyldig-ipv6/x?token={HEMMELIGHED}")),
        );
        assert!(!r.contains(HEMMELIGHED));
        assert_eq!(r, UPARSEBAR_TEKST);
    }

    /// De syv former der SLAP IGENNEM i Python indtil 2026-08-23: de to funktioner var uenige
    /// om hvad en hemmelighed ER. Her udleder begge den af samme sted.
    ///
    /// ⛔ ÉT BEN PR. FORM, ikke én loekke. En loekke stopper ved den foerste fejl og skjuler
    /// hvor mange af de syv former der faktisk er brudt - og hver af dem er en selvstaendig
    /// laekagevej der er betalt for én gang.
    macro_rules! ben_de_to_er_enige {
        ($navn:ident, $form:expr) => {
            #[test]
            fn $navn() {
                let url = $form.replace("{}", HEMMELIGHED);
                assert!(!redact(Some(&url)).contains(HEMMELIGHED), "redact: {}", $form);
                assert!(
                    !hide_secrets(Some(&format!("fejl i {url}")), Some(&url)).contains(HEMMELIGHED),
                    "hide_secrets: {}",
                    $form
                );
            }
        };
    }

    ben_de_to_er_enige!(de_to_er_enige_token, "http://10.0.0.5:8080/stream?token={}");
    ben_de_to_er_enige!(de_to_er_enige_key, "http://10.0.0.5:8080/stream?key={}");
    ben_de_to_er_enige!(de_to_er_enige_flere_felter, "http://10.0.0.5:8080/stream?a=1&token={}");
    ben_de_to_er_enige!(de_to_er_enige_bart_felt, "http://10.0.0.5:8080/stream?{}");
    ben_de_to_er_enige!(de_to_er_enige_brugernavn, "http://{}@10.0.0.5:8080/stream");
    ben_de_to_er_enige!(de_to_er_enige_kodeord, "http://bruger:{}@10.0.0.5:8080/stream");
    ben_de_to_er_enige!(de_to_er_enige_fragment, "http://10.0.0.5:8080/stream#{}");

    #[test]
    fn sti_indlejret_token_er_en_kendt_graense() {
        // Modvidne, saa graensen er MAALT frem for glemt: stien redigeres IKKE.
        // Flytter graensen sig, skal baade dette ben og dokumentationen rettes.
        let url = format!("http://10.0.0.5:8080/stream/{HEMMELIGHED}");
        assert!(redact(Some(&url)).contains(HEMMELIGHED));
    }
}
