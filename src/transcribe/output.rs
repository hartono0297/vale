//! The transcript's files: `<name>.md` and `<name>.srt` next to the audio
//! (`<name>.transcript.md` / `.srt` when those are someone else's), and the
//! `"transcript"` object in `<name>.vale.json` (the recorder's sidecar; its
//! other keys are kept as they are).
//!
//! Pure text work besides a look at which files exist, so it is tested
//! without the engine.

use std::path::{Path, PathBuf};

/// Who spoke, by channel: the mic is the user, the system sound is everyone
/// else in the meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Speaker {
    Me,
    Others,
}

/// The words for the two speakers (the caller picks the language).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Labels {
    pub me: &'static str,
    pub others: &'static str,
}

pub const LABELS_ID: Labels = Labels {
    me: "Saya",
    others: "Peserta",
};

pub const LABELS_EN: Labels = Labels {
    me: "Me",
    others: "Others",
};

impl Labels {
    pub fn of(&self, speaker: Speaker) -> &'static str {
        match speaker {
            Speaker::Me => self.me,
            Speaker::Others => self.others,
        }
    }
}

/// One timed piece of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// From the start of the recording.
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<Speaker>,
    pub text: String,
}

/// The speaker of each channel. The sidecar's `"channels"` names them
/// (`"mic"`, `"system"`); without a sidecar a two-channel file follows the
/// recorder's layout (0 = mic, 1 = system) and anything else has none.
pub fn speakers(channels: usize, sidecar: Option<&[String]>) -> Vec<Option<Speaker>> {
    match sidecar {
        Some(names) if names.len() == channels => names
            .iter()
            .map(|name| match name.as_str() {
                "mic" => Some(Speaker::Me),
                "system" => Some(Speaker::Others),
                _ => None,
            })
            .collect(),
        _ if channels == 2 => vec![Some(Speaker::Me), Some(Speaker::Others)],
        _ => vec![None; channels],
    }
}

/// How a file's channels are read: whether to mix them into one first, and
/// the speaker of each channel read. A video with two or more channels and
/// no channel names in a sidecar did not come from Vale's recorder, so its
/// channels are not "mic" and "system": it is mixed into one unlabelled
/// channel. Everything else keeps its channels (`speakers`).
pub fn channel_plan(
    audio: &Path,
    channels: usize,
    names: Option<&[String]>,
) -> (bool, Vec<Option<Speaker>>) {
    let video = audio
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
    if video && channels >= 2 && names.is_none_or(<[String]>::is_empty) {
        (true, vec![None])
    } else {
        (false, speakers(channels, names))
    }
}

/// Both channels' segments in time order (the mic first at the same start),
/// empty texts dropped.
pub fn merge(segments: impl IntoIterator<Item = Segment>) -> Vec<Segment> {
    let mut all: Vec<Segment> = segments
        .into_iter()
        .filter(|s| !s.text.trim().is_empty())
        .collect();
    all.sort_by(|a, b| {
        (a.start_ms, a.speaker, a.end_ms).cmp(&(b.start_ms, b.speaker, b.end_ms))
    });
    all
}

/// `HH:MM:SS,mmm`, the SRT time.
pub fn srt_time(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// `MM:SS`, or `H:MM:SS` from an hour on: the transcript's line times.
pub fn short_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// SubRip: one cue per segment, the speaker before the text.
pub fn srt(segments: &[Segment], labels: Labels) -> String {
    let mut out = String::new();
    for (i, s) in segments.iter().enumerate() {
        let text = one_line(&s.text);
        let line = match s.speaker {
            Some(who) => format!("{}: {text}", labels.of(who)),
            None => text,
        };
        out.push_str(&format!(
            "{}\n{} --> {}\n{line}\n\n",
            i + 1,
            srt_time(s.start_ms),
            srt_time(s.end_ms.max(s.start_ms + 1))
        ));
    }
    out
}

/// What the Markdown file says above the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    /// Already formatted for people ("2026-11-07 14:30"), when known.
    pub started: Option<String>,
    pub duration_ms: Option<u64>,
    /// The model's display name.
    pub model: String,
}

/// Same-speaker segments this close together share a paragraph...
const JOIN_GAP_MS: u64 = 2_000;
/// ...until the paragraph is this long, so its time stays useful.
const PARAGRAPH_MS: u64 = 60_000;

