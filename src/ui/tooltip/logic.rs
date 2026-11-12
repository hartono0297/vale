//! Pure tooltip logic: when a tip shows or hides (`Machine`) and where its
//! plate goes (`place`). No window, timer or clock here; the caller owns the
//! one-shot timer and tells the machine when it fires.

use crate::capture::Rect;

/// Rest time on a target before its tip appears.
pub const DELAY_MS: u32 = 500;
/// After a tip hides, how long moving to another target shows its tip at once.
pub const WARM_MS: u32 = 300;
pub const GAP_DIP: f32 = 6.0;
pub const MAX_W_DIP: f32 = 280.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TipId {
    pub owner: isize,
    pub id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Below the anchor, above it when there is no room below.
    Auto,
    /// Below the anchor only; pinned to the bottom of the work area instead of flipping.
    Below,
}

/// Plate position in screen px for a `w` x `h` plate next to `anchor`,
/// centred on it and kept inside `work`. A plate larger than `work` is cut to it.
pub fn place(anchor: Rect, w: i32, h: i32, work: Rect, gap: i32, side: Side) -> Rect {
    let w = w.min(work.w);
    let h = h.min(work.h);
    let x = (anchor.x + anchor.w / 2 - w / 2)
        .min(work.right() - w)
        .max(work.x);
    let below = anchor.bottom() + gap;
    let y = match side {
        Side::Below => below,
        Side::Auto if below + h <= work.bottom() => below,
        Side::Auto => {
            let above = anchor.y - gap - h;
            if above >= work.y { above } else { below }
        }
    };
    let y = y.min(work.bottom() - h).max(work.y);
    Rect::new(x, y, w, h)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Waiting(TipId),
    Shown(TipId),
    /// No tip, timer running; a new target inside this window shows at once.
    Warm,
    /// Pressed on this target: no tip until the pointer leaves it.
    Clicked(TipId),
}

/// What the caller must do after an input. `arm_ms` (re)arms the one timer,
/// `kill` stops it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Out {
    pub show: Option<TipId>,
    pub hide: bool,
    pub arm_ms: Option<u32>,
    pub kill: bool,
}

pub struct Machine {
    state: State,
}

impl Default for Machine {
    fn default() -> Machine {
        Machine::new()
    }
}

impl Machine {
    pub fn new() -> Machine {
        Machine { state: State::Idle }
    }

    /// The target being waited on, shown, or clicked; `None` when idle or warm.
    pub fn current(&self) -> Option<TipId> {
        match self.state {
            State::Waiting(id) | State::Shown(id) | State::Clicked(id) => Some(id),
            State::Idle | State::Warm => None,
        }
    }

    pub fn shown(&self) -> Option<TipId> {
        match self.state {
            State::Shown(id) => Some(id),
            _ => None,
        }
    }

    /// The pointer is over `id`, or over no target.
    pub fn hover(&mut self, id: Option<TipId>) -> Out {
        let mut out = Out::default();
        match (self.state, id) {
            (State::Idle, Some(b)) => self.wait(b, &mut out),
            (State::Waiting(a), Some(b)) if a != b => self.wait(b, &mut out),
            (State::Waiting(_), None) => {
                self.state = State::Idle;
                out.kill = true;
            }
            (State::Shown(a), Some(b)) if a != b => {
                self.state = State::Shown(b);
                out.show = Some(b);
            }
            (State::Shown(_), None) => {
                self.state = State::Warm;
                out.hide = true;
                out.arm_ms = Some(WARM_MS);
            }
            (State::Warm, Some(b)) => {
                self.state = State::Shown(b);
                out.show = Some(b);
                out.kill = true;
            }
            (State::Clicked(a), Some(b)) if a != b => self.wait(b, &mut out),
            (State::Clicked(_), None) => self.state = State::Idle,
            _ => {}
        }
        out
    }

