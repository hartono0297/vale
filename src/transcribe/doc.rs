//! A recording's transcript as the app uses it: loaded from the sidecar and
//! the `.srt` next to the audio, and turned into the texts of Salin, Salin +
//! prompt ringkasan and Jadikan catatan.
//!
//! Pure file and text work, so it is tested without the engine.

use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

use super::output::{self, Labels, Segment, Speaker};
use crate::i18n::{self, Key, Lang};

/// One recording's transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub audio: PathBuf,
    /// The transcript's `.md` (`output::transcript_files`; it may not exist).
    pub md: PathBuf,
    pub title: String,
    pub app: Option<String>,
    /// Local time.
    pub started: Option<NaiveDateTime>,
    pub duration_ms: Option<u64>,
    /// The catalog key the sidecar names.
    pub model: Option<String>,
    pub language: Option<String>,
    pub two_channel: bool,
    /// The transcript's `.srt` exists or the sidecar has a `"transcript"`. A
    /// finished run with no speech has the second and no segments.
    pub has_transcript: bool,
    pub segments: Vec<Segment>,
}

/// Lines of a transcript that "Jadikan catatan" keeps.
pub const NOTE_LINES: usize = 20;

/// Loads the transcript of `audio`: the sidecar and the `.srt` next to it
/// (`<stem>.srt`, or `<stem>.transcript.srt` beside someone else's files).
/// Never fails: what is missing or unreadable stays at its default, and the
/// title falls back to `fallback_title`.
pub fn load(audio: &Path, fallback_title: &str) -> Doc {
    read(audio, fallback_title, true)
}

/// `load` without reading the `.srt`: what the sidecar says about the
/// recording and its transcript, and no lines. For a running job, whose lines
/// come from the worker; a one-sidecar-less file reads as one channel.
pub fn load_meta(audio: &Path, fallback_title: &str) -> Doc {
    read(audio, fallback_title, false)
}

