//! `--demo-widgets` (debug builds): the desktop widgets on their own, with a
//! made-up notes file in the temp folder (never `%APPDATA%`, never the real
//! config). It seeds one note per paper colour (the Gelap one pinned), a
//! half-done checklist, an all-done one (the cat smiles), and a rolled-up one,
//! the first time or with the argument `reset`; after that the file keeps what
//! was typed, moved and ticked, so a restart shows it back.
//!
//! Everything is live: type, tick, colour, drag, resize, roll, pin, the +, the
//! x and delete. The widget settings come from the environment
//! (`DI_WIDGETS_LAYER=top`, `DI_WIDGETS_LOCK=1`, `DI_WIDGETS_PEEK=0`), the
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
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, KillTimer, MSG,
    PostQuitMessage, RegisterClassExW, SetTimer, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_DESTROY, WM_DISPLAYCHANGE, WM_TIMER, WNDCLASSEXW,
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
use crate::widgets::apply::{apply, sticky_for};
use crate::widgets::host::{Ctx, WM_APP_WIDGET, WidgetHost};
use crate::widgets::prefs::{WidgetLayer, WidgetPrefs};
use crate::widgets::sticky::{Paper, Sticky};

/// The save timer of this scene (the island's own is 45 too).
const SAVE_TIMER: usize = 45;
const SAVE_MS: u32 = 1500;

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
    p
}

fn store_path() -> PathBuf {
    std::env::temp_dir()
        .join("vale-widgets-demo")
        .join("notes.json")
}

/// The first run, or `reset`: the notes the scene shows.
fn seed(store: &mut NotesStore, displays: &[Display]) {
    let now = Local::now().timestamp();
    let notes: [(&str, Paper, &str, bool, bool); 6] = [
        (
            "Krem",
            Paper::Krem,
            "Belanja material\n- [x] Bata ekspos 2.000 bh\n- [x] Terakota lantai 30x30\n- [ ] Kaca low-e 6 mm\n- [ ] Baja ringan C75",
            false,
            false,
        ),
        (
            "Terakota",
            Paper::Terakota,
            "Telp Pak Budi\nJam 3 sore soal revisi atap. Tanya juga jadwal pengecoran lantai 2.",
            false,
            false,
        ),
        (
            "Plum",
            Paper::Plum,
            "Ide fasad\nKisi-kisi kayu vertikal, jarak 8 cm, bayangan sore jatuh ke ruang tamu.",
            false,
            false,
        ),
        (
            "Laut",
            Paper::Laut,
            "Catatan rapat\nSenin\nCek gambar kerja\nRevisi RAB\nJadwal survei\nKirim notulen",
            true,
            false,
        ),
        (
            "Gelap",
            Paper::Gelap,
            "Transfer DP\nJangan lupa transfer DP sebelum jam 4.",
            false,
            true,
        ),
        (
            "Sage",
            Paper::Sage,
            "Sebelum rilis\n- [x] Tes di monitor kedua\n- [x] Cek mode hemat daya\n- [x] Update README",
            false,
            false,
        ),
    ];
    for (i, (_, paper, text, rolled, pinned)) in notes.into_iter().enumerate() {
        let at = now - 60 * (6 - i as i64);
        let base = sticky_for(store.list(), displays, &NotchMonitor::Primary, None, at);
        let sticky = Sticky {
            paper,
            rolled,
            pinned,
            ..base
        };
        if let Some(id) = store.create_sticky(sticky, at) {
            store.set_text(&id, text, at);
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
        if host.count() == 0 {
            unsafe { PostQuitMessage(0) };
        }
    }

    fn events(&mut self) {
        let events = self.host.drain();
        let out = apply(&mut self.store, events, Local::now().timestamp());
        if let Some(on) = out.cat_peek {
            self.prefs.cat_peek = on;
        }
        for id in out.beside {
            self.make(Some(&id));
        }
        if out.structural {
            self.sync();
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
    fn make(&mut self, beside: Option<&str>) {
        let now = Local::now().timestamp();
        let sticky = sticky_for(
            self.store.list(),
            &self.displays,
            &NotchMonitor::Primary,
            beside,
            now,
        );
        if let Some(id) = self.store.create_sticky(sticky, now) {
            self.sync();
            self.host.focus(&id);
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
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ValeWidgetsDemoOwner"),
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
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
