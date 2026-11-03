//! What the phone link's views draw, as plain data the renderer borrows: the
//! QR code (or why there is none), the phone asking, the files arriving.
//! `app_phone.rs` builds it from its state every frame; the demo builds it
//! from made-up data.

use crate::model::PhoneStage;
use crate::phone::{DeviceKind, QrMatrix};
use crate::shelf::ShelfKind;

/// Why there is no code to scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warn {
    /// The network is Public (or its category is unknown).
    Public,
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
    /// Seconds the code is still good for; `None` once it is not (expired
    /// or voided): the code is dimmed and "Buat QR baru" shows.
    pub remaining: Option<u64>,
    /// The code counts down (until a phone is approved); then the chip says
    /// how long, and afterwards says nothing.
    pub timed: bool,
    /// The code was voided by three rejections (the chip says so).
    pub void: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QrView<'a> {
    /// The network is being looked at on the server thread.
    Starting,
    /// Why there is no code, and the network's name when it is known.
    Warning(Warn, &'a str),
    Code(CodeView<'a>),
}

/// The phone that waits for an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AskView<'a> {
    pub id: u64,
    pub kind: DeviceKind,
    pub browser: Option<&'static str>,
    pub ip: &'a str,
    pub code: &'a str,
    /// Others waiting behind it.
    pub more: usize,
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

/// How a shelf card stands in the send view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardState {
    /// Ticked for the phone. `taken`: a phone has downloaded the whole file.
    Ticked { taken: bool },
    Unticked,
    /// A folder: it cannot be sent.
    Folder,
    /// The file is gone: it cannot be sent.
    Stale,
}

impl CardState {
    /// The card can be ticked (a file that is there).
    pub fn shareable(self) -> bool {
        matches!(self, CardState::Ticked { .. } | CardState::Unticked)
    }
}

/// A shelf card as the send view draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendCard {
    pub name: String,
    pub kind: ShelfKind,
    pub state: CardState,
}

/// The send view ("Kirim ke HP"): the shelf's cards in the shelf's order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SendView<'a> {
    pub cards: &'a [SendCard],
    /// Files ticked, and their size, for the footer.
    pub count: usize,
    pub bytes: u64,
    /// "Android", "2 HP"; empty while no phone has been approved.
    pub chip: &'a str,
}

/// The phone link's view over the shelf.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneView<'a> {
    pub stage: PhoneStage,
    pub qr: QrView<'a>,
    pub ask: Option<AskView<'a>>,
    pub recv: RecvView<'a>,
    pub send: SendView<'a>,
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
                timed: true,
                void: false,
            }),
            ask: None,
            recv: RecvView {
                rows: &[],
                chip: "",
                files: 0,
                bytes: 0,
            },
            send: SendView {
                cards: &[],
                count: 0,
                bytes: 0,
                chip: "",
            },
        };
        assert!(!view(Some(5)).code_dead());
        assert!(view(None).code_dead());
    }

    #[test]
    fn only_a_ticked_or_unticked_card_can_be_ticked() {
        assert!(CardState::Ticked { taken: false }.shareable());
        assert!(CardState::Ticked { taken: true }.shareable());
        assert!(CardState::Unticked.shareable());
        assert!(!CardState::Folder.shareable());
        assert!(!CardState::Stale.shareable());
    }
}
