//! The overlay as the screen recording's picker: the same frozen frame, dim
//! and highlight as the screenshot pick, with a bar to choose a screen, a
//! window or an area, to switch the mic and the PC audio, and to start.
//!
//! A click selects what the pointer highlights, a drag in Area selects a
//! rectangle on the monitor where it began. Rekam or Enter ends with the
//! selection; Esc, a right click, a lost foreground and every failure end
//! with nothing. Either way the owner hears it once, through the
//! `PickReport` this phase holds: dropping the state is the "nothing" answer.
//!
//! The rules (what is picked, the bar's layout, the keys) are in `rec_pick`.
//! Nothing runs at idle: no timer, and the UI Automation worker starts only
//! when Area is chosen.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_RETURN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, IDC_ARROW, IDC_CROSS, IsWindow, LoadCursorW, PostQuitMessage, SetCursor,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::{DIM, Phase, State, black, finish, invalidate, pill, pointer, take_state, with};
use crate::capture::inline::{tooltip_rect, ui_for};
use crate::capture::rec_pick::{
    self, BarBtn, LABEL, LABEL_WEIGHT, PickMode, PickReport, PickTarget, Picked, REC_DOT,
    REC_DOT_GAP, RecBar, RecPick, area_on_monitor, bar_layout, place_bar,
};
use crate::capture::win::{self, TopWindow};
use crate::capture::{Candidate, Rect, is_click, size_label};
use crate::i18n::{Key, tr};
use crate::model::RecSources;
use crate::shot_edit::gfx::{Align, Gfx, bitmap, color, rect_f};
use crate::shot_edit::icons::fill_round;
use crate::theme::{self, REC_RED, WindowPalette};
use crate::ui::geometry::Rect as UiRect;
use crate::ui::icons::{Icon, IconSet};
use crate::ui::settings_controls::{mix, primary_fill, segment_fill};

/// A toggle's icon, in dip.
const ICON: f32 = 18.0;
/// The focus ring: this wide, this far outside its control.
const RING: f32 = 2.0;
const RING_GAP: f32 = 2.0;
/// The hint pill's height and its distance above the bar.
const HINT_H: f32 = 32.0;
const HINT_GAP: f32 = 10.0;

/// What the picker starts with.
pub(super) struct Seed {
    pub(super) report: PickReport,
    pub(super) sources: RecSources,
    /// The windows Jendela picks from (none of Vale's own).
    pub(super) windows: Vec<TopWindow>,
    pub(super) monitors: Vec<(isize, Rect)>,
}

/// The picker's state while the overlay is up.
pub(super) struct Rec {
    pick: RecPick,
    /// Posts "nothing picked" when it is dropped unsent.
    report: PickReport,
    windows: Vec<TopWindow>,
    monitors: Vec<(isize, Rect)>,
    /// What a click at the pointer would select.
    hover: Option<(Rect, PickTarget)>,
    /// A press outside the bar, virtual px.
    down: Option<(i32, i32)>,
    /// The area being dragged.
    drag: Option<Rect>,
    hot: Option<BarBtn>,
    pressed: Option<BarBtn>,
    /// DPI over 96 of the monitor the bar is on, and that monitor.
    ui: f32,
    monitor: Rect,
    /// Measured labels of Layar, Jendela, Area and Rekam at `ui`.
    widths: [f32; 4],
    /// The bar, placed (virtual px).
    bar: RecBar,
    icons: IconSet,
    look: Look,
    /// True until the UI Automation worker was started (the first time Area
    /// is chosen); never in the demo, whose windows are made up.
    wants_boxes: bool,
}

impl Rec {
    /// `None` when the icons cannot be made (Direct2D is not usable): the
    /// seed's report then says nothing was picked.
    pub(super) fn new(
        seed: Seed,
        gfx: &Gfx,
        cursor: (i32, i32),
        cands: &[Candidate],
        demo: bool,
    ) -> Option<Rec> {
        let icons = IconSet::new(&gfx.d2d).ok()?;
        let mut rec = Rec {
            pick: RecPick::new(seed.sources),
            report: seed.report,
            windows: seed.windows,
            monitors: seed.monitors,
            hover: None,
            down: None,
            drag: None,
            hot: None,
            pressed: None,
            ui: 0.0,
            monitor: Rect::default(),
            widths: [0.0; 4],
            bar: RecBar { items: Vec::new() },
            icons,
            look: Look::of(&theme::current_window()),
            wants_boxes: !demo,
        };
        rec.relayout(gfx, cursor);
        rec.rehover(cursor, cands);
        Some(rec)
    }