/// The Markdown transcript: a title, one line of facts, then one paragraph
/// per turn, `**MM:SS · Speaker:** text`.
pub fn markdown(header: &Header, segments: &[Segment], labels: Labels) -> String {
    let mut out = format!("# {}\n\n", header.title.trim());
    let mut facts = Vec::new();
    if let Some(started) = &header.started {
        facts.push(started.clone());
    }
    if let Some(ms) = header.duration_ms {
        facts.push(short_time(ms));
    }
    facts.push(header.model.clone());
    out.push_str(&format!("*{}*\n", facts.join(" · ")));
    let mut i = 0;
    while i < segments.len() {
        let first = &segments[i];
        let mut text = one_line(&first.text);
        let mut end = first.end_ms;
        let mut j = i + 1;
        while let Some(next) = segments.get(j) {
            if next.speaker != first.speaker
                || next.start_ms > end + JOIN_GAP_MS
                || next.end_ms > first.start_ms + PARAGRAPH_MS
            {
                break;
            }
            text.push(' ');
            text.push_str(&one_line(&next.text));
            end = end.max(next.end_ms);
            j += 1;
        }
        let when = short_time(first.start_ms);
        match first.speaker {
            Some(who) => out.push_str(&format!("\n**{when} · {}:** {text}\n", labels.of(who))),
            None => out.push_str(&format!("\n**{when}** {text}\n")),
        }
        i = j;
    }
    out
}

/// Text on one line with single spaces.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The files that go with `audio`: `.md`, `.srt` and the `.vale.json`
/// sidecar (`2026-11-07 1430 Meet.m4a` -> `2026-11-07 1430 Meet.md`, ...).
pub struct Paths {
    pub md: PathBuf,
    pub srt: PathBuf,
    pub sidecar: PathBuf,
}

pub fn paths(audio: &Path) -> Paths {
    let stem = audio.file_stem().unwrap_or_default().to_string_lossy();
    let dir = audio.parent().unwrap_or(Path::new(""));
    Paths {
        md: dir.join(format!("{stem}.md")),
        srt: dir.join(format!("{stem}.srt")),
        sidecar: dir.join(format!("{stem}.vale.json")),
    }
}

/// The `.md` and `.srt` a transcript of one recording lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    pub md: PathBuf,
    pub srt: PathBuf,
}

/// `<stem>.transcript.md` and `.srt`: where Vale writes when `<stem>.md` or
/// `<stem>.srt` is someone else's.
fn own_files(audio: &Path) -> Files {
    let stem = audio.file_stem().unwrap_or_default().to_string_lossy();
    let dir = audio.parent().unwrap_or(Path::new(""));
    Files {
        md: dir.join(format!("{stem}.transcript.md")),
        srt: dir.join(format!("{stem}.transcript.srt")),
    }
}

/// The files the sidecar's `"transcript"` names (Vale wrote them), when both
/// are plain `.md` / `.srt` file names next to the audio.
pub fn named_files(audio: &Path, sidecar: &serde_json::Value) -> Option<Files> {
    let transcript = sidecar.get("transcript")?;
    let name = |key: &str, ext: &str| {
        let name = transcript.get(key)?.as_str()?;
        let plain = Path::new(name).file_name() == Some(std::ffi::OsStr::new(name));
        let fits = name.len() > ext.len() && name.to_ascii_lowercase().ends_with(ext);
        (plain && fits).then(|| audio.with_file_name(name))
    };
    Some(Files {
        md: name("md", ".md")?,
        srt: name("srt", ".srt")?,
    })
}

/// Where the transcript of `audio` is read: the files the sidecar names,
/// else `<stem>.transcript.*` when Vale wrote there, else `<stem>.md` and
/// `<stem>.srt`.
pub fn transcript_files(audio: &Path, sidecar: Option<&serde_json::Value>) -> Files {
    if let Some(files) = sidecar.and_then(|v| named_files(audio, v)) {
        return files;
    }
    let own = own_files(audio);
    if own.srt.is_file() || own.md.is_file() {
        return own;
    }
    let plain = paths(audio);
    Files {
        md: plain.md,
        srt: plain.srt,
    }
}

