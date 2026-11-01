//! The island's side of the Susun (Arrange) page: the template step (monitor
//! chips, template cards, saved arrangements), the pick step with its
//! overlay, the mover, the notice after it, the name field of "Simpan…" and
//! the optional hotkey. A child of `app`, so it works on `App` directly. The
//! rules and arithmetic are `arrange.rs`, the Win32 is `arrange_win.rs` and
//! `arrange_overlay.rs`, the layout is `panel_layout::arrange_layout`.
//!
//! Power: nothing runs at idle. No timer and no polling: the overlay redraws
//! when its pointer target changes, the windows are listed when a step
//! starts, the mover and the icon reader are short-lived threads.

use std::collections::HashMap;
use std::time::Instant;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

use super::{App, poster};
use crate::arrange::{
    self, ArrangeEvent, BuiltIn, Counts, LayoutRef, MAX_SAVED, Monitor, Node, OpenWindow, PickFlow,
    Placement, Rect, SavedArrangement, SavedMonitor, SavedWindow,
};
use crate::arrange_overlay::{self, MSG_BACK, MSG_CANCEL, MSG_PICK, PickSpec, TileState};
use crate::arrange_win;
use crate::clipboard::image::Thumb;
use crate::config::{ArrangeHotkey, Config};
use crate::i18n::{self, Key, Lang};
use crate::model::{ArrangeSave, Event, Notice, NoticeKind, Page};
use crate::theme;
use crate::ui::cli_input::CliInput;
use crate::ui::geometry::{ShapeKind, shape_for};
use crate::ui::panel_layout::{ARR_WHEEL_PX, ArrangeShape, arrange_max_scroll};
use crate::ui::render::{
    ArrangeCardView, ArrangeNameView, ArrangeSavedView, ArrangeView, ArrangeWindowView,
};

/// `RegisterHotKey` id of the Susun hotkey (the Clipboard's is 1, the quick
/// search's 2, Devices' 3).
pub(super) const HOTKEY_ARRANGE: i32 = 4;

/// What a "Simpan…" will store: the layout, the monitor and one entry per
/// tile, captured when the windows were picked.
#[derive(Clone, Debug)]
struct SaveInfo {
    layout: LayoutRef,
    monitor: SavedMonitor,
    tiles: Vec<Option<SavedWindow>>,
}

/// What a running move is for, to word its notice when it ends.
#[derive(Clone, Debug)]
enum Run {
    Pick {
        tiles: usize,
        info: SaveInfo,
        layout_name: String,
    },
    Saved {
        wanted: usize,
        name: String,
    },
}

/// The pick step in progress.
struct Picking {
    flow: PickFlow,
    layout: LayoutRef,
    layout_name: String,
    /// The tiles in physical px on the target monitor.
    rects: Vec<Rect>,
    monitor: Monitor,
    /// The windows listed when the step began, in z-order.
    windows: Vec<OpenWindow>,
}

/// What the Susun page keeps.
#[derive(Default)]
pub(super) struct ArrangeState {
    /// The hotkey in effect, and whether registering the chosen one failed.
    hotkey: Option<ArrangeHotkey>,
    pub hotkey_failed: bool,
    monitors: Vec<Monitor>,
    /// The chip chosen (an index into `monitors`).
    monitor: usize,
    /// The page showed at the last `sync_arrange`.
    shown: bool,
    pub scroll: f32,
    hover_card: Option<usize>,
    picking: Option<Picking>,
    icons: HashMap<String, Thumb>,
    run: u64,
    pending: Option<(u64, Run)>,
    /// Where the windows were before the last arrangement, for "Batalkan".
    undo: Vec<Placement>,
    /// What "Simpan…" would store, from the last pick.
    last_save: Option<SaveInfo>,
    /// The name field is up for this.
    naming: Option<SaveInfo>,
    /// What the name field holds, as the frame draws it.
    name_text: String,
}

