//! En falsk Husk-telefon i PROCESSEN.
//!
//! Den er en HURTIG og hermetisk telefon til de ben der skal koere i hver `cargo test`,
//! uden at suiten faar en afhaengighed uden for crate'en.
//!
//! Den kan noget en rigtig telefon ikke: vaere TAVS med vilje, svare 401, lukke midt i en
//! frame og udelade Content-Length. Det er de tilstande OFFLINE-, AUTH-FEJL-,
//! genforbindelses- og boundary-benene skal kunne fremprovokere. **Et ben der ikke kan
//! fremprovokere sin fejltilstand, maaler ingenting.**
//!
//! ⛔ Modulet er en TESTFIKSTUR og har ingen kalder i produktet. Det ligger i biblioteket frem
//! for i `tests/`, fordi baade enheds- og integrationsbenene skal kunne bruge det, og en
//! `tests/`-fil kan kun naas af de sidste. Linkeren fjerner det fra den udgivne binaer, fordi
//! intet dér naevner det.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Hvordan fiksturet skal opfoere sig. Alt kan saettes FOER den startes.
#[derive(Clone)]
pub struct Opsaetning {
    /// Saet den, og ethvert kald uden det rigtige token svarer 401.
    pub kraevet_token: Option<String>,
    /// Frames pr. sekund fiksturet leverer paa /stream.
    pub fps: u32,
    /// Send headerne, og saa INTET. Bruges til at fremprovokere OFFLINE.
    pub tavs: bool,
    /// Luk forbindelsen efter saa mange frames (0 = aldrig).
    pub luk_efter_frames: u32,
    /// Udelad Content-Length pr. del, saa boundary-vejen maales.
    pub uden_content_length: bool,
    /// Den JPEG-nyttelast hver frame baerer. Default er en lille, gyldig-formet stump.
    pub frame: Vec<u8>,
    /// Kroppen `/flags` svarer med.
    pub flags_krop: String,
    /// Statuskoden `/healthz` svarer med.
    pub healthz_status: u32,
    /// Boundary-navnet i Content-Type. Husk bruger `rigframe`.
    pub boundary: String,
    /// Hvordan `/token/request` + `/token/status` opfoerer sig. Default: ruterne findes ikke,
    /// som paa en Husk foer 1.4.
    pub token_udfald: TokenUdfald,
}

/// Brugerens svar paa telefonen, som fiksturet spiller det.
#[derive(Clone, Debug)]
pub enum TokenUdfald {
    /// 404 paa begge ruter (Husk foer 1.4).
    FindesIkke,
    /// `pending` et antal gange, derefter `approved` med tokenet. Udleveres én gang.
    Godkendt { token: String, efter_poll: u32 },
    /// `denied` ved foerste status-kald.
    Afvist,
    /// `expired` ved foerste status-kald.
    Udloebet,
    /// `pending` for evigt: ingen trykker.
    Venter,
}

/// Fiksturets egen tilstand for den ene anmodning.
#[derive(Default)]
struct TokenTilstand {
    id: Option<String>,
    poll: u32,
    udleveret: bool,
    sidste_anmodning: Option<String>,
}

impl Default for Opsaetning {
    fn default() -> Self {
        Opsaetning {
            kraevet_token: None,
            fps: 50,
            tavs: false,
            luk_efter_frames: 0,
            uden_content_length: false,
            frame: standard_frame(1),
            flags_krop: r#"{"camera": false, "screen": false, "front": false}"#.to_string(),
            healthz_status: 200,
            boundary: "rigframe".to_string(),
            token_udfald: TokenUdfald::FindesIkke,
        }
    }
}

/// En frame med JPEG'ens rigtige markoerer og et kendt indhold.
///
/// ⛔ Det er IKKE et rigtigt JPEG, og det er med vilje: parser-benene maaler RAMMEN, ikke
/// afkodningen. Et rigtigt JPEG ville goere benene afhaengige af WIC, som har sine egne ben.
pub fn standard_frame(nr: u8) -> Vec<u8> {
    let mut b = vec![0xFF, 0xD8, 0xFF];
    for i in 0..512u32 {
        b.push(((nr as u32 + i) & 0xFF) as u8);
    }
    b.extend_from_slice(&[0xFF, 0xD9]);
    b
}

