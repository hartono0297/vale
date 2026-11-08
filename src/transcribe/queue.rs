//! The transcript queue: one job at a time, first in first out.
//!
//! Pure state, no I/O: the caller starts and cancels the threads, reads and
//! writes the JSON text (`to_json` / `from_json`) and decides what "ready"
//! means (the model is installed). A hold (Vale starting up, a recording, a
//! screen recording) keeps the queue from starting anything; turning one on
//! while a job runs sends that job back to the head so the caller can stop it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    Startup,
    Recording,
    ScreenRecording,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedJob {
    pub audio: PathBuf,
    /// Catalog key of the model.
    pub model: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
}

#[derive(Default)]
pub struct Queue {
    running: Option<QueuedJob>,
    waiting: VecDeque<QueuedJob>,
    holds: Vec<Hold>,
}

/// ASCII case-insensitive on the whole path.
pub fn same_audio(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

impl Queue {
    pub fn new(holds: &[Hold]) -> Queue {
        let mut q = Queue::default();
        for &h in holds {
            if !q.holds.contains(&h) {
                q.holds.push(h);
            }
        }
        q
    }

    fn is_running(&self, audio: &Path) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| same_audio(&r.audio, audio))
    }

    /// False when that audio is running. A waiting one takes the new model and
    /// keeps its place.
    pub fn push(&mut self, job: QueuedJob) -> bool {
        if self.is_running(&job.audio) {
            return false;
        }
        match self
            .waiting
            .iter_mut()
            .find(|w| same_audio(&w.audio, &job.audio))
        {
            Some(w) => w.model = job.model,
            None => self.waiting.push_back(job),
        }
        true
    }

    /// Turning a hold on while a job runs moves it back to the head and
    /// returns it, so the caller cancels its thread. Idempotent.
    pub fn set_hold(&mut self, hold: Hold, on: bool) -> Option<QueuedJob> {
        if !on {
            self.holds.retain(|&h| h != hold);
            return None;
        }
        if !self.holds.contains(&hold) {
            self.holds.push(hold);
        }
        let job = self.running.take()?;
        self.waiting.push_front(job.clone());
        Some(job)
    }

    pub fn held(&self) -> bool {
        !self.holds.is_empty()
    }

    /// Only with nothing running, no hold and a ready head. A head that is not
    /// ready blocks the rest.
    pub fn start_next(&mut self, ready: impl Fn(&QueuedJob) -> bool) -> Option<QueuedJob> {
        if self.running.is_some() || self.held() || !ready(self.waiting.front()?) {
            return None;
        }
        let job = self.waiting.pop_front()?;
        self.running = Some(job.clone());
        Some(job)
    }

    /// True when that audio was the running job.
    pub fn finish(&mut self, audio: &Path) -> bool {
        if self.is_running(audio) {
            self.running = None;
            return true;
        }
        false
    }

    /// What the job was doing, or `None` when it was not in the queue.
    pub fn cancel(&mut self, audio: &Path) -> Option<JobState> {
        if self.is_running(audio) {
            self.running = None;
            return Some(JobState::Running);
        }
        let at = self.waiting.iter().position(|w| same_audio(&w.audio, audio))?;
        self.waiting.remove(at);
        Some(JobState::Queued)
    }

    /// Empties the queue; returns the job that was running, if any.
    pub fn clear(&mut self) -> Option<QueuedJob> {
        self.waiting.clear();
        self.running.take()
    }

    pub fn state(&self, audio: &Path) -> Option<JobState> {
        if self.is_running(audio) {
            Some(JobState::Running)
        } else if self.waiting.iter().any(|w| same_audio(&w.audio, audio)) {
            Some(JobState::Queued)
        } else {
            None
        }
    }

    /// Running first, then the waiting ones in order.
    pub fn jobs(&self) -> impl Iterator<Item = &QueuedJob> {
        self.running.iter().chain(self.waiting.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_none() && self.waiting.is_empty()
    }

    pub fn to_json(&self) -> String {
        let jobs: Vec<Value> = self
            .jobs()
            .map(|j| json!({ "audio": j.audio.to_string_lossy(), "model": j.model }))
            .collect();
        json!({ "v": 1, "jobs": jobs }).to_string()
    }

    /// Lenient: not JSON, another `v` or a bad entry are skipped; a job whose
    /// audio is gone or that repeats an earlier audio is dropped.
    pub fn from_json(text: &str, exists: impl Fn(&Path) -> bool) -> Vec<QueuedJob> {
        let Ok(root) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        if root.get("v").and_then(Value::as_u64) != Some(1) {
            return Vec::new();
        }
        let Some(entries) = root.get("jobs").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut out: Vec<QueuedJob> = Vec::new();
        for e in entries {
            let (Some(audio), Some(model)) = (
                e.get("audio").and_then(Value::as_str),
                e.get("model").and_then(Value::as_str),
            ) else {
                continue;
            };
            let audio = PathBuf::from(audio);
            if out.iter().any(|j| same_audio(&j.audio, &audio)) || !exists(&audio) {
                continue;
            }
            out.push(QueuedJob {
                audio,
                model: model.to_string(),
            });
        }
        out
    }
}