/// The template cards: the built-ins, then the user's templates.
pub(super) fn cards(lang: Lang, templates: &[arrange::CustomTemplate]) -> Vec<(String, LayoutRef)> {
    BuiltIn::ALL
        .iter()
        .map(|b| {
            (
                i18n::arrange_builtin_name(lang, *b).to_owned(),
                LayoutRef::BuiltIn(*b),
            )
        })
        .chain(
            templates
                .iter()
                .map(|t| (t.name.clone(), LayoutRef::Custom(t.root.clone()))),
        )
        .collect()
}

/// The windows still free to pick: those the flow has not taken.
fn available<'a>(p: &'a Picking) -> Vec<&'a OpenWindow> {
    p.flow.available(&p.windows)
}

impl App {
    /// At startup: the hotkey.
    pub(super) fn arrange_start(&mut self) {
        self.sync_arrange_hotkey();
    }

    /// The Susun page is what the open panel shows (a notice over it does
    /// not count).
    fn arrange_page_shown(&self) -> bool {
        self.state.page() == Page::Arrange && self.state.panel_is_open() && !self.state.fullscreen()
    }

    /// The page opened: the monitors are read again, the chip is the
    /// notch's monitor and the template step shows from the top. The page
    /// leaving (or the panel closing) cancels a pick and takes the outline
    /// down.
    pub(super) fn sync_arrange(&mut self) {
        let wanted = self.arrange_page_shown();
        if wanted && !self.arrange.shown {
            self.arrange.monitors = arrange_win::monitors();
            self.arrange.monitor = self.notch_monitor();
            self.arrange.scroll = 0.0;
            self.arrange.hover_card = None;
        }
        if !wanted && self.arrange.shown {
            self.arrange_hide_outline();
            self.arrange.hover_card = None;
            if self.arrange.picking.is_some() {
                self.arrange.shown = false;
                self.cancel_pick();
            }
        }
        self.arrange.shown = wanted;
        // The name field goes with the notice it belongs to.
        if self.arrange.naming.is_some() && !self.arrange_notice_up() {
            self.end_naming();
        }
    }

    /// The Arranged notice is on screen.
    fn arrange_notice_up(&self) -> bool {
        matches!(
            self.state.view(),
            crate::model::View::Notice(Notice {
                kind: NoticeKind::Arranged { .. },
                ..
            })
        )
    }

    /// The monitor the notch is on, as an index into the monitor list.
    fn notch_monitor(&self) -> usize {
        let at = self.win.as_ref().map_or((0, 0), |w| {
            let (x, y) = w.screen_origin();
            (x + 8, y + 8)
        });
        arrange_win::monitor_index_at(&self.arrange.monitors, at)
    }

    fn arrange_monitor(&self) -> Option<&Monitor> {
        let m = &self.arrange.monitors;
        m.get(self.arrange.monitor).or_else(|| m.first())
    }

    /// The tiles of `node` in physical px on `monitor`, with the gap of
    /// Settings scaled to that monitor.
    fn arrange_rects(&self, node: &Node, monitor: &Monitor) -> Vec<Rect> {
        let gap = arrange::scaled_gap(self.config.arrange.gap, monitor.dpi);
        arrange::tile_rects(&node.to_rects(), monitor.work, gap)
    }

    fn arrange_cards(&self) -> Vec<(String, LayoutRef)> {
        cards(i18n::lang(), &self.config.arrange.templates)
    }

    // ---------------------------------------------------------- the frame

