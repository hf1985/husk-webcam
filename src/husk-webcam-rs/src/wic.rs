//! JPEG gennem WIC - Windows Imaging Component.
//!
//! ⛔ HVORFOR WIC OG IKKE EN BILLEDPAKKE.
//!
//! WPF's `BitmapDecoder`, som C#-udgaven brugte, ER WIC med en .NET-skal om. Skallen kostede
//! hele WPF i pakken; komponenten under den ligger allerede i Windows (`windowscodecs.dll`).
//! Rust-udgaven kalder derfor den samme komponent direkte. Det er ikke en ny implementering -
//! det er den samme afkoder uden mellemled, og det er netop derfor afkoder-proeven
//! kan kraeve NUL forskel i pixels.
//!
//! En tredjeparts-billedpakke ville give de MB tilbage under et andet navn. Det var netop
//! grunden til at OpenCV blev fravalgt i Python-vejen.

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_ContainerFormatPng,
    GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppBGRA, IWICBitmapEncoder,
    IWICBitmapFrameEncode, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoCreateInstance,
    CoInitializeEx,
};
use windows::core::PCWSTR;

use crate::bitmap::Bitmap32;
use crate::mjpeg_parser::MjpegParser;

thread_local! {
    static COM_KLAR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sikr at COM er initialiseret paa DENNE traad.
///
/// ⛔ COM initialiseres PR. TRAAD, ikke pr. proces. En afkodning paa en baggrundstraad uden
/// initialisering fejler med `CO_E_NOTINITIALIZED`, og fejlen ligner "billedet kunne ikke
/// afkodes" frem for "traaden var ikke klar".
///
/// `RPC_E_CHANGED_MODE` behandles som SUCCES: traaden er allerede i COM, bare i en anden
/// tilstand, og til WIC er begge tilstande brugbare.
pub fn sikr_com(apartment: bool) {
    COM_KLAR.with(|k| {
        if k.get() {
            return;
        }
        let tilstand = if apartment { COINIT_APARTMENTTHREADED } else { COINIT_MULTITHREADED };
        // SAFETY: CoInitializeEx er traad-lokal og maa kaldes flere gange; vi rydder bevidst
        // ikke op igen, fordi traaden bruger COM resten af sin levetid.
        let _ = unsafe { CoInitializeEx(None, tilstand) };
        k.set(true);
    });
}

fn fabrik() -> windows::core::Result<IWICImagingFactory> {
    sikr_com(false);
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
}

/// Afkod et JPEG til BGRA32.
///
/// Returnerer `None` naar bytes ikke kan afkodes. En halv frame er ikke en fejl der skal
/// kastes - den skal droppes, og naeste frame laeses.
pub fn afkod_jpeg(jpeg: &[u8]) -> Option<Bitmap32> {
    // Samme forudgaaende dom som C#-udgaven: ER det overhovedet et helt JPEG? Uden den ville
    // en afkortet frame naa WIC og give en COM-fejl frem for et roligt drop.
    if !MjpegParser::er_helt_jpeg(jpeg) {
        return None;
    }
    afkod_bytes(jpeg).ok()
}

fn afkod_bytes(jpeg: &[u8]) -> windows::core::Result<Bitmap32> {
    let f = fabrik()?;
    unsafe {
        let stream = f.CreateStream()?;
        // WIC vil have en skrivbar peger, men roerer den ikke ved laesning. Vi giver den en
        // KOPI frem for at caste konstanten vaek: en afkoder der skrev i kalderens buffer,
        // ville vaere en fejl ingen kunne se.
        let mut buf = jpeg.to_vec();
        stream.InitializeFromMemory(&mut buf)?;

        let afkoder = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)?;
        if afkoder.GetFrameCount()? == 0 {
            return Err(windows::core::Error::empty());
        }
        let frame = afkoder.GetFrame(0)?;

        let konv = f.CreateFormatConverter()?;
        konv.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;

        let mut b = 0u32;
        let mut h = 0u32;
        konv.GetSize(&mut b, &mut h)?;
        if b == 0 || h == 0 {
            return Err(windows::core::Error::empty());
        }

        let stride = b as usize * 4;
        let mut pixels = vec![0u8; stride * h as usize];
        konv.CopyPixels(std::ptr::null(), stride as u32, &mut pixels)?;

        Bitmap32::fra_bgra(b as usize, h as usize, pixels)
            .ok_or_else(windows::core::Error::empty)
    }
}

