//! Beskytter en hemmelighed med DPAPI for den AKTUELLE BRUGER. Port af `Secret.cs`.
//!
//! Hvad det er, og hvad det IKKE er - begge dele skal staa, fordi det andet er det der bliver
//! lovet for meget:
//!
//! ✅ Tokenet ligger ikke i klartekst i config-filen. En anden bruger paa den samme PC kan ikke
//!    laese det, og en kopi af filen kan ikke bruges paa en anden maskine.
//! ⛔ Det beskytter IKKE HTTP-transporten. Husk taler almindelig HTTP, saa tokenet gaar
//!    ubeskyttet over netvaerket. Det er derfor kalderen SKAL dokumentere om forbindelsen er
//!    LAN eller en krypteret tunnel (Tailscale), og ALDRIG kalde et indtastet token en
//!    "sikker engangsparring".
//! ⛔ Det beskytter ikke mod noget der koerer SOM brugeren. DPAPI's noegle er brugerens egen.
//!
//! Hemmeligheden naar aldrig argv eller en log: den gaar fra UI'et til dette modul og videre i
//! en HTTP-query der redigeres af [`crate::url_redactor`] foer den skrives nogen steder.

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData};

/// Ekstra entropi, saa en beskyttet blob fra ET program ikke uden videre kan afbeskyttes af et
/// andet der koerer som samme bruger.
///
/// ⛔ Det er ikke en noegle og maa ikke forveksles med en: det haever baren, det flytter den
/// ikke. Vaerdien er ordret den samme som C#-udgavens, saa en config skrevet af den ene udgave
/// kan laeses af den anden - det er hele forudsaetningen for at kunne skifte uden at brugeren
/// mister sit token.
const ENTROPI: &[u8] = b"husk-webcam/v1";