    /// What the page and its notice draw this frame (a function of the
    /// fields, so the frame can borrow them while the renderer draws).
    pub(super) fn arrange_view<'a>(
        st: &'a ArrangeState,
        cfg: &'a Config,
        input: Option<&CliInput>,
    ) -> ArrangeView<'a> {
        let picking = st.picking.as_ref();
        let windows = picking
            .map(|p| {
                available(p)
                    .into_iter()
                    .map(|w| ArrangeWindowView {
                        title: w.title.clone(),
                        key: &w.icon_key,
                        icon: st.icons.get(&w.icon_key),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let name = st.naming.as_ref().and_then(|_| {
            Some(ArrangeNameView {
                text: st.name_text.as_str(),
                selection: input?.selection(),
            })
        });
        ArrangeView {
            picking: picking.map(|p| (p.flow.current().min(p.flow.tiles() - 1), p.flow.tiles())),
            monitors: st
                .monitors
                .iter()
                .enumerate()
                .map(|(i, m)| i18n::arrange_monitor_chip(i + 1, m.rect.w, m.rect.h))
                .collect(),
            monitor: st.monitor,
            cards: cards(i18n::lang(), &cfg.arrange.templates)
                .into_iter()
                .map(|(name, layout)| ArrangeCardView {
                    name,
                    rects: layout.node().to_rects(),
                })
                .collect(),
            saved: cfg
                .arrange
                .saved
                .iter()
                .map(|s| ArrangeSavedView {
                    name: s.name.clone(),
                    rects: s.layout.node().to_rects(),
                    apps: s.app_names().join(" \u{b7} "),
                })
                .collect(),
            windows,
            scroll: st.scroll,
            hover_card: st.hover_card,
            name,
        }
    }

    fn arrange_shape(&self) -> ArrangeShape {
        let picking = self.arrange.picking.as_ref();
        ArrangeShape {
            picking: picking.is_some(),
            cards: BuiltIn::ALL.len() + self.config.arrange.templates.len(),
            saved: self.config.arrange.saved.len(),
            windows: picking.map_or(0, |p| available(p).len()),
        }
    }

    fn arrange_max_scroll(&self) -> f32 {
        let shape = shape_for(self.config.style, ShapeKind::Panel);
        arrange_max_scroll(
            shape,
            self.state.waiting_strip().is_some(),
            self.arrange_shape(),
        )
    }

    /// The wheel over the content scrolls it while it overflows (the notch
    /// is spent at either end); with everything fitting, `false`: the notch
    /// turns the page as anywhere else.
    pub(super) fn arrange_wheel(&mut self, delta: i32) -> bool {
        let max = self.arrange_max_scroll();
        if max <= 0.0 {
            return false;
        }
        let next = (self.arrange.scroll - delta as f32 / 120.0 * ARR_WHEEL_PX).clamp(0.0, max);
        if next != self.arrange.scroll {
            self.arrange.scroll = next;
            if !self.animating() {
                self.draw(Instant::now());
            }
        }
        true
    }

    // ------------------------------------------------------ hover outline

    /// The pointer is over `hit` on the page (or nowhere): a template card
    /// shows its tiles on the target monitor, dashed, through the
    /// click-through overlay; it goes when the pointer leaves the card.
    pub(super) fn arrange_hover(&mut self, hit: Option<&crate::model::Hit>) {
        if self.arrange.picking.is_some() {
            return;
        }
        let card = match hit {
            Some(crate::model::Hit::ArrangeCard(i)) => Some(*i),
            _ => None,
        };
        if card == self.arrange.hover_card {
            return;
        }
        self.arrange.hover_card = card;
        match card {
            Some(i) => self.arrange_show_outline(i),
            None => self.arrange_hide_outline(),
        }
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    fn arrange_show_outline(&self, card: usize) {
        let Some(monitor) = self.arrange_monitor() else {
            return;
        };
        let Some((_, layout)) = self.arrange_cards().into_iter().nth(card) else {
            return;
        };
        let rects = self.arrange_rects(&layout.node(), monitor);
        arrange_overlay::outline_show(monitor.work, &rects, theme::current().accent.rgb());
    }

    fn arrange_hide_outline(&self) {
        arrange_overlay::outline_hide();
    }

    // ------------------------------------------------------------ effects

    /// A monitor chip.
    pub(super) fn arrange_pick_monitor(&mut self, i: usize) {
        if i < self.arrange.monitors.len() && i != self.arrange.monitor {
            self.arrange.monitor = i;
            self.arrange_hide_outline();
            self.arrange.hover_card = None;
            if !self.animating() {
                self.draw(Instant::now());
            }
        }
    }

    /// A template card: the pick step starts on the target monitor.
    pub(super) fn arrange_card(&mut self, i: usize) {
        self.arrange_hide_outline();
        self.arrange.hover_card = None;
        if self.arrange.picking.is_some() || arrange_overlay::active() {
            return;
        }
        let Some((name, layout)) = self.arrange_cards().into_iter().nth(i) else {
            return;
        };
        // The monitors may have changed since the page opened.
        self.arrange.monitors = arrange_win::monitors();
        let Some(monitor) = self.arrange_monitor().cloned() else {
            return;
        };
        let node = layout.node();
        let rects = self.arrange_rects(&node, &monitor);
        let windows = arrange_win::open_windows();
        let area = crate::capture::win::virtual_screen();
        let spec = PickSpec {
            island: self.hwnd,
            tiles: rects.clone(),
            accent: theme::current().accent.rgb(),
            allowed: windows.iter().map(|w| w.hwnd).collect(),
            windows: crate::capture::win::top_windows(area)
                .iter()
                .map(|w| (w.hwnd, w.rect))
                .collect(),
            area,
            demo: false,
        };
        if !arrange_overlay::start(spec) {
            return;
        }
        self.request_icons(&windows);
        self.arrange.scroll = 0.0;
        self.arrange.picking = Some(Picking {
            flow: PickFlow::new(rects.len()),
            layout,
            layout_name: name,
            rects,
            monitor,
            windows,
        });
        self.apply_now(Event::ArrangePick(true));
    }

    /// Reads the icons of apps not read yet, off the UI thread.
    fn request_icons(&mut self, windows: &[OpenWindow]) {
        let mut asked: Vec<String> = Vec::new();
        for w in windows {
            if !w.exe.is_empty()
                && !self.arrange.icons.contains_key(&w.icon_key)
                && !asked.iter().any(|a| a.eq_ignore_ascii_case(&w.exe))
            {
                asked.push(w.exe.clone());
            }
        }
        arrange_win::load_icons(
            asked,
            self.tx.clone(),
            poster(self.hwnd, super::WM_APP_EVENT),
        );
    }

    /// A row of the window list: the same as clicking that window.
    pub(super) fn arrange_window(&mut self, i: usize) {
        let Some(win) = self
            .arrange
            .picking
            .as_ref()
            .and_then(|p| available(p).get(i).map(|w| w.hwnd))
        else {
            return;
        };
        self.pick_window(win);
    }

    /// "Lewati ubin".
    pub(super) fn arrange_skip(&mut self) {
        let Some(p) = self.arrange.picking.as_mut() else {
            return;
        };
        p.flow.skip();
        self.after_step();
    }

    /// A message from the pick overlay (`WM_APP_ARRANGE`).
    pub(super) fn on_arrange_msg(&mut self, code: usize, data: isize) {
        if self.arrange.picking.is_none() {
            return;
        }
        match code {
            MSG_PICK => self.pick_window(data),
            MSG_BACK => {
                if let Some(p) = self.arrange.picking.as_mut() {
                    p.flow.back();
                }
                self.after_step();
            }
            MSG_CANCEL => self.cancel_pick(),
            _ => {}
        }
    }

    fn pick_window(&mut self, win: isize) {
        let Some(p) = self.arrange.picking.as_mut() else {
            return;
        };
        if !p.windows.iter().any(|w| w.hwnd == win) || !p.flow.pick(win) {
            return;
        }
        self.after_step();
    }

    /// The flow moved: done arranges, else the overlay and the page follow.
    fn after_step(&mut self) {
        let done = self
            .arrange
            .picking
            .as_ref()
            .is_some_and(|p| p.flow.is_done());
        if done {
            self.finish_pick();
            return;
        }
        self.arrange_sync_overlay();
        self.arrange.scroll = self.arrange.scroll.min(self.arrange_max_scroll());
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// The overlay shows what the flow has: the tiles' states and the
    /// windows still free.
    fn arrange_sync_overlay(&self) {
        let Some(p) = self.arrange.picking.as_ref() else {
            return;
        };
        let picks = p.flow.picked();
        let states = (0..p.flow.tiles())
            .map(|i| match picks[i] {
                Some(h) => match p.windows.iter().find(|w| w.hwnd == h) {
                    Some(w) => TileState::Filled {
                        name: arrange::app_name(&w.exe, &w.class),
                        key: w.icon_key.clone(),
                        icon: self.arrange.icons.get(&w.icon_key).cloned(),
                    },
                    None => TileState::Skipped,
                },
                None if i < p.flow.current() => TileState::Skipped,
                None if i == p.flow.current() => TileState::Current,
                None => TileState::Waiting,
            })
            .collect();
        let allowed = available(p).iter().map(|w| w.hwnd).collect();
        arrange_overlay::update(states, allowed);
    }

    /// Esc, a lost foreground, leaving the page: nothing moves, the overlay
    /// closes and the template step shows again.
    pub(super) fn cancel_pick(&mut self) {
        if self.arrange.picking.take().is_none() {
            return;
        }
        arrange_overlay::close();
        self.apply_now(Event::ArrangePick(false));
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// After the last tile: the overlay closes and the windows are arranged
    /// at once (a tile left empty moves nothing).
    fn finish_pick(&mut self) {
        let Some(p) = self.arrange.picking.take() else {
            return;
        };
        arrange_overlay::close();
        self.apply_now(Event::ArrangePick(false));
        let mut jobs = Vec::new();
        let mut tiles: Vec<Option<SavedWindow>> = Vec::new();
        for (rect, pick) in p.rects.iter().zip(p.flow.picked()) {
            let win = pick.and_then(|h| p.windows.iter().find(|w| w.hwnd == h));
            if let Some(w) = win {
                jobs.push((w.hwnd, *rect));
            }
            tiles.push(win.map(|w| SavedWindow {
                exe: w.exe.clone(),
                class: w.class.clone(),
                title: w.title.clone(),
            }));
        }
        let info = SaveInfo {
            layout: p.layout,
            monitor: SavedMonitor {
                device: p.monitor.device.clone(),
                rect: p.monitor.rect,
            },
            tiles,
        };
        self.run_move(
            jobs,
            Run::Pick {
                tiles: p.rects.len(),
                info,
                layout_name: p.layout_name,
            },
        );
    }

    fn run_move(&mut self, jobs: Vec<(isize, Rect)>, run: Run) {
        self.arrange.run += 1;
        let id = self.arrange.run;
        self.arrange.pending = Some((id, run));
        arrange_win::spawn_move(
            id,
            jobs,
            self.tx.clone(),
            poster(self.hwnd, super::WM_APP_EVENT),
        );
    }

    /// A saved arrangement's play button: the monitor is resolved, the
    /// windows matched against what is open, nothing is ever launched.
    pub(super) fn arrange_play(&mut self, i: usize) {
        if self.arrange.picking.is_some() {
            return;
        }
        let Some(saved) = self.config.arrange.saved.get(i).cloned() else {
            return;
        };
        self.arrange.monitors = arrange_win::monitors();
        let notch = self.notch_monitor();
        let Some(at) = arrange::resolve_monitor(&saved.monitor, &self.arrange.monitors, notch)
        else {
            return;
        };
        let monitor = self.arrange.monitors[at].clone();
        let rects = self.arrange_rects(&saved.layout.node(), &monitor);
        let open = arrange_win::open_windows();
        let matches = arrange::match_windows(&saved.tiles, &open);
        let jobs: Vec<(isize, Rect)> = matches
            .iter()
            .zip(&rects)
            .filter_map(|(m, r)| m.map(|w| (open[w].hwnd, *r)))
            .collect();
        let wanted = saved.tiles.iter().flatten().count();
        self.run_move(
            jobs,
            Run::Saved {
                wanted,
                name: saved.name,
            },
        );
    }

    // ------------------------------------------------------ worker events

    /// An answer of a worker: the move ended (its notice, with the undo it
    /// can run) or an icon was read.
    pub(super) fn on_arrange_event(&mut self, event: ArrangeEvent, now: Instant) {
        match event {
            ArrangeEvent::Done {
                run,
                moved,
                skipped: _,
                undo,
            } => {
                let Some((id, what)) = self.arrange.pending.take() else {
                    return;
                };
                if id != run {
                    self.arrange.pending = Some((id, what));
                    return;
                }
                self.arrange.undo = undo;
                self.arrange.last_save = None;
                let lang = i18n::lang();
                let full = self.config.arrange.saved.len() >= MAX_SAVED;
                let (title, body, save) = match what {
                    Run::Pick {
                        tiles,
                        info,
                        layout_name,
                    } => {
                        let counts = Counts::picked(tiles, moved);
                        self.arrange.last_save = Some(info);
                        (
                            i18n::arrange_done(lang, counts.moved, counts.skipped_shown()),
                            layout_name,
                            if full {
                                ArrangeSave::Full
                            } else {
                                ArrangeSave::Offer
                            },
                        )
                    }
                    Run::Saved { wanted, name } => {
                        let c = Counts::saved_run(wanted, moved);
                        (
                            i18n::arrange_saved_run(lang, c.moved, c.wanted, c.skipped),
                            name,
                            ArrangeSave::None,
                        )
                    }
                };
                self.system.raised = true;
                self.apply(
                    Event::Notice(Notice {
                        key: "arrange-done".into(),
                        source: i18n::text(lang, Key::PageArrange).into(),
                        title,
                        body,
                        kind: NoticeKind::Arranged { save },
                    }),
                    now,
                );
            }
            ArrangeEvent::Icon { key, thumb } => {
                if let Some(thumb) = thumb {
                    self.arrange.icons.insert(key, thumb);
                    self.arrange_sync_overlay();
                    if self.arrange_page_shown() && !self.animating() {
                        self.draw(now);
                    }
                }
            }
        }
    }

    /// "Batalkan": every window goes back (position, size, maximized or
    /// minimized), on a worker.
    pub(super) fn arrange_undo(&mut self) {
        let undo = std::mem::take(&mut self.arrange.undo);
        if !undo.is_empty() {
            arrange_win::spawn_undo(undo);
        }
        self.end_naming();
    }

    // ------------------------------------------------------- the name field

    /// "Simpan…": an inline name field in the notch, prefilled "Susunan n".
    pub(super) fn arrange_save(&mut self) {
        let Some(info) = self.arrange.last_save.clone() else {
            return;
        };
        if self.config.arrange.saved.len() >= MAX_SAVED || self.arrange.naming.is_some() {
            return;
        }
        let Some(input) = self.search_input.as_ref() else {
            return;
        };
        let prefix = i18n::text(i18n::lang(), Key::ArrangeDefaultName);
        let name = arrange::default_save_name(prefix, &self.config.arrange.saved);
        input.set_text(&name);
        self.arrange.name_text = name;
        unsafe {
            let fg = GetForegroundWindow();
            if fg != self.hwnd {
                self.cli_prev = (!fg.is_invalid()).then_some(fg);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
        let input = self.search_input.as_ref().expect("checked above");
        let has = input.focus() && unsafe { GetForegroundWindow() } == self.hwnd;
        if !has {
            self.return_keyboard();
            return;
        }
        self.arrange.naming = Some(info);
        self.apply_now(Event::ArrangeNaming(true));
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// The name field is gone: the keyboard goes back and the notice runs
    /// out its time.
    pub(super) fn end_naming(&mut self) {
        if self.arrange.naming.take().is_none() {
            return;
        }
        self.return_keyboard();
        if let Some(input) = &self.search_input {
            input.set_text("");
        }
        self.arrange.name_text.clear();
        self.apply_now(Event::ArrangeNaming(false));
    }

    /// Whether the name field has the keys: it shares the quick search's
    /// hidden EDIT, which the search takes over when it opens.
    pub(super) fn arrange_naming(&self) -> bool {
        self.arrange.naming.is_some()
    }

    /// A key the field hands over while naming: Enter saves, Esc cancels,
    /// the keyboard leaving cancels.
    pub(super) fn on_arrange_name_key(&mut self, key: crate::ui::cli_input::Key) {
        use crate::ui::cli_input::Key;
        match key {
            Key::Enter => self.save_named(),
            Key::Escape => self.end_naming(),
            Key::Blur => {
                let has = self.search_input.as_ref().is_some_and(|i| i.has_focus())
                    && unsafe { GetForegroundWindow() } == self.hwnd;
                if !has {
                    self.cli_prev = None;
                    self.end_naming();
                }
            }
            _ => {}
        }
    }

    /// The field's text changed.
    pub(super) fn on_arrange_name_changed(&mut self) {
        if let Some(input) = &self.search_input {
            self.arrange.name_text = input.text();
        }
        if !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// Enter: a name of 1..=24 characters saves; anything else keeps the
    /// field up.
    fn save_named(&mut self) {
        let Some(info) = self.arrange.naming.clone() else {
            return;
        };
        let typed = self
            .search_input
            .as_ref()
            .map(|i| i.text())
            .unwrap_or_default();
        let Some(saved) = SavedArrangement::new(&typed, info.layout, info.monitor, info.tiles)
        else {
            return;
        };
        if self.config.arrange.saved.len() < MAX_SAVED {
            self.config.arrange.saved.push(saved);
            self.save_config();
            self.sync_settings();
        }
        self.arrange.last_save = None;
        self.end_naming();
        self.arrange.scroll = self.arrange.scroll.min(self.arrange_max_scroll());
    }

    // --------------------------------------------------------------- hotkey

    /// Registers the chosen hotkey (none by default). A failure (another app
    /// has it) shows in Settings and is tried again on the next sync.
    pub(super) fn sync_arrange_hotkey(&mut self) {
        let want = self.config.arrange.hotkey;
        if self.arrange.hotkey == Some(want) && !self.arrange.hotkey_failed {
            return;
        }
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ARRANGE);
        }
        self.arrange.hotkey_failed = match want {
            ArrangeHotkey::Off => false,
            ArrangeHotkey::CtrlAltS => unsafe {
                RegisterHotKey(
                    Some(self.hwnd),
                    HOTKEY_ARRANGE,
                    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                    u32::from(b'S'),
                )
            }
            .is_err(),
        };
        self.arrange.hotkey = Some(want);
    }

    /// `WM_HOTKEY`: the page opens (or closes) at the template step without
    /// taking the foreground, held open like the Devices page.
    pub(super) fn on_arrange_hotkey(&mut self) {
        self.apply_now(Event::ArrangeHotkey);
        self.arm_hotkey_hold();
    }

    /// The hotkey may have been changed by a hand edit of the file.
    pub(super) fn arrange_follow_config(&mut self) {
        if self.arrange.hotkey.is_some() {
            self.sync_arrange_hotkey();
        }
    }

    /// `WM_DESTROY`: the overlays and the hotkey go.
    pub(super) fn arrange_shutdown(&mut self) {
        arrange_overlay::close();
        arrange_overlay::outline_hide();
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ARRANGE);
        }
    }
}
