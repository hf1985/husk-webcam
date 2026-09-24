//! Appens ene vindue og dens bakke-ikon. Port af `HovedVindue.cs`, `Bakke.cs` og `Gui.cs`.
//!
//! ⛔ REN WIN32, ikke WPF. C#-udgavens vindue var skrevet i kode frem for XAML, netop fordi der
//! ingen designer er i denne kaede; her falder ogsaa det sidste lag vaek. Kontrollerne er
//! Windows' egne (`BUTTON`, `EDIT`, `LISTBOX`, `COMBOBOX`, `STATIC`), og de ligger allerede i
//! `comctl32.dll`.
//!
//! ⛔ Vinduet LUKKES ikke af brugeren - det skjules. Uden det ville et tryk paa krydset afslutte
//! processen, og autostarten ville se ud til at vaere gaaet i stykker ved naeste kamerabrug.
//! Kun "Afslut" afslutter.
//!
//! ⛔ Guiden LOVER ikke QR-parring, og den kalder ikke et indtastet token en »sikker
//! engangsparring«. Tokenet beskyttes med DPAPI paa DISKEN, men Husk taler almindelig HTTP, saa
//! det gaar ubeskyttet over netvaerket. Teksten siger det.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, FONT_CHARSET,
    FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, FillRect, GdiFlush, GetDC, HBRUSH,
    HDC, HFONT, HGDIOBJ, InvalidateRect, PAINTSTRUCT, ReleaseDC, SRCCOPY, ScreenToClient,
    SelectObject, SetBkColor, SetTextColor, SetViewportOrgEx, StretchDIBits, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, SetScrollInfo};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::bitmap::Bitmap32;
use crate::compositor::{Compositor, FrameKilde, Layout};
use crate::config::{Config, TelefonConfig};
use crate::demand::{DemandMonitor, SingletonLaas, unity_demand};
use crate::forbindelsestjek::{Forbindelsesdom, Forbindelsestjek};
use crate::husk_urls;
use crate::json;
use crate::mjpeg_source::MjpegSource;
use crate::orientation::OrientationDamper;
use crate::source_status::SourceStatus;
use crate::token_hentning::{self, Hentningsdom, TokenHenter};
use crate::unity_sender::{INGEN_MODTAGER_FORKLARING, SendResultat, UnityCaptureSender};
use crate::url_redactor;
use crate::wic;
use crate::winhttp::{BRUGERAGENT, Session};

// ---- kontrol-id'er ------------------------------------------------------------------

const ID_LISTE: i32 = 1001;
const ID_TILFOEJ: i32 = 1002;
const ID_FJERN: i32 = 1003;
const ID_NAVN: i32 = 1004;
const ID_VAERT: i32 = 1005;
const ID_TOKEN: i32 = 1006;
const ID_ROTATION: i32 = 1007;
const ID_TYNGDE: i32 = 1008;
const ID_FRONT: i32 = 1009;
const ID_TEST: i32 = 1010;
const ID_GEM: i32 = 1011;
const ID_STATUS: i32 = 1012;
const ID_LAYOUT: i32 = 1013;
const ID_STARTSTOP: i32 = 1014;
const ID_KAMERASTATUS: i32 = 1015;
const ID_HENT_TOKEN: i32 = 1016;

// Bakke-menuens id'er ligger for sig, saa de aldrig kan forveksles med en kontrol.
const ID_BAKKE_TEST: i32 = 2001;
const ID_BAKKE_VAELG: i32 = 2002;
const ID_BAKKE_PAUSE: i32 = 2003;
const ID_BAKKE_AFSLUT: i32 = 2004;

const TIMER_TICK: usize = 1;
const TIMER_DEMAND: usize = 2;

const WM_BAKKE: u32 = WM_APP + 1;
const WM_BAGGRUNDSSVAR: u32 = WM_APP + 2;

const BAGGRUND: COLORREF = COLORREF(0x1E1A18); // BGR for (24, 26, 30)

/// Et vindueshandle der maa krydse en traadgraense.
///
/// ⛔ `HWND` er en raa peger og dermed ikke `Send` - en rigtig advarsel for de fleste
/// Windows-handles. Et VINDUESHANDLE er dog en proces-global noegle, og `PostMessageW` er
/// dokumenteret traad-sikker; det er netop den vej en baggrundstraad SKAL bruge for at naa
/// GUI'en. Wrapperen siger det ét sted frem for at sprede undtagelsen ud i hver lukning.
#[derive(Clone, Copy)]
struct TraadHwnd(isize);

impl TraadHwnd {
    fn ny(h: HWND) -> Self {
        TraadHwnd(h.0 as isize)
    }

    fn hwnd(self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }
}

/// Et svar fra en baggrundstraad. GUI'en maa ALDRIG selv vente paa netvaerket.
enum Besked {
    Status(String),
    /// Testresultat: teksten, og om telefonen kan styre kameraside.
    Test(String, Option<bool>),
    /// En kildes tyngdemaaling, eller `None` hvis den ikke kunne tages.
    Tyngde(usize, Option<[f64; 3]>),
    /// »Hent fra telefonen« er faerdig: tokenet hvis brugeren godkendte, og teksten til brugeren.
    Token(Option<String>, String),
}

/// Broen mellem en levende [`MjpegSource`] og kompositoren.
struct Levende<'a> {
    kilde: &'a MjpegSource,
    rotation: i32,
    billede: Option<Bitmap32>,
}

impl FrameKilde for Levende<'_> {
    fn navn(&self) -> &str {
        self.kilde.navn()
    }
    fn status(&self) -> SourceStatus {
        self.kilde.status()
    }
    fn rotation(&self) -> i32 {
        self.rotation
    }
    fn billede(&self) -> Option<Bitmap32> {
        self.billede.clone()
    }
}

struct App {
    hwnd: HWND,
    font: HFONT,
    baggrund: HBRUSH,
    config: Config,

    kilder: Vec<MjpegSource>,
    daempere: Vec<OrientationDamper>,
    /// Den [`MjpegSource::forbindelses_nr`] vi SIDST har sat kameraside for, pr. kilde.
    ///
    /// Telefonen husker ikke valget hen over en genstart, saa siden skal saettes paa ny for
    /// hver NY forbindelse - ikke én gang ved Start.
    front_sat_for_gen: Vec<u64>,

    sender: Option<UnityCaptureSender>,
    laas: Option<SingletonLaas>,
    demand: DemandMonitor,
    koerer: bool,
    afsluttet: bool,

    /// Den senest komponerede frame. Vinduet tegner den; den sendes ogsaa til filteret.
    seneste: Option<Bitmap32>,
    /// Afkodnings-cache, saa en frame afkodes HOEJST én gang.
    afkodet: Vec<(u64, Option<Bitmap32>)>,

    sidste_tyngde: Instant,
    til_gui: Sender<Besked>,
    fra_baggrund: Receiver<Besked>,

    /// Venstre spaltes kontroller, og hvor hoejt indholdet er. Rulningen flytter netop disse.
    venstre: Vec<HWND>,
    indhold_h: i32,
    rul: i32,
}

/// Starter appen med bakke-ikon.
///
/// `skjult` er sandt naar appen er startet af autostarten ved login. Da skal vinduet IKKE vises
/// - kun bakke-ikonet. Et vindue der popper op ved hvert login er ikke et produkt.
pub fn start(skjult: bool) -> i32 {
    // Konsolvinduet frigives FOER der tegnes noget, saa brugeren aldrig ser det blinke.
    //
    // ⛔ HVORFOR DET ER NOEDVENDIGT: binaeren er en KONSOL-exe med vilje, saa
    // maalings-tilstandene kan SKRIVE deres resultat - en GUI-exe har ingen stdout, og en
    // maaling man ikke kan laese er ingen maaling. Prisen er at Windows giver processen et
    // konsolvindue. Uden FreeConsole ville autostarten aabne et sort vindue ved HVERT login -
    // praecis det bakken findes for at undgaa. Fejler kaldet (ingen konsol tilknyttet, fx
    // startet fra Stifinder), er tilstanden allerede den oenskede.
    unsafe {
        let _ = windows::Win32::System::Console::FreeConsole();
    }

    // GUI-traaden er STA: skallen (bakke-ikon, menuer) forventer det.
    wic::sikr_com(true);

    let (config, fejl) = Config::laes();

    unsafe {
        let hwnd = opret(config, fejl, true, CW_USEDEFAULT, CW_USEDEFAULT);

        SetTimer(Some(hwnd), TIMER_TICK, 100, None);
        SetTimer(Some(hwnd), TIMER_DEMAND, 500, None);

        if !skjult {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
        }

        let mut besked = MSG::default();
        while GetMessageW(&mut besked, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&besked);
            DispatchMessageW(&besked);
        }

        fjern_bakke(hwnd);
        besked.wParam.0 as i32
    }
}

