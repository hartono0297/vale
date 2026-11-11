//! What the caption card shows: finished lines, the grey line of each
//! channel, and gaps where the engine skipped speech.

use std::collections::VecDeque;

use super::live::Out;
use crate::transcribe::output::Speaker;

pub const MAX_LINES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub speaker: Option<Speaker>,
    pub text: String,
    pub start_ms: u64,
    pub gap: bool,
}

pub struct Lines {
    speakers: Vec<Option<Speaker>>,
    finals: VecDeque<Line>,
    interim: Vec<String>,
    /// The channel whose grey line was set last.
    newest: Option<usize>,
    generation: u64,
}

impl Lines {
    pub fn new(speakers: Vec<Option<Speaker>>) -> Self {
        let interim = vec![String::new(); speakers.len()];
        Self {
            speakers,
            finals: VecDeque::new(),
            interim,
            newest: None,
            generation: 0,
        }
    }

    pub fn apply(&mut self, out: Out) {
        match out {
            Out::Final(seg) => {
                if let Some(ch) = self.speakers.iter().position(|s| *s == seg.speaker) {
                    self.interim[ch].clear();
                }
                let line = Line {
                    speaker: seg.speaker,
                    text: seg.text,
                    start_ms: seg.start_ms,
                    gap: false,
                };
                let at = self
                    .finals
                    .iter()
                    .rposition(|l| l.start_ms <= line.start_ms)
                    .map_or(0, |i| i + 1);
                self.finals.insert(at, line);
                self.trim();
            }
            Out::Interim { channel, text } => {
                if let Some(slot) = self.interim.get_mut(channel) {
                    *slot = text;
                    self.newest = Some(channel);
                }
            }
            Out::ClearInterim { channel } => {
                if let Some(slot) = self.interim.get_mut(channel) {
                    slot.clear();
                }
            }
            Out::Gap => {
                let start_ms = self.finals.back().map_or(0, |l| l.start_ms);
                self.finals.push_back(Line {
                    speaker: None,
                    text: "…".into(),
                    start_ms,
                    gap: true,
                });
                self.trim();
            }
            Out::Slow => return,
        }
        self.generation += 1;
    }

    fn trim(&mut self) {
        while self.finals.len() > MAX_LINES {
            self.finals.pop_front();
        }
    }

    pub fn finals(&self) -> &VecDeque<Line> {
        &self.finals
    }

    /// The non-empty grey lines in channel order.
    pub fn interim(&self) -> Vec<(Option<Speaker>, &str)> {
        self.speakers
            .iter()
            .zip(&self.interim)
            .filter(|(_, t)| !t.is_empty())
            .map(|(s, t)| (*s, t.as_str()))
            .collect()
    }

    /// Two sources: lines carry who spoke.
    pub fn labelled(&self) -> bool {
        self.speakers.len() == 2
    }

    /// The newest grey line, else the newest final text (for the notch).
    pub fn latest(&self) -> Option<&str> {
        let grey = self
            .newest
            .and_then(|c| self.interim.get(c))
            .filter(|t| !t.is_empty())
            .or_else(|| self.interim.iter().rev().find(|t| !t.is_empty()));
        grey.map(String::as_str).or_else(|| {
            self.finals
                .iter()
                .rev()
                .find(|l| !l.gap)
                .map(|l| l.text.as_str())
        })
    }

    pub fn copy_text(&self, me: &str, others: &str) -> String {
        let labelled = self.labelled();
        self.finals
            .iter()
            .map(|l| {
                let prefix = match (labelled && !l.gap, l.speaker) {
                    (true, Some(Speaker::Me)) => format!("{me}: "),
                    (true, Some(Speaker::Others)) => format!("{others}: "),
                    _ => String::new(),
                };
                format!("{prefix}{}", l.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Bumps on every change.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::output::Segment;

    fn seg(start_ms: u64, speaker: Option<Speaker>, text: &str) -> Out {
        Out::Final(Segment {
            start_ms,
            end_ms: start_ms + 500,
            speaker,
            text: text.into(),
        })
    }

    fn one() -> Lines {
        Lines::new(vec![None])
    }

    fn two() -> Lines {
        Lines::new(vec![Some(Speaker::Me), Some(Speaker::Others)])
    }

    #[test]
    fn finals_stay_in_start_order() {
        let mut l = one();
        l.apply(seg(3000, None, "c"));
        l.apply(seg(1000, None, "a"));
        l.apply(seg(2000, None, "b"));
        let t: Vec<_> = l.finals().iter().map(|x| x.text.as_str()).collect();
        assert_eq!(t, ["a", "b", "c"]);
    }

    #[test]
    fn the_501st_line_drops_the_oldest() {
        let mut l = one();
        for i in 0..=MAX_LINES as u64 {
            l.apply(seg(i, None, &i.to_string()));
        }
        assert_eq!(l.finals().len(), MAX_LINES);
        assert_eq!(l.finals().front().unwrap().text, "1");
        assert_eq!(l.finals().back().unwrap().text, MAX_LINES.to_string());
    }

    #[test]
    fn a_final_clears_its_channels_grey_line() {
        let mut l = two();
        l.apply(Out::Interim {
            channel: 0,
            text: "sa".into(),
        });
        l.apply(Out::Interim {
            channel: 1,
            text: "ok".into(),
        });
        l.apply(seg(0, Some(Speaker::Others), "Oke."));
        assert_eq!(l.interim(), vec![(Some(Speaker::Me), "sa")]);
        l.apply(Out::ClearInterim { channel: 0 });
        assert!(l.interim().is_empty());
    }

    #[test]
    fn copy_labels_only_with_two_sources() {
        let mut l = two();
        l.apply(seg(0, Some(Speaker::Others), "Oke."));
        l.apply(seg(1000, Some(Speaker::Me), "Setuju."));
        assert_eq!(
            l.copy_text("Saya", "Peserta"),
            "Peserta: Oke.\nSaya: Setuju."
        );

        let mut l = one();
        l.apply(seg(0, None, "Oke."));
        l.apply(seg(1000, None, "Setuju."));
        assert_eq!(l.copy_text("Saya", "Peserta"), "Oke.\nSetuju.");

        l.apply(Out::Gap);
        assert_eq!(l.copy_text("Saya", "Peserta"), "Oke.\nSetuju.\n…");
    }

    #[test]
    fn latest_prefers_the_grey_line() {
        let mut l = one();
        assert_eq!(l.latest(), None);
        l.apply(seg(0, None, "done"));
        assert_eq!(l.latest(), Some("done"));
        l.apply(Out::Interim {
            channel: 0,
            text: "typing".into(),
        });
        assert_eq!(l.latest(), Some("typing"));
    }

    #[test]
    fn generation_bumps_on_change_but_not_on_slow() {
        let mut l = one();
        let g = l.generation();
        l.apply(Out::Slow);
        assert_eq!(l.generation(), g);
        l.apply(Out::Gap);
        assert!(l.generation() > g);
    }
}