/// 0..=99 while running; `None` without a total.
pub fn percent(done_ms: u64, total_ms: Option<u64>) -> Option<u8> {
    let total = total_ms.filter(|&t| t > 0)?;
    Some((u128::from(done_ms.min(total)) * 100 / u128::from(total)).min(99) as u8)
}

/// `None` before 5 % done or 3 s elapsed; else `elapsed * (total - done) /
/// done`, rounded up to whole seconds.
pub fn eta(elapsed: Duration, done_ms: u64, total_ms: Option<u64>) -> Option<Duration> {
    let total = total_ms.filter(|&t| t > 0)?;
    if done_ms == 0
        || u128::from(done_ms) * 100 < u128::from(total) * 5
        || elapsed < Duration::from_secs(3)
    {
        return None;
    }
    let left = u128::from(total.saturating_sub(done_ms));
    let nanos = elapsed.as_nanos() * left / u128::from(done_ms);
    Some(Duration::from_secs(nanos.div_ceil(1_000_000_000) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(a: &str, m: &str) -> QueuedJob {
        QueuedJob {
            audio: PathBuf::from(a),
            model: m.into(),
        }
    }
    fn ready(_: &QueuedJob) -> bool {
        true
    }
    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn jobs_run_one_at_a_time_first_in_first_out() {
        let mut q = Queue::new(&[]);
        assert!(q.push(job("a.m4a", "s")));
        assert!(q.push(job("b.m4a", "s")));
        assert_eq!(q.start_next(ready), Some(job("a.m4a", "s")));
        assert_eq!(q.start_next(ready), None);
        assert!(q.finish(&p("a.m4a")));
        assert!(!q.finish(&p("a.m4a")));
        assert_eq!(q.start_next(ready), Some(job("b.m4a", "s")));
        assert!(q.finish(&p("b.m4a")));
        assert!(q.is_empty());
        assert_eq!(q.start_next(ready), None);
    }

    #[test]
    fn a_hold_sends_the_running_job_back_to_the_head() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "s"));
        q.push(job("b.m4a", "s"));
        q.start_next(ready);
        assert_eq!(q.set_hold(Hold::Recording, true), Some(job("a.m4a", "s")));
        assert_eq!(q.state(&p("a.m4a")), Some(JobState::Queued));
        assert_eq!(q.jobs().next(), Some(&job("a.m4a", "s")));
        assert_eq!(q.start_next(ready), None);
        assert_eq!(q.set_hold(Hold::Recording, false), None);
        assert_eq!(q.start_next(ready), Some(job("a.m4a", "s")));
    }

    #[test]
    fn two_holds_hold_until_both_are_off() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "s"));
        q.set_hold(Hold::Recording, true);
        q.set_hold(Hold::ScreenRecording, true);
        q.set_hold(Hold::Recording, false);
        assert!(q.held());
        assert_eq!(q.start_next(ready), None);
        q.set_hold(Hold::ScreenRecording, false);
        assert!(!q.held());
        assert!(q.start_next(ready).is_some());
    }

    #[test]
    fn holds_are_idempotent() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "s"));
        q.set_hold(Hold::Recording, true);
        assert_eq!(q.set_hold(Hold::Recording, true), None);
        q.set_hold(Hold::Recording, false);
        assert!(!q.held());
        assert!(q.start_next(ready).is_some());
    }

    #[test]
    fn a_job_waits_for_startup_to_settle() {
        let mut q = Queue::new(&[Hold::Startup]);
        q.push(job("a.m4a", "s"));
        assert_eq!(q.start_next(ready), None);
        q.set_hold(Hold::Startup, false);
        assert!(q.start_next(ready).is_some());
    }

    #[test]
    fn a_head_that_is_not_ready_blocks_the_rest() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "big"));
        q.push(job("b.m4a", "small"));
        assert_eq!(q.start_next(|j| j.model == "small"), None);
        assert_eq!(q.start_next(ready), Some(job("a.m4a", "big")));
    }

    #[test]
    fn pushing_a_waiting_audio_again_changes_its_model_not_its_place() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "s"));
        q.push(job("b.m4a", "s"));
        assert!(q.push(job("A.M4A", "big")));
        let all: Vec<_> = q.jobs().cloned().collect();
        assert_eq!(all, vec![job("a.m4a", "big"), job("b.m4a", "s")]);
    }

    #[test]
    fn pushing_the_running_audio_is_refused() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "s"));
        q.start_next(ready);
        assert!(!q.push(job("a.m4a", "big")));
        assert_eq!(q.jobs().count(), 1);
    }

    #[test]
    fn paths_compare_without_case() {
        assert!(same_audio(&p(r"C:\Rec\A.m4a"), &p(r"c:\rec\a.M4A")));
        assert!(!same_audio(&p("a.m4a"), &p("b.m4a")));
    }

    #[test]
    fn cancel_and_clear_say_what_was_running() {
        let mut q = Queue::new(&[]);
        q.push(job("a.m4a", "s"));
        q.push(job("b.m4a", "s"));
        q.push(job("c.m4a", "s"));
        q.start_next(ready);
        assert_eq!(q.cancel(&p("b.m4a")), Some(JobState::Queued));
        assert_eq!(q.cancel(&p("b.m4a")), None);
        assert_eq!(q.cancel(&p("A.m4a")), Some(JobState::Running));
        q.start_next(ready);
        assert_eq!(q.clear(), Some(job("c.m4a", "s")));
        assert!(q.is_empty());
        assert_eq!(q.clear(), None);
    }

    #[test]
    fn the_queue_file_round_trips_with_the_running_job_first() {
        let mut q = Queue::new(&[]);
        q.push(job(r"C:\Rec\a.m4a", "whisper-small"));
        q.push(job(r"C:\Rec\b.m4a", "whisper-large"));
        q.start_next(ready);
        let text = q.to_json();
        assert_eq!(
            text,
            r#"{"v":1,"jobs":[{"audio":"C:\\Rec\\a.m4a","model":"whisper-small"},{"audio":"C:\\Rec\\b.m4a","model":"whisper-large"}]}"#
        );
        let back = Queue::from_json(&text, |_| true);
        assert_eq!(back, q.jobs().cloned().collect::<Vec<_>>());
    }

    #[test]
    fn a_broken_queue_file_loads_empty() {
        for t in [
            "",
            "x",
            r#"{"v":2,"jobs":[{"audio":"a","model":"m"}]}"#,
            r#"{"jobs":5}"#,
            r#"{"v":1,"jobs":5}"#,
        ] {
            assert!(Queue::from_json(t, |_| true).is_empty(), "{t}");
        }
        let mixed =
            r#"{"v":1,"jobs":[{"audio":"a"},5,{"audio":"b","model":"m"},{"audio":"B","model":"x"}]}"#;
        assert_eq!(Queue::from_json(mixed, |_| true), vec![job("b", "m")]);
    }

    #[test]
    fn jobs_whose_audio_is_gone_are_dropped_at_load() {
        let text = r#"{"v":1,"jobs":[{"audio":"gone","model":"m"},{"audio":"here","model":"m"}]}"#;
        let got = Queue::from_json(text, |a| a == Path::new("here"));
        assert_eq!(got, vec![job("here", "m")]);
    }

    #[test]
    fn percent_and_eta() {
        let s = Duration::from_secs;
        assert_eq!(percent(30_000, Some(60_000)), Some(50));
        assert_eq!(percent(60_000, Some(60_000)), Some(99));
        assert_eq!(percent(1, None), None);
        assert_eq!(eta(s(10), 30_000, Some(60_000)), Some(s(10)));
        assert_eq!(eta(s(10), 2_000, Some(60_000)), None);
        assert_eq!(eta(s(2), 30_000, Some(60_000)), None);
        assert_eq!(eta(s(10), 30_000, None), None);
        assert_eq!(
            eta(Duration::from_millis(3_100), 30_000, Some(60_000)),
            Some(s(4))
        );
    }
}
