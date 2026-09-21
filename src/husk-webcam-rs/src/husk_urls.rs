//! Husks endpoints, bygget ét sted. Port af `HuskUrls.cs`.
//!
//! Tokenet laegges ALTID i `?token=`, aldrig i stien: [`crate::url_redactor`] redigerer
//! query-vaerdier, men ikke stien, saa et token i stien ville naa loggen.

/// Husks kontrol-server. Ikke 8127/5037/27183/8022 - de er rig-portene.
pub const DEFAULT_PORT: u16 = 8090;

/// Vaert med port. En vaert der selv baerer ":port" bruges uaendret.
pub fn host(vaert: &str) -> String {
    if vaert.contains(':') {
        vaert.to_string()
    } else {
        format!("{vaert}:{DEFAULT_PORT}")
    }
}

pub fn stream(vaert: &str, token: Option<&str>) -> String {
    byg(vaert, "/stream", token, None)
}

pub fn snapshot(vaert: &str, token: Option<&str>) -> String {
    byg(vaert, "/snapshot", token, None)
}

pub fn healthz(vaert: &str) -> String {
    byg(vaert, "/healthz", None, None)
}

pub fn flags(vaert: &str, token: Option<&str>) -> String {
    byg(vaert, "/flags", token, None)
}

/// Tyngdesensoren, som orienteringen laeses af.
pub fn gravity(vaert: &str, token: Option<&str>) -> String {
    byg(vaert, "/sensor", token, Some("type=gravity"))
}

/// Vaelg kameraside.
///
/// Findes FRA HUSK 1.1: en aeldre telefon svarer 200 og ignorerer parameteren tavst, saa
/// kalderen skal bekraefte valget i `/flags.front` frem for at tro paa svaret (maaleregel 3).
pub fn set_front(vaert: &str, front: bool, token: Option<&str>) -> String {
    byg(vaert, "/set", token, Some(if front { "front=1" } else { "front=0" }))
}

fn byg(vaert: &str, sti: &str, token: Option<&str>, query: Option<&str>) -> String {
    let u = format!("http://{}{}", host(vaert), sti);
    let mut dele: Vec<String> = Vec::with_capacity(2);
    if let Some(q) = query {
        if !q.is_empty() {
            dele.push(q.to_string());
        }
    }
    if let Some(t) = token {
        if !t.is_empty() {
            dele.push(format!("token={}", escape_data_string(t)));
        }
    }
    if dele.is_empty() {
        u
    } else {
        format!("{u}?{}", dele.join("&"))
    }
}

/// Som .NET's `Uri.EscapeDataString`: alt undtagen RFC 3986's *unreserved* saet
/// (`A-Z a-z 0-9 - _ . ~`) procent-kodes, byte for byte i UTF-8.
///
/// ⛔ Ikke "alt undtagen alfanumerisk": `+` SKAL kodes (ellers laeses det som et mellemrum i
/// en form-kodet query), og `~` maa IKKE kodes. Den slags smaa forskelle er dem der faar et
/// token til at ankomme forkert uden at noget fejler.
pub fn escape_data_string(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut ud = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                ud.push(*b as char)
            }
            _ => {
                ud.push('%');
                ud.push(HEX[(b >> 4) as usize] as char);
                ud.push(HEX[(b & 0x0F) as usize] as char);
            }
        }
    }
    ud
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn host_tilfoejer_standardporten() {
        assert_eq!(host("10.0.0.5"), "10.0.0.5:8090");
    }

    #[test]
    fn host_med_egen_port_bruges_uaendret() {
        assert_eq!(host("10.0.0.5:9000"), "10.0.0.5:9000");
    }

    #[test]
    fn stream_uden_token_har_ingen_query() {
        assert_eq!(stream("10.0.0.5", None), "http://10.0.0.5:8090/stream");
    }

    #[test]
    fn stream_med_token_laegger_det_i_query_ikke_i_stien() {
        let u = stream("10.0.0.5", Some("abc"));
        assert_eq!(u, "http://10.0.0.5:8090/stream?token=abc");
        assert!(!u.contains("/abc"), "tokenet maa ALDRIG ende i stien");
    }

    #[test]
    fn tomt_token_giver_ingen_query() {
        assert_eq!(stream("10.0.0.5", Some("")), "http://10.0.0.5:8090/stream");
    }

    #[test]
    fn gravity_har_baade_type_og_token() {
        assert_eq!(
            gravity("h", Some("t")),
            "http://h:8090/sensor?type=gravity&token=t"
        );
    }

    #[test]
    fn set_front_skriver_begge_retninger() {
        assert!(set_front("h", true, None).ends_with("/set?front=1"));
        assert!(set_front("h", false, None).ends_with("/set?front=0"));
    }

    #[test]
    fn healthz_baerer_aldrig_et_token() {
        // /healthz kraever ALDRIG et token; en 401 paa /flags bagefter er saa entydigt et
        // ADGANGS-problem frem for et netvaerks-problem.
        assert_eq!(healthz("h"), "http://h:8090/healthz");
    }

    #[test]
    fn escape_data_string_koder_plus_og_mellemrum_men_ikke_tilde() {
        assert_eq!(escape_data_string("tegn+med specialer"), "tegn%2Bmed%20specialer");
        assert_eq!(escape_data_string("a~b-c_d.e"), "a~b-c_d.e");
    }

    #[test]
    fn escape_data_string_koder_utf8_byte_for_byte() {
        // ae = U+00E6 = C3 A6 i UTF-8.
        assert_eq!(escape_data_string("\u{00e6}"), "%C3%A6");
    }

    #[test]
    fn token_med_specialtegn_url_kodes_i_stream() {
        let u = stream("10.0.0.5", Some("tegn+med specialer"));
        assert!(u.contains("%2B"), "faktisk: {u}");
        assert!(!u.contains("tegn+med"));
    }
}