pub struct TestTelefon {
    praefiks: String,
    stop: Arc<AtomicBool>,
    forbindelser: Arc<AtomicU32>,
    afsluttede: Arc<AtomicU32>,
    opsaetning: Arc<Mutex<Opsaetning>>,
    token: Arc<Mutex<TokenTilstand>>,
}

impl TestTelefon {
    pub fn start(opsaetning: Opsaetning) -> std::io::Result<TestTelefon> {
        let lytter = TcpListener::bind("127.0.0.1:0")?;
        let port = lytter.local_addr()?.port();
        let praefiks = format!("http://127.0.0.1:{port}");

        let stop = Arc::new(AtomicBool::new(false));
        let forbindelser = Arc::new(AtomicU32::new(0));
        let afsluttede = Arc::new(AtomicU32::new(0));
        let ops = Arc::new(Mutex::new(opsaetning));
        let token = Arc::new(Mutex::new(TokenTilstand::default()));

        let s = Arc::clone(&stop);
        let f = Arc::clone(&forbindelser);
        let a = Arc::clone(&afsluttede);
        let o = Arc::clone(&ops);
        let tk = Arc::clone(&token);
        // En kort accept-frist, saa loekken kan se stop-flaget uden at vente paa en klient.
        lytter.set_nonblocking(true)?;
        std::thread::Builder::new()
            .name(format!("husk-fikstur-{port}"))
            .spawn(move || loop_accept(lytter, s, f, a, o, tk))?;

        Ok(TestTelefon { praefiks, stop, forbindelser, afsluttede, opsaetning: ops, token })
    }

    /// Query-strengen i den sidste `/token/request`, saa et ben kan se hvad klienten SENDTE.
    pub fn sidste_token_anmodning(&self) -> Option<String> {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).sidste_anmodning.clone()
    }

    pub fn ny() -> std::io::Result<TestTelefon> {
        Self::start(Opsaetning::default())
    }

    pub fn praefiks(&self) -> &str {
        &self.praefiks
    }

    /// Vaerten som `HuskUrls` vil have den: `127.0.0.1:<port>`.
    pub fn vaert(&self) -> String {
        self.praefiks.trim_start_matches("http://").to_string()
    }

    pub fn stream_url(&self) -> String {
        format!("{}/stream", self.praefiks)
    }

    /// Antal /stream-forbindelser fiksturet har accepteret.
    pub fn forbindelser(&self) -> u32 {
        self.forbindelser.load(Ordering::SeqCst)
    }

    /// Antal /stream-forbindelser der er AFSLUTTET - altsaa hvor socketen er lukket.
    ///
    /// ⛔ Det er den maaling der beviser at et "stop" er ægte. Telefonens kamera er DOVENT og
    /// slippes foerst ~4 s efter sidste HTTP-forbruger, saa en kilde der satte et flag men lod
    /// socketen leve, ville holde kameraet taendt for evigt - og et flag-tjek ville melde
    /// groent hele vejen.
    pub fn afsluttede(&self) -> u32 {
        self.afsluttede.load(Ordering::SeqCst)
    }

    /// Ret opsaetningen mens fiksturet koerer.
    pub fn saet(&self, f: impl FnOnce(&mut Opsaetning)) {
        let mut o = self.opsaetning.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut o);
    }
}

impl Drop for TestTelefon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// En vaert der ACCEPTERER forbindelsen og derefter tier.
///
/// ⛔ To FORSKELLIGE veje foerer til dommen »intet svar«, og begge skal maales (maaleregel 69):
/// en AFVIST forbindelse (ingen lytter) og en vaert der svarer paa TCP men aldrig paa HTTP.
/// De giver hver sin TEKST, og et ben der pinnede den ene tekst ville gaa roedt af noget der
/// ikke er en fejl. Det der SKAL holde, er DOMMEN: en uopnaaelig telefon maa aldrig forveksles
/// med et afvist token, for de to kraever modsatte handlinger af brugeren.
pub struct TavsVaert {
    vaert: String,
    stop: Arc<AtomicBool>,
}