    /// Places the bar: under the selection, or near the bottom of the
    /// monitor under the pointer while nothing is selected. Sized for that
    /// monitor's DPI.
    fn relayout(&mut self, gfx: &Gfx, cursor: (i32, i32)) {
        let sel = self.pick.sel.map(|(r, _)| r);
        let (monitor, dpi) = win::monitor_at(sel.map_or(cursor, |r| r.center()));
        let ui = ui_for(dpi);
        if ui != self.ui {
            self.ui = ui;
            self.widths = [
                Key::ScreenPickScreen,
                Key::ScreenPickWindow,
                Key::ScreenPickArea,
                Key::ScreenPickRecord,
            ]
            .map(|k| gfx.measure(tr(k), LABEL * ui, LABEL_WEIGHT).0);
        }
        self.monitor = monitor;
        let bar = bar_layout(ui, &self.widths);
        let size = bar.rect();
        let at = place_bar(sel, (size.w, size.h), monitor);
        self.bar = bar.moved_to(at.x, at.y);
    }

    /// What the pointer at `p` highlights (nothing while it is on the bar).
    fn rehover(&mut self, p: (i32, i32), cands: &[Candidate]) {
        self.hot = self.bar.hit(p);
        self.hover = if self.bar.rect().contains(p) {
            None
        } else {
            self.pick.hover(p, cands, &self.windows, &self.monitors)
        };
    }

    /// The pointer moved to `p`; true when what is drawn changed.
    fn moved(&mut self, p: (i32, i32), cands: &[Candidate], gfx: &Gfx) -> bool {
        let before = (self.hover, self.drag, self.hot, self.bar.rect());
        match self.down {
            Some(down) if self.pick.mode == PickMode::Area && !is_click(down, p) => {
                self.drag = Some(area_on_monitor(down, p, &self.monitors));
                self.hover = None;
                self.hot = None;
            }
            _ => {
                self.drag = None;
                self.relayout(gfx, p);
                self.rehover(p, cands);
            }
        }
        before != (self.hover, self.drag, self.hot, self.bar.rect())
    }

    /// The rectangle left undimmed: the drag, else the selection, else what
    /// the pointer highlights.
    fn lit(&self) -> Option<Rect> {
        self.drag
            .or(self.pick.sel.map(|(r, _)| r))
            .or(self.hover.map(|(r, _)| r))
    }
}

/// What the overlay does after an input.
enum After {
    Nothing,
    Redraw,
    Record,
    Cancel,
}

/// The picker with the rest of the state it needs, while the overlay is the
/// picker.
fn rec<R>(f: impl FnOnce(&mut Rec, &mut Env) -> R) -> Option<R> {
    with(|s| {
        let State {
            phase,
            gfx,
            cands,
            cursor,
            ..
        } = s;
        let Phase::Rec(r) = phase else { return None };
        let mut env = Env { gfx, cands, cursor };
        Some(f(r, &mut env))
    })
    .flatten()
}

struct Env<'a> {
    gfx: &'a Gfx,
    cands: &'a [Candidate],
    cursor: &'a mut (i32, i32),
}

/// Starts the UI Automation worker the first time Area is chosen: the boxes
/// inside the windows are only snapped to there.
fn want_boxes(hwnd: HWND) {
    let job = with(|s| {
        let Phase::Rec(r) = &mut s.phase else {
            return None;
        };
        if r.pick.mode != PickMode::Area || !r.wants_boxes {
            return None;
        }
        r.wants_boxes = false;
        Some((s.id, r.windows.clone(), s.stop.clone()))
    })
    .flatten();
    if let Some((id, windows, stop)) = job {
        // Never waited for: the windows are snappable already.
        crate::capture::uia::spawn(hwnd, id, windows, stop);
    }
}