/// Kod et BGRA32-billede som JPEG med WIC's egen encoder.
///
/// Bruges KUN til at lave testfiksturer: afkoder-proeven skal have et FAST
/// testbillede, og en fikstur der laves af Windows' egen encoder er den mest neutrale kilde vi
/// kan give den. Produktet selv koder aldrig JPEG - det laeser dem.
/// ⛔ Kvaliteten saettes IKKE - WIC's egen default gaelder.
///
/// At skrue paa den ville kraeve `IPropertyBag2` og dermed hele OLE-fladen i bindingerne, og
/// fiksturet har ikke brug for et bestemt kvalitetstal: det skal bare vaere det SAMME hver gang.
/// Det er defaulten per konstruktion.
pub fn kod_jpeg(bm: &Bitmap32) -> windows::core::Result<Vec<u8>> {
    kod_billede(bm, &GUID_ContainerFormatJpeg)
}

/// Kod som PNG. Bruges af vindue-roegtjekket, hvor et tabsgivende format ville skjule netop
/// den forskel tjekket leder efter.
pub fn kod_png(bm: &Bitmap32) -> windows::core::Result<Vec<u8>> {
    kod_billede(bm, &GUID_ContainerFormatPng)
}

/// Sletter sin fil naar den gaar ud af skope - ogsaa naar funktionen forlades med `?`.
///
/// ⛔ Her stod indtil 2026-09-20 et enkelt `remove_file` paa SUCCES-stien alene, saa hvert
/// eneste fejlende WIC-kald efterlod en temp-fil. De ligner ikke skade og bliver derfor ikke
/// ryddet af nogen - samme klasse som et redirect der lander et uventet sted.
struct MidlertidigFil(std::path::PathBuf);

impl Drop for MidlertidigFil {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn kod_billede(bm: &Bitmap32, container: &windows::core::GUID) -> windows::core::Result<Vec<u8>> {
    let f = fabrik()?;
    unsafe {
        let stream = f.CreateStream()?;
        let vagt = MidlertidigFil(std::env::temp_dir().join(format!(
            "husk-wic-{}-{:?}.jpg",
            std::process::id(),
            std::thread::current().id()
        )));
        let midlertidig = vagt.0.clone();
        let bred: Vec<u16> = midlertidig
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // ⛔ `dwDesiredAccess` er en FIL-adgangsmaske (GENERIC_READ/GENERIC_WRITE), ikke en
        // STGM-vaerdi. Maalt her 2026-09-20: `STGM_WRITE | STGM_CREATE` gav
        // `WINCODEC_ERR_INTERNALERROR` foerst ved `Commit()`, altsaa langt fra det kald der var
        // forkert. Encoderen skal kunne LAESE sin egen stroem tilbage, saa begge flag skal med.
        stream.InitializeFromFilename(PCWSTR(bred.as_ptr()), GENERIC_READ.0 | GENERIC_WRITE.0)?;

        let koder: IWICBitmapEncoder = f.CreateEncoder(container, std::ptr::null())?;
        koder.Initialize(&stream, WICBitmapEncoderNoCache)?;

        let mut frame: Option<IWICBitmapFrameEncode> = None;
        let mut muligheder = None;
        koder.CreateNewFrame(&mut frame, &mut muligheder)?;
        let frame = frame.ok_or_else(windows::core::Error::empty)?;

        frame.Initialize(muligheder.as_ref())?;
        frame.SetSize(bm.bredde() as u32, bm.hoejde() as u32)?;

        // JPEG kan ikke baere alfa; 24bppBGR er WIC's naturlige JPEG-format.
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format)?;

        let mut bgr = vec![0u8; bm.bredde() * bm.hoejde() * 3];
        for (i, p) in bm.pixels().chunks_exact(4).enumerate() {
            bgr[i * 3] = p[0];
            bgr[i * 3 + 1] = p[1];
            bgr[i * 3 + 2] = p[2];
        }
        frame.WritePixels(bm.hoejde() as u32, (bm.bredde() * 3) as u32, &bgr)?;
        frame.Commit()?;
        koder.Commit()?;
        drop(stream);

        let data = std::fs::read(&midlertidig).map_err(|e| windows::core::Error::new(
            windows::core::HRESULT(-2147024894), // ERROR_FILE_NOT_FOUND som HRESULT
            e.to_string(),
        ))?;
        drop(vagt);
        Ok(data)
    }
}

/// Laes en FIL helt ind i hukommelsen og afkod den som JPEG. Bruges af maalings-tilstandene.
pub fn afkod_jpeg_fil(sti: &std::path::Path) -> Option<Bitmap32> {
    let data = std::fs::read(sti).ok()?;
    afkod_jpeg(&data)
}

