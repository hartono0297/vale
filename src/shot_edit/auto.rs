//! Auto-copy of Vale's own screenshot, the pure part: when to render, when
//! to write, when to give up. The window (`window`) holds one `AutoCopy` per
//! editor and acts on what it answers; the worker, the clipboard and the
//! timers are not here, so every rule is tested without any of them.
//!
//! The picture goes to the clipboard when the editor opens (revision 0) and
//! again, debounced, after each finished change. A write replaces only what
//! this editor wrote itself: if the clipboard's sequence number is no longer
//! the one its last write left, somebody else copied since and auto-copy
//! stops (rule 3) until the owner presses Copy. `P` is whatever a retry has
//! to keep (the rendered formats); the model never looks inside it.

/// Wait after the last finished change before the picture is rendered again.
pub const DEBOUNCE_MS: u32 = 300;
/// Wait before the clipboard is tried again after it was busy.
pub const RETRY_MS: u32 = 150;
/// Retries after the first busy clipboard, then the footer says it failed.
pub const RETRIES: u8 = 3;

/// May the editor write now? (rule 3)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteGate {
    Write,
    /// Somebody else copied since this editor's last write: stop silently.
    NotOurs,
}

/// What to do with a rendered picture that came back from the worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultAction {
    /// It is the current document: write it.
    Write,
    /// Nothing to do with it (stale and a newer render is already due,
    /// already copied, or auto-copy is off or stopped).
    Drop,
    /// Stale: drop it and render the current revision now.
    Restart,
}

/// What the debounce timer firing means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerAction {
    Nothing,
    /// An EDIT is open or a drag is running: arm the timer again.
    Rearm,
    /// Start a worker for the current revision.
    Start,
}

/// What to do after the clipboard was busy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Try again after this many milliseconds.
    Again(u32),
    /// Leave it; the footer says "press Copy".
    GiveUp,
}

/// `retry_step` for a clipboard that stayed busy: `tries` retries have been
/// made already, `RETRIES` are allowed.
pub fn retry_step(tries: u8) -> Step {
    if tries < RETRIES {
        Step::Again(RETRY_MS)
    } else {
        Step::GiveUp
    }
}

#[derive(Debug)]
pub struct AutoCopy<P> {
    /// The setting latched when the editor opened; false in demos.
    enabled: bool,
    /// Rule 3 fired: nothing auto-writes until a manual Copy.
    stopped: bool,
    /// The clipboard sequence this editor's last successful write left.
    last_seq: Option<u32>,
    /// The revision that write held.
    copied_rev: Option<u64>,
    /// The revision a worker is rendering.
    in_flight: Option<u64>,
    /// A change came while a worker was busy.
    again: bool,
    /// The debounce is armed.
    due: bool,
    /// Retries made for the write waiting in `waiting`.
    retry_tries: u8,
    /// The rendered formats that wait for a retry, and their revision.
    waiting: Option<(u64, P)>,
    /// A first write gave up: with nothing recorded to compare against the
    /// clipboard is no longer known to be ours.
    failed: bool,
    /// A worker was started at least once.
    attempted: bool,
}