/// Byg vinduet og dets kontroller. Delt af [`start`] og [`roegtjek`].
///
/// ⛔ Roegtjekket SKAL gaa gennem samme funktion. Et tjek der byggede sit eget vindue ville
/// maale sin egen kopi af koden, ikke den brugeren faar (maaleregel 135).
unsafe fn opret(config: Config, fejl: Option<String>, med_bakke: bool, x: i32, y: i32) -> HWND {
    unsafe {
        let instans = GetModuleHandleW(None).expect("GetModuleHandleW");
        let klasse = WNDCLASSW {
            lpfnWndProc: Some(vindue_proc),
            hInstance: instans.into(),
            lpszClassName: w!("HuskWebcamVindue"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            ..Default::default()
        };
        RegisterClassW(&klasse);

        let (til_gui, fra_baggrund) = channel();
        let app = Box::new(App {
            hwnd: HWND(std::ptr::null_mut()),
            font: {
                // Navnet skal LEVE til efter kaldet: CreateFontW kopierer det ikke ud af en
                // midlertidig. En `bred(..).as_ptr()` inline ville frigive bufferen foerst.
                let navn = bred("Segoe UI");
                CreateFontW(
                    -14,
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    FONT_CHARSET(0),
                    FONT_OUTPUT_PRECISION(0),
                    FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(0),
                    0,
                    PCWSTR(navn.as_ptr()),
                )
            },
            baggrund: CreateSolidBrush(BAGGRUND),
            config,
            kilder: Vec::new(),
            daempere: Vec::new(),
            front_sat_for_gen: Vec::new(),
            sender: None,
            laas: None,
            demand: DemandMonitor::default(),
            koerer: false,
            afsluttet: false,
            seneste: None,
            afkodet: Vec::new(),
            sidste_tyngde: Instant::now() - Duration::from_secs(10),
            til_gui,
            fra_baggrund,
            venstre: Vec::new(),
            indhold_h: 0,
            rul: 0,
        });
        let raa = Box::into_raw(app);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("HuskWebcamVindue"),
            w!("Husk Webcam"),
            // WS_VSCROLL: venstre spalte kan vaere hoejere end skaermen, og en kontrol man
            // ikke kan RULLE hen til, er usynlig.
            WS_OVERLAPPEDWINDOW | WS_VSCROLL,
            x,
            y,
            1000,
            // Starthoejden er kun et gaet; den rigtige udledes af indholdet i tilpas_hoejde()
            // lige efter kontrollerne er bygget.
            700,
            None,
            None,
            Some(instans.into()),
            Some(raa as *mut core::ffi::c_void),
        )
        .expect("CreateWindowExW");

        (*raa).hwnd = hwnd;
        byg_kontroller(&mut *raa, instans.into());
        tilpas_hoejde(&mut *raa);
        opdater_rullepanel(&mut *raa);
        indlaes_liste(&mut *raa);
        if med_bakke {
            tilfoej_bakke(hwnd, instans.into());
        }

        if let Some(f) = fejl {
            saet_tekst(
                hwnd,
                ID_STATUS,
                &format!(
                    "⚠️ {f}\nOpsætningen er IKKE slettet - den kunne bare ikke læses. \
                     Ret filen, eller opret telefonen igen."
                ),
            );
        } else if (*raa).config.telefoner.is_empty() {
            vis_guide(hwnd);
        }

        hwnd
    }
}