impl TavsVaert {
    pub fn start() -> std::io::Result<TavsVaert> {
        let lytter = TcpListener::bind("127.0.0.1:0")?;
        let vaert = format!("127.0.0.1:{}", lytter.local_addr()?.port());
        lytter.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let s = Arc::clone(&stop);
        std::thread::Builder::new().name("husk-tavs".into()).spawn(move || {
            // Forbindelserne HOLDES i live; lukkes de, ville klienten se en lukning frem for
            // tavshed, og det er en anden tilstand.
            let mut aabne = Vec::new();
            while !s.load(Ordering::SeqCst) {
                match lytter.accept() {
                    Ok((k, _)) => aabne.push(k),
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        })?;
        Ok(TavsVaert { vaert, stop })
    }

    pub fn vaert(&self) -> &str {
        &self.vaert
    }
}

impl Drop for TavsVaert {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// En port INGEN lytter paa. Forbindelsen afvises med det samme (ECONNREFUSED).
pub fn ledig_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let p = l.local_addr().expect("local_addr").port();
    drop(l);
    p
}

fn loop_accept(
    lytter: TcpListener,
    stop: Arc<AtomicBool>,
    forbindelser: Arc<AtomicU32>,
    afsluttede: Arc<AtomicU32>,
    opsaetning: Arc<Mutex<Opsaetning>>,
    token: Arc<Mutex<TokenTilstand>>,
) {
    while !stop.load(Ordering::SeqCst) {
        match lytter.accept() {
            Ok((s, _)) => {
                let stop2 = Arc::clone(&stop);
                let f2 = Arc::clone(&forbindelser);
                let a2 = Arc::clone(&afsluttede);
                let o2 = Arc::clone(&opsaetning);
                let t2 = Arc::clone(&token);
                let _ = std::thread::Builder::new()
                    .name("husk-fikstur-klient".into())
                    .spawn(move || {
                        let _ = betjen(s, stop2, f2, a2, o2, t2);
                    });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
}

fn betjen(
    mut s: TcpStream,
    stop: Arc<AtomicBool>,
    forbindelser: Arc<AtomicU32>,
    afsluttede: Arc<AtomicU32>,
    opsaetning: Arc<Mutex<Opsaetning>>,
    token: Arc<Mutex<TokenTilstand>>,
) -> std::io::Result<()> {
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;

    let mut raa = Vec::new();
    let mut buf = [0u8; 1024];
    // Læs indtil header-blokken er hel. En HTTP-anmodning uden krop slutter ved CRLFCRLF.
    loop {
        let n = s.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        raa.extend_from_slice(&buf[..n]);
        if raa.windows(4).any(|w| w == b"\r\n\r\n") || raa.len() > 16 * 1024 {
            break;
        }
    }

    let tekst = String::from_utf8_lossy(&raa).into_owned();
    let foerste = tekst.lines().next().unwrap_or("");
    let mut dele = foerste.split_whitespace();
    let _verbum = dele.next().unwrap_or("");
    let maal = dele.next().unwrap_or("/");
    let (sti, query) = match maal.find('?') {
        Some(i) => (&maal[..i], &maal[i + 1..]),
        None => (maal, ""),
    };

    let ops = opsaetning.lock().unwrap_or_else(|e| e.into_inner()).clone();

    // Token-kontrollen gaelder ALT undtagen /healthz og de to token-ruter, praecis som Husk selv.
    if sti == "/token/request" || sti == "/token/status" {
        return token_rute(&mut s, sti, query, &ops.token_udfald, &token);
    }
    if sti != "/healthz" {
        if let Some(kraevet) = &ops.kraevet_token {
            let givet = query
                .split('&')
                .find_map(|f| f.strip_prefix("token="))
                .unwrap_or("");
            if givet != kraevet {
                return svar_tekst(&mut s, 401, "text/plain", b"nej");
            }
        }
    }

    match sti {
        "/healthz" => {
            if ops.healthz_status == 200 {
                svar_tekst(&mut s, 200, "text/plain", b"ok")
            } else {
                svar_tekst(&mut s, ops.healthz_status, "text/plain", b"nej")
            }
        }
        "/flags" => svar_tekst(&mut s, 200, "application/json", ops.flags_krop.as_bytes()),
        "/sensor" => svar_tekst(&mut s, 200, "application/json", br#"{"values": [0.0, 9.8, 0.1]}"#),
        "/set" => svar_tekst(&mut s, 200, "text/plain", b"ok"),
        "/snapshot" => svar_tekst(&mut s, 200, "image/jpeg", &ops.frame),
        "/stream" => {
            forbindelser.fetch_add(1, Ordering::SeqCst);
            let r = stroem(&mut s, &ops, &stop);
            afsluttede.fetch_add(1, Ordering::SeqCst);
            r
        }
        _ => svar_tekst(&mut s, 404, "text/plain", b"findes ikke"),
    }
}

/// `/token/request` og `/token/status` som Husk 1.4 svarer dem, styret af [`TokenUdfald`].
fn token_rute(
    s: &mut TcpStream,
    sti: &str,
    query: &str,
    udfald: &TokenUdfald,
    tilstand: &Mutex<TokenTilstand>,
) -> std::io::Result<()> {
    if matches!(udfald, TokenUdfald::FindesIkke) {
        return svar_tekst(s, 404, "text/plain", b"not found");
    }
    let mut t = tilstand.lock().unwrap_or_else(|e| e.into_inner());
    if sti == "/token/request" {
        t.sidste_anmodning = Some(query.to_string());
        let id = "0123456789abcdef0123456789abcdef".to_string();
        t.id = Some(id.clone());
        t.poll = 0;
        t.udleveret = false;
        let krop = format!(r#"{{"id":"{id}","expires_in":120}}"#);
        return svar_tekst(s, 200, "application/json", krop.as_bytes());
    }
    let givet = query.split('&').find_map(|f| f.strip_prefix("id=")).unwrap_or("");
    if t.id.as_deref() != Some(givet) || t.udleveret {
        return svar_tekst(s, 200, "application/json", br#"{"status":"expired"}"#);
    }
    t.poll += 1;
    let krop = match udfald {
        TokenUdfald::Godkendt { token, efter_poll } if t.poll > *efter_poll => {
            t.udleveret = true;
            format!(r#"{{"status":"approved","token":"{token}"}}"#)
        }
        TokenUdfald::Afvist => r#"{"status":"denied"}"#.to_string(),
        TokenUdfald::Udloebet => r#"{"status":"expired"}"#.to_string(),
        _ => r#"{"status":"pending"}"#.to_string(),
    };
    svar_tekst(s, 200, "application/json", krop.as_bytes())
}

fn svar_tekst(s: &mut TcpStream, status: u32, ct: &str, krop: &[u8]) -> std::io::Result<()> {
    let hoved = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        if status == 200 { "OK" } else { "NEJ" },
        krop.len()
    );
    s.write_all(hoved.as_bytes())?;
    s.write_all(krop)?;
    s.flush()
}

fn stroem(s: &mut TcpStream, ops: &Opsaetning, stop: &AtomicBool) -> std::io::Result<()> {
    let hoved = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary={}\r\n\
         Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
        ops.boundary
    );
    s.write_all(hoved.as_bytes())?;
    s.flush()?;

    if ops.tavs {
        // Headerne er sendt; nu tier vi. Det er OFFLINE-benets stimulus: en server der SVARER
        // men ikke leverer, er en anden fejl end en der slet ikke svarer.
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(20));
        }
        return Ok(());
    }

    let pause = Duration::from_micros(1_000_000 / ops.fps.max(1) as u64);
    let mut n: u32 = 0;
    while !stop.load(Ordering::SeqCst) {
        let mut del = format!("--{}\r\nContent-Type: image/jpeg\r\n", ops.boundary);
        if !ops.uden_content_length {
            del.push_str(&format!("Content-Length: {}\r\n", ops.frame.len()));
        }
        del.push_str("\r\n");

        s.write_all(del.as_bytes())?;
        s.write_all(&ops.frame)?;
        s.write_all(b"\r\n")?;
        s.flush()?;

        n += 1;
        if ops.luk_efter_frames > 0 && n >= ops.luk_efter_frames {
            return Ok(());
        }
        std::thread::sleep(pause);
    }

    Ok(())
}
