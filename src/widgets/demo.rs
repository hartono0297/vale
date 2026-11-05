//! `--demo-widgets` (debug builds): the desktop widgets on their own, with a
//! made-up notes file in the temp folder (never `%APPDATA%`, never the real
//! config). It seeds eighteen notes, the first time or with the argument
//! `reset`: one per paper colour (the Gelap one pinned), a half-done checklist
//! (with its progress bar), an all-done one, a rolled-up one, and the v2 things
//! (a reminder chip, an overdue reminder with the clock cat, Tangan and Mono,
//! each ruling, tape and a pin, the stamps, a 60 % see-through note), one as
//! "Isi dari clipboard" would start it (from a fixed sample string, never the
//! real clipboard), a stack of three sheets ("Proyek Dago") and a note tucked
//! to the right edge of the notch's monitor (hover its tab), one formatted like
//! mockup B (bold, italic, underline, strike, two highlights, red text, bullets
//! and a checklist) and one with two images (gradients made up here and written
//! to the demo's own `note-images` folder). After that the file keeps what was
//! typed, moved and ticked, so a restart shows it back.
//!
//! `DI_WIDGETS_BAR=1|marker|more` (debug only) draws the format bar on the
//! formatted note WITHOUT giving it the keyboard or the foreground, for a
//! look at the drawing; `marker` and `more` also show the popover or the
//! overflow menu (a 200 dip wide note) with `SW_SHOWNA`.
//!
//! Everything is live: type, tick, colour, drag, resize, roll, pin, the +, the
//! x and delete. The widget settings come from the environment
//! (`DI_WIDGETS_LAYER=top`, `DI_WIDGETS_LOCK=1`, `DI_WIDGETS_PEEK=0`,
//! `DI_WIDGETS_TILT=1` for "Miring sedikit"), the
//! theme from `DI_ACCENT`, `DI_SURFACE`, `DI_WINDOW`, the font from `DI_FONT`
//! and the language from `DI_LANG`. The scene ends when its last note is gone
//! (or with Ctrl+C in the console).

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use chrono::Local;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MSG,
    PostQuitMessage, RegisterClassExW, SetTimer, TranslateMessage, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::w;

use crate::arrange_win;
use crate::config::{NotchMonitor, UiFont};
use crate::i18n;
use crate::notch_monitor::Display;
use crate::notes::NotesStore;
use crate::theme::{self, CatColor};
use crate::ui::frame_clock::{FrameClock, WM_APP_FRAME};
use crate::ui::tray;
use crate::widgets::apply::{Outcome, apply, set_reminder, settle, stack_out, sticky_for};
use crate::widgets::cliptext;
use crate::widgets::host::{Ctx, WM_APP_WIDGET, WidgetEvent, WidgetHost};
use crate::widgets::styles::{Fg, Hl, Run, Style};
use crate::widgets::list::{self, Undo};
use crate::widgets::list_win::{self, Anchor, ListData, ListEvent};
use crate::widgets::prefs::{WidgetLayer, WidgetPrefs};
use crate::widgets::remind;
use crate::widgets::sticky::{Deco, DipRect, NoteFont, Paper, Pattern, Stamp, Sticky};
use crate::widgets::templates::{self, Template};
use crate::widgets::{place, tuck};

/// The save timer of this scene (the island's own is 45 too).
const SAVE_TIMER: usize = 45;
const SAVE_MS: u32 = 1500;
/// The reminders' one-shot, the island's `TIMER_STICKY_REMIND` (47). The demo
/// has no island: a fire raises the note and shakes it, and shows no notice.
const REMIND_TIMER: usize = 47;

struct Demo {
    owner: HWND,
    store: NotesStore,
    host: WidgetHost,
    prefs: WidgetPrefs,
    displays: Vec<Display>,
    clock: Option<FrameClock>,
    font: UiFont,
    /// What the list's last delete took away (`--demo-note-list`).
    undo: Option<Undo>,
    /// The image files some note referenced since the last save, as the
    /// island keeps them (`App::widgets_images_release`).
    known_images: HashSet<String>,
}