/// Nulstil en buffer saa optimizeren ikke kan fjerne skrivningen.
///
/// ⛔ EN ALMINDELIG LOEKKE ER IKKE NOK. Her stod `raa.iter_mut().for_each(|b| *b = 0)` paa en
/// `Vec` der droppes umiddelbart efter, og det er per definition en dead store: med
/// `opt-level = "z"` og LTO MAA LLVM fjerne den. Loeftet »den maa ikke ligge og flyde i heapen«
/// var derfor ubevist - og et loefte om en hemmelighed der kun holder naar optimizeren ikke
/// gider, er ingen beskyttelse.
///
/// `write_volatile` er netop den skrivning compileren ikke maa fjerne, og hegnet holder den
/// foran det efterfoelgende drop.
fn nulstil(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Nulstil DPAPI's egen udbuffer FOER den frigives.
///
/// `LocalFree` giver hukommelsen tilbage til heapen med indholdet i behold; den naeste
/// allokering i processen kan faa klarteksten med.
unsafe fn nulstil_og_frigiv(ud: &windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) {
    if !ud.pbData.is_null() {
        let s = unsafe { std::slice::from_raw_parts_mut(ud.pbData, ud.cbData as usize) };
        nulstil(s);
        unsafe { let _ = LocalFree(Some(HLOCAL(ud.pbData as *mut _))); }
    }
}

/// Beskyt en streng. Returnerer base64 der kan staa i en JSON-fil.
pub fn beskyt(klartekst: &str) -> Option<String> {
    let mut raa = klartekst.as_bytes().to_vec();
    let mut entropi = ENTROPI.to_vec();

    let ind = CRYPT_INTEGER_BLOB { cbData: raa.len() as u32, pbData: raa.as_mut_ptr() };
    let ent = CRYPT_INTEGER_BLOB { cbData: entropi.len() as u32, pbData: entropi.as_mut_ptr() };
    let mut ud = CRYPT_INTEGER_BLOB::default();

    let ok = unsafe {
        CryptProtectData(&ind, None, Some(&ent), None, None, 0, &mut ud)
    };

    // Ryd klarteksten, uanset udfald. Den maa ikke ligge og flyde i heapen.
    nulstil(&mut raa);

    ok.ok()?;
    // Udbufferen er den KRYPTEREDE blob, saa den er ikke selv en hemmelighed - men den
    // frigives med samme rutine, saa der kun er een vej ud af dette modul.
    let blob = unsafe { std::slice::from_raw_parts(ud.pbData, ud.cbData as usize) }.to_vec();
    unsafe { nulstil_og_frigiv(&ud) };
    Some(base64_kod(&blob))
}

/// Afbeskyt.
///
/// Returnerer `None` naar blobben ikke kan laeses - fx fordi den blev lavet af en ANDEN
/// Windows-bruger eller paa en anden maskine. Det maa ikke kaste: en config der er kopieret fra
/// en anden PC skal give en laesbar fejl i UI'et, ikke et nedbrud.
pub fn afbeskyt(base64: Option<&str>) -> Option<String> {
    let s = base64?;
    if s.is_empty() {
        return None;
    }
    let mut blob = base64_afkod(s)?;
    let mut entropi = ENTROPI.to_vec();

    let ind = CRYPT_INTEGER_BLOB { cbData: blob.len() as u32, pbData: blob.as_mut_ptr() };
    let ent = CRYPT_INTEGER_BLOB { cbData: entropi.len() as u32, pbData: entropi.as_mut_ptr() };
    let mut ud = CRYPT_INTEGER_BLOB::default();

    unsafe { CryptUnprotectData(&ind, None, Some(&ent), None, None, 0, &mut ud) }.ok()?;

    // ⛔ HER LIGGER KLARTEKSTEN. Udbufferen ER tokenet, og den blev indtil 2026-09-20
    // LocalFree'et uden at blive rydet - altsaa den ene af de to steder hvor hemmeligheden
    // beviseligt blev efterladt i heapen.
    let mut raa = unsafe { std::slice::from_raw_parts(ud.pbData, ud.cbData as usize) }.to_vec();
    unsafe { nulstil_og_frigiv(&ud) };

    // `from_utf8` paa en LAANT skive frem for paa en klon: en klon der fejler validering,
    // ville blive droppet unulstillet.
    let tekst = std::str::from_utf8(&raa).ok().map(|s| s.to_string());
    nulstil(&mut raa);
    // Og blobben vi selv afkodede fra base64.
    nulstil(&mut blob);
    // GRAENSE: den returnerede String ligger stadig i heapen indtil kalderen dropper den.
    // Det kan kun lukkes med en egen hemmeligheds-type der nulstiller i Drop, og det er en
    // stoerre aendring end denne. Skrevet ned frem for lovet vaek.
    tekst
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 med udfyldning, som .NET's `Convert.ToBase64String`.
pub fn base64_kod(data: &[u8]) -> String {
    let mut ud = String::with_capacity(data.len().div_ceil(3) * 4);
    for blok in data.chunks(3) {
        let b0 = blok[0] as u32;
        let b1 = *blok.get(1).unwrap_or(&0) as u32;
        let b2 = *blok.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        ud.push(B64[(n >> 18) as usize & 63] as char);
        ud.push(B64[(n >> 12) as usize & 63] as char);
        ud.push(if blok.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        ud.push(if blok.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    ud
}

/// Standard base64 med udfyldning. Returnerer `None` paa alt der ikke er gyldig base64.
///
/// ⛔ Afkoderen er INJEKTIV: den accepterer kun den kanoniske form. En tolerant afkoder der
/// springer ukendte tegn over, ville lade to forskellige strenge give samme bytes, og saa kan
/// en sammenligning ikke afvise det den lover (maaleregel 17c).
pub fn base64_afkod(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() % 4 != 0 {
        return None;
    }

    let vaerdi = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };

    let mut ud = Vec::with_capacity(b.len() / 4 * 3);
    for (i, blok) in b.chunks(4).enumerate() {
        let sidste = i == b.len() / 4 - 1;
        let pad = if sidste {
            blok.iter().filter(|&&c| c == b'=').count()
        } else {
            0
        };
        if pad > 2 || blok[..4 - pad].iter().any(|&c| c == b'=') {
            return None;
        }
        let mut n = 0u32;
        for j in 0..4 {
            n = (n << 6) | if j < 4 - pad { vaerdi(blok[j])? } else { 0 };
        }
        ud.push((n >> 16) as u8);
        if pad < 2 {
            ud.push((n >> 8) as u8);
        }
        if pad < 1 {
            ud.push(n as u8);
        }
    }

    Some(ud)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn base64_rundtur() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0u8, 255, 128, 1][..]] {
            let k = base64_kod(data);
            if data.is_empty() {
                assert_eq!(k, "");
                continue;
            }
            assert_eq!(base64_afkod(&k).unwrap(), data.to_vec());
        }
    }

    #[test]
    fn base64_kender_de_kendte_facitter() {
        assert_eq!(base64_kod(b"f"), "Zg==");
        assert_eq!(base64_kod(b"fo"), "Zm8=");
        assert_eq!(base64_kod(b"foo"), "Zm9v");
        assert_eq!(base64_kod(b"foob"), "Zm9vYg==");
    }

    #[test]
    fn base64_afkoder_er_injektiv() {
        // Ikke-base64-tegn, forkert laengde og udfyldning midt i skal ALLE afvises.
        assert!(base64_afkod("Zm9v!").is_none());
        assert!(base64_afkod("Zm9").is_none());
        assert!(base64_afkod("Z=9v").is_none());
        assert!(base64_afkod("====").is_none());
        assert!(base64_afkod("").is_none());
    }

    #[test]
    fn dpapi_rundtur_giver_samme_klartekst() {
        let hemmelig = "s3kr3t-token-\u{00e6}\u{00f8}\u{00e5}";
        let b = beskyt(hemmelig).expect("CryptProtectData skal virke som den aktuelle bruger");
        assert!(!b.contains(hemmelig), "blobben maa ikke baere klarteksten");
        assert_eq!(afbeskyt(Some(&b)).as_deref(), Some(hemmelig));
    }

    #[test]
    fn afbeskyt_fejler_blooedt_paa_skrald() {
        // En config kopieret fra en anden PC skal give en laesbar fejl, ikke et nedbrud.
        assert!(afbeskyt(None).is_none());
        assert!(afbeskyt(Some("")).is_none());
        assert!(afbeskyt(Some("ikke-base64!")).is_none());
        assert!(afbeskyt(Some("AAAAAAAAAAAAAAAA")).is_none());
    }

    #[test]
    fn beskyt_af_tom_streng_kan_laeses_tilbage() {
        let b = beskyt("").expect("tom klartekst er gyldig");
        assert_eq!(afbeskyt(Some(&b)).as_deref(), Some(""));
    }
}
