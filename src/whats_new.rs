//! "What's new": what each release changed, written by hand for the people
//! who use Vale, one entry per version in both languages. Shown after an
//! update (`app_update`) and in Settings → Tentang. A test fails when the
//! version in `Cargo.toml` has no entry, so a release cannot skip it.

use crate::i18n::Lang;
use crate::update::parse_version;

/// One release: 1-5 short points in each language.
pub struct Entry {
    pub version: &'static str,
    pub id: &'static [&'static str],
    pub en: &'static [&'static str],
}

/// Newest first. Add the entry for a version before bumping it.
pub static ENTRIES: &[Entry] = &[
    Entry {
        version: "5.6.2",
        id: &[
            "Tanda tangan ketik: pilih satu dari empat gaya tulisan tangan.",
            "Tanda tangan dan paraf yang sudah ditempel bisa diganti hitam, biru, atau merah.",
            "Tombol Ulangi di tab Gambar kini menghapus coretan, tidak membuat titik.",
            "Notch kembali ke paling atas bila tertutup jendela aplikasi lain.",
        ],
        en: &[
            "Typed signature: pick one of four handwriting styles.",
            "A placed signature or initials can switch to black, blue or red.",
            "Start over in the Draw tab now clears the drawing instead of adding a dot.",
            "The notch comes back on top when another app's window covers it.",
        ],
    },
    Entry {
        version: "5.6.1",
        id: &[
            "Falk Abu-abu kini Kuning: kuning seluruhnya, tanpa pita abu-abu di bawah pipi.",
            "Pilihanmu tetap tersimpan: falk Abu-abu otomatis menjadi Kuning.",
        ],
        en: &[
            "Falk's Grey coat is now Yellow: yellow all over, no grey band under the cheeks.",
            "Your pick is kept: a Grey falk becomes Yellow on its own.",
        ],
    },
    Entry {
        version: "5.6.0",
        id: &[
            "Tanda tangani dokumen: klik kanan kartu shelf → Tanda tangani (PDF, gambar, Office).",
            "Tambahkan tanda tangan, paraf, tanggal, teks atau centang. Hasilnya file baru.",
            "Teks bisa diberi font, tebal, miring, garis bawah dan warna (Ctrl+B, I, U).",
            "Screenshot bisa diputar di editor: tombol putar atau Ctrl+R.",
            "Mode Langsung di layar kini memutar potongan di tempat, tanpa membuka editor.",
        ],
        en: &[
            "Sign documents: right-click a shelf card → Sign (PDF, images, Office files).",
            "Add a signature, initials, date, text or check. You get a new file next to it.",
            "Text can take a font, bold, italic, underline and colour (Ctrl+B, I, U).",
            "Screenshots can turn in the editor: the rotate buttons or Ctrl+R.",
            "On screen mode now turns the picked region in place, without opening the editor.",
        ],
    },
    Entry {
        version: "5.5.7",
        id: &[
            "Transkrip tidak lagi berisi tanda seru saja: hasil GPU yang rusak diulang di CPU.",
            "Notch Jadi muka: mata maskot tidak lagi gepeng saat Claude bekerja.",
            "Notifikasi Claude kini kecil di samping lirik, tidak lagi menutupi barisnya.",
            "Lirik yang terlambat (contoh: Heaven, Bryan Adams) kini pas, diambil ulang sekali.",
            "Lirik di notch Jadi muka dengan Headphone tidak lagi terpotong.",
        ],
        en: &[
            "Transcripts no longer come out as only exclamation marks: a broken GPU run retries on CPU.",
            "Notch Face: the mascot's eyes are no longer flat while Claude works.",
            "A Claude notice is now small beside the lyric, so it no longer covers the line.",
            "Late lyrics (for example Heaven by Bryan Adams) are now in time, fetched once more.",
            "Lyrics on the Face notch with the Headphone are no longer cut off.",
        ],
    },
    Entry {
        version: "5.5.6",
        id: &[
            "Maskot bisa bermain tiap jam bulat: Pengaturan → Maskot → Tiap jam.",
            "Tombol info di Pengaturan → Maskot menjelaskan tiap pilihan animasi otomatis.",
        ],
        en: &[
            "The mascot can play on the hour: Settings → Mascot → On the hour.",
            "The info button in Settings → Mascot explains each automatic animation choice.",
        ],
    },
    Entry {
        version: "5.5.5",
        id: &[
            "Notch kini punya warna sendiri: Pengaturan → Tampilan → Warna notch.",
            "Polos tetap gelap seperti bezel. Gradasi memudar ke warna maskot.",
            "Jadi muka mengubah notch jadi wajah maskot, lengkap dengan aksesorinya.",
        ],
        en: &[
            "The notch can have its own colour: Settings → Appearance → Notch colour.",
            "Plain stays dark like the bezel. Gradient fades into your mascot's colour.",
            "Face turns the notch into your mascot's face, accessories included.",
        ],
    },
    Entry {
        version: "5.5.2",
        id: &[
            "Shelf: salin link HP dari tampilan QR, lalu buka di browser laptop lain.",
            "Di halaman itu, file bisa langsung ditarik dan dilepas untuk dikirim.",
            "Laptop yang tersambung kini disebut Laptop, bukan HP.",
        ],
        en: &[
            "Shelf: copy the phone link from the QR view and open it in another laptop's browser.",
            "On that page you can drag and drop files to send them.",
            "A connected laptop is now called Laptop, not phone.",
        ],
    },
    Entry {
        version: "5.5.1",
        id: &[
            "Cerita maskot tidak berhenti lagi saat mouse digeser, hanya saat notch diklik.",
            "Mengecil jadi mata kini memakai wajah maskot pilihanmu: kucing, falk, atau gurita.",
        ],
        en: &[
            "Mascot stories no longer stop when the mouse moves, only when you click the notch.",
            "Shrink to eyes now shows the face of the mascot you picked: cat, cockatiel or octopus.",
        ],
    },
    Entry {
        version: "5.5.0",
        id: &[
            "Maskot kini memainkan cerita pendek di sekitar notch: sepuluh cerita untuk tiga maskot.",
            "Klik kanan notch → Mainkan maskot. Seberapa sering: Pengaturan → Maskot.",
            "Shelf: kartu gambar punya gambar kecil, klik kanan → Pratinjau gambar.",
            "Clipboard: tombol Hapus semua mengosongkan riwayat, item yang di-pin tetap ada.",
            "Klik kanan di notch tidak lagi menutup panel yang sedang terbuka.",
        ],
        en: &[
            "The mascot now plays short stories around the notch: ten stories for three mascots.",
            "Right-click the notch → Play the mascot. How often: Settings → Mascot.",
            "Shelf: image cards show a small picture; right-click → Preview image.",
            "Clipboard: a Clear all button empties the history; pinned items stay.",
            "A right-click on the notch no longer closes the open panel.",
        ],
    },
    Entry {
        version: "5.3.1",
        id: &[
            "Pindah ke PC lain: ekspor dan impor pengaturan di Pengaturan → Tentang.",
            "Link HP bisa lewat Tailscale: pilih Wi-Fi atau Tailscale di tampilan QR.",
        ],
        en: &[
            "Move to another PC: export and import your settings in Settings → About.",
            "The phone link can run over Tailscale: pick Wi-Fi or Tailscale on the QR view.",
        ],
    },
];

