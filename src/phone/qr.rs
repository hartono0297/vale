//! The QR code of the phone link. `qrcodegen` (Nayuki) makes the modules;
//! this decides whether the cat can sit in the middle.

use qrcodegen::{QrCode, QrCodeEcc};

/// The cat's plate covers a square this many modules wide in the centre.
pub const LOGO_MODULES: usize = 7;

/// The logo may hide at most this share of the code's area.
const LOGO_MAX_SHARE: f64 = 0.04;

/// A finished code: `size` x `size` modules, dark ones `true`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: Vec<bool>,
    /// The cat's plate may be drawn in the centre (`LOGO_MODULES` square).
    pub logo: bool,
}

impl QrMatrix {
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }
}

fn matrix(code: &QrCode, logo: bool) -> QrMatrix {
    let size = code.size() as usize;
    let mut modules = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            modules.push(code.get_module(x as i32, y as i32));
        }
    }
    QrMatrix {
        size,
        modules,
        logo,
    }
}

/// Whether a centred plate of `LOGO_MODULES` is small enough for a code of
/// `size` modules.
fn logo_fits(size: usize) -> bool {
    let hole = (LOGO_MODULES * LOGO_MODULES) as f64;
    hole / ((size * size) as f64) < LOGO_MAX_SHARE
}

/// The code for `text`. Error correction High, with the logo allowed when the
/// code that came out is High and the plate is under 4 % of its area; else
/// Medium (a smaller code that scans better) and no logo. `None` only when
/// the text does not fit any QR code.
pub fn make(text: &str) -> Option<QrMatrix> {
    if let Ok(code) = QrCode::encode_text(text, QrCodeEcc::High)
        && code.error_correction_level() == QrCodeEcc::High
        && logo_fits(code.size() as usize)
    {
        return Some(matrix(&code, true));
    }
    let code = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    Some(matrix(&code, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typical_link_gets_the_logo() {
        let url = "http://192.168.100.200:47399/s/AbCdEfGhIjKlMnOpQrStUv";
        assert_eq!(url.len(), 31 + 22);
        let m = make(url).expect("qr");
        assert!(m.logo, "size {}", m.size);
        assert_eq!(m.size * m.size, m.modules.len());
        // Version 6 at High: 41 modules.
        assert_eq!(m.size, 41);
        // The corner finder pattern is there.
        assert!(m.get(0, 0) && m.get(6, 6) && !m.get(7, 7));
        assert!(!m.get(m.size, 0));
    }

    #[test]
    fn a_short_link_that_is_too_small_for_the_logo_has_none() {
        // 21 modules (version 1): the 7x7 plate would be 11 % of it.
        let m = make("http://a/").expect("qr");
        assert!(!m.logo);
        assert!(!logo_fits(21));
        assert!(!logo_fits(29));
        assert!(logo_fits(37));
    }

    #[test]
    fn a_text_too_long_for_any_code_gives_none() {
        let long = "x".repeat(4000);
        assert!(make(&long).is_none());
    }

    #[test]
    fn the_same_text_makes_the_same_code() {
        assert_eq!(make("http://10.0.0.2:47390/s/abc"), make("http://10.0.0.2:47390/s/abc"));
    }
}