thread_local! {
    static DEMO: RefCell<Option<Demo>> = const { RefCell::new(None) };
}

fn with_demo<R>(f: impl FnOnce(&mut Demo) -> R) -> Option<R> {
    DEMO.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn prefs_from_env() -> WidgetPrefs {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let mut p = WidgetPrefs::default();
    if var("DI_WIDGETS_LAYER").eq_ignore_ascii_case("top") {
        p.layer = WidgetLayer::Top;
    }
    p.lock = var("DI_WIDGETS_LOCK") == "1";
    p.cat_peek = var("DI_WIDGETS_PEEK") != "0";
    p.tilt = var("DI_WIDGETS_TILT") == "1";
    p
}

fn store_path() -> PathBuf {
    std::env::temp_dir()
        .join("vale-widgets-demo")
        .join("notes.json")
}

/// One seeded note: its paper, text, whether it is rolled and pinned, and
/// what else it shows (a reminder, a font, a ruling, tape, a stamp, ...).
struct Seed {
    paper: Paper,
    text: &'static str,
    rolled: bool,
    pinned: bool,
    more: fn(&mut Sticky, i64),
}

fn nothing(_: &mut Sticky, _: i64) {}

/// What a copied address would make of itself in a new note ("Isi dari
/// clipboard"): a fixed sample, with the line breaks, trailing blank lines and
/// spaces a real copy brings, run through the real clean-up. The demo never
/// reads or writes the clipboard.
const CLIP_SAMPLE: &str = "Jl. Kenanga No. 12, Bandung 40115\r\nKirim sebelum Jumat\r\n\r\n\r\n\r\n  ";

fn clip_sample_text() -> String {
    cliptext::prefill(CLIP_SAMPLE).unwrap_or_default()
}

/// The first run, or `reset`: the notes the scene shows. Twelve notes: every
/// paper, a checklist with its progress bar (2 of 4), an all-done one, a
/// rolled one, a pinned Gelap one, and the v2 things: a reminder chip in the
/// future, an overdue one (the cat holds a clock), Tangan and Mono, each ruling
/// (Garis, Kotak, Titik), tape and a pin, every stamp but two, and a note at
/// 60 % see-through, and the one a clipboard prefill makes.
fn seed(store: &mut NotesStore, displays: &[Display]) {
    let now = Local::now().timestamp();
    let notes: [Seed; 11] = [
        Seed {
            paper: Paper::Krem,
            text: "Belanja material\n- [x] Bata ekspos 2.000 bh\n- [x] Terakota lantai 30x30\n- [ ] Kaca low-e 6 mm\n- [ ] Baja ringan C75",
            rolled: false,
            pinned: false,
            more: |s, _| {
                s.stamp = Stamp::Penting;
                s.pattern = Pattern::Garis;
            },
        },
        Seed {
            paper: Paper::Terakota,
            text: "Telp Pak Budi\nJam 3 sore soal revisi atap. Tanya juga jadwal pengecoran lantai 2.",
            rolled: false,
            pinned: false,
            // Overdue: it went off half an hour ago and nobody opened it.
            more: |s, now| {
                s.remind_at = Some(now - 1800);
                s.remind_fired = true;
                s.stamp = Stamp::Mendesak;
                s.touched = now + 5;
            },
        },
        Seed {
            paper: Paper::Plum,
            text: "Ide fasad\nKisi-kisi kayu vertikal, jarak 8 cm, bayangan sore jatuh ke ruang tamu.",
            rolled: false,
            pinned: false,
            more: |s, _| {
                s.stamp = Stamp::Ide;
                s.opacity = 60;
            },
        },
        Seed {
            paper: Paper::Laut,
            text: "Catatan rapat\nSenin\nCek gambar kerja\nRevisi RAB\nJadwal survei\nKirim notulen",
            rolled: true,
            pinned: false,
            more: nothing,
        },
        Seed {
            paper: Paper::Gelap,
            text: "Transfer DP\nJangan lupa transfer DP sebelum jam 4.",
            rolled: false,
            pinned: true,
            more: |s, _| s.stamp = Stamp::Nanti,
        },
        Seed {
            paper: Paper::Sage,
            text: "Sebelum rilis\n- [x] Tes di monitor kedua\n- [x] Cek mode hemat daya\n- [x] Update README",
            rolled: false,
            pinned: false,
            more: |s, _| s.stamp = Stamp::Beres,
        },
        // A reminder later today (or tomorrow), with a ruled page.
        Seed {
            paper: Paper::Krem,
            text: "Kirim revisi denah\nKe klien sebelum jam 5.",
            rolled: false,
            pinned: false,
            more: |s, now| {
                s.remind_at = Some(now + 3 * 3600);
                s.pattern = Pattern::Garis;
                s.deco = Deco::Selotip;
            },
        },
        // Handwriting, a grid and tape.
        Seed {
            paper: Paper::Sage,
            text: "Tulisan tangan\nCatatan ini pakai huruf Tangan, di atas kertas kotak.",
            rolled: false,
            pinned: false,
            more: |s, _| {
                s.font = NoteFont::Tangan;
                s.pattern = Pattern::Kotak;
                s.deco = Deco::Selotip;
            },
        },
        // Mono, dots and a pin.
        Seed {
            paper: Paper::Laut,
            text: "cargo test --locked\nHuruf Mono di atas kertas titik, dengan paku.",
            rolled: false,
            pinned: false,
            more: |s, _| {
                s.font = NoteFont::Mono;
                s.pattern = Pattern::Titik;
                s.deco = Deco::Paku;
            },
        },
        // See-through, with a heart.
        Seed {
            paper: Paper::Plum,
            text: "Tembus pandang\nCatatan ini 60 persen sampai kursor datang.",
            rolled: false,
            pinned: false,
            more: |s, _| {
                s.opacity = 60;
                s.stamp = Stamp::Suka;
            },
        },
        Seed {
            paper: Paper::Terakota,
            text: "Besok\n- [ ] Survei lokasi\n- [ ] Foto kondisi eksisting\n- [ ] Ukur ulang bentang",
            rolled: false,
            pinned: false,
            more: |s, _| s.pattern = Pattern::Kotak,
        },
    ];
    let count = notes.len() as i64 + 1;
    for (i, n) in notes.into_iter().enumerate() {
        let at = now - 60 * (count - i as i64);
        let base = sticky_for(store.list(), displays, &NotchMonitor::Primary, None, at);
        let mut sticky = Sticky {
            paper: n.paper,
            rolled: n.rolled,
            pinned: n.pinned,
            ..base
        };
        (n.more)(&mut sticky, now);
        if let Some(id) = store.create_sticky(sticky, at) {
            store.set_text(&id, n.text, at);
        }
    }
    // The clipboard prefill's note: the first line is its title.
    let at = now - 40;
    let base = sticky_for(store.list(), displays, &NotchMonitor::Primary, None, at);
    if let Some(id) = store.create_sticky(Sticky { paper: Paper::Krem, ..base }, at) {
        store.set_text(&id, &clip_sample_text(), at);
    }
    seed_stack_and_tuck(store, displays, now);
    seed_format_notes(store, displays, now);
}

/// The utf-16 span of `needle` in `text` (its first occurrence).
fn span(text: &str, needle: &str) -> (usize, usize) {
    let at = text.find(needle).unwrap_or(0);
    let s = text[..at].encode_utf16().count();
    (s, s + needle.encode_utf16().count())
}

/// Two more notes: one formatted like mockup B, one with two images. The
/// images are gradients made here, written to the demo's own folder.
fn seed_format_notes(store: &mut NotesStore, displays: &[Display], now: i64) {
    let text = "Trip Bandung\nBerangkat Sabtu 06.00, kumpul di stasiun, jangan telat.\n\u{2022} Bawa jaket tebal\n\u{2022} Charger dan power bank\n- [x] Tiket kereta\n- [ ] Booking hotel";
    let style = |b, i, u, x, hl, fg| Style { b, i, u, x, hl, fg };
    let marks = [
        ("Trip Bandung", style(true, false, false, false, None, None)),
        ("06.00", style(true, false, false, false, Some(Hl::Kuning), None)),
        ("kumpul di stasiun", style(false, true, false, false, None, None)),
        ("jangan telat", style(false, false, true, false, None, Some(Fg::Merah))),
        ("jaket tebal", style(false, false, false, true, None, None)),
        ("power bank", style(false, false, false, false, Some(Hl::Pink), None)),
    ];
    let runs: Vec<Run> = marks
        .iter()
        .map(|(needle, style)| {
            let (s, e) = span(text, needle);
            Run { s, e, style: *style }
        })
        .collect();
    let at = now - 50;
    let base = sticky_for(store.list(), displays, &NotchMonitor::Primary, None, at);
    let sticky = Sticky {
        paper: Paper::Krem,
        ..base
    };
    if let Some(id) = store.create_sticky(sticky, at) {
        store.set_text(&id, text, at);
        // The runs go in after the text: they are fitted to the text there is.
        if let Some(mut s) = store.get(&id).and_then(|n| n.sticky.clone()) {
            s.styles = runs;
            store.set_sticky(&id, Some(s));
        }
    }
    // Two generated images on a Gelap note.
    let dir = store.images_dir();
    let names = ["demo-1.png", "demo-2.png"];
    let mut have = Vec::new();
    for (k, name) in names.iter().enumerate() {
        if crate::widgets::image_io::write_gradient(&dir.join(name), 320, 220, 40 * k as u8) {
            have.push((*name).to_owned());
        }
    }
    let at = now - 45;
    let base = sticky_for(store.list(), displays, &NotchMonitor::Primary, None, at);
    let sticky = Sticky {
        paper: Paper::Gelap,
        images: have,
        ..base
    };
    if let Some(id) = store.create_sticky(sticky, at) {
        store.set_text(&id, "Bug login\nMuncul di Android 14 saja", at);
    }
}

/// A stack of three sheets on different papers ("Proyek Dago", sheet 1
/// shown: flip it with the wheel over its strip or the arrows, drag another
/// note onto it with Shift held to add one) and a note tucked to the right
/// edge of the notch's monitor (hover its tab).
fn seed_stack_and_tuck(store: &mut NotesStore, displays: &[Display], now: i64) {
    let base = sticky_for(store.list(), displays, &NotchMonitor::Primary, None, now);
    let sheets = [
        (
            Paper::Krem,
            "Proyek Dago
Revisi fasad, cek ulang RAB
- [x] Gambar fasad
- [ ] Hitung ulang RAB",
        ),
        (Paper::Laut, "Dago: struktur
Kolom lantai 2 pakai K-300, cek sengkang."),
        (Paper::Plum, "Dago: finishing
Pilih cat eksterior dan contoh bata ekspos."),
    ];
    for (k, (paper, text)) in sheets.into_iter().enumerate() {
        let at = now - 30 + k as i64;
        let sticky = Sticky {
            paper,
            rect: DipRect {
                w: 280.0,
                ..base.rect
            },
            stack: Some("stk-dago".into()),
            stack_order: k as u32,
            stack_top: k == 0,
            ..base.clone()
        };
        if let Some(id) = store.create_sticky(sticky, at) {
            store.set_text(&id, text, at);
        }
    }
    // Tucked right on the notch's monitor: flush with its edge, 160 dip down.
    let area = place::resolve(&base.monitor, displays)
        .and_then(|(i, _)| displays.get(i))
        .map_or(DipRect::new(0.0, 0.0, 1920.0, 1040.0), place::snap_area);
    let size = Sticky::default().rect;
    let at = DipRect::new(0.0, 160.0, size.w, size.h);
    let sticky = Sticky {
        paper: Paper::Sage,
        rect: tuck::out_rect(&at, tuck::Edge::Right, &area),
        tuck: Some(tuck::Edge::Right),
        ..base
    };
    if let Some(id) = store.create_sticky(sticky, now - 20) {
        store.set_text(&id, "Nomor penting
Pak Budi 0812-555-0147 (kontraktor).", now - 20);
    }
}

/// `reset`: start from the seed again.
pub fn run(reset: bool) {
    run_scene(reset, false);
}

/// `--demo-note-list [reset]`: the same scene, notes file and seeded notes,
/// with the sticky note list opened at its default anchor once the windows are
/// up. The owner handles the list's events itself, with the same pure
/// functions the island uses.
pub fn run_list(reset: bool) {
    run_scene(reset, true);
}

fn run_scene(reset: bool, with_list: bool) {
    theme::set_system_light(theme::read_system_light());
    theme::set_current(theme::theme_from_env());
    tray::follow_system_menu_theme();
    let path = store_path();
    if reset {
        let _ = std::fs::remove_file(&path);
    }
    let mut store = NotesStore::open(path);
    let displays = arrange_win::displays();
    if store.list().is_empty() {
        seed(&mut store, &displays);
        let _ = store.save();
    }
    // A stack shows the sheet whose reminder is overdue.
    settle(&mut store, Local::now().timestamp(), true);
    // The images in use are known; the day-old strays are cleaned, once.
    let known_images = store.referenced_images();
    crate::widgets::image_io::sweep_store(&store, Local::now().timestamp());
    let Some(owner) = create_owner() else {
        eprintln!("widgets-demo: cannot create the owner window");
        return;
    };
    let font = UiFont::from_name(&std::env::var("DI_FONT").unwrap_or_default()).unwrap_or_default();
    let host = WidgetHost::new(owner);
    DEMO.with(|cell| {
        *cell.borrow_mut() = Some(Demo {
            owner,
            store,
            host,
            prefs: prefs_from_env(),
            displays,
            clock: None,
            font,
            undo: None,
            known_images,
        });
    });
    with_demo(Demo::sync);
    with_demo(Demo::remind_fire);
    with_demo(Demo::bar_preview);
    if with_list {
        with_demo(|d| d.open_list(None));
    }
    println!("widgets-demo: notes in {}", store_path().display());
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    with_demo(|d| {
        d.save();
        d.clock = None;
        d.host.shutdown();
    });
    DEMO.with(|cell| *cell.borrow_mut() = None);
}

impl Demo {
    fn sync(&mut self) {
        let Demo {
            store,
            host,
            prefs,
            displays,
            font,
            ..
        } = self;
        host.set_images_dir(store.images_dir());
        host.sync(&Ctx {
            notes: store.list(),
            prefs: *prefs,
            displays,
            lang: i18n::lang(),
            font: *font,
            coat: CatColor::Terracotta,
            animate: true,
        });
        crate::widgets::set_sticky_count(store.sticky_count());
        host.mood_tick(Local::now().timestamp());
        // The scene ends with its last note, unless the list is open (it may
        // still undo the delete).
        if host.count() == 0 && !list_win::is_open() {
            unsafe { PostQuitMessage(0) };
        }
        self.remind_arm();
        self.list_refresh();
    }

    // ------------------------------------------------------------ the list

    fn list_data(&self) -> ListData {
        let room = crate::widgets::sticky::can_add(self.store.sticky_count());
        ListData {
            notes: self.store.list().to_vec(),
            lang: i18n::lang(),
            ui: self.font,
            can_add: room,
            can_undo: self.undo.is_some() && room,
        }
    }

    fn list_refresh(&self) {
        if list_win::is_open() {
            list_win::refresh(self.list_data());
        }
    }

    /// Opens the list beside `beside`, else at the top right of the notch's
    /// monitor.
    fn open_list(&mut self, beside: Option<&str>) {
        let anchor = match beside.and_then(|id| self.host.note_rect_px(id)) {
            Some(note_px) => Anchor::Beside { note_px },
            None => {
                let work = crate::notch_monitor::resolve(&NotchMonitor::Primary, &self.displays)
                    .and_then(|i| self.displays.get(i))
                    .map_or(crate::capture::Rect::new(0, 0, 1920, 1040), |d| d.monitor.work);
                Anchor::Default { work_px: work }
            }
        };
        list_win::open(self.owner, anchor, self.list_data());
    }

    /// Writes the notes, then deletes the image files nothing refers to (and
    /// the list's undo does not hold): the island's rule with the demo's store.
    fn save(&mut self) {
        if self.store.save().is_ok() {
            self.release_images();
        }
    }

    /// The files that were referenced and are not now go from the folder.
    fn release_images(&mut self) {
        let mut keep = self.store.referenced_images();
        if let Some(undo) = &self.undo {
            keep.extend(undo.images());
        }
        let gone = crate::widgets::images::to_release(&self.known_images, &keep);
        crate::widgets::image_io::delete_named(&self.store.images_dir(), &gone);
        self.known_images = keep;
    }

    /// `DI_WIDGETS_BAR` (debug builds): the bar on the formatted note without
    /// the keyboard or the foreground; `marker` and `more` also show the popup.
    fn bar_preview(&mut self) {
        let mode = std::env::var("DI_WIDGETS_BAR").unwrap_or_default();
        if mode.is_empty() || mode == "0" {
            return;
        }
        let id = self
            .store
            .list()
            .iter()
            .find(|n| n.sticky.as_ref().is_some_and(|s| !s.styles.is_empty()))
            .map(|n| n.id.clone());
        let Some(id) = id else { return };
        if mode == "more" {
            // A narrow note, so some tools move into the menu.
            if let Some(mut s) = self.store.get(&id).and_then(|n| n.sticky.clone()) {
                s.rect.w = 200.0;
                self.store.set_sticky(&id, Some(s));
                self.sync();
            }
        }
        self.host.preview_bar(&id, &mode);
    }

    fn save_soon(&self) {
        unsafe {
            SetTimer(Some(self.owner), SAVE_TIMER, SAVE_MS, None);
        }
    }

    /// What the user did in the list, applied with the same pure rules as the
    /// island (`list::show_events`, `Undo`) and the host's `bring_front`.
    fn list_events(&mut self) {
        let now = Local::now().timestamp();
        for ev in list_win::drain() {
            match ev {
                ListEvent::Show(id) => {
                    let events = list::show_events(self.store.list(), &id);
                    if !events.is_empty() {
                        apply_known(&mut self.store, &mut self.known_images, events, now);
                    }
                    self.sync();
                    self.host.bring_front(&id);
                    list_win::close();
                    self.save_soon();
                }
                ListEvent::Delete(id) => {
                    self.undo = Undo::capture(self.store.list(), &id);
                    let out = apply_known(
                        &mut self.store,
                        &mut self.known_images,
                        vec![WidgetEvent::Delete { id }],
                        now,
                    );
                    if out.structural {
                        self.sync();
                    }
                    self.list_refresh();
                    self.save_soon();
                }
                ListEvent::Undo => {
                    if let Some(undo) = self.undo.clone()
                        && undo.restore(&mut self.store)
                    {
                        self.undo = None;
                        settle(&mut self.store, now, false);
                        self.sync();
                        self.save_soon();
                    }
                }
                ListEvent::New => self.make(None, Template::Kosong),
                ListEvent::Closed | ListEvent::UndoExpired => {
                    self.undo = None;
                    self.release_images();
                }
            }
        }
    }

    /// The one-shot for the nearest unfired reminder.
    fn remind_arm(&self) {
        unsafe {
            let _ = KillTimer(Some(self.owner), REMIND_TIMER);
            if let Some(at) = remind::nearest(self.store.list()) {
                let wait = remind::wait_ms(at, Local::now().timestamp());
                SetTimer(Some(self.owner), REMIND_TIMER, wait, None);
            }
        }
    }

    /// The reminders that are due fire (at startup also the missed ones): the
    /// note is raised and shakes; no notice, there is no island.
    fn remind_fire(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.owner), REMIND_TIMER);
        }
        let due = remind::due(self.store.list(), Local::now().timestamp());
        for id in &due {
            let at = self
                .store
                .get(id)
                .and_then(|n| n.sticky.as_ref())
                .and_then(|s| s.remind_at);
            set_reminder(&mut self.store, id, at, true);
        }
        if !due.is_empty() {
            self.sync();
            self.host.raise(&due);
            unsafe {
                SetTimer(Some(self.owner), SAVE_TIMER, SAVE_MS, None);
            }
        }
        self.remind_arm();
    }

    fn events(&mut self) {
        self.list_events();
        let events = self.host.drain();
        let out = apply_known(&mut self.store, &mut self.known_images, events, Local::now().timestamp());
        if let Some(on) = out.cat_peek {
            self.prefs.cat_peek = on;
        }
        if let Some(id) = &out.open_list {
            self.open_list(Some(id));
        }
        // The demo has no island and no phone link: the choice is only said.
        if let Some(id) = &out.send_phone {
            println!("demo-widgets: send to phone ignored (note {id}): no phone link here");
        }
        for (id, template) in out.beside {
            self.make(Some(&id), template);
        }
        let mut changed = out.changed;
        for id in &out.unstack {
            changed |= stack_out(&mut self.store, &self.displays, id);
        }
        if out.structural {
            self.sync();
        } else if out.reminders {
            self.remind_arm();
        }
        if changed {
            unsafe {
                SetTimer(Some(self.owner), SAVE_TIMER, SAVE_MS, None);
            }
        }
        if out.clock && self.clock.is_none() {
            self.clock = Some(FrameClock::start(self.owner.0 as isize));
        }
    }

    /// A new empty note, with the keyboard (the + of a note's header).
    fn make(&mut self, beside: Option<&str>, template: Template) {
        let now = Local::now().timestamp();
        let sticky = sticky_for(
            self.store.list(),
            &self.displays,
            &NotchMonitor::Primary,
            beside,
            now,
        );
        if let Some(id) = self.store.create_sticky(sticky, now) {
            let start = templates::text_of(i18n::lang(), template, Local::now().date_naive());
            if !start.is_empty() {
                self.store.set_text(&id, &start, now);
            }
            self.sync();
            self.host
                .focus_at(&id, (!start.is_empty()).then(|| templates::caret_of(&start)));
            unsafe {
                SetTimer(Some(self.owner), SAVE_TIMER, SAVE_MS, None);
            }
        }
    }

    fn frame(&mut self) {
        if let Some(clock) = &self.clock {
            clock.ack();
        }
        let now = Instant::now();
        self.host.tick(now);
        if !self.host.moving(now) {
            self.clock = None;
        }
    }
}

fn create_owner() -> Option<HWND> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(owner_proc),
        hInstance: module.into(),
        lpszClassName: w!("ValeWidgetsDemoOwner"),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
        // A hidden top-level window, not a message-only one: only a top-level
        // window gets the broadcasts (`WM_DISPLAYCHANGE`, `TaskbarCreated`), so
        // the demo follows a resolution change and an Explorer restart like the
        // island does. No parent, never shown, a tool window: no taskbar button
        // and no Alt+Tab entry. It is only the event and timer target; the
        // notes' own owner is still the shell window (`GWLP_HWNDPARENT`).
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("ValeWidgetsDemoOwner"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .ok()
}

unsafe extern "system" fn owner_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_WIDGET => {
            with_demo(Demo::events);
            return LRESULT(0);
        }
        WM_APP_FRAME => {
            with_demo(Demo::frame);
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == SAVE_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), SAVE_TIMER);
            }
            with_demo(Demo::save);
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == REMIND_TIMER => {
            with_demo(Demo::remind_fire);
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE => {
            let displays = arrange_win::displays();
            with_demo(|d| {
                d.displays = displays;
                d.host.displays_changed(&d.displays);
            });
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        _ if tray::is_taskbar_created(msg) => {
            with_demo(|d| d.host.on_taskbar_created());
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// `apply`, and the images the store then refers to join the known set: an
/// image added and taken off again before any save is still known, so the
/// next save deletes its file (the island's `widgets_apply_list` does the same).
fn apply_known(
    store: &mut NotesStore,
    known: &mut HashSet<String>,
    events: Vec<WidgetEvent>,
    now: i64,
) -> Outcome {
    let out = apply(store, events, now);
    known.extend(store.referenced_images());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_added_and_removed_before_a_save_is_still_released() {
        let dir = std::env::temp_dir().join(format!("vale-demo-known-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = NotesStore::open(dir.join("notes.json"));
        let id = store
            .create_sticky(crate::widgets::sticky::Sticky::default(), 1)
            .unwrap();
        let mut known = store.referenced_images();
        assert!(known.is_empty());
        let images = |list: &[&str]| WidgetEvent::Images {
            id: id.clone(),
            images: list.iter().map(|s| (*s).to_owned()).collect(),
        };
        apply_known(&mut store, &mut known, vec![images(&["a.png"])], 2);
        apply_known(&mut store, &mut known, vec![images(&[])], 3);
        let keep = store.referenced_images();
        assert_eq!(
            crate::widgets::images::to_release(&known, &keep),
            vec!["a.png".to_owned()],
            "the next save deletes its file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_demo_seeds_eighteen_notes_with_the_clip_note_a_stack_a_tuck_and_the_two_format_notes() {
        let dir = std::env::temp_dir().join(format!("vale-demo-seed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = NotesStore::open(dir.join("notes.json"));
        seed(&mut store, &[]);
        assert_eq!(store.sticky_count(), 18, "eighteen notes");
        assert!(crate::widgets::sticky::can_add(store.sticky_count()), "under the cap of 20");
        let sample = clip_sample_text();
        assert_eq!(store.list().iter().filter(|n| n.text == sample).count(), 1, "the clip note");
        let dago: Vec<_> = store
            .list()
            .iter()
            .filter(|n| n.sticky.as_ref().is_some_and(|s| s.stack.as_deref() == Some("stk-dago")))
            .collect();
        assert_eq!(dago.len(), 3, "three sheets of 'Proyek Dago'");
        assert_eq!(
            dago.iter().filter(|n| n.sticky.as_ref().is_some_and(|s| s.stack_top)).count(),
            1,
            "one shown"
        );
        assert_eq!(
            store
                .list()
                .iter()
                .filter(|n| n.sticky.as_ref().is_some_and(|s| s.tuck.is_some()))
                .count(),
            1,
            "one tucked note"
        );
        let mut ids: Vec<&str> = store.list().iter().map(|n| n.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 18, "no two notes share an id");
        // The formatted note: runs inside its text, every kind of formatting.
        let formatted = store
            .list()
            .iter()
            .find(|n| n.sticky.as_ref().is_some_and(|s| !s.styles.is_empty()))
            .expect("the formatted note");
        let runs = &formatted.sticky.as_ref().unwrap().styles;
        let len = formatted.text.encode_utf16().count();
        assert!(runs.iter().all(|r| r.e <= len && r.s < r.e));
        assert!(runs.iter().any(|r| r.style.b) && runs.iter().any(|r| r.style.i));
        assert!(runs.iter().any(|r| r.style.u) && runs.iter().any(|r| r.style.x));
        assert!(runs.iter().any(|r| r.style.hl == Some(Hl::Kuning)));
        assert!(runs.iter().any(|r| r.style.hl == Some(Hl::Pink)));
        assert!(runs.iter().any(|r| r.style.fg == Some(Fg::Merah)));
        assert_eq!(formatted.text.matches('\u{2022}').count(), 2, "two bullets");
        assert_eq!(crate::widgets::checklist::counts(&formatted.text), (1, 2));
        // The image note: two generated files in the demo's own folder.
        let imaged = store
            .list()
            .iter()
            .find(|n| n.sticky.as_ref().is_some_and(|s| !s.images.is_empty()))
            .expect("the image note");
        let images = &imaged.sticky.as_ref().unwrap().images;
        assert_eq!(images.len(), 2);
        for name in images {
            assert!(crate::widgets::images::valid_name(name));
            assert!(store.images_dir().join(name).is_file(), "{name} was written");
        }
        assert!(store.images_dir().starts_with(&dir), "in the demo's own folder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sample_note_is_what_a_prefill_makes_of_a_copied_address() {
        assert_eq!(
            clip_sample_text(),
            "Jl. Kenanga No. 12, Bandung 40115\nKirim sebelum Jumat"
        );
    }
}
