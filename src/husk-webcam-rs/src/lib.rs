//! Husk Webcam - kernen.
//!
//! Ét modul pr. ansvar: billedbygning, konfiguration, efterspoergsel, HTTP, JPEG-afkodning,
//! orientering, hemmeligheder og afsendelsen til kamera-filteret.
//!
//! ⛔ Hvorfor der kun er ÉN ekstern afhaengighed: se kommentaren i `Cargo.toml`. Kort sagt
//!    ligger al billedafkodning, al HTTP og al kryptering i Windows selv, og det er dér de
//!    170,9 MB runtime forsvinder hen.

pub mod bitmap;
pub mod compositor;
pub mod config;
pub mod demand;
pub mod forbindelsestjek;
pub mod gui;
pub mod husk_urls;
pub mod json;
pub mod mjpeg_parser;
pub mod mjpeg_source;
pub mod orientation;
pub mod placeholder;
pub mod secret;
pub mod source_status;
pub mod testfikstur;
pub mod unity_sender;
pub mod url_redactor;
pub mod wic;
pub mod winhttp;

pub use bitmap::Bitmap32;
pub use compositor::{Compositor, FrameKilde, Layout};
pub use config::{Config, TelefonConfig};
pub use demand::{DemandMonitor, HandleTaelling, SingletonLaas, unity_demand};
pub use forbindelsestjek::{Forbindelsesdom, Forbindelsessvar, Forbindelsestjek};
pub use mjpeg_parser::{MjpegParser, ParserAfvisning};
pub use mjpeg_source::{JpegFrame, MjpegSource};
pub use orientation::{OrientationDamper, front_rotation};
pub use source_status::{Backoff, SourceStatus, StaleClock};
pub use unity_sender::{SendResultat, UnityCaptureSender};