pub fn points(entry: &Entry, lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Id => entry.id,
        Lang::En => entry.en,
    }
}

/// The entries newer than `seen` up to `current`, newest first. When `seen`
/// is not a version (empty, or edited by hand) only `current`'s own entry.
pub fn since(seen: &str, current: &str) -> Vec<&'static Entry> {
    since_in(ENTRIES, seen, current)
}

fn since_in<'a>(entries: &'a [Entry], seen: &str, current: &str) -> Vec<&'a Entry> {
    let Some(current) = parse_version(current) else {
        return Vec::new();
    };
    match parse_version(seen) {
        Some(seen) => entries
            .iter()
            .filter(|e| matches!(parse_version(e.version), Some(v) if v > seen && v <= current))
            .collect(),
        None => entries
            .iter()
            .filter(|e| parse_version(e.version) == Some(current))
            .collect(),
    }
}

/// The newest `n` entries (the Settings card).
pub fn latest(n: usize) -> &'static [Entry] {
    &ENTRIES[..n.min(ENTRIES.len())]
}

/// What a start does about What's new.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartAction {
    /// Nothing: the version was already seen.
    None,
    /// Remember the running version in memory, show nothing (a fresh install
    /// or a downgrade). The next save writes it.
    Store,
    /// Remember it, write it now and show the notice.
    Show,
}

impl StartAction {
    /// Whether the start writes `config.json` itself. Only a shown notice
    /// must not repeat; a write for the others would move a hand-edited file
    /// that failed to parse aside at launch.
    pub fn saves_now(self) -> bool {
        self == StartAction::Show
    }
}

