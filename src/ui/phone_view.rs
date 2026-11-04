//! What the phone link's views draw, as plain data the renderer borrows: the
//! QR code (or why there is none) and the files arriving.
//! `app_phone.rs` builds it from its state every frame; the demo builds it
//! from made-up data.

use crate::model::PhoneStage;
use crate::phone::QrMatrix;
use crate::shelf::ShelfKind;

/// Why there is no code to scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warn {
    NoLan,
    PortsBusy,
    Failed,
}

/// A QR code to scan and what goes with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CodeView<'a> {
    pub qr: &'a QrMatrix,
    /// The network's name.
    pub network: &'a str,
    /// "192.168.18.42:47390".
    pub addr: &'a str,
    /// Seconds the code is still good for; `None` once it is not (it could
    /// not be renewed): the code is dimmed and "Buat QR baru" shows.
    pub remaining: Option<u64>,
    /// The network is Public: the network line says so and the hint is the
    /// warning instead of the firewall one.
    pub public: bool,
    /// "Tersambung: Android" once a phone joined (step 2's sub line); empty
    /// before.
    pub joined: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QrView<'a> {
    /// The network is being looked at on the server thread.
    Starting,
    /// Why there is no code.
    Warning(Warn),
    Code(CodeView<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    /// Coming in, 0 to 100 percent.
    Receiving(u8),
    Done,
    Cancelled,
    Failed,
}

/// A file received (or on its way).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowView {
    pub name: String,
    pub size: u64,
    pub kind: ShelfKind,
    pub status: RowStatus,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecvView<'a> {
    /// Oldest first; the view shows the newest that fit.
    pub rows: &'a [RowView],
    /// "Android", "2 HP".
    pub chip: &'a str,
    /// Files and bytes of the session so far, for the footer.
    pub files: usize,
    pub bytes: u64,
}

/// The phone link's view over the shelf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneView<'a> {
    pub stage: PhoneStage,
    pub qr: QrView<'a>,
    pub recv: RecvView<'a>,
}

impl PhoneView<'_> {
    /// The QR view's code is dead: it shows dimmed with "Buat QR baru".
    pub fn code_dead(&self) -> bool {
        matches!(self.qr, QrView::Code(c) if c.remaining.is_none())
    }
}

/// The modules of `qr` as runs along each row: `(row, first column, length)`,
/// dark modules only, so a code is a few hundred fills instead of thousands.
pub fn dark_runs(qr: &QrMatrix) -> Vec<(usize, usize, usize)> {
    let mut runs = Vec::new();
    for y in 0..qr.size {
        let mut x = 0;
        while x < qr.size {
            if !qr.get(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < qr.size && qr.get(x, y) {
                x += 1;
            }
            runs.push((y, start, x - start));
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cover_exactly_the_dark_modules() {
        let qr = crate::phone::qr::make("http://192.168.1.2:47390/s/AbCdEfGhIjKlMnOpQrStUv")
            .expect("qr");
        let runs = dark_runs(&qr);
        let covered: usize = runs.iter().map(|r| r.2).sum();
        let dark = qr.modules.iter().filter(|m| **m).count();
        assert_eq!(covered, dark);
        assert!(runs.len() < dark, "runs merge neighbours");
        for (y, x, n) in runs {
            assert!((x..x + n).all(|c| qr.get(c, y)));
            assert!(x == 0 || !qr.get(x - 1, y), "a run starts at its first module");
            assert!(x + n == qr.size || !qr.get(x + n, y), "and ends at its last");
        }
    }

    #[test]
    fn a_code_is_dead_once_it_has_no_time_left() {
        let qr = crate::phone::qr::make("http://10.0.0.2:47390/s/x").expect("qr");
        let view = |remaining| PhoneView {
            stage: PhoneStage::Qr,
            qr: QrView::Code(CodeView {
                qr: &qr,
                network: "n",
                addr: "a",
                remaining,
                public: false,
                joined: "",
            }),
            recv: RecvView {
                rows: &[],
                chip: "",
                files: 0,
                bytes: 0,
            },
        };
        assert!(!view(Some(5)).code_dead());
        assert!(view(None).code_dead());
    }
}