/// Roeg-tjek af GUI'en: byg vinduet, TEGN det, og gem billedet.
///
/// ⛔ Et groent build beviser ikke at vinduet kan tegnes. Maalt paa C#-siden 2026-09-19 gik
/// netop det galt: `InvariantGlobalization` braekkede WPF's culture-opslag, og appen doede ved
/// foerste binding - mens buildet var groent hele vejen. Uden et ben der faktisk RENDERER,
/// ville suiten have sendt en app ud der ikke kan starte (maaleregel 37).
///
/// Tjekket bruger sin EGEN config-mappe, saa det aldrig roerer brugerens opsaetning, og det
/// laegger INTET bakke-ikon: et ikon der blinkede i proceslinjen ved hver maaling ville vaere
/// en bivirkning af maalingen selv.
pub fn roegtjek(ud: &std::path::Path) -> i32 {
    wic::sikr_com(true);

    let midlertidig = std::env::temp_dir().join(format!(
        "husk-roegtjek-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::env::set_var("HUSKWEBCAM_CONFIGDIR", &midlertidig);
    let (config, fejl) = Config::laes();

    let kode = unsafe {
        // Uden for skaermen: tjekket maa ikke stjaele fokus eller vises for brugeren.
        let hwnd = opret(config, fejl, false, -4000, -4000);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = UpdateWindow(hwnd);

        // Lad beskedsloejfen loebe layoutet faerdigt FOER vi tegner. Uden det ville billedet
        // vaere tomt - og et tomt billede kan ikke skelnes fra et brudt vindue.
        let indtil = Instant::now() + Duration::from_millis(1200);
        let mut besked = MSG::default();
        while Instant::now() < indtil {
            while PeekMessageW(&mut besked, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&besked);
                DispatchMessageW(&besked);
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // ⛔ Bill­edet alene kan ikke se om en kontrol har MISTET sit indhold: en tom kombo og
        // en kombo der tegner sit valg utydeligt, ser ens ud. De to lister udspoerges derfor
        // direkte, og tallene skrives ud sammen med farvetaellingen.
        let (boern, sidste_bund) = maal_boern(hwnd);
        let mut klient = RECT::default();
        let _ = GetClientRect(hwnd, &mut klient);
        let (indhold_h, raekkevidde) = match app_fra(hwnd) {
            Some(a) => (a.indhold_h, maks_rul(a)),
            None => (0, 0),
        };
        println!(
            "kontroller={boern} rotation={} visning={} sidste-bund={sidste_bund} \
             klienthoejde={} indhold={indhold_h} rullebart={raekkevidde}",
            kombo_valg(hwnd, ID_ROTATION),
            kombo_valg(hwnd, ID_LAYOUT),
            klient.bottom
        );

        let mut k = fang_og_doem(hwnd, ud);

        // ⛔ Kravet er at hver kontrol kan NAAS, ikke at alt kan ses paa én gang.
        //
        // Et krav om at alt skal vaere synligt uden at rulle kan kun opfyldes paa en skaerm
        // der er hoej nok - og Windows klemmer et vindue ind i skrivebordet. Maalt her
        // 2026-09-20: et vindue bedt om 840 px fik 749 px, fordi maalingen koerte i
        // Windows-session 0. Et krav ingen uovervaaget koersel kan opfylde, bliver laert vaek
        // (maaleregel 287). Rulningen er garantien, og DEN kan maales hvor som helst.
        if sidste_bund > klient.bottom && raekkevidde < sidste_bund - klient.bottom {
            eprintln!(
                "KONTROL KAN IKKE NAAS: nederste kontrol slutter i {sidste_bund}, klientfladen \
                 er {} hoej, og der kan kun rulles {raekkevidde} px.",
                klient.bottom
            );
            k = 8;
        }
        let _ = DestroyWindow(hwnd);
        // Toem koeen, saa WM_DESTROY faktisk koerer og App-boksen frigives.
        while PeekMessageW(&mut besked, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&besked);
            DispatchMessageW(&besked);
        }
        k
    };

    std::env::remove_var("HUSKWEBCAM_CONFIGDIR");
    let _ = std::fs::remove_dir_all(&midlertidig);
    kode
}

/// Roeg-tjek af BAKKEN: baerer den de fire kommandoer produktet har lovet, og kan ikonet
/// overhovedet lægges i proceslinjen?
///
/// ⛔ De to spoergsmaal er skilt ad med vilje. MENUEN kan maales hvor som helst - den er ren
/// Win32 uden et skrivebord. IKONET kan kun laegges hvor der ER en proceslinje, og en
/// uovervaaget koersel i Windows-session 0 har ingen. Et tjek der slog de to sammen, ville
/// melde "bakken er i stykker" om en app hvis menu er helt i orden (maaleregel 27), og
/// lukke-betingelsen ville aldrig kunne blive groen uden et menneske (maaleregel 287).
pub fn bakke_roegtjek() -> i32 {
    let forventet = ["Test kamera", "Vælg telefon", "Fortsæt", "", "Afslut"];
    unsafe {
        let menu = byg_bakkemenu(false);
        let antal = GetMenuItemCount(Some(menu));
        let mut fundet: Vec<String> = Vec::new();
        for i in 0..antal.max(0) {
            let mut buf = [0u16; 128];
            let n = GetMenuStringW(menu, i as u32, Some(&mut buf), MF_BYPOSITION) as usize;
            fundet.push(String::from_utf16_lossy(&buf[..n]));
        }
        let _ = DestroyMenu(menu);

        println!("bakkemenu={}", fundet.join(" | "));

        if fundet.len() != forventet.len() || fundet.iter().zip(forventet).any(|(a, b)| a != b) {
            eprintln!(
                "BAKKEMENUEN ER AENDRET: forventede {} punkter i raekkefoelgen {}",
                forventet.len(),
                forventet.join(" | ")
            );
            return 9;
        }

        // Ikonet: proev det, og MELD hvad der skete. Et fravaerende skrivebord er en maaling
        // om MILJOEET, ikke om produktet, saa det er ikke en fejl her.
        let instans = GetModuleHandleW(None).expect("GetModuleHandleW");
        let ikon = LoadIconW(Some(instans.into()), PCWSTR(1 as *const u16));
        println!("ikon-ressource={}", if ikon.is_ok() { "fundet" } else { "MANGLER" });
        if ikon.is_err() {
            eprintln!("IKON-RESSOURCEN MANGLER I EXE'EN - byg med res/husk.rc.");
            return 10;
        }
        0
    }
}

/// Fang vinduets klientflade, tael farver og gem billedet.
///
/// ⛔ Dommen er paa at der FAKTISK blev tegnet noget. Et vindue der ikke kan tegne, giver et
/// ensfarvet billede - og det er praecis den fejl vi leder efter.
///
/// ⛔ FANGSTEN GAAR GENNEM `WM_PRINTCLIENT`, IKKE GENNEM `PrintWindow`.
///
/// `PrintWindow` beder skrivebordets compositor om at gengive vinduet, og en uovervaaget
/// koersel har intet skrivebord: maalt her 2026-09-20 i Windows-session 0 svarede den med et
/// HELT ENSFARVET billede (forskellige-farver=1) om et vindue der var bygget korrekt. Det
/// udfald kan ikke skelnes fra et vindue der ikke kan tegne - altsaa praecis den fejl tjekket
/// findes for (maaleregel 27: en negativ maaling i en beholder du ikke selv kan aabne, er
/// ingen maaling).
///
/// `WM_PRINTCLIENT` gaar derimod direkte til VORES EGEN `tegn()` og til hver kontrols egen
/// tegnekode, med ren GDI ind i vores eget DIB. Det er det lag tjekket vogter (maaleregel 282),
/// og det virker uanset hvilken session processen koerer i.
unsafe fn fang_og_doem(hwnd: HWND, ud: &std::path::Path) -> i32 {
    unsafe {
        let mut klient = RECT::default();
        let _ = GetClientRect(hwnd, &mut klient);
        let b = (klient.right - klient.left).max(1) as usize;
        let h = (klient.bottom - klient.top).max(1) as usize;

        let skaerm = GetDC(None);
        let dc = CreateCompatibleDC(Some(skaerm));
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: b as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .expect("CreateDIBSection");
        SelectObject(dc, HGDIOBJ(bitmap.0));

        SendMessageW(
            hwnd,
            WM_PRINTCLIENT,
            Some(WPARAM(dc.0 as usize)),
            Some(LPARAM((PRF_CLIENT | PRF_ERASEBKGND | PRF_CHILDREN) as isize)),
        );
        let _ = GdiFlush();

        let raa = std::slice::from_raw_parts(bits as *const u8, b * h * 4);
        let mut pixels = raa.to_vec();
        for p in pixels.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let bm = Bitmap32::fra_bgra(b, h, pixels).expect("DIB-stoerrelse");

        let mut set = std::collections::HashSet::new();
        for p in bm.pixels().chunks_exact(4) {
            if set.len() >= 50 {
                break;
            }
            set.insert(u32::from_le_bytes([p[0], p[1], p[2], p[3]]));
        }

        let gemt = wic::kod_png(&bm)
            .ok()
            .and_then(|d| std::fs::write(ud, d).ok())
            .is_some();

        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        ReleaseDC(None, skaerm);

        println!(
            "vindue={b}x{h} forskellige-farver={} fil={}",
            set.len(),
            ud.display()
        );
        if !gemt {
            eprintln!("ROEGTJEK: billedet kunne ikke gemmes");
            return 7;
        }
        if set.len() < 3 {
            eprintln!("VINDUET BLEV IKKE TEGNET: billedet er naesten ensfarvet.");
            return 6;
        }
        0
    }
}

fn bred(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn app_fra(hwnd: HWND) -> Option<&'static mut App> {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if p.is_null() { None } else { Some(unsafe { &mut *p }) }
}

// ---- opbygning ------------------------------------------------------------------

struct Bygger {
    hwnd: HWND,
    instans: windows::Win32::Foundation::HINSTANCE,
    font: HFONT,
    y: i32,
    /// Hver kontrol der er oprettet gennem byggeren. Venstre spalte skal kunne RULLES, og en
    /// rulning er en flytning af netop de kontroller - ikke af hele vinduet.
    samlede: Vec<HWND>,
}

impl Bygger {
    fn kontrol(
        &mut self,
        klasse: &str,
        tekst: &str,
        stil: WINDOW_STYLE,
        x: i32,
        b: i32,
        h: i32,
        id: i32,
    ) -> HWND {
        unsafe {
            let k = bred(klasse);
            let t = bred(tekst);
            let c = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(k.as_ptr()),
                PCWSTR(t.as_ptr()),
                WS_CHILD | WS_VISIBLE | stil,
                x,
                self.y,
                b,
                h,
                Some(self.hwnd),
                Some(HMENU(id as *mut core::ffi::c_void)),
                Some(self.instans),
                None,
            )
            .expect("CreateWindowExW (kontrol)");
            SendMessageW(c, WM_SETFONT, Some(WPARAM(self.font.0 as usize)), Some(LPARAM(1)));
            self.samlede.push(c);
            self.y += h + 4;
            c
        }
    }

    fn etiket(&mut self, tekst: &str, b: i32, h: i32) {
        self.kontrol("STATIC", tekst, WINDOW_STYLE(0), 12, b, h, 0);
    }
}

/// Venstre spaltes bredde, og hvor preview-omraadet begynder.
///
/// ⛔ Bredere end C#-udgavens 360: dér laa spalten i en `ScrollViewer` med sin egen bredde, og
/// de to advarselstekster om token og netvaerk er lange med vilje - de er produktloefter, ikke
/// pynt. Ved 340 px blev de fire og seks linjer hoeje, og hele opsaetningen kunne ikke vaere i
/// et vindue der passer paa en lav skaerm.
const VENSTRE_B: i32 = 460;
const HOEJRE_X: i32 = VENSTRE_B + 32;

unsafe fn byg_kontroller(app: &mut App, instans: windows::Win32::Foundation::HINSTANCE) {
    let felt_b = VENSTRE_B - 24;
    let mut b = Bygger { hwnd: app.hwnd, instans, font: app.font, y: 12, samlede: Vec::new() };

    b.etiket("Telefoner", felt_b, 18);
    b.kontrol(
        "LISTBOX",
        "",
        WINDOW_STYLE(LBS_NOTIFY as u32 | WS_VSCROLL.0 | WS_BORDER.0),
        12,
        felt_b,
        100,
        ID_LISTE,
    );

    let knap_y = b.y;
    b.kontrol("BUTTON", "Tilføj", WINDOW_STYLE(BS_PUSHBUTTON as u32), 12, 90, 26, ID_TILFOEJ);
    b.y = knap_y;
    b.kontrol("BUTTON", "Fjern", WINDOW_STYLE(BS_PUSHBUTTON as u32), 108, 90, 26, ID_FJERN);

    b.etiket("Navn", felt_b, 18);
    b.kontrol("EDIT", "", WINDOW_STYLE(WS_BORDER.0), 12, felt_b, 24, ID_NAVN);

    b.etiket("Telefonens adresse", felt_b, 18);
    b.kontrol("EDIT", "", WINDOW_STYLE(WS_BORDER.0), 12, felt_b, 24, ID_VAERT);
    b.etiket(
        "Husk viser adressen i appens egen statusvisning. Telefon og PC skal kunne nå hinanden \
         på netværket - at gemme en adresse her skaber ikke i sig selv netværksadgang.",
        felt_b,
        58,
    );

    b.etiket("Token (kun hvis telefonen har et)", felt_b, 18);
    b.kontrol(
        "EDIT",
        "",
        WINDOW_STYLE(WS_BORDER.0 | ES_PASSWORD as u32),
        12,
        felt_b,
        24,
        ID_TOKEN,
    );
    b.kontrol(
        "BUTTON",
        "Hent fra telefonen",
        WINDOW_STYLE(BS_PUSHBUTTON as u32),
        12,
        160,
        26,
        ID_HENT_TOKEN,
    );
    b.etiket(
        "Kræver Husk 1.4 eller nyere: telefonen spørger om PC'en må få tokenet, og du godkender dér.",
        felt_b,
        38,
    );
    b.etiket(
        "Tokenet gemmes beskyttet med Windows' egen brugerkryptering, så det ikke står i \
         klartekst på disken. Det beskytter IKKE forbindelsen: Husk taler almindelig HTTP, så \
         brug den kun på dit eget LAN eller gennem en krypteret tunnel som Tailscale.",
        felt_b,
        76,
    );

    b.etiket("Grundrotation", felt_b, 18);
    let rot = b.kontrol(
        "COMBOBOX",
        "",
        WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_HASSTRINGS) as u32 | WS_VSCROLL.0),
        12,
        120,
        200,
        ID_ROTATION,
    );
    for g in ["0°", "90°", "180°", "270°"] {
        let s = bred(g);
        SendMessageW(rot, CB_ADDSTRING, None, Some(LPARAM(s.as_ptr() as isize)));
    }
    SendMessageW(rot, CB_SETCURSEL, Some(WPARAM(0)), None);
    // Kombo-kontroller angiver deres UDFOLDEDE hoejde ved oprettelse, saa vi retter y'en
    // tilbage til den lukkede hoejde. Uden det ville alt under staa 176 px for langt nede.
    b.y -= 200 - 24;

    b.kontrol(
        "BUTTON",
        "Følg telefonens tyngdesensor",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        12,
        felt_b,
        22,
        ID_TYNGDE,
    );
    b.kontrol(
        "BUTTON",
        "Brug forsidekameraet (kræver Husk 1.1)",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        12,
        felt_b,
        22,
        ID_FRONT,
    );

    let knap2_y = b.y;
    b.kontrol("BUTTON", "Test forbindelsen", WINDOW_STYLE(BS_PUSHBUTTON as u32), 12, 160, 28, ID_TEST);
    b.y = knap2_y;
    b.kontrol("BUTTON", "Gem", WINDOW_STYLE(BS_PUSHBUTTON as u32), 178, 160, 28, ID_GEM);
    b.kontrol("STATIC", "", WINDOW_STYLE(0), 12, felt_b, 158, ID_STATUS);

    app.indhold_h = b.y + 12;
    app.venstre = std::mem::take(&mut b.samlede);

    // --- hoejre side ---
    // ⛔ Hoejre spalte ruller IKKE med. Previewet skal blive staaende mens man ruller ned i
    // opsaetningen - et billede der forsvandt under rulningen ville goere "Test kamera"
    // ubrugelig, for den bygger netop paa at et menneske kan SE billedet.
    let mut h = Bygger { hwnd: app.hwnd, instans, font: app.font, y: 12, samlede: Vec::new() };
    h.kontrol("STATIC", "Visning:", WINDOW_STYLE(0), HOEJRE_X, 60, 20, 0);
    h.y = 12;
    let lay = h.kontrol(
        "COMBOBOX",
        "",
        WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_HASSTRINGS) as u32),
        HOEJRE_X + 64,
        110,
        120,
        ID_LAYOUT,
    );
    for s in ["Solo", "Gitter"] {
        let v = bred(s);
        SendMessageW(lay, CB_ADDSTRING, None, Some(LPARAM(v.as_ptr() as isize)));
    }
    SendMessageW(lay, CB_SETCURSEL, Some(WPARAM(0)), None);
    h.y = 12;
    h.kontrol(
        "BUTTON",
        "Start kamera",
        WINDOW_STYLE(BS_PUSHBUTTON as u32),
        HOEJRE_X + 184,
        140,
        26,
        ID_STARTSTOP,
    );

    h.y = 600;
    h.kontrol("STATIC", "", WINDOW_STYLE(0), HOEJRE_X, 400, 40, ID_KAMERASTATUS);
}