/// Where a new transcript of `audio` is written: the files the sidecar
/// names, or `<stem>.transcript.*` once Vale wrote there; else `<stem>.md`
/// and `<stem>.srt` when they are free or Vale's (the `.md` starts with
/// Vale's header), and `<stem>.transcript.*` when either is someone else's.
pub fn target_files(audio: &Path, sidecar: Option<&serde_json::Value>) -> Files {
    if let Some(files) = sidecar.and_then(|v| named_files(audio, v)) {
        return files;
    }
    let own = own_files(audio);
    if own.srt.exists() || own.md.exists() {
        return own;
    }
    let plain = paths(audio);
    let ours = match std::fs::read(&plain.md) {
        Ok(bytes) => is_vale_markdown(&String::from_utf8_lossy(&bytes)),
        // No `.md`: an `.srt` alone is not Vale's.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => !plain.srt.exists(),
        Err(_) => false,
    };
    if ours {
        Files {
            md: plain.md,
            srt: plain.srt,
        }
    } else {
        own
    }
}

/// The model's name in the Markdown header: `"Whisper small Q5_1"`.
pub fn model_label(model: &super::catalog::Model) -> String {
    format!("{} {}", model.name, model.quant)
}

/// Whether `text` starts with the header `markdown` writes: the title, a
/// blank line, and the facts line in italics ending with a catalog model's
/// label (or a model file's name, from the debug CLI).
pub fn is_vale_markdown(text: &str) -> bool {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let (Some(title), Some(blank), Some(facts)) = (lines.next(), lines.next(), lines.next()) else {
        return false;
    };
    let Some(facts) = facts.strip_prefix('*').and_then(|f| f.strip_suffix('*')) else {
        return false;
    };
    let model = facts.rsplit(" · ").next().unwrap_or_default();
    let known = super::catalog::MODELS.iter().any(|m| model == model_label(m))
        || model.to_ascii_lowercase().ends_with(".gguf");
    title.starts_with("# ") && blank.is_empty() && known
}

/// What the sidecar records about a finished transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptInfo {
    /// The catalog key, `"qwen3-asr-0.6b"`.
    pub model: String,
    pub engine: String,
    /// What the model detected or was told, when it says.
    pub language: Option<String>,
    /// RFC 3339, local time.
    pub created: String,
    /// File names (not paths) of the `.md` and `.srt`.
    pub md: String,
    pub srt: String,
    pub segments: usize,
    /// How long the transcription took.
    pub elapsed_ms: u64,
}