    /// A mouse button went down: hide, and stay quiet until the pointer leaves.
    pub fn press(&mut self) -> Out {
        let mut out = Out::default();
        match self.state {
            State::Shown(a) => {
                self.state = State::Clicked(a);
                out.hide = true;
                out.kill = true;
            }
            State::Waiting(a) => {
                self.state = State::Clicked(a);
                out.kill = true;
            }
            State::Warm => {
                self.state = State::Idle;
                out.kill = true;
            }
            State::Idle | State::Clicked(_) => {}
        }
        out
    }

    /// Forced reset (focus lost, window gone): no warm window afterwards.
    pub fn hide(&mut self) -> Out {
        let out = Out {
            hide: matches!(self.state, State::Shown(_)),
            kill: matches!(self.state, State::Waiting(_) | State::Shown(_) | State::Warm),
            ..Out::default()
        };
        self.state = State::Idle;
        out
    }

    /// The one-shot timer fired.
    pub fn timer(&mut self) -> Out {
        let mut out = Out::default();
        match self.state {
            State::Waiting(a) => {
                self.state = State::Shown(a);
                out.show = Some(a);
                out.kill = true;
            }
            State::Warm => self.state = State::Idle,
            _ => {}
        }
        out
    }

    fn wait(&mut self, id: TipId, out: &mut Out) {
        self.state = State::Waiting(id);
        out.arm_ms = Some(DELAY_MS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: TipId = TipId { owner: 1, id: 1 };
    const B: TipId = TipId { owner: 1, id: 2 };

    fn shown_a() -> Machine {
        let mut m = Machine::new();
        m.hover(Some(A));
        m.timer();
        m
    }

    #[test]
    fn rest_then_show() {
        let mut m = Machine::new();
        let o = m.hover(Some(A));
        assert_eq!(o.arm_ms, Some(500));
        assert_eq!(o.show, None);
        let o = m.timer();
        assert_eq!(o.show, Some(A));
        assert!(o.kill);
        assert_eq!(m.shown(), Some(A));
    }

    #[test]
    fn leave_before_delay_shows_nothing() {
        let mut m = Machine::new();
        m.hover(Some(A));
        assert!(m.hover(None).kill);
        assert_eq!(m.timer(), Out::default());
    }

    #[test]
    fn move_while_shown_switches_at_once() {
        let mut m = shown_a();
        let o = m.hover(Some(B));
        assert_eq!(o.show, Some(B));
        assert_eq!(o.arm_ms, None);
        assert!(!o.kill);
        assert_eq!(m.shown(), Some(B));
    }

    #[test]
    fn warm_after_hide() {
        let mut m = shown_a();
        let o = m.hover(None);
        assert!(o.hide);
        assert_eq!(o.arm_ms, Some(300));
        let o = m.hover(Some(B));
        assert_eq!(o.show, Some(B));
        assert!(o.kill);
        assert_eq!(m.shown(), Some(B));
    }

    #[test]
    fn warm_expires() {
        let mut m = shown_a();
        m.hover(None);
        m.timer();
        let o = m.hover(Some(B));
        assert_eq!(o.arm_ms, Some(500));
        assert_eq!(o.show, None);
    }

    #[test]
    fn press_hides_and_blocks_until_leave() {
        let mut m = shown_a();
        let o = m.press();
        assert!(o.hide);
        assert!(o.kill);
        assert_eq!(m.hover(Some(A)), Out::default());
        m.hover(None);
        assert_eq!(m.hover(Some(A)).arm_ms, Some(500));
    }

    #[test]
    fn press_while_waiting_blocks_too() {
        let mut m = Machine::new();
        m.hover(Some(A));
        assert!(m.press().kill);
        assert_eq!(m.timer(), Out::default());
        assert_eq!(m.hover(Some(A)), Out::default());
    }

    #[test]
    fn hide_clears_everything() {
        let mut m = shown_a();
        let o = m.hide();
        assert!(o.hide);
        assert!(o.kill);
        assert_eq!(o.arm_ms, None);
        assert_eq!(m.hover(Some(B)).arm_ms, Some(500));
    }

    #[test]
    fn same_id_hover_is_quiet() {
        let mut m = shown_a();
        assert_eq!(m.hover(Some(A)), Out::default());
    }

    #[test]
    fn clicked_then_another_target_waits() {
        let mut m = shown_a();
        m.press();
        let o = m.hover(Some(B));
        assert_eq!(o.arm_ms, Some(500));
        assert_eq!(o.show, None);
    }

    #[test]
    fn clicked_then_leaving_goes_idle() {
        let mut m = shown_a();
        m.press();
        assert_eq!(m.hover(None), Out::default());
        assert_eq!(m.hover(Some(A)).arm_ms, Some(500));
    }

    #[test]
    fn press_while_warm_goes_idle() {
        let mut m = shown_a();
        m.hover(None);
        let o = m.press();
        assert!(o.kill);
        assert!(!o.hide);
        assert_eq!(m.hover(Some(B)).arm_ms, Some(500));
    }

    #[test]
    fn hide_from_clicked_clears_the_block() {
        let mut m = shown_a();
        m.press();
        assert_eq!(m.hide(), Out::default());
        assert_eq!(m.hover(Some(A)).arm_ms, Some(500));
    }

    #[test]
    fn hide_from_warm_kills_the_timer_and_ends_the_window() {
        let mut m = shown_a();
        m.hover(None);
        let o = m.hide();
        assert!(o.kill);
        assert!(!o.hide);
        let o = m.hover(Some(B));
        assert_eq!(o.show, None);
        assert_eq!(o.arm_ms, Some(500));
    }

    #[test]
    fn current_is_the_waiting_shown_or_clicked_target() {
        let mut m = Machine::default();
        assert_eq!(m.current(), None);
        m.hover(Some(A));
        assert_eq!(m.current(), Some(A));
        m.timer();
        assert_eq!(m.current(), Some(A));
        m.press();
        assert_eq!(m.current(), Some(A));
        m.hover(None);
        assert_eq!(m.current(), None);
        m.hover(Some(A));
        m.timer();
        m.hover(None);
        assert_eq!(m.current(), None);
    }

    const WORK: Rect = Rect::new(0, 0, 1920, 1040);

    fn put(anchor: Rect, side: Side) -> Rect {
        place(anchor, 200, 50, WORK, 6, side)
    }

    #[test]
    fn below_centred() {
        assert_eq!(
            put(Rect::new(100, 100, 24, 24), Side::Auto),
            Rect::new(12, 130, 200, 50)
        );
    }

    #[test]
    fn flips_above_near_bottom() {
        let r = put(Rect::new(100, 1000, 24, 24), Side::Auto);
        assert_eq!(r.y, 1000 - 6 - 50);
    }

    #[test]
    fn below_side_never_flips() {
        let r = put(Rect::new(100, 1000, 24, 24), Side::Below);
        assert_eq!(r.y, 990);
        assert!(r.y >= WORK.y);
    }

    #[test]
    fn clamped_left_and_right() {
        assert_eq!(put(Rect::new(0, 100, 24, 24), Side::Auto).x, 0);
        assert_eq!(put(Rect::new(1910, 100, 10, 24), Side::Auto).right(), 1920);
    }

    #[test]
    fn wider_than_work_is_cut() {
        let r = place(Rect::new(100, 100, 24, 24), 3000, 50, WORK, 6, Side::Auto);
        assert_eq!(r.w, 1920);
        assert_eq!(r.x, 0);
    }

    #[test]
    fn second_monitor_offsets() {
        let work = Rect::new(1920, 0, 2560, 1400);
        let r = place(Rect::new(1925, 50, 24, 24), 200, 50, work, 6, Side::Auto);
        assert_eq!(r.x, 1920);
    }
}