unsafe fn vis_guide(hwnd: HWND) {
    saet_tekst(
        hwnd,
        ID_STATUS,
        "Velkommen. Sådan kommer du i gang:\n\
         1. Tryk Tilføj, og skriv telefonens adresse (Husk viser den i appen).\n\
         2. Har telefonen et token, så skriv det. Ellers lad feltet stå tomt.\n\
         3. Tryk Test forbindelsen.\n\
         4. Tryk Gem, og derefter Start kamera.\n\
         5. Vælg »Husk Webcam« som kamera i mødeappen.\n\n\
         Du skal ikke bruge en terminal, WSL, SSH eller adb til noget af det.",
    );
}

// ---- kontrol-hjaelpere ------------------------------------------------------------

unsafe fn kontrol(hwnd: HWND, id: i32) -> HWND {
    unsafe { GetDlgItem(Some(hwnd), id).unwrap_or_default() }
}

unsafe fn saet_tekst(hwnd: HWND, id: i32, s: &str) {
    unsafe {
        // ⛔ Win32's STATIC- og EDIT-kontroller bryder linjen paa CRLF, ikke paa et bart LF.
        // Et `\n` alene tegner enten intet linjeskift eller et firkant-tegn, og resten af
        // teksten forsvinder ud af kontrollen. Normaliseringen ligger HER frem for hos hver
        // kalder, saa der er ét sted at rette.
        let w = bred(&s.replace("\r\n", "\n").replace('\n', "\r\n"));
        let c = kontrol(hwnd, id);
        if !c.is_invalid() {
            SendMessageW(c, WM_SETTEXT, None, Some(LPARAM(w.as_ptr() as isize)));
        }
    }
}

unsafe fn hent_tekst(hwnd: HWND, id: i32) -> String {
    unsafe {
        let c = kontrol(hwnd, id);
        if c.is_invalid() {
            return String::new();
        }
        let n = SendMessageW(c, WM_GETTEXTLENGTH, None, None).0 as usize;
        let mut buf = vec![0u16; n + 1];
        let laest = SendMessageW(
            c,
            WM_GETTEXT,
            Some(WPARAM(buf.len())),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        )
        .0 as usize;
        String::from_utf16_lossy(&buf[..laest.min(buf.len())])
    }
}

unsafe fn afkrydset(hwnd: HWND, id: i32) -> bool {
    unsafe { SendMessageW(kontrol(hwnd, id), BM_GETCHECK, None, None).0 == BST_CHECKED.0 as isize }
}

unsafe fn saet_afkrydset(hwnd: HWND, id: i32, v: bool) {
    unsafe {
        SendMessageW(
            kontrol(hwnd, id),
            BM_SETCHECK,
            Some(WPARAM(if v { BST_CHECKED.0 as usize } else { BST_UNCHECKED.0 as usize })),
            None,
        );
    }
}

/// Gør vinduet hoejt nok til at HELE opsaetningen er synlig uden at rulle.
///
/// ⛔ Hoejden UDLEDES af indholdet frem for at staa som et tal. C#-udgaven havde en
/// `ScrollViewer` om venstre spalte; en ren Win32-app har ikke en gratis af slagsen, saa
/// vinduet maa passe sig selv til. Og et hardkodet tal ville vaere forkert ved enhver anden
/// DPI end den det blev maalt ved: rammen omkring klientfladen vokser med DPI'en, saa den
/// samme vindueshoejde giver forskellig klienthoejde paa to maskiner. Maalt her 2026-09-20:
/// et vindue bedt om 840 px gav 749 px klienthoejde, og de nederste 13 px af opsaetningen laa
/// uden for kanten.
unsafe fn tilpas_hoejde(app: &App) {
    unsafe {
        let mut klient = RECT::default();
        let mut vindue = RECT::default();
        if GetClientRect(app.hwnd, &mut klient).is_err()
            || GetWindowRect(app.hwnd, &mut vindue).is_err()
        {
            return;
        }
        let mangler = app.indhold_h - klient.bottom;
        if mangler <= 0 {
            return;
        }
        // ⚠️ Windows KLEMMER et vindue ind i skrivebordet, saa en forhoejelse kan give mindre
        // end der blev bedt om. Det er netop derfor rullepanelet findes: hoejden er et FORSOEG,
        // rulningen er garantien.
        let _ = SetWindowPos(
            app.hwnd,
            None,
            0,
            0,
            vindue.right - vindue.left,
            (vindue.bottom - vindue.top) + mangler,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Saet rullepanelets omraade efter indholdets hoejde og vinduets aktuelle klienthoejde.
unsafe fn opdater_rullepanel(app: &mut App) {
    unsafe {
        let mut klient = RECT::default();
        if GetClientRect(app.hwnd, &mut klient).is_err() {
            return;
        }
        let side = klient.bottom.max(1);
        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            nMin: 0,
            nMax: (app.indhold_h - 1).max(0),
            nPage: side as u32,
            nPos: app.rul,
            nTrackPos: 0,
        };
        SetScrollInfo(app.hwnd, SB_VERT, &si, true);
        // Er vinduet blevet hoejere, kan den nuvaerende rulning ligge uden for det nye omraade.
        rul_til(app, app.rul);
    }
}

/// Hvor langt der maksimalt kan rulles. 0 betyder at alt er synligt uden at rulle.
unsafe fn maks_rul(app: &App) -> i32 {
    unsafe {
        let mut klient = RECT::default();
        if GetClientRect(app.hwnd, &mut klient).is_err() {
            return 0;
        }
        (app.indhold_h - klient.bottom).max(0)
    }
}

/// Rul venstre spalte til en given position. Kun VENSTRE kontroller flyttes.
unsafe fn rul_til(app: &mut App, ny: i32) {
    unsafe {
        let ny = ny.clamp(0, maks_rul(app));
        let delta = app.rul - ny;
        if delta == 0 {
            return;
        }
        app.rul = ny;

        for h in &app.venstre {
            let mut r = RECT::default();
            if GetWindowRect(*h, &mut r).is_err() {
                continue;
            }
            let mut p = [POINT { x: r.left, y: r.top }];
            let _ = ScreenToClient(app.hwnd, p.as_mut_ptr());
            let _ = SetWindowPos(
                *h,
                None,
                p[0].x,
                p[0].y + delta,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }

        let si = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_POS,
            nPos: ny,
            ..Default::default()
        };
        SetScrollInfo(app.hwnd, SB_VERT, &si, true);
        let _ = InvalidateRect(Some(app.hwnd), None, true);
    }
}

/// Antal boern, og hvor langt nede den nederste af dem slutter (i foraelderens klientkoordinater).
///
/// ⛔ Kombo-kontroller maales paa deres LUKKEDE hoejde, ikke paa den udfoldede liste: en kombo
/// oprettes med dropdown-hoejden som sin hoejde, saa `GetWindowRect` ville ellers melde en
/// kontrol der naar 176 px laengere ned end den staar. `CB_GETCOMBOBOXINFO` er omvejen; her er
/// det nok at spoerge om kontrollens klasse.
unsafe fn maal_boern(foraelder: HWND) -> (usize, i32) {
    unsafe {
        let mut antal = 0usize;
        let mut bund = 0i32;
        let mut barn = GetWindow(foraelder, GW_CHILD).unwrap_or_default();
        while !barn.is_invalid() {
            antal += 1;
            let mut r = RECT::default();
            if GetWindowRect(barn, &mut r).is_ok() {
                let mut p = [POINT { x: r.left, y: r.bottom }];
                let _ = ScreenToClient(foraelder, p.as_mut_ptr());
                let mut b = p[0].y;
                let mut klasse = [0u16; 32];
                let n = GetClassNameW(barn, &mut klasse) as usize;
                if String::from_utf16_lossy(&klasse[..n]).eq_ignore_ascii_case("ComboBox") {
                    let mut lukket = RECT::default();
                    if GetClientRect(barn, &mut lukket).is_ok() {
                        // Klientfladen paa en lukket CBS_DROPDOWNLIST er selve feltet.
                        let mut top = [POINT { x: r.left, y: r.top }];
                        let _ = ScreenToClient(foraelder, top.as_mut_ptr());
                        b = top[0].y + lukket.bottom.min(40);
                    }
                }
                bund = bund.max(b);
            }
            barn = GetWindow(barn, GW_HWNDNEXT).unwrap_or_default();
        }
        (antal, bund)
    }
}

/// Den valgte tekst i en kombo, eller `<ingen>`. Bruges af roegtjekket.
unsafe fn kombo_valg(hwnd: HWND, id: i32) -> String {
    unsafe {
        let c = kontrol(hwnd, id);
        let i = SendMessageW(c, CB_GETCURSEL, None, None).0;
        if i < 0 {
            return "<ingen>".to_string();
        }
        let n = SendMessageW(c, CB_GETLBTEXTLEN, Some(WPARAM(i as usize)), None).0;
        if n <= 0 {
            return "<tom>".to_string();
        }
        let mut buf = vec![0u16; n as usize + 1];
        SendMessageW(
            c,
            CB_GETLBTEXT,
            Some(WPARAM(i as usize)),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        );
        let laengde = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..laengde])
    }
}