/// Resultatet af en pixel-for-pixel-sammenligning af to raa BGRA-dumps.
#[derive(Debug, PartialEq, Eq)]
pub enum Sammenligning {
    /// De to filer er lige store og afviger i `afvigende` pixels.
    Maalt { bredde: u32, hoejde: u32, afvigende: u64, stoerste_kanalforskel: u8 },
    /// Dimensionerne er ikke ens. Da er "nul afvigende pixels" meningsloest.
    ForskelligStoerrelse { a: (u32, u32), b: (u32, u32) },
    /// Mindst én fil er for kort til at baere en header, eller de er ulige lange.
    Ulaeselig(&'static str),
}

/// Sammenlign to raa BGRA-dumps pixel for pixel.
///
/// ⛔ Skilt fra kommandolinjen, saa den kan MAALES. En sammenligning der aldrig er set sige nej,
/// er ikke afproevet - og netop denne er dommen i afkoder-proeven, saa en fail-open her ville
/// goere hele afkoder-proeven groen uanset hvad.
pub fn sammenlign_raa_bgra(a: &[u8], b: &[u8]) -> Sammenligning {
    if a.len() < 8 || b.len() < 8 {
        return Sammenligning::Ulaeselig("mindst én fil er for kort til at baere en header");
    }
    let dim = |d: &[u8]| {
        (
            u32::from_le_bytes([d[0], d[1], d[2], d[3]]),
            u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
        )
    };
    let da = dim(a);
    let db = dim(b);
    if da != db {
        return Sammenligning::ForskelligStoerrelse { a: da, b: db };
    }

    let pa = &a[8..];
    let pb = &b[8..];
    if pa.len() != pb.len() {
        return Sammenligning::Ulaeselig("samme dimensioner, men forskellig bytelaengde");
    }
    if pa.len() != da.0 as usize * da.1 as usize * 4 {
        return Sammenligning::Ulaeselig("bytelaengden passer ikke til dimensionerne");
    }

    let mut afvigende = 0u64;
    let mut stoerste = 0u8;
    for (x, y) in pa.chunks_exact(4).zip(pb.chunks_exact(4)) {
        if x != y {
            afvigende += 1;
            for k in 0..4 {
                stoerste = stoerste.max(x[k].abs_diff(y[k]));
            }
        }
    }

    Sammenligning::Maalt {
        bredde: da.0,
        hoejde: da.1,
        afvigende,
        stoerste_kanalforskel: stoerste,
    }
}

/// Gem raa BGRA-bytes med en lille header, saa to udgaver kan sammenlignes byte for byte.
///
/// Formatet er med vilje det dummeste der findes: 8 bytes header (bredde og hoejde som
/// little-endian u32) og derefter pixels. En sammenligning maa ikke afhaenge af et
/// billedformat der selv kan fortolke noget forskelligt.
pub fn skriv_raa_bgra(bm: &Bitmap32, sti: &std::path::Path) -> std::io::Result<()> {
    let mut ud = Vec::with_capacity(8 + bm.pixels().len());
    ud.extend_from_slice(&(bm.bredde() as u32).to_le_bytes());
    ud.extend_from_slice(&(bm.hoejde() as u32).to_le_bytes());
    ud.extend_from_slice(bm.pixels());
    std::fs::write(sti, ud)
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    fn testmoenster(b: usize, h: usize) -> Bitmap32 {
        let mut bm = Bitmap32::new(b, h);
        for y in 0..h {
            for x in 0..b {
                let venstre = x < b / 2;
                let oeverst = y < h / 2;
                let (bb, gg, rr) = match (venstre, oeverst) {
                    (true, true) => (0u8, 0u8, 255u8),
                    (false, true) => (0, 255, 0),
                    (true, false) => (255, 0, 0),
                    _ => (255, 255, 255),
                };
                bm.saet_pixel(x, y, bb, gg, rr, 255);
            }
        }
        bm
    }

    #[test]
    fn afkoder_et_jpeg_wic_selv_har_kodet() {
        let ind = testmoenster(64, 32);
        let jpeg = kod_jpeg(&ind).expect("WIC skal kunne kode JPEG");
        assert!(MjpegParser::er_helt_jpeg(&jpeg), "encoderen skal give et helt JPEG");

        let ud = afkod_jpeg(&jpeg).expect("WIC skal kunne afkode sit eget JPEG");
        assert_eq!((ud.bredde(), ud.hoejde()), (64, 32));
        // JPEG er tabsgivende, saa vi doemmer paa at de fire felter er GENKENDELIGE, ikke paa
        // byte-lighed. Et graat billede kunne bestaa uanset (maaleregel 8).
        let (b, g, r, a) = ud.pixel(8, 4);
        assert!(r > 200 && g < 60 && b < 60, "oeverst venstre skal vaere roed: {r},{g},{b}");
        assert_eq!(a, 255, "alfa skal vaere uigennemsigtig");
        let (b2, g2, r2, _) = ud.pixel(56, 4);
        assert!(g2 > 200 && r2 < 80 && b2 < 80, "oeverst hoejre skal vaere groen");
        let (b3, g3, r3, _) = ud.pixel(8, 28);
        assert!(b3 > 200 && r3 < 80 && g3 < 80, "nederst venstre skal vaere blaa");
    }

    #[test]
    fn afkod_afviser_det_der_ikke_er_et_helt_jpeg() {
        assert!(afkod_jpeg(&[]).is_none());
        assert!(afkod_jpeg(&[0xFF, 0xD8, 0xFF]).is_none());
        // Rigtige markoerer, men skrald imellem: WIC selv skal afvise det.
        let mut skrald = vec![0xFF, 0xD8, 0xFF];
        skrald.extend_from_slice(&[0u8; 64]);
        skrald.extend_from_slice(&[0xFF, 0xD9]);
        assert!(afkod_jpeg(&skrald).is_none());
    }

    fn dump(b: u32, h: u32, pixels: Vec<u8>) -> Vec<u8> {
        let mut d = b.to_le_bytes().to_vec();
        d.extend_from_slice(&h.to_le_bytes());
        d.extend_from_slice(&pixels);
        d
    }

    #[test]
    fn to_ens_dumps_giver_NUL_afvigende() {
        let a = dump(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(
            sammenlign_raa_bgra(&a, &a.clone()),
            Sammenligning::Maalt { bredde: 2, hoejde: 1, afvigende: 0, stoerste_kanalforskel: 0 }
        );
    }

    #[test]
    fn EN_aendret_byte_faelder_sammenligningen() {
        // ⛔ Modvidnet. En sammenligning der aldrig er set sige nej, er ikke afproevet - og
        // netop denne er dommen i hele afkoder-proeven.
        let a = dump(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        let mut b = a.clone();
        b[8 + 5] = b[8 + 5].wrapping_add(7);
        assert_eq!(
            sammenlign_raa_bgra(&a, &b),
            Sammenligning::Maalt { bredde: 2, hoejde: 1, afvigende: 1, stoerste_kanalforskel: 7 }
        );
    }

    #[test]
    fn forskellig_stoerrelse_er_sin_EGEN_dom() {
        // "Nul afvigende pixels" om to billeder af forskellig stoerrelse ville vaere et
        // loegnagtigt groent svar.
        let a = dump(2, 1, vec![0; 8]);
        let b = dump(1, 2, vec![0; 8]);
        assert_eq!(
            sammenlign_raa_bgra(&a, &b),
            Sammenligning::ForskelligStoerrelse { a: (2, 1), b: (1, 2) }
        );
    }

    #[test]
    fn en_afkortet_fil_er_ULAESELIG_ikke_ens() {
        assert!(matches!(sammenlign_raa_bgra(&[], &[]), Sammenligning::Ulaeselig(_)));
        let a = dump(2, 1, vec![0; 8]);
        let b = dump(2, 1, vec![0; 4]);
        assert!(matches!(sammenlign_raa_bgra(&a, &b), Sammenligning::Ulaeselig(_)));
    }

    #[test]
    fn en_header_der_lyver_om_stoerrelsen_afvises() {
        // Samme bytelaengde paa begge sider, men headeren passer ikke til den.
        let a = dump(100, 100, vec![0; 8]);
        let b = dump(100, 100, vec![0; 8]);
        assert!(matches!(sammenlign_raa_bgra(&a, &b), Sammenligning::Ulaeselig(_)));
    }

    #[test]
    fn raa_bgra_har_den_aftalte_header() {
        let bm = testmoenster(4, 2);
        let sti = std::env::temp_dir().join(format!("husk-raa-{}.bin", std::process::id()));
        skriv_raa_bgra(&bm, &sti).unwrap();
        let d = std::fs::read(&sti).unwrap();
        let _ = std::fs::remove_file(&sti);
        assert_eq!(&d[0..4], &4u32.to_le_bytes());
        assert_eq!(&d[4..8], &2u32.to_le_bytes());
        assert_eq!(d.len(), 8 + 4 * 2 * 4);
    }
}