/// The sidecar text with `"transcript"` set to `info`; every other key stays
/// as it was, in its place.
pub fn with_transcript(sidecar: &str, info: &TranscriptInfo) -> Result<String, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(sidecar).map_err(|e| format!("sidecar: {e}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "sidecar is not a JSON object".to_string())?;
    object.insert(
        "transcript".into(),
        serde_json::json!({
            "model": info.model,
            "engine": info.engine,
            "language": info.language,
            "created": info.created,
            "md": info.md,
            "srt": info.srt,
            "segments": info.segments,
            "elapsed_ms": info.elapsed_ms,
        }),
    );
    let mut text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

/// The sidecar's `"channels"` (`["mic", "system"]`), when it has them.
pub fn sidecar_channels(sidecar: &str) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(sidecar).ok()?;
    value
        .get("channels")?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// The sidecar's `"title"`, when it is not empty.
pub fn sidecar_title(sidecar: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(sidecar).ok()?;
    let title = value.get("title")?.as_str()?.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// Writes `text` to `path` through a temporary file and a rename, so a
/// reader never sees half a file.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: u64, end: u64, speaker: Option<Speaker>, text: &str) -> Segment {
        Segment {
            start_ms: start,
            end_ms: end,
            speaker,
            text: text.into(),
        }
    }

    use Speaker::{Me, Others};

    #[test]
    fn times_are_formatted_for_srt_and_for_people() {
        assert_eq!(srt_time(0), "00:00:00,000");
        assert_eq!(srt_time(3_723_045), "01:02:03,045");
        assert_eq!(short_time(7_999), "00:07");
        assert_eq!(short_time(862_000), "14:22");
        assert_eq!(short_time(3_723_045), "1:02:03");
    }

    #[test]
    fn the_two_channels_merge_in_time_order() {
        let mic = vec![seg(15_000, 18_000, Some(Me), "Onboarding design udah final.")];
        let system = vec![
            seg(0, 6_000, Some(Others), "Oke, kita mulai ya."),
            seg(7_000, 12_000, Some(Others), "Pertama status onboarding."),
            seg(15_000, 16_000, Some(Others), "Ya."),
            seg(20_000, 21_000, Some(Others), "   "),
        ];
        let merged = merge(mic.into_iter().chain(system));
        let order: Vec<(u64, Option<Speaker>)> =
            merged.iter().map(|s| (s.start_ms, s.speaker)).collect();
        assert_eq!(
            order,
            vec![
                (0, Some(Others)),
                (7_000, Some(Others)),
                (15_000, Some(Me)),
                (15_000, Some(Others)),
            ]
        );
    }

    #[test]
    fn srt_numbers_cues_and_names_the_speaker() {
        let segments = vec![
            seg(0, 6_500, Some(Others), "Oke,\nkita  mulai ya."),
            seg(15_000, 18_250, Some(Me), "Boleh."),
            seg(20_000, 20_000, None, "Tanpa label."),
        ];
        let text = srt(&segments, LABELS_ID);
        assert_eq!(
            text,
            "1\n00:00:00,000 --> 00:00:06,500\nPeserta: Oke, kita mulai ya.\n\n\
             2\n00:00:15,000 --> 00:00:18,250\nSaya: Boleh.\n\n\
             3\n00:00:20,000 --> 00:00:20,001\nTanpa label.\n\n"
        );
    }

    #[test]
    fn markdown_joins_a_speakers_close_lines_into_one_turn() {
        let header = Header {
            title: "Sync mingguan".into(),
            started: Some("2026-11-07 10:02".into()),
            duration_ms: Some(2_527_000),
            model: "Qwen3-ASR 0.6B".into(),
        };
        let segments = vec![
            seg(0, 6_000, Some(Others), "Oke, kita mulai ya."),
            seg(7_000, 12_000, Some(Others), "Agenda hari ini tiga hal."),
            seg(15_000, 18_000, Some(Me), "Onboarding design udah final."),
            seg(862_000, 866_000, Some(Others), "Can we push the release?"),
            seg(870_000, 872_000, Some(Me), "Boleh."),
        ];
        let md = markdown(&header, &segments, LABELS_ID);
        assert_eq!(
            md,
            "# Sync mingguan\n\n*2026-11-07 10:02 · 42:07 · Qwen3-ASR 0.6B*\n\
             \n**00:00 · Peserta:** Oke, kita mulai ya. Agenda hari ini tiga hal.\n\
             \n**00:15 · Saya:** Onboarding design udah final.\n\
             \n**14:22 · Peserta:** Can we push the release?\n\
             \n**14:30 · Saya:** Boleh.\n"
        );
        let english = markdown(&header, &segments[2..3], LABELS_EN);
        assert!(english.contains("**00:15 · Me:**"));
    }

    #[test]
    fn a_long_turn_is_broken_up_so_its_times_stay_useful() {
        let segments: Vec<Segment> = (0..10)
            .map(|i| seg(i * 10_000, i * 10_000 + 9_000, Some(Me), "kata"))
            .collect();
        let header = Header {
            title: "x".into(),
            started: None,
            duration_ms: None,
            model: "m".into(),
        };
        let md = markdown(&header, &segments, LABELS_ID);
        assert_eq!(md.matches("Saya:**").count(), 2, "{md}");
        assert!(md.contains("**01:00 · Saya:**"));
    }

    #[test]
    fn a_video_without_a_sidecar_is_downmixed_unlabelled() {
        assert_eq!(channel_plan(Path::new("a.mp4"), 2, None), (true, vec![None]));
        assert_eq!(channel_plan(Path::new("A.MP4"), 2, None), (true, vec![None]));
        assert_eq!(channel_plan(Path::new("a.mp4"), 6, Some(&[])), (true, vec![None]));
    }

    #[test]
    fn a_vale_video_reads_its_mix_channel() {
        let mix = ["mix".to_string()];
        assert_eq!(channel_plan(Path::new("a.mp4"), 1, Some(&mix)), (false, vec![None]));
        assert_eq!(channel_plan(Path::new("a.mp4"), 1, None), (false, vec![None]));
    }

    #[test]
    fn an_m4a_keeps_its_two_labels() {
        assert_eq!(
            channel_plan(Path::new("a.m4a"), 2, None),
            (false, vec![Some(Me), Some(Others)])
        );
    }

    #[test]
    fn speakers_follow_the_sidecar_or_the_recorder_layout() {
        let both = vec!["mic".to_string(), "system".to_string()];
        assert_eq!(speakers(2, Some(&both)), vec![Some(Me), Some(Others)]);
        assert_eq!(speakers(1, Some(&["system".to_string()])), vec![Some(Others)]);
        assert_eq!(speakers(1, Some(&["mic".to_string()])), vec![Some(Me)]);
        assert_eq!(speakers(2, None), vec![Some(Me), Some(Others)]);
        assert_eq!(speakers(1, None), vec![None]);
        // A sidecar that does not match the file is not trusted.
        assert_eq!(speakers(1, Some(&both)), vec![None]);
    }

    #[test]
    fn the_files_sit_next_to_the_audio() {
        let p = paths(Path::new(r"C:\Users\x\Documents\Vale\Rekaman\2026-11-07 1430 Zoom.m4a"));
        assert_eq!(p.md, PathBuf::from(r"C:\Users\x\Documents\Vale\Rekaman\2026-11-07 1430 Zoom.md"));
        assert_eq!(p.srt, PathBuf::from(r"C:\Users\x\Documents\Vale\Rekaman\2026-11-07 1430 Zoom.srt"));
        assert_eq!(
            p.sidecar,
            PathBuf::from(r"C:\Users\x\Documents\Vale\Rekaman\2026-11-07 1430 Zoom.vale.json")
        );
        let dotted = paths(Path::new("2026-11-07 1430 v1.2 review.m4a"));
        assert_eq!(dotted.md, PathBuf::from("2026-11-07 1430 v1.2 review.md"));
    }

    const SIDECAR: &str = r#"{
  "v": 1,
  "kind": "audio",
  "title": "Sync mingguan",
  "started": "2026-11-07T10:02:00+07:00",
  "duration_ms": 2527000,
  "channels": ["mic", "system"],
  "app": "Teams",
  "cursor_log": null,
  "future_key": {"kept": true}
}"#;

    fn info() -> TranscriptInfo {
        TranscriptInfo {
            model: "qwen3-asr-0.6b".into(),
            engine: "0.4.1".into(),
            language: Some("id".into()),
            created: "2026-11-07T11:00:00+07:00".into(),
            md: "x.md".into(),
            srt: "x.srt".into(),
            segments: 120,
            elapsed_ms: 360_000,
        }
    }

    #[test]
    fn the_sidecar_gains_a_transcript_and_keeps_everything_else() {
        let text = with_transcript(SIDECAR, &info()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["future_key"]["kept"], true);
        assert_eq!(value["app"], "Teams");
        assert_eq!(value["transcript"]["model"], "qwen3-asr-0.6b");
        assert_eq!(value["transcript"]["segments"], 120);
        // Key order is kept, the new key goes last.
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(keys.first().map(|k| k.as_str()), Some("v"));
        assert_eq!(keys.last().map(|k| k.as_str()), Some("transcript"));
        // Writing again replaces the transcript, it does not add a second.
        let again = with_transcript(&text, &TranscriptInfo { segments: 3, ..info() }).unwrap();
        assert_eq!(again.matches("\"transcript\"").count(), 1);
        assert!(with_transcript("[1, 2]", &info()).is_err());
        assert!(with_transcript("{broken", &info()).is_err());
    }

    #[test]
    fn the_sidecar_tells_channels_and_title() {
        assert_eq!(
            sidecar_channels(SIDECAR),
            Some(vec!["mic".to_string(), "system".to_string()])
        );
        assert_eq!(sidecar_title(SIDECAR).as_deref(), Some("Sync mingguan"));
        assert_eq!(sidecar_title(r#"{"title": " "}"#), None);
        assert_eq!(sidecar_channels("{}"), None);
    }

    #[test]
    fn vales_header_is_told_from_someone_elses_markdown() {
        let header = |model: String| Header {
            title: "Rapat".into(),
            started: Some("2026-11-07 10:02".into()),
            duration_ms: Some(60_000),
            model,
        };
        let segs = [seg(0, 1_000, Some(Me), "halo")];
        for m in &crate::transcribe::catalog::MODELS {
            assert!(is_vale_markdown(&markdown(&header(model_label(m)), &segs, LABELS_ID)));
        }
        assert!(is_vale_markdown(&markdown(&header("model.gguf".into()), &[], LABELS_EN)));
        assert!(!is_vale_markdown("# Rapat\n\nCatatan saya sendiri.\n"));
        assert!(!is_vale_markdown("# Rapat\n\n*penting*\n"));
        assert!(!is_vale_markdown("Catatan\n"));
        assert!(!is_vale_markdown(""));
    }

    #[test]
    fn the_sidecar_names_only_plain_md_and_srt_files() {
        let audio = Path::new(r"C:\r\a.m4a");
        let named = |md: &str, srt: &str| {
            let v = serde_json::json!({ "transcript": { "md": md, "srt": srt } });
            named_files(audio, &v)
        };
        assert_eq!(
            named("a.md", "a.srt"),
            Some(Files {
                md: PathBuf::from(r"C:\r\a.md"),
                srt: PathBuf::from(r"C:\r\a.srt"),
            })
        );
        assert_eq!(named("a.transcript.md", "a.transcript.srt").unwrap().md, PathBuf::from(r"C:\r\a.transcript.md"));
        assert_eq!(named(r"..\b.md", "a.srt"), None, "not beside the audio");
        assert_eq!(named("a.m4a", "a.srt"), None, "not a transcript file");
        assert_eq!(named("a.md", ".srt"), None);
        assert_eq!(named_files(audio, &serde_json::json!({ "title": "x" })), None);
    }

    fn files_dir(name: &str) -> PathBuf {
        let store = crate::transcribe::store::tests::temp_store(name);
        store.root().to_path_buf()
    }

    #[test]
    fn a_new_transcript_never_lands_on_someone_elses_md_or_srt() {
        let dir = files_dir("output-target");
        let audio = dir.join("meeting.wav");
        let plain = Files {
            md: dir.join("meeting.md"),
            srt: dir.join("meeting.srt"),
        };
        let own = Files {
            md: dir.join("meeting.transcript.md"),
            srt: dir.join("meeting.transcript.srt"),
        };
        // Nothing beside it: the plain names, read and written.
        assert_eq!(target_files(&audio, None), plain);
        assert_eq!(transcript_files(&audio, None), plain);
        // The user's own notes: Vale writes beside them.
        std::fs::write(&plain.md, "# Meeting\n\nmy notes\n").unwrap();
        assert_eq!(target_files(&audio, None), own);
        // An .srt alone is not Vale's either.
        std::fs::remove_file(&plain.md).unwrap();
        std::fs::write(&plain.srt, "1\n00:00:00,000 --> 00:00:01,000\nhi\n\n").unwrap();
        assert_eq!(target_files(&audio, None), own);
        // Vale's own .md (its header) with its .srt: written again in place.
        let header = Header {
            title: "Meeting".into(),
            started: None,
            duration_ms: None,
            model: model_label(&crate::transcribe::catalog::MODELS[1]),
        };
        std::fs::write(&plain.md, markdown(&header, &[], LABELS_ID)).unwrap();
        assert_eq!(target_files(&audio, None), plain);
        // Once Vale wrote the .transcript files, they are read and written.
        std::fs::write(&plain.md, "# Meeting\n\nmy notes\n").unwrap();
        std::fs::write(&own.srt, "").unwrap();
        assert_eq!(target_files(&audio, None), own);
        assert_eq!(transcript_files(&audio, None), own);
        // The sidecar's names win.
        let side = serde_json::json!({ "transcript": { "md": "meeting.md", "srt": "meeting.srt" } });
        assert_eq!(target_files(&audio, Some(&side)), plain);
        assert_eq!(transcript_files(&audio, Some(&side)), plain);
    }

    #[test]
    fn an_atomic_write_replaces_the_file() {
        let store = crate::transcribe::store::tests::temp_store("output-write");
        let path = store.root().join("a.vale.json");
        write_atomic(&path, "one").unwrap();
        write_atomic(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert!(!store.root().join("a.vale.json.tmp").exists());
    }
}