/// A control was activated (clicked, or Space / Enter on it).
fn activated(r: &mut Rec, env: &mut Env, key: rec_pick::Key) -> After {
    match key {
        rec_pick::Key::Record => After::Record,
        rec_pick::Key::Cancel => After::Cancel,
        rec_pick::Key::Toggle(_) | rec_pick::Key::Focus(_) => {
            // A new mode cleared the selection: the bar goes back down.
            r.relayout(env.gfx, *env.cursor);
            r.rehover(*env.cursor, env.cands);
            After::Redraw
        }
        rec_pick::Key::None => After::Nothing,
    }
}

fn act(hwnd: HWND, after: Option<After>) {
    match after {
        Some(After::Record) => record(),
        Some(After::Cancel) => finish(None),
        Some(After::Redraw) => {
            want_boxes(hwnd);
            invalidate(hwnd);
        }
        Some(After::Nothing) | None => {}
    }
}

pub(super) fn on_move(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    const MK_LBUTTON: usize = 0x0001;
    let held = wparam.0 & MK_LBUTTON != 0;
    let p = with(|s| pointer(s, lparam));
    let changed = rec(|r, env| {
        let Some(p) = p else { return false };
        *env.cursor = p;
        if !held {
            // The release went elsewhere (capture lost): no press, no drag.
            r.down = None;
            r.pressed = None;
        }
        r.moved(p, env.cands, env.gfx)
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

pub(super) fn on_down(hwnd: HWND, lparam: LPARAM) {
    let p = with(|s| pointer(s, lparam));
    let pressed = rec(|r, env| {
        let Some(p) = p else { return false };
        *env.cursor = p;
        // The pointer took over: the keyboard's ring goes.
        r.pick.focus = None;
        r.pressed = r.bar.hit(p);
        // A press on the bar's background starts nothing.
        r.down = (!r.bar.rect().contains(p)).then_some(p);
        true
    });
    if pressed == Some(true) {
        unsafe {
            let _ = SetCapture(hwnd);
        }
        invalidate(hwnd);
    }
}

pub(super) fn on_up(hwnd: HWND, lparam: LPARAM) {
    let p = with(|s| pointer(s, lparam));
    let after = rec(|r, env| {
        let Some(p) = p else { return After::Nothing };
        *env.cursor = p;
        if let Some(btn) = r.pressed.take() {
            // A button acts when the release is still on it.
            if r.bar.hit(p) != Some(btn) {
                return After::Redraw;
            }
            let key = r.pick.activate(btn);
            return match activated(r, env, key) {
                After::Nothing => After::Redraw,
                other => other,
            };
        }
        let Some(down) = r.down.take() else {
            return After::Nothing;
        };
        r.drag = None;
        if r.pick.mode == PickMode::Area && !is_click(down, p) {
            let area = area_on_monitor(down, p, &r.monitors);
            // A straight line selects nothing: what was selected stays.
            if !area.is_empty() {
                r.pick.sel = Some((area, PickTarget::Area(area)));
            }
        } else {
            // A click takes what the pointer highlights; on nothing it
            // clears the selection.
            r.pick.sel = r.pick.hover(p, env.cands, &r.windows, &r.monitors);
        }
        r.relayout(env.gfx, p);
        r.rehover(p, env.cands);
        After::Redraw
    });
    // Released after the state above is settled, so WM_CAPTURECHANGED finds
    // no press left to cancel.
    unsafe {
        let _ = ReleaseCapture();
    }
    act(hwnd, after);
}

/// Capture lost mid-press (another window took it): the release will not
/// come, so the press and its drag go.
pub(super) fn on_capture_lost(hwnd: HWND) {
    let changed = rec(|r, env| {
        let (down, pressed, drag) = (r.down.take(), r.pressed.take(), r.drag.take());
        r.rehover(*env.cursor, env.cands);
        down.is_some() || pressed.is_some() || drag.is_some()
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

pub(super) fn on_key(hwnd: HWND, vk: u16, repeat: bool) {
    // A held Enter or Space acts once (see `rec_pick::repeats`).
    if repeat && !rec_pick::repeats(vk) {
        return;
    }
    let shift = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
    let after = rec(|r, env| {
        // A press or a drag in progress takes no key but Esc.
        if (r.down.is_some() || r.pressed.is_some()) && vk != 0x1B {
            return After::Nothing;
        }
        let key = r.pick.key(vk, shift);
        if key == rec_pick::Key::None
            && vk == VK_RETURN.0
            && r.pick.focus.is_none()
            && r.pick.sel.is_none()
            && r.hover.is_some()
        {
            // Like the screenshot pick: Enter takes what the pointer
            // highlights. A second Enter records it.
            r.pick.sel = r.hover;
            r.relayout(env.gfx, *env.cursor);
            r.rehover(*env.cursor, env.cands);
            return After::Redraw;
        }
        activated(r, env, key)
    });
    act(hwnd, after);
}

/// Boxes from the UI Automation worker arrived (already added to the
/// candidates): true when the highlight changed.
pub(super) fn boxes_arrived(s: &mut State) -> bool {
    let State {
        phase,
        cands,
        cursor,
        ..
    } = s;
    let Phase::Rec(r) = phase else { return false };
    if r.down.is_some() {
        return false;
    }
    let was = r.hover;
    r.rehover(*cursor, cands);
    r.hover != was
}

pub(super) fn set_cursor() {
    let over_bar = with(|s| match &s.phase {
        Phase::Rec(r) => r.bar.rect().contains(s.cursor) && r.drag.is_none(),
        _ => false,
    }) == Some(true);
    let id = if over_bar { IDC_ARROW } else { IDC_CROSS };
    unsafe {
        if let Ok(c) = LoadCursorW(None, id) {
            SetCursor(Some(c));
        }
    }
}

/// Rekam: the overlay closes and the owner gets the selection. Idempotent
/// like `finish`: the state leaves its slot first.
fn record() {
    let Some(mut state) = take_state(None) else {
        return;
    };
    state.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let (hwnd, demo) = (state.hwnd, state.demo);
    let phase = std::mem::replace(&mut state.phase, Phase::Pick);
    // The frame, the picture and the target go now.
    drop(state);
    if let Phase::Rec(r) = phase {
        let r = *r;
        if let (true, Some((_, target))) = (r.pick.can_record(), r.pick.sel) {
            let title = match target {
                // The demo's windows are made up.
                PickTarget::Window(h) if !demo => win::window_title(h),
                _ => String::new(),
            };
            let picked = Picked {
                target,
                mic: r.pick.mic,
                system: r.pick.system,
                title,
            };
            if demo {
                eprintln!("vale: picked {picked:?} (nothing records in the demo)");
            }
            r.report.send(picked);
        }
        // Otherwise the report is dropped here: nothing picked.
    }
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        if demo {
            PostQuitMessage(0);
        }
    }
}

// ---- drawing ----

/// The bar's colours, from the window palette in effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Look {
    /// The bar itself and its hairline (`0xRRGGBBAA`).
    back: u32,
    edge: u32,
    /// The switch's track, an unselected segment under the pointer, and
    /// their labels.
    track: u32,
    track_hot: u32,
    text: u32,
    /// The selected segment and its label.
    segment: u32,
    on_segment: u32,
    /// A toggle that is off: its icon, and its fill under the pointer.
    quiet: u32,
    off_hot: u32,
    /// Rekam and its label, at rest and under the pointer.
    record: u32,
    record_hot: u32,
    on_record: u32,
    /// Rekam with nothing to record (its label is the quiet ink).
    idle: u32,
    ring: u32,
}

impl Look {
    fn of(p: &WindowPalette) -> Look {
        let record = primary_fill(p);
        Look {
            back: p.card,
            edge: p.border,
            track: p.button,
            track_hot: p.button_hover,
            text: p.text,
            segment: segment_fill(p),
            on_segment: p.on_accent,
            quiet: p.secondary,
            off_hot: mix(p.card, p.button, 0.6),
            record,
            // The step Settings' primary buttons take under the pointer.
            record_hot: mix(record, p.text, 0.1),
            on_record: p.on_accent,
            // Lighter than the track: the quiet ink on the track itself is
            // 4.23:1 in a light window.
            idle: mix(p.card, p.button, 0.5),
            ring: p.accent,
        }
    }
}

/// What the bar is drawn with.
struct Paint<'a> {
    rt: &'a ID2D1RenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    gfx: &'a Gfx,
    ui: f32,
    origin: (i32, i32),
    look: Look,
}

impl Paint<'_> {
    fn rect(&self, r: Rect) -> UiRect {
        UiRect {
            x: (r.x - self.origin.0) as f32,
            y: (r.y - self.origin.1) as f32,
            w: r.w as f32,
            h: r.h as f32,
        }
    }

    fn fill(&self, r: UiRect, radius: f32, rgb: u32) {
        fill_round(self.rt, self.brush, r, radius * self.ui, rgb, 1.0);
    }

    fn stroke(&self, r: UiRect, radius: f32, width: f32, rgb: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: rect_f(r.x, r.y, r.w, r.h),
                    radiusX: radius * self.ui,
                    radiusY: radius * self.ui,
                },
                self.brush,
                width,
                None,
            );
        }
    }

    fn label(&self, text: &str, r: UiRect, rgb: u32) {
        self.gfx.text_in(
            self.rt,
            self.brush,
            text,
            (r.x, r.y, r.w, r.h),
            LABEL * self.ui,
            LABEL_WEIGHT,
            rgb,
            1.0,
            Align::Center,
        );
    }

    fn dot(&self, cx: f32, cy: f32, radius: f32, rgb: u32) {
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            self.rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2::new(cx, cy),
                    radiusX: radius,
                    radiusY: radius,
                },
                self.brush,
            );
        }
    }

    /// The bar's card: a dark line just outside it (so a light bar and a
    /// dark one both stand off whatever the frozen screen shows), the fill
    /// and the palette's hairline.
    fn back(&self, r: UiRect) {
        let ui = self.ui;
        let out = UiRect {
            x: r.x - 0.5 * ui,
            y: r.y - 0.5 * ui,
            w: r.w + ui,
            h: r.h + ui,
        };
        self.stroke(out, 12.5, ui, 0, 0.45);
        self.fill(r, 12.0, self.look.back);
        let inner = UiRect {
            x: r.x + 0.5 * ui,
            y: r.y + 0.5 * ui,
            w: r.w - ui,
            h: r.h - ui,
        };
        let alpha = (self.look.edge & 0xFF) as f32 / 255.0;
        self.stroke(inner, 11.5, ui, self.look.edge >> 8, alpha);
    }

    /// One segment of the switch (its track is drawn before the three).
    fn segment(&self, r: UiRect, text: &str, on: bool, hot: bool) {
        let ui = self.ui;
        let pill = UiRect {
            x: r.x + 2.0 * ui,
            y: r.y + 2.0 * ui,
            w: r.w - 4.0 * ui,
            h: r.h - 4.0 * ui,
        };
        if on {
            self.fill(pill, 6.0, self.look.segment);
        } else if hot {
            self.fill(pill, 6.0, self.look.track_hot);
        }
        let ink = if on {
            self.look.on_segment
        } else {
            self.look.text
        };
        self.label(text, r, ink);
    }

    /// A toggle: filled with its icon when on, bare with the struck-out
    /// icon in the quiet ink when off, so the state never rests on colour.
    fn toggle(&self, icons: &IconSet, r: UiRect, on: bool, hot: bool, glyphs: (Icon, Icon)) {
        let look = self.look;
        match (on, hot) {
            (true, true) => self.fill(r, 8.0, look.track_hot),
            (true, false) => self.fill(r, 8.0, look.track),
            (false, true) => self.fill(r, 8.0, look.off_hot),
            (false, false) => {}
        }
        let (icon, ink) = if on {
            (glyphs.0, look.text)
        } else {
            (glyphs.1, look.quiet)
        };
        // The icon's stroke is a fixed width in the target's units: scaling
        // the target scales the stroke with the rest.
        let ui = self.ui;
        let area = UiRect {
            x: r.x / ui,
            y: r.y / ui,
            w: r.w / ui,
            h: r.h / ui,
        };
        unsafe {
            self.rt.SetTransform(&Matrix3x2 {
                M11: ui,
                M12: 0.0,
                M21: 0.0,
                M22: ui,
                M31: 0.0,
                M32: 0.0,
            });
            let _ = icons.draw(self.rt, self.brush, icon, area, ICON, ink, 1.0, None);
            self.rt.SetTransform(&Matrix3x2::identity());
        }
    }

    /// Rekam: the primary fill with a red dot on a disc of the label's ink
    /// (red alone would sink into a red-brown accent). With nothing to
    /// record it is the quiet button with a hollow dot.
    fn record(&self, r: UiRect, enabled: bool, hot: bool) {
        let (ui, look) = (self.ui, self.look);
        let (fill, ink) = match (enabled, hot) {
            (true, true) => (look.record_hot, look.on_record),
            (true, false) => (look.record, look.on_record),
            (false, _) => (look.idle, look.quiet),
        };
        self.fill(r, 8.0, fill);
        let text = tr(Key::ScreenPickRecord);
        let tw = self.gfx.measure(text, LABEL * ui, LABEL_WEIGHT).0;
        let (dot, gap) = (REC_DOT * ui, REC_DOT_GAP * ui);
        let x = r.x + (r.w - dot - gap - tw) / 2.0;
        let (cx, cy) = (x + dot / 2.0, r.y + r.h / 2.0);
        self.dot(cx, cy, dot / 2.0, ink);
        self.dot(
            cx,
            cy,
            dot / 2.0 - 2.0 * ui,
            if enabled { REC_RED } else { fill },
        );
        self.gfx.text_in(
            self.rt,
            self.brush,
            text,
            (x + dot + gap, r.y, tw + ui, r.h),
            LABEL * ui,
            LABEL_WEIGHT,
            ink,
            1.0,
            Align::Left,
        );
    }

    /// The keyboard's ring around `r`.
    fn ring(&self, r: UiRect, radius: f32) {
        let off = (RING_GAP + RING / 2.0) * self.ui;
        let around = UiRect {
            x: r.x - off,
            y: r.y - off,
            w: r.w + 2.0 * off,
            h: r.h + 2.0 * off,
        };
        self.stroke(
            around,
            radius + RING_GAP + RING / 2.0,
            RING * self.ui,
            self.look.ring,
            1.0,
        );
    }

    fn pill(&self, r: UiRect, text: &str, size: f32) {
        pill(
            self.rt,
            self.brush,
            self.gfx,
            (r.x, r.y, r.w, r.h),
            text,
            size,
            0,
            self.ui,
        );
    }
}

