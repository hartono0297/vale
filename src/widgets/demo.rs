//! `--demo-widgets` (debug builds): the desktop widgets on their own, with a
//! made-up notes file in the temp folder (never `%APPDATA%`, never the real
//! config). It seeds eleven notes, the first time or with the argument
//! `reset`: one per paper colour (the Gelap one pinned), a half-done checklist
//! (with its progress bar), an all-done one, a rolled-up one, and the v2 things
//! (a reminder chip, an overdue reminder with the clock cat, Tangan and Mono,
//! each ruling, tape and a pin, the stamps, a 60 % see-through note). After
//! that the file keeps what was typed, moved and ticked, so a restart shows it
//! back.
//!
//! Everything is live: type, tick, colour, drag, resize, roll, pin, the +, the
//! x and delete. The widget settings come from the environment
//! (`DI_WIDGETS_LAYER=top`, `DI_WIDGETS_LOCK=1`, `DI_WIDGETS_PEEK=0`,
//! `DI_WIDGETS_TILT=1` for "Miring sedikit"), the
//! theme from `DI_ACCENT`, `DI_SURFACE`, `DI_WINDOW`, the font from `DI_FONT`
//! and the language from `DI_LANG`. The scene ends when its last note is gone
//! (or with Ctrl+C in the console).

use std::cell::RefCell;
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
use crate::widgets::apply::{apply, set_reminder, sticky_for};
use crate::widgets::host::{Ctx, WM_APP_WIDGET, WidgetHost};
use crate::widgets::prefs::{WidgetLayer, WidgetPrefs};
use crate::widgets::remind;
use crate::widgets::sticky::{Deco, NoteFont, Paper, Pattern, Stamp, Sticky};
use crate::widgets::templates::{self, Template};

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

/// The first run, or `reset`: the notes the scene shows. Eleven notes: every
/// paper, a checklist with its progress bar (2 of 4), an all-done one, a
/// rolled one, a pinned Gelap one, and the v2 things: a reminder chip in the
/// future, an overdue one (the cat holds a clock), Tangan and Mono, each ruling
/// (Garis, Kotak, Titik), tape and a pin, every stamp but two, and a note at
/// 60 % see-through.
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
    let count = notes.len() as i64;
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
}

/// `reset`: start from the seed again.
pub fn run(reset: bool) {
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
        });
    });
    with_demo(Demo::sync);
    with_demo(Demo::remind_fire);
    println!("widgets-demo: notes in {}", store_path().display());
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    with_demo(|d| {
        let _ = d.store.save();
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
        if host.count() == 0 {
            unsafe { PostQuitMessage(0) };
        }
        self.remind_arm();
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
        let events = self.host.drain();
        let out = apply(&mut self.store, events, Local::now().timestamp());
        if let Some(on) = out.cat_peek {
            self.prefs.cat_peek = on;
        }
        for (id, template) in out.beside {
            self.make(Some(&id), template);
        }
        if out.structural {
            self.sync();
        } else if out.reminders {
            self.remind_arm();
        }
        if out.changed {
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
            with_demo(|d| {
                let _ = d.store.save();
            });
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