unsafe fn valgt_indeks(hwnd: HWND, id: i32, besked: u32) -> i32 {
    unsafe { SendMessageW(kontrol(hwnd, id), besked, None, None).0 as i32 }
}

unsafe fn saet_valgt(hwnd: HWND, id: i32, besked: u32, i: i32) {
    unsafe {
        SendMessageW(kontrol(hwnd, id), besked, Some(WPARAM(i as usize)), None);
    }
}

// ---- telefonliste ------------------------------------------------------------------

unsafe fn indlaes_liste(app: &mut App) {
    unsafe {
        let l = kontrol(app.hwnd, ID_LISTE);
        SendMessageW(l, LB_RESETCONTENT, None, None);
        for t in &app.config.telefoner {
            let s = bred(&format!("{}  ({})", t.navn, t.vaert));
            SendMessageW(l, LB_ADDSTRING, None, Some(LPARAM(s.as_ptr() as isize)));
        }
        if !app.config.telefoner.is_empty() {
            let i = app.config.solo_indeks.min(app.config.telefoner.len() - 1) as i32;
            saet_valgt(app.hwnd, ID_LISTE, LB_SETCURSEL, i);
            valgt_skiftet(app);
        }
        saet_valgt(
            app.hwnd,
            ID_LAYOUT,
            CB_SETCURSEL,
            if app.config.layout == Layout::Grid { 1 } else { 0 },
        );
    }
}

unsafe fn valgt_nr(app: &App) -> Option<usize> {
    let i = unsafe { valgt_indeks(app.hwnd, ID_LISTE, LB_GETCURSEL) };
    if i < 0 || i as usize >= app.config.telefoner.len() { None } else { Some(i as usize) }
}

unsafe fn valgt_skiftet(app: &mut App) {
    unsafe {
        let Some(i) = valgt_nr(app) else { return };
        let t = app.config.telefoner[i].clone();
        saet_tekst(app.hwnd, ID_NAVN, &t.navn);
        saet_tekst(app.hwnd, ID_VAERT, &t.vaert);
        saet_tekst(app.hwnd, ID_TOKEN, t.token().as_deref().unwrap_or(""));
        saet_valgt(app.hwnd, ID_ROTATION, CB_SETCURSEL, (t.grundrotation / 90).clamp(0, 3));
        saet_afkrydset(app.hwnd, ID_TYNGDE, t.foelg_tyngde);
        saet_afkrydset(app.hwnd, ID_FRONT, t.front);
        app.config.solo_indeks = i;
    }
}

unsafe fn gem(app: &mut App) {
    unsafe {
        if let Some(i) = valgt_nr(app) {
            let navn = hent_tekst(app.hwnd, ID_NAVN);
            let t = &mut app.config.telefoner[i];
            t.navn = if navn.trim().is_empty() { "Telefon".into() } else { navn.trim().into() };
            t.vaert = hent_tekst(app.hwnd, ID_VAERT).trim().to_string();
            let token = hent_tekst(app.hwnd, ID_TOKEN);
            t.saet_token(Some(&token));
            t.grundrotation = valgt_indeks(app.hwnd, ID_ROTATION, CB_GETCURSEL).max(0) * 90;
            t.foelg_tyngde = afkrydset(app.hwnd, ID_TYNGDE);
            t.front = afkrydset(app.hwnd, ID_FRONT);
        }

        app.config.guide_fuldfoert = !app.config.telefoner.is_empty();
        match app.config.gem() {
            Ok(()) => saet_tekst(
                app.hwnd,
                ID_STATUS,
                &format!("Gemt i {}", Config::mappe().display()),
            ),
            Err(e) => saet_tekst(app.hwnd, ID_STATUS, &format!("Kunne ikke gemme: {e}")),
        }

        let valgt = valgt_indeks(app.hwnd, ID_LISTE, LB_GETCURSEL);
        indlaes_liste(app);
        if valgt >= 0 {
            saet_valgt(app.hwnd, ID_LISTE, LB_SETCURSEL, valgt);
        }
    }
}

// ---- kamera ------------------------------------------------------------------------

unsafe fn start_stop(app: &mut App) {
    unsafe {
        if app.koerer {
            stop_alt(app);
            saet_tekst(app.hwnd, ID_STARTSTOP, "Start kamera");
            return;
        }

        gem(app);
        if app.config.telefoner.is_empty() {
            saet_tekst(app.hwnd, ID_STATUS, "Tilføj en telefon først.");
            return;
        }

        app.laas = SingletonLaas::tag_standard();
        if app.laas.is_none() {
            saet_tekst(
                app.hwnd,
                ID_STATUS,
                "Husk Webcam kører allerede. Kun én kopi må sende til kameraet.",
            );
            return;
        }

        app.sender = Some(UnityCaptureSender::standard());
        for t in &app.config.telefoner {
            let mut k = MjpegSource::ny(&t.navn, &husk_urls::stream(&t.vaert, t.token().as_deref()));
            k.start();
            app.kilder.push(k);
            app.daempere.push(OrientationDamper::new(t.grundrotation));
            app.front_sat_for_gen.push(0);
        }

        // ⛔ Kameravalget sendes IKKE herfra. Det sker i genanvend_front_ved_ny_forbindelse(),
        // som fyrer naar stroemmen faktisk er etableret - baade foerste gang og efter hver
        // genforbindelse. Et engangs-kald bag `if t.front` ville vaere forkert paa TO maader:
        // `front=0` blev aldrig sendt, saa et bagkamera-valg kunne ikke fortryde en tidligere
        // sessions `front=1`; og valget blev aldrig gentaget, saa en genstart af telefonen
        // tavst gav bagkameraet igen.

        app.koerer = true;
        saet_tekst(app.hwnd, ID_STARTSTOP, "Stop kamera");
    }
}

fn stop_alt(app: &mut App) {
    for k in &mut app.kilder {
        k.stop();
    }
    app.kilder.clear();
    app.daempere.clear();
    app.front_sat_for_gen.clear();
    app.afkodet.clear();
    app.sender = None;
    app.laas = None;
    app.koerer = false;
}