fn hint(mode: PickMode) -> Key {
    match mode {
        PickMode::Screen => Key::ScreenPickHintScreen,
        PickMode::Window => Key::ScreenPickHintWindow,
        PickMode::Area => Key::ScreenPickHintArea,
    }
}

/// The name a toggle shows under it (the pointer on it, or the keyboard).
fn tip(btn: BarBtn) -> Option<Key> {
    match btn {
        BarBtn::Mic => Some(Key::RecSourceMic),
        BarBtn::System => Some(Key::RecSourceSystem),
        _ => None,
    }
}

pub(super) fn draw(s: &mut State) -> windows::core::Result<()> {
    let (w, h) = (s.frame.w(), s.frame.h());
    if s.target.is_none() {
        s.target = Some(s.gfx.hwnd_target(s.hwnd, (w, h))?);
        s.picture = None;
    }
    let State {
        target,
        picture,
        gfx,
        frame,
        phase,
        accent,
        ..
    } = s;
    let Phase::Rec(r) = phase else {
        return Ok(());
    };
    let t = target.as_ref().expect("made above");
    let rt: &ID2D1RenderTarget = &t.rt;
    let brush = &t.brush;
    if picture.is_none() {
        *picture = Some(bitmap(rt, w, h, frame.bgra())?);
    }
    let picture = picture.as_ref().expect("made above");
    let ui = r.ui;
    let p = Paint {
        rt,
        brush,
        gfx,
        ui,
        origin: frame.origin,
        look: r.look,
    };
    let (full_w, full_h) = (w as f32, h as f32);
    let lit = r.lit();
    unsafe {
        rt.BeginDraw();
        rt.DrawBitmap(
            picture,
            Some(&rect_f(0.0, 0.0, full_w, full_h)),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
        brush.SetColor(&black(DIM));
        let dim = |x: f32, y: f32, w: f32, h: f32| {
            if w > 0.0 && h > 0.0 {
                rt.FillRectangle(&rect_f(x, y, w, h), brush);
            }
        };
        match lit.map(|l| p.rect(l)) {
            None => dim(0.0, 0.0, full_w, full_h),
            Some(l) => {
                dim(0.0, 0.0, full_w, l.y);
                dim(0.0, l.bottom(), full_w, full_h - l.bottom());
                dim(0.0, l.y, l.x, l.h);
                dim(l.right(), l.y, full_w - l.right(), l.h);
            }
        }
        // With something selected the pointer still shows what a click
        // would take instead: a thin line, black with white dashes on it,
        // so it reads on any picture.
        if let (Some((sel, _)), Some((hover, _)), None) = (r.pick.sel, r.hover, r.drag)
            && hover != sel
        {
            let o = p.rect(hover);
            let b = rect_f(o.x - 0.5, o.y - 0.5, o.w + 1.0, o.h + 1.0);
            brush.SetColor(&color(0, 1.0));
            rt.DrawRectangle(&b, brush, 1.0, None);
            brush.SetColor(&color(0xFFFFFF, 1.0));
            rt.DrawRectangle(&b, brush, 1.0, &gfx.dash);
        }
        if let Some(l) = lit {
            let o = p.rect(l);
            brush.SetColor(&color(*accent, 1.0));
            // Just outside the highlight, so the picture inside is untouched.
            rt.DrawRectangle(
                &rect_f(o.x - ui, o.y - ui, o.w + 2.0 * ui, o.h + 2.0 * ui),
                brush,
                2.0 * ui,
                None,
            );
            let label = size_label(l);
            let lw = gfx.measure(&label, 12.0 * ui, 500).0 + 16.0 * ui;
            let lh = 22.0 * ui;
            // Above the top-left corner; inside it when there is no room.
            let above = o.y - 8.0 * ui - lh;
            let ly = if above >= 0.0 { above } else { o.y + 8.0 * ui };
            let lx = o.x.min(full_w - lw).max(0.0);
            pill(
                rt,
                brush,
                gfx,
                (lx, ly, lw, lh),
                &label,
                12.0 * ui,
                *accent,
                ui,
            );
        }
    }
    // The bar steps aside while an area is dragged.
    if r.drag.is_none() {
        let bar = p.rect(r.bar.rect());
        p.back(bar);
        for sep in r.bar.seps() {
            let o = p.rect(sep);
            unsafe {
                brush.SetColor(&color(p.look.text, 0.16));
                rt.FillRectangle(&rect_f(o.x, o.y, ui.max(1.0), o.h), brush);
            }
        }
        // The switch's track, under its three segments.
        if let (Some(first), Some(last)) =
            (r.bar.rect_of(BarBtn::Screen), r.bar.rect_of(BarBtn::Area))
        {
            let track = Rect::new(first.x, first.y, last.right() - first.x, first.h);
            p.fill(p.rect(track), 8.0, p.look.track);
        }
        // A pressed button shows its press only while the pointer is on it.
        let hot = |btn: BarBtn| r.hot == Some(btn) && r.pressed.is_none_or(|b| b == btn);
        for (btn, at) in &r.bar.items {
            let o = p.rect(*at);
            match btn {
                BarBtn::Screen | BarBtn::Window | BarBtn::Area => {
                    let key = match btn {
                        BarBtn::Screen => Key::ScreenPickScreen,
                        BarBtn::Window => Key::ScreenPickWindow,
                        _ => Key::ScreenPickArea,
                    };
                    let on = btn.mode() == Some(r.pick.mode);
                    p.segment(o, tr(key), on, hot(*btn));
                }
                BarBtn::Mic => p.toggle(
                    &r.icons,
                    o,
                    r.pick.mic,
                    hot(*btn),
                    (Icon::Mic, Icon::MicOff),
                ),
                BarBtn::System => p.toggle(
                    &r.icons,
                    o,
                    r.pick.system,
                    hot(*btn),
                    (Icon::Volume, Icon::VolumeMute),
                ),
                BarBtn::Record => p.record(o, r.pick.can_record(), hot(*btn)),
            }
        }
        if let Some(at) = r.pick.focus.and_then(|f| r.bar.rect_of(f)) {
            let radius = if r.pick.focus.and_then(BarBtn::mode).is_some() {
                6.0
            } else {
                8.0
            };
            p.ring(p.rect(at), radius);
        }
        // What to do, above the bar, until something is selected.
        if r.pick.sel.is_none() {
            let text = tr(hint(r.pick.mode));
            let size = 13.0 * ui;
            let hw = gfx.measure(text, size, 500).0 + 36.0 * ui;
            let hh = HINT_H * ui;
            let m = p.rect(r.monitor);
            let x = (bar.x + (bar.w - hw) / 2.0).min(m.right() - hw).max(m.x);
            p.pill(
                UiRect {
                    x,
                    y: bar.y - HINT_GAP * ui - hh,
                    w: hw,
                    h: hh,
                },
                text,
                size,
            );
        }
        // A toggle's name: under the pointer, or where the keyboard is.
        let named = r.hot.or(r.pick.focus);
        if let Some(btn) = named
            && let Some(key) = tip(btn)
            && let Some(at) = r.bar.rect_of(btn)
        {
            let text = tr(key);
            let tw = gfx.measure(text, 12.0 * ui, 500).0 + 18.0 * ui;
            let th = 24.0 * ui;
            let place = tooltip_rect(
                at,
                (tw.ceil() as i32, th.ceil() as i32),
                r.monitor,
                false,
                ((RING_GAP + RING + 4.0) * ui).round() as i32,
            );
            p.pill(p.rect(place), text, 12.0 * ui);
        }
    }
    unsafe { rt.EndDraw(None, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Accent, Surface, WindowTheme, window_for};
    use crate::widgets::sticky::contrast;

    /// Every pair the bar draws, in the light, warm dark and Terminal dark
    /// windows with every accent: labels at 4.5:1 on what they sit on, icons,
    /// the ring and the dot at 3:1.
    #[test]
    fn the_bar_reads_in_every_window() {
        for accent in Accent::ALL {
            for (surface, mode) in [
                (Surface::Espresso, WindowTheme::Light),
                (Surface::Espresso, WindowTheme::Dark),
                (Surface::Terminal, WindowTheme::Dark),
            ] {
                let l = Look::of(&window_for(*accent, surface, mode, false));
                let at = format!("{accent:?} {surface:?} {mode:?}");
                let text = |name: &str, ink: u32, on: u32| {
                    let c = contrast(ink, on);
                    assert!(c >= 4.5, "{at}: {name} {c:.2}");
                };
                let mark = |name: &str, ink: u32, on: u32| {
                    let c = contrast(ink, on);
                    assert!(c >= 3.0, "{at}: {name} {c:.2}");
                };
                text("the selected segment", l.on_segment, l.segment);
                text("a segment", l.text, l.track);
                text("a segment under the pointer", l.text, l.track_hot);
                text("Rekam", l.on_record, l.record);
                text("Rekam under the pointer", l.on_record, l.record_hot);
                text("Rekam with nothing selected", l.quiet, l.idle);
                mark("a toggle that is on", l.text, l.track);
                mark(
                    "a toggle that is on, under the pointer",
                    l.text,
                    l.track_hot,
                );
                mark("a toggle that is off", l.quiet, l.back);
                mark(
                    "a toggle that is off, under the pointer",
                    l.quiet,
                    l.off_hot,
                );
                mark("the red dot on its disc", REC_RED, l.on_record);
                mark("the dot's disc on Rekam", l.on_record, l.record);
                mark("the ring on the bar", l.ring, l.back);
                mark("the ring beside the track", l.ring, l.track);
            }
        }
    }

    #[test]
    fn each_mode_has_its_own_hint_and_only_the_toggles_a_name() {
        let hints = [PickMode::Screen, PickMode::Window, PickMode::Area].map(hint);
        assert_ne!(hints[0], hints[1]);
        assert_ne!(hints[1], hints[2]);
        assert_eq!(tip(BarBtn::Mic), Some(Key::RecSourceMic));
        assert_eq!(tip(BarBtn::System), Some(Key::RecSourceSystem));
        for btn in [BarBtn::Screen, BarBtn::Window, BarBtn::Area, BarBtn::Record] {
            assert_eq!(tip(btn), None, "{btn:?} carries its label");
        }
    }

    #[test]
    fn no_picker_exists_in_a_test() {
        assert!(rec(|_, _| ()).is_none());
    }
}