/// `fresh`: the first run, before onboarding ended. A `seen` that is not a
/// version on an existing install is an upgrade from a release without this
/// feature.
pub fn start_action(seen: &str, current: &str, fresh: bool) -> StartAction {
    let Some(now) = parse_version(current) else {
        return StartAction::None;
    };
    match parse_version(seen) {
        Some(was) if was == now => StartAction::None,
        Some(was) if was > now => StartAction::Store,
        Some(_) => StartAction::Show,
        Option::None if seen.is_empty() && fresh => StartAction::Store,
        Option::None => StartAction::Show,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn entry(version: &'static str) -> Entry {
        Entry {
            version,
            id: &["x"],
            en: &["x"],
        }
    }

    static FIXTURE: [Entry; 4] = [entry("5.3.4"), entry("5.3.3"), entry("5.3.2"), entry("5.3.1")];
    static GAP: [Entry; 2] = [entry("5.3.4"), entry("5.3.2")];

    fn versions(found: Vec<&Entry>) -> Vec<&str> {
        found.into_iter().map(|e| e.version).collect()
    }

    #[test]
    fn table_has_an_entry_for_the_running_version() {
        assert!(
            ENTRIES.iter().any(|e| e.version == env!("CARGO_PKG_VERSION")),
            "write the What's new entry in src/whats_new.rs before bumping the version"
        );
    }

    #[test]
    fn table_is_well_formed() {
        let mut last = None;
        for e in ENTRIES {
            let v = crate::update::parse_version(e.version)
                .unwrap_or_else(|| panic!("{} does not parse", e.version));
            if let Some(last) = last {
                assert!(v < last, "{} is not older than the entry before it", e.version);
            }
            last = Some(v);
            for points in [e.id, e.en] {
                assert!((1..=5).contains(&points.len()), "{}: 1-5 points", e.version);
                for p in points {
                    assert!(!p.trim().is_empty(), "{}: empty point", e.version);
                    assert!(p.chars().count() <= 90, "{}: point too long: {p}", e.version);
                }
            }
        }
    }

    #[test]
    fn since_lists_the_range_newest_first() {
        assert_eq!(
            versions(since_in(&FIXTURE, "5.3.1", "5.3.4")),
            ["5.3.4", "5.3.3", "5.3.2"]
        );
        assert!(since_in(&FIXTURE, "5.3.4", "5.3.4").is_empty());
        assert_eq!(versions(since_in(&GAP, "5.3.1", "5.3.4")), ["5.3.4", "5.3.2"]);
        assert!(since_in(&GAP, "5.3.4", "5.4.0").is_empty());
    }

    #[test]
    fn since_with_unknown_seen_gives_only_the_current_entry() {
        for seen in ["", "abc", "5.3"] {
            assert_eq!(versions(since_in(&FIXTURE, seen, "5.3.3")), ["5.3.3"], "{seen:?}");
            assert!(since_in(&GAP, seen, "5.3.3").is_empty(), "{seen:?}");
        }
    }

    #[test]
    fn since_ahead_of_current_is_empty() {
        assert!(since_in(&FIXTURE, "6.0.0", "5.3.4").is_empty());
    }

    #[test]
    fn start_action_rules() {
        use StartAction::{None, Show, Store};
        let cases = [
            ("5.3.1", "5.3.1", false, None),
            // A fresh install that quit before onboarding ended: stays silent.
            ("5.3.1", "5.3.1", true, None),
            ("", "5.3.1", true, Store),
            ("", "5.3.1", false, Show),
            ("abc", "5.3.1", false, Show),
            ("5.3.0", "5.3.1", false, Show),
            ("5.3.0", "5.3.1", true, Show),
            ("6.0.0", "5.3.1", false, Store),
            ("5.3.0", "not-a-version", false, None),
        ];
        for (seen, current, fresh, want) in cases {
            assert_eq!(start_action(seen, current, fresh), want, "{seen:?} {current:?} {fresh}");
        }
    }

    #[test]
    fn only_show_writes_the_config_at_start() {
        // A write at start would move a hand-edited config.json that failed
        // to parse aside; a fresh install or a downgrade waits for the next save.
        assert!(StartAction::Show.saves_now());
        assert!(!StartAction::Store.saves_now());
        assert!(!StartAction::None.saves_now());
    }

    #[test]
    fn latest_clamps() {
        assert!(latest(0).is_empty());
        assert_eq!(latest(99).len(), ENTRIES.len());
    }
}