fn read(audio: &Path, fallback_title: &str, with_srt: bool) -> Doc {
    let sidecar = std::fs::read_to_string(output::paths(audio).sidecar).ok();
    let value: Option<serde_json::Value> = sidecar
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let files = output::transcript_files(audio, value.as_ref());
    let srt = with_srt
        .then(|| std::fs::read(&files.srt).ok())
        .flatten()
        .map(|b| String::from_utf8_lossy(&b).into_owned());

    let str_at = |v: &serde_json::Value, key: &str| {
        v.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let transcript = value.as_ref().and_then(|v| v.get("transcript"));
    let channels = sidecar.as_deref().and_then(output::sidecar_channels);
    let two_channel = match &channels {
        Some(names) => names.len() == 2,
        None => srt
            .as_deref()
            .is_some_and(|t| parse_srt(t, true).iter().any(|s| s.speaker.is_some())),
    };

    // A sidecar with no start (the epoch) says nothing about when.
    let started = value
        .as_ref()
        .and_then(|v| str_at(v, "started"))
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .filter(|d| d.timestamp() > 0)
        .map(|d| d.with_timezone(&chrono::Local).naive_local());

    Doc {
        audio: audio.to_path_buf(),
        md: files.md,
        title: sidecar
            .as_deref()
            .and_then(output::sidecar_title)
            .unwrap_or_else(|| fallback_title.to_string()),
        app: value.as_ref().and_then(|v| str_at(v, "app")),
        started,
        duration_ms: value
            .as_ref()
            .and_then(|v| v.get("duration_ms"))
            .and_then(serde_json::Value::as_u64),
        model: transcript.and_then(|t| str_at(t, "model")),
        language: transcript.and_then(|t| str_at(t, "language")),
        two_channel,
        has_transcript: srt.is_some() || transcript.is_some_and(serde_json::Value::is_object),
        segments: srt.map(|t| parse_srt(&t, two_channel)).unwrap_or_default(),
    }
}

/// `HH:MM:SS,mmm` (a dot works too) in milliseconds.
fn parse_srt_time(text: &str) -> Option<u64> {
    let (clock, frac) = text.trim().split_once([',', '.'])?;
    let mut parts = clock.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || frac.is_empty() || frac.len() > 3 {
        return None;
    }
    let num = |t: &str| t.parse::<u64>().ok();
    // "5" after the comma is 500 ms.
    let ms = num(frac)? * 10u64.pow(3 - frac.len() as u32);
    // A hand-edited hour field can be huge: such a cue is skipped.
    num(h)?
        .checked_mul(3600)?
        .checked_add(num(m)?.checked_mul(60)?)?
        .checked_add(num(s)?)?
        .checked_mul(1000)?
        .checked_add(ms)
}

/// The cues of a SubRip file, found by their `-->` lines. A speaker label
/// before the text is read only when `two_channel`: a one-channel file's
/// "Saya: halo" is what was said.
pub fn parse_srt(text: &str, two_channel: bool) -> Vec<Segment> {
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().collect();
    let arrows: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("-->"))
        .collect();
    let mut out = Vec::new();
    for (n, &at) in arrows.iter().enumerate() {
        let Some((start, end)) = lines[at].split_once("-->") else {
            continue;
        };
        let (Some(start_ms), Some(end_ms)) = (parse_srt_time(start), parse_srt_time(end)) else {
            continue;
        };
        // The cue's lines run to the first blank line, or to the next cue's
        // number when the blank line between them is missing.
        let mut stop = arrows.get(n + 1).copied().unwrap_or(lines.len());
        if n + 1 < arrows.len() && stop > at + 1 {
            let before = lines[stop - 1].trim();
            if !before.is_empty() && before.chars().all(|c| c.is_ascii_digit()) {
                stop -= 1;
            }
        }
        let body: Vec<&str> = lines[at + 1..stop]
            .iter()
            .map(|l| l.trim())
            .take_while(|l| !l.is_empty())
            .collect();
        let joined = body.join(" ");
        let (speaker, text) = if two_channel {
            split_label(&joined)
        } else {
            (None, joined.as_str())
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        out.push(Segment {
            start_ms,
            end_ms,
            speaker,
            text: text.to_string(),
        });
    }
    // A hand-edited file may list cues out of order; the playing line
    // (`current_line`) needs them by start.
    out.sort_by_key(|s| s.start_ms);
    out
}

/// The speaker a line starts with (`Saya: `, `Peserta: `, `Me: `, `Others: `)
/// and the rest of it.
fn split_label(line: &str) -> (Option<Speaker>, &str) {
    for set in [output::LABELS_ID, output::LABELS_EN] {
        for who in [Speaker::Me, Speaker::Others] {
            if let Some(rest) = line
                .strip_prefix(set.of(who))
                .and_then(|r| r.strip_prefix(": "))
            {
                return (Some(who), rest);
            }
        }
    }
    (None, line)
}

pub fn labels(lang: Lang) -> Labels {
    match lang {
        Lang::Id => output::LABELS_ID,
        Lang::En => output::LABELS_EN,
    }
}

/// The catalog's name for a model key, else the key itself.
pub fn model_name(key: &str) -> String {
    super::catalog::model(key).map_or_else(|| key.to_string(), |m| m.name.to_string())
}

/// "Indonesia" / "Indonesian" for `"id"`, "Inggris" / "English" for `"en"`,
/// any other code in capitals.
pub fn language_name(lang: Lang, code: &str) -> String {
    match code {
        "id" => i18n::text(lang, Key::TrLangId).to_string(),
        "en" => i18n::text(lang, Key::TrLangEn).to_string(),
        other => other.to_uppercase(),
    }
}

/// "7 Nov 10.02": the start's date and time in the language's style.
fn when(doc: &Doc, lang: Lang) -> Option<String> {
    doc.started.map(|s| {
        format!(
            "{} {}",
            i18n::short_date(lang, s.date()),
            i18n::time_hm(lang, s)
        )
    })
}

/// The known parts of date and time, length, model and language, joined by
/// " · ".
pub fn meta_line(doc: &Doc, lang: Lang) -> String {
    let parts = [
        when(doc, lang),
        doc.duration_ms
            .map(|ms| i18n::rec_duration(lang, std::time::Duration::from_millis(ms))),
        doc.model.as_deref().map(model_name),
        doc.language.as_deref().map(|c| language_name(lang, c)),
    ];
    parts
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// One line of the transcript: "14:22 Peserta: pagi".
fn line_of(segment: &Segment, labels: Labels) -> String {
    let time = output::short_time(segment.start_ms);
    match segment.speaker {
        Some(who) => format!("{time} {}: {}", labels.of(who), segment.text),
        None => format!("{time} {}", segment.text),
    }
}

/// The title, the meta line and every line of the transcript: what Salin
/// copies.
pub fn plain_text(doc: &Doc, lang: Lang) -> String {
    let labels = labels(lang);
    let mut out = format!("{}\n{}\n\n", doc.title, meta_line(doc, lang));
    for segment in &doc.segments {
        out.push_str(&line_of(segment, labels));
        out.push('\n');
    }
    out
}

/// The summary prompt, then `plain_text`: what Salin + prompt ringkasan copies.
pub fn prompt_text(doc: &Doc, lang: Lang) -> String {
    format!(
        "{}\n\n{}",
        i18n::text(lang, Key::TrPrompt),
        plain_text(doc, lang)
    )
}

/// What Jadikan catatan puts in the note: the title, the date, the first
/// `NOTE_LINES` lines and, when there are more, where the whole transcript is.
pub fn note_text(doc: &Doc, lang: Lang) -> String {
    let labels = labels(lang);
    let mut out = format!("{}\n", doc.title);
    if let Some(when) = when(doc, lang) {
        out.push_str(&when);
        out.push('\n');
    }
    out.push('\n');
    for segment in doc.segments.iter().take(NOTE_LINES) {
        out.push_str(&line_of(segment, labels));
        out.push('\n');
    }
    if doc.segments.len() > NOTE_LINES {
        out.push_str(&format!(
            "\n{}: {}",
            i18n::text(lang, Key::TrFullTranscript),
            doc.md.display()
        ));
    }
    out
}

/// The line playing at `position_ms`: the last segment that has started.
pub fn current_line(segments: &[Segment], position_ms: u64) -> Option<usize> {
    segments
        .partition_point(|s| s.start_ms <= position_ms)
        .checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::output::{LABELS_EN, LABELS_ID};
    use super::*;
    use chrono::NaiveDate;
    use Speaker::{Me, Others};

    fn seg(start: u64, end: u64, speaker: Option<Speaker>, text: &str) -> Segment {
        Segment {
            start_ms: start,
            end_ms: end,
            speaker,
            text: text.into(),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("doc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    fn started() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 7)
            .unwrap()
            .and_hms_opt(10, 2, 0)
            .unwrap()
    }

    fn doc(segments: Vec<Segment>) -> Doc {
        Doc {
            audio: PathBuf::from(r"C:\r\a.m4a"),
            md: PathBuf::from(r"C:\r\a.md"),
            title: "Sync mingguan".into(),
            app: None,
            started: Some(started()),
            duration_ms: Some(42 * 60_000 + 10_000),
            model: Some("whisper-large-v3-turbo".into()),
            language: Some("id".into()),
            two_channel: true,
            has_transcript: true,
            segments,
        }
    }

    fn many(n: u64) -> Vec<Segment> {
        (0..n)
            .map(|i| seg(i * 1000, i * 1000 + 900, Some(Me), &format!("baris {i}")))
            .collect()
    }

    #[test]
    fn parse_srt_reads_what_output_writes() {
        let segs = vec![
            seg(0, 2_500, Some(Me), "halo semua"),
            seg(2_600, 9_000, Some(Others), "selamat pagi"),
            seg(3_700_000, 3_701_500, None, "tanpa pembicara"),
        ];
        for labels in [LABELS_ID, LABELS_EN] {
            assert_eq!(parse_srt(&output::srt(&segs, labels), true), segs);
        }
    }

    #[test]
    fn parse_srt_tolerates_crlf_bom_and_missing_blank_lines() {
        let text = "\u{feff}1\r\n00:00:00,000 --> 00:00:02,000\r\nSaya: halo\r\n2\r\n00:00:03,000 --> 00:00:04,500\r\nPeserta: pagi\r\n";
        assert_eq!(
            parse_srt(text, true),
            vec![
                seg(0, 2_000, Some(Me), "halo"),
                seg(3_000, 4_500, Some(Others), "pagi"),
            ]
        );
    }

    #[test]
    fn parse_srt_reads_labels_only_for_two_channels() {
        let text = "1\n00:00:00,000 --> 00:00:02,000\nSaya: halo\n\n";
        assert_eq!(parse_srt(text, false), vec![seg(0, 2_000, None, "Saya: halo")]);
        assert_eq!(parse_srt(text, true), vec![seg(0, 2_000, Some(Me), "halo")]);
    }

    #[test]
    fn parse_srt_skips_a_bad_time_line() {
        let text = "1\n00:00:xx,000 --> 00:00:02,000\nrusak\n\n2\n00:00:03,000 --> 00:00:04,000\nutuh\n\n";
        assert_eq!(parse_srt(text, false), vec![seg(3_000, 4_000, None, "utuh")]);
    }

    #[test]
    fn parse_srt_skips_a_cue_whose_time_overflows() {
        let text = "1
99999999999999999:00:00,000 --> 99999999999999999:00:01,000
terlalu jauh

                    2
5124095576030431:00:00,000 --> 00:00:01,000
kali seribu

                    3
00:00:03,000 --> 00:00:04,000
utuh

";
        assert_eq!(parse_srt(text, false), vec![seg(3_000, 4_000, None, "utuh")]);
    }

    #[test]
    fn parse_srt_puts_cues_out_of_order_by_their_start() {
        let text = "1
00:00:05,000 --> 00:00:06,000
ketiga

                    2
00:00:01,000 --> 00:00:02,000
pertama

                    3
00:00:03,000 --> 00:00:04,000
kedua

";
        let segs = parse_srt(text, false);
        assert_eq!(
            segs,
            vec![
                seg(1_000, 2_000, None, "pertama"),
                seg(3_000, 4_000, None, "kedua"),
                seg(5_000, 6_000, None, "ketiga"),
            ]
        );
        assert_eq!(current_line(&segs, 3_500), Some(1));
    }

    #[test]
    fn load_finds_a_transcript_vale_wrote_beside_someone_elses_files() {
        let dir = temp_dir("own");
        std::fs::write(dir.join("a.md"), "# a

my notes
").unwrap();
        std::fs::write(dir.join("a.transcript.srt"), "1
00:00:01,000 --> 00:00:02,000
halo

").unwrap();
        let d = load(&dir.join("a.m4a"), "Rekaman");
        assert!(d.has_transcript);
        assert_eq!(d.md, dir.join("a.transcript.md"));
        assert_eq!(d.segments, vec![seg(1_000, 2_000, None, "halo")]);
    }

    #[test]
    fn parse_srt_joins_a_cue_s_lines() {
        let text = "1\n00:00:00,000 --> 00:00:02,000\nbaris satu\nbaris dua\n\n";
        assert_eq!(
            parse_srt(text, false),
            vec![seg(0, 2_000, None, "baris satu baris dua")]
        );
    }

    #[test]
    fn load_of_a_missing_recording_is_an_empty_doc() {
        let dir = temp_dir("missing");
        let d = load(&dir.join("none.m4a"), "Rekaman");
        assert_eq!(d.title, "Rekaman");
        assert!(d.segments.is_empty());
        assert!(!d.has_transcript);
        assert_eq!(d.md, dir.join("none.md"));
        assert_eq!(d.started, None);
        assert_eq!(d.model, None);
    }

    #[test]
    fn load_reads_the_sidecar_and_the_srt() {
        let dir = temp_dir("sidecar");
        let audio = dir.join("2026-11-07 1002 Meet.m4a");
        std::fs::write(
            dir.join("2026-11-07 1002 Meet.vale.json"),
            r#"{"v":1,"kind":"audio","title":"Sync mingguan","started":"2026-11-07T10:02:00+07:00","duration_ms":2520000,"channels":["mic","system"],"app":"Teams","transcript":{"model":"whisper-large-v3-turbo","engine":"0.4.1","language":"id","created":"2026-11-07T11:00:00+07:00","md":"2026-11-07 1002 Meet.md","srt":"2026-11-07 1002 Meet.srt","segments":2,"elapsed_ms":90000}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("2026-11-07 1002 Meet.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nSaya: halo\n\n2\n00:00:03,000 --> 00:00:04,000\nPeserta: pagi\n\n",
        )
        .unwrap();
        let d = load(&audio, "fallback");
        assert_eq!(d.audio, audio);
        assert_eq!(d.md, dir.join("2026-11-07 1002 Meet.md"));
        assert_eq!(d.title, "Sync mingguan");
        assert_eq!(d.app.as_deref(), Some("Teams"));
        assert!(d.started.is_some());
        assert_eq!(d.duration_ms, Some(2_520_000));
        assert_eq!(d.model.as_deref(), Some("whisper-large-v3-turbo"));
        assert_eq!(d.language.as_deref(), Some("id"));
        assert!(d.two_channel);
        assert!(d.has_transcript);
        assert_eq!(
            d.segments,
            vec![
                seg(1_000, 2_000, Some(Me), "halo"),
                seg(3_000, 4_000, Some(Others), "pagi"),
            ]
        );
    }

    #[test]
    fn load_meta_reads_the_sidecar_and_leaves_the_srt() {
        let dir = temp_dir("meta");
        std::fs::write(
            dir.join("a.vale.json"),
            r#"{"v":1,"title":"Sync mingguan","duration_ms":60000,"channels":["mic","system"],"transcript":{"model":"whisper-small","segments":1}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("a.srt"), "1\n00:00:01,000 --> 00:00:02,000\nSaya: halo\n\n").unwrap();
        let audio = dir.join("a.m4a");
        let meta = load_meta(&audio, "Rekaman");
        assert!(meta.segments.is_empty());
        assert_eq!(meta, Doc { segments: Vec::new(), ..load(&audio, "Rekaman") });
    }

    #[test]
    fn load_of_an_empty_result_has_a_transcript_and_no_segments() {
        let dir = temp_dir("empty");
        std::fs::write(
            dir.join("a.vale.json"),
            r#"{"v":1,"title":"","channels":["mic"],"transcript":{"model":"whisper-small","segments":0}}"#,
        )
        .unwrap();
        let d = load(&dir.join("a.m4a"), "Rekaman");
        assert!(d.has_transcript);
        assert!(d.segments.is_empty());
        assert!(!d.two_channel);
        assert_eq!(d.title, "Rekaman");
    }

    #[test]
    fn load_without_a_sidecar_sniffs_the_labels_in_the_srt() {
        let dir = temp_dir("sniff");
        std::fs::write(
            dir.join("a.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nMe: hi\n\n",
        )
        .unwrap();
        let d = load(&dir.join("a.m4a"), "Rekaman");
        assert!(d.has_transcript);
        assert!(d.two_channel);
        assert_eq!(d.segments, vec![seg(1_000, 2_000, Some(Me), "hi")]);
    }

    #[test]
    fn meta_line_in_both_languages() {
        let d = doc(vec![]);
        let when = |lang| {
            format!(
                "{} {}",
                i18n::short_date(lang, started().date()),
                i18n::time_hm(lang, started())
            )
        };
        assert_eq!(
            meta_line(&d, Lang::Id),
            format!("{} · 42 mnt · Whisper large-v3-turbo · Indonesia", when(Lang::Id))
        );
        assert_eq!(
            meta_line(&d, Lang::En),
            format!("{} · 42 min · Whisper large-v3-turbo · Indonesian", when(Lang::En))
        );
    }

    #[test]
    fn meta_line_leaves_out_what_is_unknown() {
        let mut d = doc(vec![]);
        d.started = None;
        d.model = None;
        d.language = Some("fr".into());
        assert_eq!(meta_line(&d, Lang::Id), "42 mnt · FR");
        d.duration_ms = None;
        d.language = None;
        assert_eq!(meta_line(&d, Lang::Id), "");
    }

    #[test]
    fn names_of_models_and_languages() {
        assert_eq!(model_name("whisper-small"), "Whisper small");
        assert_eq!(model_name("unknown-model"), "unknown-model");
        assert_eq!(language_name(Lang::Id, "en"), "Inggris");
        assert_eq!(language_name(Lang::En, "en"), "English");
        assert_eq!(language_name(Lang::En, "ja"), "JA");
    }

    #[test]
    fn plain_text_has_times_and_labels() {
        let d = doc(vec![
            seg(7_000, 9_000, Some(Me), "halo"),
            seg(862_000, 865_000, Some(Others), "pagi"),
            seg(3_723_000, 3_725_000, None, "tanpa"),
        ]);
        let meta = meta_line(&d, Lang::Id);
        assert_eq!(
            plain_text(&d, Lang::Id),
            format!("Sync mingguan\n{meta}\n\n00:07 Saya: halo\n14:22 Peserta: pagi\n1:02:03 tanpa\n")
        );
        assert!(plain_text(&d, Lang::En).contains("00:07 Me: halo\n14:22 Others: pagi\n"));
    }

    #[test]
    fn prompt_text_starts_with_the_spec_prompt() {
        let d = doc(vec![seg(0, 1_000, Some(Me), "halo")]);
        let p = prompt_text(&d, Lang::Id);
        assert!(p.starts_with(i18n::text(Lang::Id, Key::TrPrompt)));
        assert!(p.starts_with("Ringkas transkrip meeting di bawah ini. Tulis: keputusan yang diambil, action item beserta siapa yang mengerjakan dan tenggatnya kalau disebut, dan pertanyaan yang belum terjawab. \"Saya\" adalah orang yang merekam, \"Peserta\" adalah orang lain di meeting."));
        assert_eq!(
            p,
            format!("{}\n\n{}", i18n::text(Lang::Id, Key::TrPrompt), plain_text(&d, Lang::Id))
        );
    }

    #[test]
    fn note_text_keeps_twenty_lines_and_the_md_path() {
        let when = format!(
            "{} {}",
            i18n::short_date(Lang::Id, started().date()),
            i18n::time_hm(Lang::Id, started())
        );
        let long = note_text(&doc(many(25)), Lang::Id);
        assert!(long.starts_with(&format!("Sync mingguan\n{when}\n\n00:00 Saya: baris 0\n")));
        assert!(long.contains("00:19 Saya: baris 19"));
        assert!(!long.contains("baris 20"));
        assert!(long.ends_with("baris 19\n\nTranskrip lengkap: C:\\r\\a.md"));
        assert_eq!(long.matches("Saya: baris").count(), NOTE_LINES);

        let exact = note_text(&doc(many(20)), Lang::Id);
        assert!(exact.contains("baris 19"));
        assert!(!exact.contains("Transkrip lengkap"));
        assert!(exact.ends_with("baris 19\n"));

        let mut no_date = doc(many(1));
        no_date.started = None;
        assert_eq!(
            note_text(&no_date, Lang::En),
            "Sync mingguan\n\n00:00 Me: baris 0\n"
        );
    }

    #[test]
    fn current_line_follows_the_position() {
        let segs = many(3);
        assert_eq!(current_line(&[], 5_000), None);
        assert_eq!(current_line(&segs, 0), Some(0));
        assert_eq!(current_line(&segs, 999), Some(0));
        assert_eq!(current_line(&segs, 1_000), Some(1));
        assert_eq!(current_line(&segs, 99_000), Some(2));
        let late = vec![seg(500, 900, None, "x")];
        assert_eq!(current_line(&late, 100), None);
    }
}
