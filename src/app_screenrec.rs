//! The island's side of the screen recorder (`crate::screenrec`): where a
//! screen recording stands, from the picker through the 3-2-1 to the saved
//! file, and what the Rekam page's Layar tab, the screen hotkey and the tray
//! ask of it. A child of `app`, so it works on `App` directly.
//!
//! Power: nothing here runs at idle. A thread, a timer or a hook exists only
//! while the picker is up, the countdown runs or a screen recording runs.

use super::App;
use crate::clipboard::image::Thumb;

/// Where a screen recording stands.
// The phases past `Idle` are entered from Task 12 on.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ScreenPhase {
    /// None: nothing is picked, counted down or recorded.
    #[default]
    Idle,
    /// The picker is up: a screen, a window or an area is being chosen.
    Picking,
    /// The 3-2-1 before the recording, with the number on show.
    Countdown(u8),
    /// The start thread opens the capture and the encoder.
    Starting,
    Recording,
    /// The stop thread finalizes the file.
    Stopping,
}

// Asked from Task 12 on (the toggle, the other kind's start, the transcript
// hold).
#[allow(dead_code)]
impl ScreenPhase {
    /// A screen recording is somewhere between the picker and the saved
    /// file: an audio recording must not start.
    pub(super) fn busy(self) -> bool {
        self != ScreenPhase::Idle
    }

    /// It is about to record, records or finalizes: transcript jobs hold
    /// (the picker alone holds nothing, it may still be cancelled).
    pub(super) fn engaged(self) -> bool {
        !matches!(self, ScreenPhase::Idle | ScreenPhase::Picking)
    }

    pub(super) fn recording(self) -> bool {
        self == ScreenPhase::Recording
    }
}

/// The screen recording's session, as `App` keeps it.
#[derive(Default)]
pub(super) struct ScreenState {
    phase: ScreenPhase,
    /// The newest saved video's thumbnail and the id its notice names
    /// (`NoticeKind::RecSaved::thumb`).
    // Filled from Task 12 on, drawn from Task 10 on.
    #[allow(dead_code)]
    thumb: Option<(u64, Thumb)>,
}

// Asked from Task 12 on.
#[allow(dead_code)]
impl ScreenState {
    pub(super) fn busy(&self) -> bool {
        self.phase.busy()
    }

    pub(super) fn engaged(&self) -> bool {
        self.phase.engaged()
    }

    pub(super) fn recording(&self) -> bool {
        self.phase.recording()
    }
}

impl App {
    // The four below do nothing until the recorder is wired in (Tasks 9 and
    // 12); the Layar tab that asks for them is not drawn before that.

    /// Opens the picker, cancels the countdown or stops the recording.
    pub(super) fn screen_toggle(&mut self) {}

    /// Stops the screen recording and sends its files to the Recycle Bin.
    pub(super) fn screen_discard(&mut self) {}

    /// Opens the video folder in Explorer.
    pub(super) fn screen_open_folder(&mut self) {}

    /// Shows the folder picker for the video folder.
    pub(super) fn screen_pick_folder(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hold_follows_the_screen_phase() {
        use ScreenPhase::*;
        // (phase, busy, engaged, recording)
        let table = [
            (Idle, false, false, false),
            (Picking, true, false, false),
            (Countdown(3), true, true, false),
            (Starting, true, true, false),
            (Recording, true, true, true),
            (Stopping, true, true, false),
        ];
        for (phase, busy, engaged, recording) in table {
            assert_eq!(phase.busy(), busy, "{phase:?} busy");
            assert_eq!(phase.engaged(), engaged, "{phase:?} engaged");
            assert_eq!(phase.recording(), recording, "{phase:?} recording");
            let state = ScreenState {
                phase,
                ..ScreenState::default()
            };
            assert_eq!(
                (state.busy(), state.engaged(), state.recording()),
                (busy, engaged, recording),
                "{phase:?} through the state"
            );
        }
    }
}
