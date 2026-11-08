//! The island's side of transcripts: the queue, the transcript window, the
//! file picker and the "off" word. Task 7 only names the methods the
//! reducer's effects call; Tasks 8, 9 and 11 fill them. A child of `app`,
//! so it works on `App` directly.

use std::path::PathBuf;

use super::App;

impl App {
    /// Queues a transcript of `audio` with `model` (`None`: the chosen one).
    pub(super) fn transcribe_queue(
        &mut self,
        audio: PathBuf,
        model: Option<&'static crate::transcribe::catalog::Model>,
    ) {
        let _ = (audio, model);
    }

    /// Opens the transcript window of `audio`.
    pub(super) fn open_transcript(&mut self, audio: PathBuf) {
        let _ = audio;
    }

    /// Opens Settings at the Transkrip card.
    pub(super) fn open_transcript_settings(&mut self) {}

    /// Shows the file picker for a transcript of any audio file.
    pub(super) fn rec_pick_file(&mut self) {}

    /// Says that transcripts are off and where to turn them on.
    pub(super) fn transcript_off_note(&mut self) {}
}