/// Saet kameravalget paa hver kilde der har faaet en NY forbindelse siden sidst.
///
/// Telefonen gemmer IKKE valget: `Rig.useFront` er en almindelig statisk variabel i Husk 1.1,
/// saa app- eller enheds-genstart falder tilbage til bagkameraet. Et engangs-kald ved Start
/// ville derfor tavst holde op med at gaelde. Vi noegler paa
/// [`MjpegSource::forbindelses_nr`] frem for paa et ur, saa et stabilt link koster nul kald,
/// mens hver genforbindelse koster praecis ét.
fn genanvend_front_ved_ny_forbindelse(app: &mut App) {
    let n = app
        .kilder
        .len()
        .min(app.front_sat_for_gen.len())
        .min(app.config.telefoner.len());
    for i in 0..n {
        let gen = app.kilder[i].forbindelses_nr();
        if gen == 0 || gen == app.front_sat_for_gen[i] {
            continue;
        }
        app.front_sat_for_gen[i] = gen;
        let t = app.config.telefoner[i].clone();
        let svar = app.til_gui.clone();
        let hwnd = TraadHwnd::ny(app.hwnd);
        let _ = std::thread::Builder::new()
            .name("husk-front".into())
            .spawn(move || {
                let tekst = saet_front(&t);
                let _ = svar.send(Besked::Status(tekst));
                // ⛔ Vaek GUI'en FOERST naar svaret er sendt. En vaekning foer kaldet ville
                // toemme en tom kanal, og resultatet ville ligge og vente paa den naeste
                // tilfaeldige vaekning.
                vaek_gui(hwnd.hwnd());
            });
    }
}

/// Vaelg kameraside over HTTP.
///
/// ⚠️ Et 200 er IKKE et bevis for at valget slog igennem: en ældre Husk svarer 200 og ignorerer
/// parameteren tavst. Vi læser derfor /flags bagefter og siger det, hvis feltet ikke findes
/// eller ikke fulgte med (maaleregel 3).
fn saet_front(t: &TelefonConfig) -> String {
    let token = t.token();
    let url = husk_urls::set_front(&t.vaert, t.front, token.as_deref());
    let Ok(s) = Session::ny(BRUGERAGENT) else {
        return "Kunne ikke oprette en HTTP-session.".to_string();
    };

    let svar = match s.get(&url, 6000) {
        Ok(s) => s,
        Err(e) => {
            return format!(
                "Kameraside kunne ikke sættes på {}: {}",
                t.navn,
                url_redactor::hide_secrets(Some(&e.to_string()), Some(&url))
            );
        }
    };

    if svar.status == 409 {
        // 409 er Husk 1.1's egen dom: siden findes ikke paa enheden. Den er MAALT foer noget
        // som helst blev aendret, saa kaldet var en ren no-op.
        return format!(
            "{} har ikke det valgte kamera ({}) - valget er ikke sat.",
            t.navn,
            if t.front { "forside" } else { "bagside" }
        );
    }
    if !svar.er_ok() {
        return format!("Kameraside kunne ikke sættes på {}: HTTP {}.", t.navn, svar.status);
    }

    let Ok(tjek) = Forbindelsestjek::ny() else {
        return String::new();
    };
    let f = tjek.tjek(&t.vaert, token.as_deref());
    if f.front.is_none() && t.front {
        // Kun naar der faktisk blev BEDT om forsiden. En telefon uden »front«-felt staar paa
        // bagkameraet, saa et `front=0` mod den ER opfyldt - at kalde det en fejl ville lyve
        // om en tilstand der er præcis som ønsket.
        format!(
            "{} svarede OK, men har intet »front«-felt - valget slog IKKE igennem. \
             Telefonen er ældre end Husk 1.1.",
            t.navn
        )
    } else if f.front != Some(t.front) && f.front.is_some() {
        format!(
            "{} svarede OK, men /flags viser stadig den anden kameraside. \
             Valget slog ikke igennem.",
            t.navn
        )
    } else {
        String::new()
    }
}

unsafe fn opdater_preview(app: &mut App) {
    unsafe {
        if app.kilder.is_empty() {
            return;
        }

        // Afkod hver kildes nyeste frame HOEJST én gang: preview og kamerastroem deler
        // resultatet.
        let mut billeder: Vec<Option<Bitmap32>> = Vec::with_capacity(app.kilder.len());
        if app.afkodet.len() != app.kilder.len() {
            app.afkodet = vec![(0, None); app.kilder.len()];
        }
        for (i, k) in app.kilder.iter().enumerate() {
            match k.nyeste() {
                None => billeder.push(None),
                Some(f) => {
                    if app.afkodet[i].0 != f.sekvens_nr {
                        app.afkodet[i] = (f.sekvens_nr, wic::afkod_jpeg(&f.data));
                    }
                    billeder.push(app.afkodet[i].1.clone());
                }
            }
        }

        let valgt = valgt_indeks(app.hwnd, ID_LISTE, LB_GETCURSEL).max(0) as usize;
        let levende: Vec<Levende> = app
            .kilder
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let cfg = &app.config.telefoner[i];
                let rot = if cfg.foelg_tyngde { app.daempere[i].rotation() } else { cfg.grundrotation };
                Levende { kilde: k, rotation: rot, billede: billeder[i].clone() }
            })
            .collect();
        let pegere: Vec<&dyn FrameKilde> = levende.iter().map(|l| l as &dyn FrameKilde).collect();

        let mut komp = Compositor::new(&pegere, 1280, 720);
        komp.layout = app.config.layout;
        komp.solo_indeks = valgt;
        let ramme = komp.byg();

        if app.koerer && app.demand.leverer() {
            if let Some(s) = app.sender.as_mut() {
                if s.send(&ramme, false, 1000) == SendResultat::IngenModtager {
                    saet_tekst(app.hwnd, ID_KAMERASTATUS, INGEN_MODTAGER_FORKLARING);
                }
            }
        }

        app.seneste = Some(ramme);
        let _ = InvalidateRect(Some(app.hwnd), None, false);

        if app.sidste_tyngde.elapsed() > Duration::from_millis(750) {
            app.sidste_tyngde = Instant::now();
            start_tyngdemaaling(app);
        }
    }
}

fn start_tyngdemaaling(app: &mut App) {
    for (i, t) in app.config.telefoner.iter().enumerate() {
        if !t.foelg_tyngde || i >= app.daempere.len() {
            continue;
        }
        let vaert = t.vaert.clone();
        let token = t.token();
        let svar = app.til_gui.clone();
        let hwnd = TraadHwnd::ny(app.hwnd);
        let _ = std::thread::Builder::new()
            .name("husk-tyngde".into())
            .spawn(move || {
                let v = hent_tyngde(&vaert, token.as_deref());
                let _ = svar.send(Besked::Tyngde(i, v));
                vaek_gui(hwnd.hwnd());
            });
    }
}

fn hent_tyngde(vaert: &str, token: Option<&str>) -> Option<[f64; 3]> {
    let s = Session::ny(BRUGERAGENT).ok()?;
    let svar = s.get(&husk_urls::gravity(vaert, token), 4000).ok()?;
    if !svar.er_ok() {
        return None;
    }
    let j = json::parse(&svar.krop_som_tekst()).ok()?;
    let a = j.get("values")?.som_array()?;
    if a.len() < 3 {
        return None;
    }
    let mut ud = [0.0f64; 3];
    for (i, v) in a.iter().take(3).enumerate() {
        ud[i] = v.som_tal()?;
    }
    Some(ud)
}

fn vaek_gui(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_BAGGRUNDSSVAR, WPARAM(0), LPARAM(0));
    }
}

unsafe fn opdater_efterspoergsel(app: &mut App) {
    unsafe {
        app.demand.preview_aabent = GetForegroundWindow() == app.hwnd && IsWindowVisible(app.hwnd).as_bool();

        let navn = app
            .sender
            .as_ref()
            .map(|s| s.mutex_navn())
            .unwrap_or_else(|| format!("{}_Mutx", crate::unity_sender::HUSK_PRAEFIKS));
        let mut t = unity_demand::tael(&navn);

        // Vores egen sender holder ogsaa et handle. Traek det fra, saa tallet er MODTAGERE.
        if app.koerer && t.maalt && t.antal > 0 {
            t.antal -= 1;
        }

        app.demand.tick(&t);
        genanvend_front_ved_ny_forbindelse(app);

        let m = match app.demand.sidste_maalefejl() {
            Some(f) => format!("kunne ikke måles ({f}) - behandles som optaget"),
            None => format!("{} modtager(e)", app.demand.sidste_modtagere()),
        };
        let tekst = if app.koerer {
            // ⛔ »preview taeller med« er BETINGET: `preview_aabent` saettes ovenfor til
            // »vinduet er i forgrunden OG synligt«, og `DemandMonitor::tick` taeller det kun
            // med naar flaget staar. I den almindelige drift - appen i bakken, modtageren i
            // forgrunden - staar det IKKE, saa en ubetinget tekst ville sige det modsatte af
            // hvad der sker.
            format!(
                "Sender: {} · {m}{}",
                if app.demand.leverer() { "ja" } else { "nej" },
                if app.demand.preview_aabent { " · preview tæller med" } else { "" }
            )
        } else {
            format!("Stoppet · {m}")
        };
        saet_tekst(app.hwnd, ID_KAMERASTATUS, &tekst);
    }
}

// ---- bakke ---------------------------------------------------------------------------

fn bakke_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    }
}