impl<P> AutoCopy<P> {
    pub fn new(enabled: bool) -> Self {
        AutoCopy {
            enabled,
            stopped: false,
            last_seq: None,
            copied_rev: None,
            in_flight: None,
            again: false,
            due: false,
            retry_tries: 0,
            waiting: None,
            failed: false,
            attempted: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    #[cfg(test)]
    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// The sequence the next write must still find on the clipboard (`None`
    /// for the first write of a capture).
    pub fn expected(&self) -> Option<u32> {
        self.last_seq
    }

    fn active(&self) -> bool {
        self.enabled && !self.stopped
    }

    /// (a) The write is allowed when this is the capture's first write, or
    /// the clipboard still holds what this editor wrote last. Not ours:
    /// auto-copy stops for good (until a manual Copy).
    pub fn may_write(&mut self, current_seq: u32) -> WriteGate {
        if !self.active() {
            return WriteGate::NotOurs;
        }
        let ours = match self.last_seq {
            None => !self.failed,
            Some(last) => last == current_seq,
        };
        if ours {
            WriteGate::Write
        } else {
            self.stopped = true;
            self.waiting = None;
            self.retry_tries = 0;
            WriteGate::NotOurs
        }
    }

    /// Rule 3 hit inside the write itself (the clipboard changed between
    /// the check and the open): stop, as `may_write` does.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.waiting = None;
        self.retry_tries = 0;
    }

    /// The window killed the debounce and retry timers: nothing is due any
    /// more and no write waits for a retry, so the next timer starts afresh
    /// instead of waiting on a retry that will never fire.
    pub fn timers_cancelled(&mut self) {
        self.due = false;
        self.waiting = None;
        self.retry_tries = 0;
    }

    /// A write emptied the clipboard but could set none of its formats
    /// (memory ran out): the empty clipboard, at sequence `seq`, is still
    /// this editor's, so the retry is not mistaken for somebody else's copy.
    /// The caller then treats the write as busy (`on_busy`).
    pub fn on_emptied(&mut self, seq: u32) {
        self.last_seq = Some(seq);
    }

    /// The editor opened: the first worker starts at once.
    pub fn wants_first(&self) -> bool {
        self.active() && !self.attempted
    }

    /// A worker starts for `rev`.
    pub fn begin_worker(&mut self, rev: u64) {
        self.in_flight = Some(rev);
        self.attempted = true;
        self.again = false;
    }

    /// The worker for `rev` could not be started: nothing is in flight.
    pub fn worker_failed(&mut self, rev: u64) {
        if self.in_flight == Some(rev) {
            self.in_flight = None;
        }
    }

    /// A change was finished: arm the debounce. True when the timer should
    /// be (re)set.
    pub fn on_change(&mut self) -> bool {
        if !self.active() {
            return false;
        }
        self.due = true;
        true
    }

    /// (c) The debounce timer fired. `busy`: an EDIT is open or a drag runs.
    pub fn on_timer(&mut self, rev: u64, busy: bool) -> TimerAction {
        self.due = false;
        if !self.active() {
            return TimerAction::Nothing;
        }
        if busy {
            self.due = true;
            return TimerAction::Rearm;
        }
        if self.in_flight.is_some() {
            self.again = true;
            return TimerAction::Nothing;
        }
        if self.copied_rev == Some(rev) {
            return TimerAction::Nothing;
        }
        match &self.waiting {
            Some((r, _)) if *r == rev => return TimerAction::Nothing,
            Some(_) => {
                self.waiting = None;
                self.retry_tries = 0;
            }
            None => {}
        }
        TimerAction::Start
    }

    /// (b) A worker's picture for `result_rev` arrived while the document is
    /// at `current_rev`.
    pub fn on_result(&mut self, result_rev: u64, current_rev: u64) -> ResultAction {
        if self.in_flight == Some(result_rev) {
            self.in_flight = None;
        }
        let again = std::mem::take(&mut self.again);
        if !self.active() {
            return ResultAction::Drop;
        }
        if result_rev != current_rev {
            // A timer still to come starts the next render itself; one that
            // already fired (`again`) found this worker busy and will not.
            if (self.due && !again) || self.copied_rev == Some(current_rev) {
                return ResultAction::Drop;
            }
            return ResultAction::Restart;
        }
        if self.copied_rev == Some(result_rev) {
            return ResultAction::Drop;
        }
        ResultAction::Write
    }

    /// (e) The clipboard was busy for the write of `rev`: keep `prepared` and
    /// say whether to try again.
    pub fn on_busy(&mut self, rev: u64, prepared: P) -> Step {
        let step = retry_step(self.retry_tries);
        match step {
            Step::Again(_) => {
                self.retry_tries += 1;
                self.waiting = Some((rev, prepared));
            }
            Step::GiveUp => {
                self.waiting = None;
                self.retry_tries = 0;
                self.failed = true;
            }
        }
        step
    }

    /// The retry timer fired: the formats waiting, when they are still the
    /// current revision and auto-copy still runs.
    pub fn take_retry(&mut self, current_rev: u64) -> Option<(u64, P)> {
        let (rev, p) = self.waiting.take()?;
        (self.active() && rev == current_rev).then_some((rev, p))
    }

    /// (f) A write of `rev` landed and left sequence `seq`.
    pub fn on_written(&mut self, seq: u32, rev: u64) {
        self.last_seq = Some(seq);
        self.copied_rev = Some(rev);
        self.waiting = None;
        self.retry_tries = 0;
        self.failed = false;
    }

    /// (f) The owner pressed Copy: the clipboard is this editor's again, and
    /// auto-updating resumes. Any retry or debounce for a revision up to
    /// `rev` is moot.
    pub fn on_manual_copy(&mut self, seq: u32, rev: u64) {
        self.on_written(seq, rev);
        self.stopped = false;
        self.due = false;
    }

    /// (d) Closing: the picture must still be written when auto-copy runs
    /// and the clipboard may lack the current revision.
    pub fn needs_flush(&self, rev: u64) -> bool {
        self.active()
            && (self.due
                || self.in_flight.is_some()
                || self.waiting.is_some()
                || (self.attempted && self.copied_rev != Some(rev)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto() -> AutoCopy<&'static str> {
        AutoCopy::new(true)
    }

    #[test]
    fn the_first_write_always_goes_and_later_ones_need_the_same_sequence() {
        let mut a = auto();
        assert_eq!(a.expected(), None);
        assert_eq!(a.may_write(999), WriteGate::Write, "the first write");
        a.on_written(10, 0);
        assert_eq!(a.expected(), Some(10));
        assert_eq!(a.may_write(10), WriteGate::Write);
        assert_eq!(a.may_write(11), WriteGate::NotOurs);
        assert!(a.stopped());
        // Once stopped nothing writes, even with the matching sequence.
        assert_eq!(a.may_write(10), WriteGate::NotOurs);
    }

    #[test]
    fn a_disabled_editor_never_writes_or_arms_anything() {
        let mut a: AutoCopy<&str> = AutoCopy::new(false);
        assert_eq!(a.may_write(1), WriteGate::NotOurs);
        assert!(!a.wants_first());
        assert!(!a.on_change());
        assert_eq!(a.on_timer(3, false), TimerAction::Nothing);
        assert!(!a.needs_flush(3));
        assert_eq!(a.on_result(0, 0), ResultAction::Drop);
    }

    #[test]
    fn a_first_write_that_gave_up_is_no_longer_known_to_be_ours() {
        let mut a = auto();
        a.begin_worker(0);
        for _ in 0..RETRIES {
            assert_eq!(a.on_busy(0, "p"), Step::Again(RETRY_MS));
            assert!(a.take_retry(0).is_some());
        }
        assert_eq!(a.on_busy(0, "p"), Step::GiveUp);
        assert_eq!(a.may_write(5), WriteGate::NotOurs);
        assert!(a.stopped());
        // A manual Copy puts it back in charge.
        a.on_manual_copy(6, 0);
        assert!(!a.stopped());
        assert_eq!(a.may_write(6), WriteGate::Write);
    }

    #[test]
    fn a_result_for_an_old_revision_is_dropped_and_the_current_one_rendered() {
        let mut a = auto();
        a.begin_worker(3);
        assert_eq!(a.on_result(3, 5), ResultAction::Restart);
        a.begin_worker(5);
        assert_eq!(a.on_result(5, 5), ResultAction::Write);
        // A pending debounce renders the next one itself.
        a.begin_worker(5);
        assert!(a.on_change());
        assert_eq!(a.on_result(5, 6), ResultAction::Drop);
        // Already on the clipboard: nothing to write.
        a.on_written(7, 6);
        a.begin_worker(6);
        a.on_timer(6, false);
        assert_eq!(a.on_result(6, 6), ResultAction::Drop);
    }

    #[test]
    fn the_debounce_waits_while_editing_and_while_a_worker_is_busy() {
        let mut a = auto();
        assert!(a.on_change());
        assert_eq!(
            a.on_timer(1, true),
            TimerAction::Rearm,
            "mid-typing or drag"
        );
        assert!(a.needs_flush(1), "still due");
        assert_eq!(a.on_timer(1, false), TimerAction::Start);
        a.begin_worker(1);
        assert!(a.on_change());
        assert_eq!(a.on_timer(2, false), TimerAction::Nothing, "a worker runs");
        // The worker's result is for the old revision: a new render starts.
        assert_eq!(a.on_result(1, 2), ResultAction::Restart);
    }

    #[test]
    fn the_timer_does_nothing_when_the_revision_is_already_copied() {
        let mut a = auto();
        a.on_written(4, 2);
        assert!(a.on_change());
        assert_eq!(a.on_timer(2, false), TimerAction::Nothing);
        assert_eq!(a.on_timer(3, false), TimerAction::Start);
    }

    #[test]
    fn a_change_replaces_the_formats_waiting_for_a_retry() {
        let mut a = auto();
        a.begin_worker(1);
        assert_eq!(a.on_result(1, 1), ResultAction::Write);
        assert_eq!(a.on_busy(1, "old"), Step::Again(RETRY_MS));
        assert!(a.needs_flush(1), "a retry is pending");
        assert!(a.on_change());
        // Same revision: the waiting write stays.
        assert_eq!(a.on_timer(1, false), TimerAction::Nothing);
        assert!(a.on_change());
        assert_eq!(a.on_timer(2, false), TimerAction::Start);
        assert_eq!(a.take_retry(2), None, "the old formats are gone");
    }

    #[test]
    fn a_retry_for_an_old_revision_is_not_taken() {
        let mut a = auto();
        a.begin_worker(1);
        a.on_busy(1, "p");
        assert_eq!(a.take_retry(2), None);
        a.on_busy(2, "q");
        assert_eq!(a.take_retry(2), Some((2, "q")));
    }

    #[test]
    fn the_retry_policy_is_three_times_at_150_ms() {
        assert_eq!(retry_step(0), Step::Again(150));
        assert_eq!(retry_step(2), Step::Again(150));
        assert_eq!(retry_step(3), Step::GiveUp);
        assert_eq!((DEBOUNCE_MS, RETRY_MS, RETRIES), (300, 150, 3));
    }

    #[test]
    fn closing_flushes_unless_the_current_revision_is_on_the_clipboard() {
        let mut a = auto();
        assert!(!a.needs_flush(0), "no worker was ever started");
        a.begin_worker(0);
        assert!(a.needs_flush(0), "the first copy is still in flight");
        assert_eq!(a.on_result(0, 0), ResultAction::Write);
        assert!(a.needs_flush(0), "not written yet");
        a.on_written(9, 0);
        assert!(!a.needs_flush(0), "on the clipboard");
        assert!(a.needs_flush(1), "a change since");
        a.stop();
        assert!(!a.needs_flush(1), "stopped: the discard bar decides");
    }

    #[test]
    fn a_manual_copy_resumes_and_drops_the_pending_work() {
        let mut a = auto();
        a.begin_worker(0);
        a.on_written(3, 0);
        assert_eq!(a.may_write(4), WriteGate::NotOurs);
        assert!(a.stopped());
        a.begin_worker(1);
        a.on_busy(1, "p");
        a.on_change();
        a.on_manual_copy(8, 1);
        assert!(!a.stopped());
        assert_eq!(a.expected(), Some(8));
        assert_eq!(a.take_retry(1), None, "the retry is dropped");
        assert_eq!(a.on_result(1, 1), ResultAction::Drop, "already copied");
        assert!(!a.needs_flush(1));
        assert_eq!(a.on_timer(1, false), TimerAction::Nothing);
        assert_eq!(a.may_write(8), WriteGate::Write);
    }

    #[test]
    fn a_worker_that_could_not_start_leaves_nothing_in_flight() {
        let mut a = auto();
        a.begin_worker(2);
        a.worker_failed(2);
        assert!(a.on_change());
        assert_eq!(a.on_timer(2, false), TimerAction::Start);
    }

    #[test]
    fn a_retry_whose_timer_was_killed_does_not_block_the_next_render() {
        // A busy write leaves a retry waiting; closing kills its timer, the
        // flush is busy again and the debounce is armed instead.
        let mut a = auto();
        a.begin_worker(1);
        assert_eq!(a.on_result(1, 1), ResultAction::Write);
        assert_eq!(a.on_busy(1, "p"), Step::Again(RETRY_MS));
        a.timers_cancelled();
        assert!(a.on_change());
        assert_eq!(
            a.on_timer(1, false),
            TimerAction::Start,
            "no retry timer is left to write revision 1"
        );
    }

    #[test]
    fn a_write_that_only_emptied_the_clipboard_keeps_it_ours() {
        let mut a = auto();
        a.begin_worker(0);
        a.on_written(10, 0);
        // The next write emptied the clipboard (sequence 11) but set nothing.
        a.on_emptied(11);
        assert_eq!(a.on_busy(1, "p"), Step::Again(RETRY_MS));
        assert_eq!(a.take_retry(1), Some((1, "p")));
        assert_eq!(
            a.may_write(11),
            WriteGate::Write,
            "the empty clipboard is ours"
        );
        assert!(!a.stopped());
        // Somebody else copying after that still stops it.
        assert_eq!(a.may_write(12), WriteGate::NotOurs);
    }

    #[test]
    fn a_stopped_editor_arms_nothing() {
        let mut a = auto();
        a.on_written(1, 0);
        assert_eq!(a.may_write(2), WriteGate::NotOurs);
        assert!(!a.on_change());
        assert_eq!(a.on_timer(1, false), TimerAction::Nothing);
        assert_eq!(a.on_result(1, 1), ResultAction::Drop);
    }
}