unsafe fn tilfoej_bakke(hwnd: HWND, instans: windows::Win32::Foundation::HINSTANCE) {
    unsafe {
        let mut d = bakke_data(hwnd);
        d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        d.uCallbackMessage = WM_BAKKE;
        // Ikon-ressource 1 er appens eget ikon (se res/husk.rc). Falder det bort, bruger vi
        // systemets program-ikon frem for at kaste: et manglende ikon maa ikke koste hele
        // bakken, og dermed brugerens eneste vej til Afslut.
        d.hIcon = LoadIconW(Some(instans), PCWSTR(1 as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();
        saet_tip(&mut d, "Husk Webcam - klar");
        let _ = Shell_NotifyIconW(NIM_ADD, &d);
    }
}

/// ⛔ Tippet AFKORTES her med vilje.
///
/// `szTip` er 128 tegn inklusive NUL, og en for lang tekst ville blive skaaret midt i et tegn
/// eller loebe ud af feltet. C#-siden maalte det samme paa `NotifyIcon.Text`, som KASTER over
/// 63 tegn i stedet for at afkorte (maaleregel 461) - samme klasse, anden fejlretning.
fn saet_tip(d: &mut NOTIFYICONDATAW, s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(d.szTip.len() - 1).collect();
    d.szTip[..w.len()].copy_from_slice(&w);
    d.szTip[w.len()] = 0;
}

unsafe fn opdater_bakke_tip(app: &App) {
    unsafe {
        let mut d = bakke_data(app.hwnd);
        d.uFlags = NIF_TIP;
        saet_tip(
            &mut d,
            if app.koerer { "Husk Webcam - sender" } else { "Husk Webcam - klar" },
        );
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

unsafe fn fjern_bakke(hwnd: HWND) {
    unsafe {
        let d = bakke_data(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &d);
    }
}

/// Bakkens menu: de fire kommandoer produktet har lovet.
///
/// ⛔ Bygget ÉT sted, saa roegtjekket maaler den menu brugeren faktisk faar - ikke en kopi af
/// den (maaleregel 135).
unsafe fn byg_bakkemenu(koerer: bool) -> HMENU {
    unsafe {
        let menu = CreatePopupMenu().expect("CreatePopupMenu");
        let _ = AppendMenuW(menu, MF_STRING, ID_BAKKE_TEST as usize, w!("Test kamera"));
        let _ = AppendMenuW(menu, MF_STRING, ID_BAKKE_VAELG as usize, w!("Vælg telefon"));
        // Menuen faar sin tilstand naar den AABNES, ikke naar den blev bygget: brugeren kan
        // have startet eller stoppet kameraet i vinduet imellem to hoejreklik.
        let pause = bred(if koerer { "Pause" } else { "Fortsæt" });
        let _ = AppendMenuW(menu, MF_STRING, ID_BAKKE_PAUSE as usize, PCWSTR(pause.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_BAKKE_AFSLUT as usize, w!("Afslut"));
        menu
    }
}

unsafe fn vis_bakkemenu(app: &App) {
    unsafe {
        let menu = byg_bakkemenu(app.koerer);

        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        // ⛔ SetForegroundWindow FOER TrackPopupMenu. Uden den bliver menuen haengende paa
        // skaermen naar brugeren klikker ved siden af - en dokumenteret Windows-adfaerd, ikke
        // en fejl i menuen.
        let _ = SetForegroundWindow(app.hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, p.x, p.y, Some(0), app.hwnd, None);
        let _ = PostMessageW(Some(app.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

unsafe fn vis_vindue(app: &App) {
    unsafe {
        let _ = ShowWindow(app.hwnd, SW_SHOW);
        let _ = ShowWindow(app.hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(app.hwnd);
    }
}

// ---- tegning -------------------------------------------------------------------------

unsafe fn tegn(app: &App, hdc: HDC, klient: RECT) {
    unsafe {
        FillRect(hdc, &klient, app.baggrund);

        // Preview-omraadet: hele hoejre side under kontrolraekken og over statuslinjen.
        let omr = preview_rekt(klient);
        let sort = CreateSolidBrush(COLORREF(0));
        FillRect(hdc, &omr, sort);
        let _ = DeleteObject(HGDIOBJ(sort.0));

        let Some(bm) = app.seneste.as_ref() else { return };

        // Bevar aspektet: aldrig straekning. Et strakt ansigt i et moede er vaerre end sorte
        // bjaelker.
        let bb = (omr.right - omr.left).max(1);
        let hh = (omr.bottom - omr.top).max(1);
        let skala = f64::min(bb as f64 / bm.bredde() as f64, hh as f64 / bm.hoejde() as f64);
        let nb = ((bm.bredde() as f64 * skala) as i32).max(1);
        let nh = ((bm.hoejde() as f64 * skala) as i32).max(1);
        let ox = omr.left + (bb - nb) / 2;
        let oy = omr.top + (hh - nh) / 2;

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: bm.bredde() as i32,
            biHeight: -(bm.hoejde() as i32), // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        StretchDIBits(
            hdc,
            ox,
            oy,
            nb,
            nh,
            0,
            0,
            bm.bredde() as i32,
            bm.hoejde() as i32,
            Some(bm.pixels().as_ptr() as *const core::ffi::c_void),
            &bmi,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

// PRF-flagene har intet navn i bindingerne; de staar her med deres betydning.
const PRF_NONCLIENT: u32 = 0x0000_0002;
const PRF_CLIENT: u32 = 0x0000_0004;
const PRF_ERASEBKGND: u32 = 0x0000_0008;
const PRF_CHILDREN: u32 = 0x0000_0010;

/// Bed hver kontrol om at tegne sig selv ind i den samme DC, paa sin egen plads.
///
/// Windows tegner normalt boern i deres EGEN DC, saa en kopi af foraelderens klientflade ville
/// vaere tom paa netop de steder hvor kontrollerne staar - og et tjek der doemte paa det,
/// ville melde "intet tegnet" om et helt korrekt vindue.
unsafe fn tegn_boern(foraelder: HWND, hdc: HDC) {
    unsafe {
        let mut barn = GetWindow(foraelder, GW_CHILD).unwrap_or_default();
        while !barn.is_invalid() {
            let mut r = RECT::default();
            if GetWindowRect(barn, &mut r).is_ok() {
                let mut hjoerne = [POINT { x: r.left, y: r.top }];
                let _ = ScreenToClient(foraelder, hjoerne.as_mut_ptr());
                let mut gammel = POINT::default();
                let _ = SetViewportOrgEx(hdc, hjoerne[0].x, hjoerne[0].y, Some(&mut gammel));
                // ⛔ `WM_PRINT` og ikke `WM_PRINTCLIENT` til boernene. Maalt her 2026-09-20:
                // med `WM_PRINTCLIENT` tegnede kun STATIC og LISTBOX sig, mens BUTTON, EDIT og
                // COMBOBOX blev VAEK - og et tomt felt kan ikke skelnes fra en kontrol der
                // aldrig blev oprettet. `WM_PRINT` gaar gennem kontrollens DefWindowProc, som
                // selv kalder baade ramme- og klienttegningen.
                SendMessageW(
                    barn,
                    WM_PRINT,
                    Some(WPARAM(hdc.0 as usize)),
                    Some(LPARAM((PRF_CLIENT | PRF_NONCLIENT | PRF_ERASEBKGND) as isize)),
                );
                let _ = SetViewportOrgEx(hdc, gammel.x, gammel.y, None);
            }
            barn = GetWindow(barn, GW_HWNDNEXT).unwrap_or_default();
        }
    }
}

fn preview_rekt(klient: RECT) -> RECT {
    RECT {
        left: HOEJRE_X,
        top: 48,
        right: (klient.right - 12).max(HOEJRE_X + 12),
        bottom: (klient.bottom - 56).max(96),
    }
}

// ---- beskedsloejfen --------------------------------------------------------------------

unsafe extern "system" fn vindue_proc(hwnd: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_NCCREATE => {
                let cs = lp.0 as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
                DefWindowProcW(hwnd, m, wp, lp)
            }

            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                SetTextColor(hdc, COLORREF(0x00FFFFFF));
                SetBkColor(hdc, BAGGRUND);
                LRESULT(app.baggrund.0 as isize)
            }

            WM_COMMAND => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                let id = (wp.0 & 0xFFFF) as i32;
                let kode = ((wp.0 >> 16) & 0xFFFF) as u32;
                haandter_kommando(app, id, kode);
                LRESULT(0)
            }

            WM_TIMER => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                match wp.0 {
                    TIMER_TICK => opdater_preview(app),
                    TIMER_DEMAND => opdater_efterspoergsel(app),
                    _ => {}
                }
                LRESULT(0)
            }

            WM_BAGGRUNDSSVAR => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                while let Ok(b) = app.fra_baggrund.try_recv() {
                    match b {
                        Besked::Status(s) if !s.is_empty() => saet_tekst(hwnd, ID_STATUS, &s),
                        Besked::Status(_) => {}
                        Besked::Test(s, front) => {
                            saet_tekst(hwnd, ID_STATUS, &s);
                            // Kan telefonen ikke styre kameravalg, saa slaa afkrydsningen fra
                            // frem for at lade brugeren vaelge noget der ikke kan virke.
                            let c = kontrol(hwnd, ID_FRONT);
                            match front {
                                Some(_) => {
                                    let _ = EnableWindow(c, true);
                                }
                                None => {
                                    saet_afkrydset(hwnd, ID_FRONT, false);
                                    let _ = EnableWindow(c, false);
                                }
                            }
                        }
                        Besked::Token(token, tekst) => {
                            let _ = EnableWindow(kontrol(hwnd, ID_HENT_TOKEN), true);
                            if let Some(t) = token {
                                // Feltet fyldes, og gem() skriver det DPAPI-beskyttet via
                                // TelefonConfig::saet_token - samme vej som et indtastet token.
                                saet_tekst(hwnd, ID_TOKEN, &t);
                                gem(app);
                            }
                            // Efter gem(), som selv skriver en statuslinje.
                            saet_tekst(hwnd, ID_STATUS, &tekst);
                        }
                        Besked::Tyngde(i, v) => {
                            if i < app.daempere.len() {
                                match v {
                                    Some(t) => {
                                        app.daempere[i].maaling(Some(&t));
                                    }
                                    None => app.daempere[i].fejlede(),
                                }
                            }
                        }
                    }
                }
                LRESULT(0)
            }

            WM_BAKKE => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                match lp.0 as u32 {
                    WM_RBUTTONUP | WM_CONTEXTMENU => vis_bakkemenu(app),
                    WM_LBUTTONDBLCLK => vis_vindue(app),
                    _ => {}
                }
                LRESULT(0)
            }

            WM_PAINT => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut klient = RECT::default();
                let _ = GetClientRect(hwnd, &mut klient);
                tegn(app, hdc, klient);
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }

            // Samme tegning som WM_PAINT, men ind i en DC kalderen leverer. Roegtjekket bruger
            // den; den deler kode med den rigtige tegning frem for at gen-udlede den
            // (maaleregel 135).
            WM_PRINTCLIENT => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                let mut klient = RECT::default();
                let _ = GetClientRect(hwnd, &mut klient);
                tegn(app, hdc, klient);
                if lp.0 as u32 & PRF_CHILDREN != 0 {
                    tegn_boern(hwnd, hdc);
                }
                LRESULT(0)
            }

            WM_VSCROLL => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                let mut klient = RECT::default();
                let _ = GetClientRect(hwnd, &mut klient);
                let side = klient.bottom.max(1);
                let ny = match SCROLLBAR_COMMAND((wp.0 & 0xFFFF) as i32) {
                    SB_LINEUP => app.rul - 24,
                    SB_LINEDOWN => app.rul + 24,
                    SB_PAGEUP => app.rul - side,
                    SB_PAGEDOWN => app.rul + side,
                    SB_THUMBTRACK | SB_THUMBPOSITION => ((wp.0 >> 16) & 0xFFFF) as i32,
                    SB_TOP => 0,
                    SB_BOTTOM => maks_rul(app),
                    _ => app.rul,
                };
                rul_til(app, ny);
                LRESULT(0)
            }

            WM_MOUSEWHEEL => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                let hjul = ((wp.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                rul_til(app, app.rul - (hjul / 120) * 48);
                LRESULT(0)
            }

            WM_SIZE => {
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                opdater_rullepanel(app);
                let mut klient = RECT::default();
                let _ = GetClientRect(hwnd, &mut klient);
                let _ = SetWindowPos(
                    kontrol(hwnd, ID_KAMERASTATUS),
                    None,
                    HOEJRE_X,
                    klient.bottom - 48,
                    (klient.right - HOEJRE_X - 12).max(100),
                    40,
                    SWP_NOZORDER,
                );
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }

            WM_CLOSE => {
                // ⛔ Vinduet SKJULES; kun "Afslut" i bakken afslutter. Uden det ville et tryk
                // paa krydset stoppe processen, og autostarten ville se brudt ud ved naeste
                // kamerabrug.
                let Some(app) = app_fra(hwnd) else {
                    return DefWindowProcW(hwnd, m, wp, lp);
                };
                if app.afsluttet {
                    let _ = DestroyWindow(hwnd);
                } else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                LRESULT(0)
            }

            WM_DESTROY => {
                if let Some(app) = app_fra(hwnd) {
                    stop_alt(app);
                    let _ = DeleteObject(HGDIOBJ(app.font.0));
                    let _ = DeleteObject(HGDIOBJ(app.baggrund.0));
                    let raa = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(raa));
                }
                PostQuitMessage(0);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, m, wp, lp),
        }
    }
}

unsafe fn haandter_kommando(app: &mut App, id: i32, kode: u32) {
    unsafe {
        match id {
            ID_LISTE if kode == LBN_SELCHANGE => valgt_skiftet(app),
            ID_TILFOEJ => {
                let n = app.config.telefoner.len() + 1;
                app.config.telefoner.push(TelefonConfig {
                    navn: format!("Telefon {n}"),
                    ..Default::default()
                });
                indlaes_liste(app);
                saet_valgt(app.hwnd, ID_LISTE, LB_SETCURSEL, (n - 1) as i32);
                valgt_skiftet(app);
            }
            ID_FJERN => {
                if let Some(i) = valgt_nr(app) {
                    app.config.telefoner.remove(i);
                    gem(app);
                    indlaes_liste(app);
                }
            }
            ID_GEM => gem(app),
            ID_TEST => {
                saet_tekst(app.hwnd, ID_STATUS, "Tester ...");
                let vaert = hent_tekst(app.hwnd, ID_VAERT).trim().to_string();
                let token = hent_tekst(app.hwnd, ID_TOKEN);
                let svar = app.til_gui.clone();
                let hwnd = TraadHwnd::ny(app.hwnd);
                let _ = std::thread::Builder::new().name("husk-test".into()).spawn(move || {
                    let t = match Forbindelsestjek::ny() {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = svar.send(Besked::Test(format!("📡 {e}"), None));
                            vaek_gui(hwnd.hwnd());
                            return;
                        }
                    };
                    let s = t.tjek(&vaert, if token.is_empty() { None } else { Some(&token) });
                    let praefiks = match s.dom {
                        Forbindelsesdom::Ok => "✅ ",
                        Forbindelsesdom::AdgangNaegtet => "🔒 ",
                        Forbindelsesdom::ForkertEllerForGammel => "❓ ",
                        Forbindelsesdom::IntetSvar => "📡 ",
                    };
                    let front = if s.dom == Forbindelsesdom::Ok { s.front } else { Some(true) };
                    let _ = svar.send(Besked::Test(format!("{praefiks}{}", s.tekst), front));
                    vaek_gui(hwnd.hwnd());
                });
            }
            ID_HENT_TOKEN => {
                let vaert = hent_tekst(app.hwnd, ID_VAERT).trim().to_string();
                if vaert.is_empty() {
                    saet_tekst(app.hwnd, ID_STATUS, "Indtast telefonens adresse først.");
                    return;
                }
                saet_tekst(app.hwnd, ID_STATUS, "Beder telefonen om tokenet …");
                let _ = EnableWindow(kontrol(app.hwnd, ID_HENT_TOKEN), false);
                let svar = app.til_gui.clone();
                let hwnd = TraadHwnd::ny(app.hwnd);
                let _ = std::thread::Builder::new().name("husk-token".into()).spawn(move || {
                    let dom = match TokenHenter::ny() {
                        Ok(h) => h.hent(&vaert, &token_hentning::computernavn(), |s| {
                            let _ = svar.send(Besked::Status(s.to_string()));
                            vaek_gui(hwnd.hwnd());
                        }),
                        Err(e) => Hentningsdom::Fejl(e.to_string()),
                    };
                    let tekst = dom.tekst();
                    let token = match dom {
                        Hentningsdom::Godkendt(t) => Some(t),
                        _ => None,
                    };
                    let _ = svar.send(Besked::Token(token, tekst));
                    vaek_gui(hwnd.hwnd());
                });
            }
            ID_LAYOUT if kode == CBN_SELCHANGE => {
                app.config.layout = if valgt_indeks(app.hwnd, ID_LAYOUT, CB_GETCURSEL) == 1 {
                    Layout::Grid
                } else {
                    Layout::Solo
                };
            }
            ID_STARTSTOP => {
                start_stop(app);
                opdater_bakke_tip(app);
            }

            // --- bakkens menu ---
            ID_BAKKE_TEST => {
                // "Test kamera" viser billedet frem for at paastaa noget om det: vinduet
                // baerer previewet, og et menneske kan se om der kommer et billede. En knap
                // der kun svarede "OK" ville ikke kunne skelne et levende kamera fra et tomt.
                vis_vindue(app);
                if !app.koerer {
                    start_stop(app);
                }
                opdater_bakke_tip(app);
            }
            ID_BAKKE_VAELG => vis_vindue(app),
            ID_BAKKE_PAUSE => {
                start_stop(app);
                opdater_bakke_tip(app);
            }
            ID_BAKKE_AFSLUT => {
                app.afsluttet = true;
                fjern_bakke(app.hwnd);
                let _ = DestroyWindow(app.hwnd);
            }
            _ => {}
        }
    }
}
