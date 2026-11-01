//! Synced lyrics for the notch: LRC parsing, the line at a playback position,
//! and lookups at LRCLIB on a worker thread, with a disk cache (results and
//! 24 h "not found" markers) behind `App`'s in-memory LRU.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::google::http::{Http, form_encode};
use crate::model::{Event, MediaInfo, Timeline};

/// Tracks remembered in memory (with or without lyrics).
pub const MEMORY_SLOTS: usize = 50;
/// A "not found" answer is trusted this long before asking again.
pub const NEGATIVE_TTL_SECS: i64 = 24 * 3600;
/// A fresh reading closer than this to where playback was predicted is the
/// same playback: the media watcher sends no new timeline for it. Anything
/// farther is sent, so a reading that is more accurate than the prediction
/// replaces it (a bias is never kept; a seek is just a large one).
pub const RESYNC_MS: u64 = 100;
/// A line shows this long after its time, never before: the player's position
/// is where it *plays*, and the sound reaches the ear later (output buffers,
/// Bluetooth 150-300 ms), so a line at exactly its time looks early. 3.2.0
/// went 150 ms *early* instead, which read as "too fast".
pub const OUTPUT_LAG_MS: i64 = 100;
/// Crossfade between two lyric lines.
pub const FADE: Duration = Duration::from_millis(150);
/// The worker waits this long for a newer query before looking one up, so a
/// track change reported in pieces (title first, timeline later) is looked
/// up once, with the new length.
pub const SETTLE: Duration = Duration::from_secs(1);
/// A lookup that failed on the way (no network, a stalled or refused
/// request) is asked again this long after, up to `RETRIES` times per track.
pub const RETRY_AFTER: Duration = Duration::from_secs(30);
pub const RETRIES: u32 = 3;
/// One request to LRCLIB that has not answered by then has failed.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A length change this large under the same title means the next track's
/// timeline arrived before its title.
const LENGTH_JUMP_MS: u64 = 2_000;
const USER_AGENT: &str = concat!(
    "Vale/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/hartono0297/dynamic-island)"
);
const API: &str = "https://lrclib.net/api/get";
const SEARCH_API: &str = "https://lrclib.net/api/search";
/// LRC files use this extension. The older `.lrc` files (written by a pick
/// that ignored how the playing version differs from the upload, and without
/// the intro shift) are never read, so those tracks are looked up again once.
const LRC_EXT: &str = "lrc2";
/// Not-found markers use this extension: ones written by an older, weaker
/// lookup (`.none` .. `.none3`: no retry of a busy server, no free-text
/// search, no "Artist - Title" split, no intro shift for a music video whose
/// lyrics are only found that way) are ignored, so a track they gave up on is
/// asked again.
const NONE_EXT: &str = "none4";
/// A search result is the track only when its length is this close.
const SEARCH_DURATION_MS: u64 = 3_000;
/// The whole lookup (every step, retries and waits included) stops sending
/// after this long; with nothing found that is a failure, not "no lyrics".
const LOOKUP_BUDGET: Duration = Duration::from_secs(30);
/// A busy server (429, 5xx) is asked again this many times per URL.
const HTTP_RETRIES: u32 = 2;
/// `pick_best` weighs at most this many candidates, and the first
/// `MATCH_LINES` timed lines of each; a pair needs `MIN_MATCHED` shared lines
/// to say how far apart two versions are.
const MAX_CANDIDATES: usize = 20;
const MATCH_LINES: usize = 300;
const MIN_MATCHED: usize = 3;
/// An upload this close to the playing length is the same recording: when
/// any is, only those are weighed.
const TIGHT_DURATION_MS: u64 = 1_500;
/// Two uploads whose timings differ by at most this are one family (copies of
/// one timing), whatever their lengths.
const FAMILY_MS: u64 = 500;
/// The playing version is longer than its family's reference by more than
/// `MIN_INTRO_MS` and at most `MAX_INTRO_MS`: its extra intro is added to
/// every line.
const MIN_INTRO_MS: u64 = 3_000;
const MAX_INTRO_MS: u64 = 60_000;
/// LRCLIB answers and cached `.lrc2` files larger than this are refused
/// unparsed: a song's synced lyrics are a few KB.
pub const MAX_BODY_BYTES: usize = 512 * 1024;
/// `parse_lrc` keeps at most this many time tags of one line (more are
/// skipped), this many characters of its text, and this many lines.
const MAX_TAGS_PER_LINE: usize = 16;
const MAX_LINE_CHARS: usize = 500;
const MAX_LINES: usize = 5_000;

/// One timed line; an empty `text` is an instrumental gap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LrcLine {
    pub at_ms: u64,
    pub text: String,
}

pub type Lines = Arc<Vec<LrcLine>>;

/// What the worker found for `key`. `lines: None` is "no synced lyrics";
/// `transient` marks a network failure, which is not remembered.
#[derive(Clone, Debug, PartialEq)]
pub struct LyricsFound {
    pub key: String,
    pub lines: Option<Lines>,
    pub transient: bool,
}

/// A track to look up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub key: String,
    pub artist: String,
    pub title: String,
    pub duration_ms: u64,
    /// The length may still be the previous track's (no timeline reported
    /// since the title changed): a "not found" for it is not remembered.
    pub provisional: bool,
}

/// Parses LRC text: every `[mm:ss.xx]` tag on a line times a copy of its text,
/// `[offset:±ms]` shifts all lines (positive = earlier), metadata tags and
/// lines without a time tag are skipped. The result is sorted by time
/// (stable: equal times keep their order). Untrusted text stays bounded: a
/// line keeps its first `MAX_TAGS_PER_LINE` times and `MAX_LINE_CHARS`
/// characters, and at most `MAX_LINES` lines come out.
pub fn parse_lrc(text: &str) -> Vec<LrcLine> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut offset: i64 = 0;
    let mut lines: Vec<(u64, String)> = Vec::new();
    for raw in text.lines() {
        if lines.len() >= MAX_LINES {
            break;
        }
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tag_end) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
            let tag = &rest[1..=tag_end];
            if let Some(ms) = parse_time(tag) {
                if times.len() < MAX_TAGS_PER_LINE {
                    times.push(ms);
                }
            } else if let Some(value) = tag_value(tag, "offset") {
                if let Ok(v) = value.trim().parse::<i64>() {
                    offset = v;
                }
                break;
            } else {
                // Metadata ([ar:…], [ti:…], [length:…]) or junk.
                break;
            }
            rest = rest[tag_end + 2..].trim_start();
        }
        let text = rest.trim();
        let text = match text.char_indices().nth(MAX_LINE_CHARS) {
            Some((cut, _)) => text[..cut].trim_end(),
            None => text,
        };
        let room = MAX_LINES - lines.len();
        lines.extend(
            times
                .into_iter()
                .take(room)
                .map(|at_ms| (at_ms, text.to_owned())),
        );
    }
    let mut out: Vec<LrcLine> = lines
        .into_iter()
        .map(|(at, text)| LrcLine {
            at_ms: u64::try_from(i64::try_from(at).unwrap_or(i64::MAX).saturating_sub(offset))
                .unwrap_or(0),
            text,
        })
        .collect();
    out.sort_by_key(|l| l.at_ms);
    out
}

/// `mm:ss`, `mm:ss.f`, `mm:ss.ff`, `mm:ss.fff` or `mm:ss:ff` in milliseconds.
fn parse_time(tag: &str) -> Option<u64> {
    let (min, sec) = tag.split_once(':')?;
    if min.is_empty() || !min.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (whole, frac) = match sec.split_once(['.', ':']) {
        Some((w, f)) => (w, f),
        None => (sec, ""),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || whole.len() > 2 || (!frac.is_empty() && (!digits(frac) || frac.len() > 3))
    {
        return None;
    }
    let secs: u64 = whole.parse().ok()?;
    if secs >= 60 {
        return None;
    }
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<u64>().ok()? * 100,
        2 => frac.parse::<u64>().ok()? * 10,
        _ => frac.parse::<u64>().ok()?,
    };
    min.parse::<u64>()
        .ok()?
        .checked_mul(60_000)?
        .checked_add(secs * 1000 + frac_ms)
}

/// The value of a `name:value` tag, the name compared without case.
fn tag_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let (n, v) = tag.split_once(':')?;
    n.trim().eq_ignore_ascii_case(name).then_some(v)
}

/// The line showing at `pos_ms`: the last one whose time has come. `None`
/// before the first line.
pub fn current_line(lines: &[LrcLine], pos_ms: u64) -> Option<(usize, &str)> {
    let i = lines.partition_point(|l| l.at_ms <= pos_ms);
    let i = i.checked_sub(1)?;
    Some((i, lines[i].text.as_str()))
}

/// When the shown line next changes after `pos_ms`; `None` after the last.
pub fn next_change(lines: &[LrcLine], pos_ms: u64) -> Option<u64> {
    let i = lines.partition_point(|l| l.at_ms <= pos_ms);
    lines.get(i).map(|l| l.at_ms)
}

/// Whether the lyrics have any words (not only instrumental gaps).
pub fn has_words(lines: &[LrcLine]) -> bool {
    lines.iter().any(|l| !l.text.is_empty())
}

/// The playback position at `now`: frozen while paused, advancing at the
/// reported rate while playing, never past the end when the end is known.
pub fn position_at(t: &Timeline, playing: bool, now: Instant) -> u64 {
    let pos = if playing {
        let rate = usable_rate(t.rate);
        let elapsed = now.saturating_duration_since(t.updated).as_secs_f64() * 1000.0 * rate;
        t.position_ms.saturating_add(elapsed as u64)
    } else {
        t.position_ms
    };
    if t.end_ms > 0 { pos.min(t.end_ms) } else { pos }
}

/// A playback rate to count with: kept in 0.1..=4, and 1 for one that is
/// not a number or infinite (`clamp` keeps a NaN, which would make the
/// next-line timer fire every millisecond).
fn usable_rate(rate: f32) -> f64 {
    let rate = f64::from(rate);
    if rate.is_finite() {
        rate.clamp(0.1, 4.0)
    } else {
        1.0
    }
}

/// The farthest lyric position this track can reach: `position_at` stops at
/// its end, so with the output lag applied (`lyric_position`) a line after
/// this never comes. `None` when the player reports no length.
pub fn lyric_reach(end_ms: u64) -> Option<u64> {
    (end_ms > 0).then(|| lyric_position(end_ms))
}

/// The position the lyric line is chosen at: the playback position minus
/// `OUTPUT_LAG_MS`, never before 0. A line is therefore never chosen before
/// its time.
pub fn lyric_position(raw_ms: u64) -> u64 {
    let shifted = i64::try_from(raw_ms)
        .unwrap_or(i64::MAX)
        .saturating_sub(OUTPUT_LAG_MS);
    u64::try_from(shifted).unwrap_or(0)
}

/// The same moment re-expressed at `now`: the position then, as of now. Used
/// when play/pause flips, so a player that does not refresh its timeline
/// neither runs on while paused nor jumps when it resumes.
pub fn rebase(t: &Timeline, was_playing: bool, now: Instant) -> Timeline {
    Timeline {
        position_ms: position_at(t, was_playing, now),
        end_ms: t.end_ms,
        updated: now,
        rate: t.rate,
    }
}

/// Whether a freshly read timeline differs from the one last sent: one of
/// them missing, another length or rate, or a position more than `RESYNC_MS`
/// away from where the last one says playback should be.
pub fn timeline_moved(
    last: Option<&Timeline>,
    next: Option<&Timeline>,
    playing: bool,
    now: Instant,
) -> bool {
    match (last, next) {
        (None, None) => false,
        (Some(a), Some(b)) => {
            a.end_ms != b.end_ms
                || a.rate != b.rate
                || position_at(a, playing, now).abs_diff(position_at(b, playing, now)) > RESYNC_MS
        }
        _ => true,
    }
}

/// Lower case, blanks collapsed to one space, trimmed.
fn normalise(s: &str) -> String {
    s.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The cache key of a track: normalised artist and title, and its length in
/// whole seconds (rounded).
pub fn track_key(artist: &str, title: &str, duration_ms: u64) -> String {
    let secs = duration_ms.saturating_add(500) / 1000;
    format!("{}|{}|{secs}", normalise(artist), normalise(title))
}

/// The lookup for what is playing: needs a title, an artist and a known
/// length (without a timeline nothing can be synced anyway).
pub fn track_query(media: Option<&MediaInfo>, timeline: Option<&Timeline>) -> Option<Query> {
    let m = media?;
    let t = timeline?;
    let (artist, title) = (m.artist.trim(), m.title.trim());
    if artist.is_empty() || title.is_empty() || t.end_ms == 0 {
        return None;
    }
    Some(Query {
        key: track_key(artist, title, t.end_ms),
        artist: artist.to_owned(),
        title: title.to_owned(),
        duration_ms: t.end_ms,
        provisional: false,
    })
}

/// FNV-1a, 64 bit, as 16 hex digits: a stable file name for a key.
pub fn key_hash(key: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The LRCLIB lookup URL. The album is not sent: players report it
/// unreliably, and the length (±2 s) already pins the recording.
pub fn lrclib_url(q: &Query) -> String {
    lrclib_url_for(&q.title, &q.artist, q.duration_ms)
}

/// A title as a streaming site decorates it, without the decoration:
/// "(Official Video)", "[Lyrics]", "(Audio)", a trailing "- Remastered"
/// and the like. Returns `title` itself when nothing is to cut.
pub fn clean_title(title: &str) -> String {
    const NOISE: &[&str] = &[
        "official",
        "video",
        "music video",
        "lyric",
        "lyrics",
        "audio",
        "hd",
        "hq",
        "4k",
        "mv",
        "visualizer",
        "visualiser",
        "remaster",
        "remastered",
        "live",
        "version",
    ];
    let mut out = title.trim().to_owned();
    loop {
        let before = out.clone();
        for (open, close) in [('(', ')'), ('[', ']')] {
            if out.ends_with(close)
                && let Some(start) = out.rfind(open)
            {
                let inner = out[start + 1..out.len() - 1].to_lowercase();
                if NOISE.iter().any(|n| inner.contains(n)) {
                    out = out[..start].trim_end().to_owned();
                }
            }
        }
        if let Some((head, tail)) = out.rsplit_once(" - ")
            && NOISE.iter().any(|n| tail.to_lowercase().contains(n))
        {
            out = head.trim_end().to_owned();
        }
        if out == before {
            break;
        }
    }
    if out.is_empty() {
        title.trim().to_owned()
    } else {
        out
    }
}

/// An artist without YouTube Music's " - Topic" suffix and a trailing
/// "VEVO".
pub fn clean_artist(artist: &str) -> String {
    let a = artist.trim();
    let a = a.strip_suffix(" - Topic").unwrap_or(a).trim_end();
    let lower = a.to_lowercase();
    let a = if lower.ends_with("vevo") && a.len() > 4 {
        a[..a.len() - 4].trim_end()
    } else {
        a
    };
    if a.is_empty() {
        artist.trim().to_owned()
    } else {
        a.to_owned()
    }
}

/// The LRCLIB lookup URL for an explicit title and artist.
fn lrclib_url_for(title: &str, artist: &str, duration_ms: u64) -> String {
    let secs = (duration_ms as f64 / 1000.0).clamp(1.0, 3600.0);
    let duration = format!("{}", secs.round() as u64);
    let query = form_encode(&[
        ("track_name", title),
        ("artist_name", artist),
        ("duration", &duration),
    ]);
    format!("{API}?{query}")
}

/// The search fallback's URL: no length (a different version of the song has
/// another one); `pick_best` chooses by it.
pub fn lrclib_search_url(title: &str, artist: &str) -> String {
    let query = form_encode(&[("track_name", title), ("artist_name", artist)]);
    format!("{SEARCH_API}?{query}")
}

/// The free-text search URL (`q=<artist> <title>`): it finds a cover whose
/// title carries the original's name, which the field search does not.
fn lrclib_query_url(title: &str, artist: &str) -> String {
    let query = form_encode(&[("q", &format!("{artist} {title}"))]);
    format!("{SEARCH_API}?{query}")
}

/// `title` without a trailing "(...)" or "[...]" group ("Red (Taylor Swift)"
/// is "Red"); `None` when it has none or nothing would be left.
fn without_trailing_group(title: &str) -> Option<String> {
    let t = title.trim_end();
    let open = match t.chars().last()? {
        ')' => '(',
        ']' => '[',
        _ => return None,
    };
    let head = t[..t.rfind(open)?].trim_end();
    (!head.is_empty()).then(|| head.to_owned())
}

/// Letters and digits only, lowercase: "Avril Lavigne" and "AvrilLavigne"
/// compare equal.
fn alnum_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// YouTube videos name the song "Artist - Title" under a channel that is the
/// artist ("AvrilLavigneVEVO", "Artist - Topic"): the artist and title that
/// are meant, cleaned of decoration. `None` when the title is not of that form
/// or its first part is not the artist.
pub fn split_artist_title(title: &str, artist: &str) -> Option<(String, String)> {
    let cleaned = clean_title(title);
    let (head, tail) = cleaned.split_once(" - ")?;
    let (head, tail) = (head.trim(), tail.trim());
    if head.is_empty() || tail.is_empty() {
        return None;
    }
    let raw_artist = artist.trim();
    let channel = raw_artist.to_lowercase().ends_with("vevo") || raw_artist.ends_with(" - Topic");
    if !channel && alnum_key(head) != alnum_key(&clean_artist(artist)) {
        return None;
    }
    Some((clean_artist(head), clean_title(tail)))
}

/// The artist and title to search by: split out of "Artist - Title" when the
/// title is of that form, else each cleaned of decoration.
fn search_names(title: &str, artist: &str) -> (String, String) {
    split_artist_title(title, artist).unwrap_or_else(|| (clean_artist(artist), clean_title(title)))
}

/// One LRCLIB entry with synced lyrics.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub lrc: String,
    pub lines: Vec<LrcLine>,
    /// The entry's length; `None` when LRCLIB did not say (a `/api/get` hit
    /// already matched the length).
    pub duration_ms: Option<u64>,
}

fn candidate_of(item: &serde_json::Value) -> Option<Candidate> {
    let lrc = item.get("syncedLyrics")?.as_str()?;
    let lines = parse_lrc(lrc);
    if lines.is_empty() {
        return None;
    }
    let duration_ms = item
        .get("duration")
        .and_then(|d| d.as_f64())
        .filter(|d| d.is_finite() && *d >= 0.0)
        .map(|d| (d * 1000.0).round() as u64);
    Some(Candidate {
        lrc: lrc.to_owned(),
        lines,
        duration_ms,
    })
}

/// The entries of a search body that have synced lyrics, in order. A body
/// that is over `MAX_BODY_BYTES` or not a list is read as none.
#[cfg_attr(not(test), allow(dead_code))]
pub fn candidates(body: &str) -> Vec<Candidate> {
    if body.len() > MAX_BODY_BYTES {
        return Vec::new();
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    json.as_array()
        .map(|items| items.iter().filter_map(candidate_of).collect())
        .unwrap_or_default()
}

/// Lower median (integers only); `values` is not empty.
fn lower_median<T: Ord + Copy>(values: &mut [T]) -> T {
    values.sort_unstable();
    values[(values.len() - 1) / 2]
}

type Times = std::collections::HashMap<String, Vec<u64>>;

/// Times of each distinct text (letters and digits, lowercase) among the
/// first `MATCH_LINES` lines, in order; empty text (a gap) is left out.
fn times_by_text(lines: &[LrcLine]) -> Times {
    let mut map = Times::new();
    for line in lines.iter().take(MATCH_LINES) {
        let key = alnum_key(&line.text);
        if !key.is_empty() {
            map.entry(key).or_default().push(line.at_ms);
        }
    }
    map
}

/// The median of `a - b` over the lines the two share (the k-th occurrence of
/// a text in one with the k-th in the other); `None` under `MIN_MATCHED`.
fn shift_between(a: &Times, b: &Times) -> Option<i64> {
    let mut diffs: Vec<i64> = Vec::new();
    for (text, at_a) in a {
        if let Some(at_b) = b.get(text) {
            diffs.extend(at_a.iter().zip(at_b).map(|(x, y)| *x as i64 - *y as i64));
        }
    }
    (diffs.len() >= MIN_MATCHED).then(|| lower_median(&mut diffs))
}

/// The index (into `times`) of the candidate among `idx` that is least
/// shifted from the others (the median of its shifts to each of them), then
/// the closest length, then the first. `idx` is not empty.
fn consensus(times: &[Times], idx: &[usize], gap: impl Fn(usize) -> u64) -> usize {
    if let [only] = idx {
        return *only;
    }
    let score = |i: usize| -> u64 {
        let mut away: Vec<u64> = idx
            .iter()
            .filter(|&&j| j != i)
            .filter_map(|&j| shift_between(&times[i], &times[j]))
            .map(i64::unsigned_abs)
            .collect();
        if away.is_empty() {
            u64::MAX
        } else {
            lower_median(&mut away)
        }
    };
    idx.iter()
        .copied()
        .min_by_key(|&i| (score(i), gap(i), i))
        .unwrap_or(idx[0])
}

/// How much later than the upload `cands[chosen]` plays in the version of
/// `duration_ms`: the playing length minus the reference length of the
/// chosen timing's family, when that is over `MIN_INTRO_MS` and at most
/// `MAX_INTRO_MS`. The family is every candidate (any length) whose timing is
/// within `FAMILY_MS` of the chosen one (measured against it, not chained);
/// its reference is the lower median of their lengths. A longer intro (a
/// music video) lengthens the recording but not the song, so the sung lines
/// come later; a shorter playing version is never shifted.
fn intro_shift(
    cands: &[Candidate],
    times: &[Times],
    chosen: usize,
    duration_ms: u64,
) -> Option<u64> {
    let mut lengths: Vec<u64> = (0..cands.len())
        .filter(|&x| {
            x == chosen
                || shift_between(&times[chosen], &times[x])
                    .is_some_and(|s| s.unsigned_abs() <= FAMILY_MS)
        })
        .filter_map(|x| cands[x].duration_ms)
        .collect();
    if lengths.is_empty() {
        return None;
    }
    let extra = duration_ms.checked_sub(lower_median(&mut lengths))?;
    (MIN_INTRO_MS < extra && extra <= MAX_INTRO_MS).then_some(extra)
}

/// `lines` as LRC text, one `[mm:ss.xxx]text` line each (an empty text, a gap,
/// is just the tag; minutes may pass 59). `parse_lrc` reads it back as it was.
pub fn format_lrc(lines: &[LrcLine]) -> String {
    let mut out = String::new();
    for line in lines {
        let at = line.at_ms;
        out.push_str(&format!(
            "[{:02}:{:02}.{:03}]{}\n",
            at / 60_000,
            at / 1000 % 60,
            at % 1000,
            line.text
        ));
    }
    out
}

/// The synced lyrics to show out of the entries of a search reply, for a
/// track of `duration_ms`. LRCLIB entries are user uploads whose timing
/// varies; copies of one timing agree, so among the entries within
/// `TIGHT_DURATION_MS` of the length (else within `SEARCH_DURATION_MS`) the
/// one that is least shifted from the others wins (the median of its shifts to
/// each of them), then the closest length, then the first.
///
/// The upload may be of a shorter version than the one playing (a music video
/// with a long intro): the chosen timing is then shifted later by the extra
/// length (see `intro_shift`) and the shifted LRC is returned. With nothing
/// within `SEARCH_DURATION_MS` the choice is made among all of them and
/// returned only when it needs that shift.
pub fn pick_best(cands: Vec<Candidate>, duration_ms: u64) -> Option<String> {
    let mut cands: Vec<Candidate> = cands.into_iter().take(MAX_CANDIDATES).collect();
    let gap = |i: usize| cands[i].duration_ms.map_or(0, |d| d.abs_diff(duration_ms));
    let within =
        |limit: u64| -> Vec<usize> { (0..cands.len()).filter(|&i| gap(i) <= limit).collect() };
    let mut pool = within(TIGHT_DURATION_MS);
    if pool.is_empty() {
        pool = within(SEARCH_DURATION_MS);
    }
    let fallback = pool.is_empty();
    if fallback {
        pool = (0..cands.len()).collect();
    }
    if pool.is_empty() {
        return None;
    }
    let times: Vec<Times> = cands.iter().map(|c| times_by_text(&c.lines)).collect();
    let chosen = consensus(&times, &pool, gap);
    let shifted = intro_shift(&cands, &times, chosen, duration_ms).map(|extra| {
        let lines: Vec<LrcLine> = cands[chosen]
            .lines
            .iter()
            .map(|l| LrcLine {
                at_ms: l.at_ms.saturating_add(extra),
                text: l.text.clone(),
            })
            .collect();
        format_lrc(&lines)
    });
    match shifted {
        Some(lrc) => Some(lrc),
        None if fallback => None,
        None => Some(cands.swap_remove(chosen).lrc),
    }
}

/// The lyrics of a `/api/get` reply (one entry that LRCLIB matched by the
/// length): returned when its length is within `SEARCH_DURATION_MS` of
/// `duration_ms` (a missing length counts as 0 off). Never shifted.
fn pick_exact(cands: Vec<Candidate>, duration_ms: u64) -> Option<String> {
    cands
        .into_iter()
        .find(|c| {
            c.duration_ms
                .is_none_or(|d| d.abs_diff(duration_ms) <= SEARCH_DURATION_MS)
        })
        .map(|c| c.lrc)
}

/// What the worker found out: the synced lyrics, nothing (every request was
/// answered and none has them), or trouble on the way.
#[derive(Debug, PartialEq)]
pub enum Answer {
    /// The `syncedLyrics` LRC text.
    Synced(String),
    /// Known not to have synced lyrics (404, instrumental, plain only).
    None,
    /// Server or network trouble: ask again another time.
    Failed,
}

/// What one LRCLIB reply says.
#[derive(Debug, PartialEq)]
pub enum Reading {
    /// Entries with synced lyrics.
    Found(Vec<Candidate>),
    /// Answered: nothing usable (404, a bad request, no synced lyrics).
    Empty,
    /// Not a usable answer (server trouble, a body that cannot be read, one
    /// over `MAX_BODY_BYTES`): ask again another time.
    Transient,
}

/// What a reply says: a JSON list when `many` (`/api/search`), else one
/// entry (`/api/get`).
pub fn read_reply(status: u16, body: &str, many: bool) -> Reading {
    match status {
        200 if body.len() > MAX_BODY_BYTES => Reading::Transient,
        200 => {
            let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
                return Reading::Transient;
            };
            let found: Vec<Candidate> = if many {
                match json.as_array() {
                    Some(items) => items.iter().filter_map(candidate_of).collect(),
                    None => return Reading::Transient,
                }
            } else if json.is_object() {
                candidate_of(&json).into_iter().collect()
            } else {
                return Reading::Transient;
            };
            if found.is_empty() {
                Reading::Empty
            } else {
                Reading::Found(found)
            }
        }
        // 404, 400 and the like: refused for good.
        s if (400..500).contains(&s) && s != 429 => Reading::Empty,
        _ => Reading::Transient,
    }
}

/// How long to wait before asking again, from a `Retry-After` header: whole
/// seconds, kept within 1..=3 (a missing, unreadable or date value is 1).
pub fn retry_delay(header: Option<&str>) -> Duration {
    let secs = header
        .and_then(|h| h.trim().parse::<u64>().ok())
        .unwrap_or(1);
    Duration::from_secs(secs.clamp(1, 3))
}

/// A reply: status, body, `Retry-After`.
type Reply = (u16, String, Option<String>);

/// The network, the clock and sleeping, so the lookup can run without them.
trait Net {
    fn get(&mut self, url: &str) -> Result<Reply, ()>;
    fn sleep(&mut self, how_long: Duration);
    fn now(&self) -> Instant;
}

struct HttpNet<'a>(&'a Http);

impl Net for HttpNet<'_> {
    fn get(&mut self, url: &str) -> Result<Reply, ()> {
        self.0
            .get_with_headers_capped_retry(url, &[("User-Agent", USER_AGENT)], MAX_BODY_BYTES)
            .map_err(|_| ())
    }

    fn sleep(&mut self, how_long: Duration) {
        std::thread::sleep(how_long);
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// One URL's outcome after its retries.
enum Fetched {
    Done(u16, String),
    Transient,
}

struct Chain<'a, N: Net> {
    net: &'a mut N,
    deadline: Instant,
    duration_ms: u64,
    /// Some request gave no definitive answer (or the budget ran out).
    transient: bool,
}

impl<N: Net> Chain<'_, N> {
    fn expired(&self) -> bool {
        self.net.now() >= self.deadline
    }

    /// GET with retries: a 429 or 5xx waits `retry_delay` and asks again; a
    /// network error waits a second and asks again, but only once. All of
    /// them share one cap of `HTTP_RETRIES` retries per URL. Past the budget
    /// nothing more is sent (or slept for).
    fn fetch(&mut self, url: &str) -> Fetched {
        let (mut retries, mut errored) = (0, false);
        loop {
            if self.expired() {
                return Fetched::Transient;
            }
            let wait = match self.net.get(url) {
                Ok((status, body, _)) if status != 429 && status < 500 => {
                    return Fetched::Done(status, body);
                }
                Ok((_, _, header)) => retry_delay(header.as_deref()),
                Err(()) if errored => return Fetched::Transient,
                Err(()) => {
                    errored = true;
                    retry_delay(None)
                }
            };
            retries += 1;
            if retries > HTTP_RETRIES {
                return Fetched::Transient;
            }
            if self.expired() {
                return Fetched::Transient;
            }
            self.net.sleep(wait);
        }
    }

    /// One step: the synced lyrics `pick_best` (a search) or `pick_exact` (a
    /// `/api/get`) takes from the reply to `url`.
    fn step(&mut self, url: &str, many: bool) -> Option<String> {
        let reading = match self.fetch(url) {
            Fetched::Done(status, body) => read_reply(status, &body, many),
            Fetched::Transient => Reading::Transient,
        };
        match reading {
            Reading::Found(cands) if many => pick_best(cands, self.duration_ms),
            Reading::Found(cands) => pick_exact(cands, self.duration_ms),
            Reading::Empty => None,
            Reading::Transient => {
                self.transient = true;
                None
            }
        }
    }
}

/// The lookup at LRCLIB, step by step until one has synced lyrics: a field
/// search by the cleaned names, `/api/get` by the names as the player gives
/// them and then cleaned, a free-text search ("artist title", then with the
/// title's trailing group cut). A failed or empty step never stops the next.
/// `None` only when every request was answered; any trouble (or the
/// `LOOKUP_BUDGET` running out) with nothing found is `Failed`.
fn lookup_chain(q: &Query, net: &mut impl Net) -> Answer {
    let (artist, title) = search_names(&q.title, &q.artist);
    let mut steps: Vec<(String, bool)> = vec![
        (lrclib_search_url(&title, &artist), true),
        (lrclib_url(q), false),
    ];
    if title != q.title || artist != q.artist {
        steps.push((lrclib_url_for(&title, &artist, q.duration_ms), false));
    }
    let mut free_text = vec![lrclib_query_url(&title, &artist)];
    free_text.extend(without_trailing_group(&title).map(|short| lrclib_query_url(&short, &artist)));
    for url in free_text {
        if !steps.iter().any(|(seen, _)| *seen == url) {
            steps.push((url, true));
        }
    }
    let deadline = net.now() + LOOKUP_BUDGET;
    let mut chain = Chain {
        net,
        deadline,
        duration_ms: q.duration_ms,
        transient: false,
    };
    for (url, many) in &steps {
        if let Some(lrc) = chain.step(url, *many) {
            return Answer::Synced(lrc);
        }
    }
    if chain.transient {
        Answer::Failed
    } else {
        Answer::None
    }
}

/// The last `MEMORY_SLOTS` tracks looked up, most recent first, with the
/// Unix time each was learnt. A "not found" expires like the disk marker.
pub struct MemoryCache {
    cap: usize,
    entries: VecDeque<(String, Option<Lines>, i64)>,
}

impl MemoryCache {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            entries: VecDeque::new(),
        }
    }

    /// `Some(None)`: known to have no synced lyrics (for under
    /// `NEGATIVE_TTL_SECS`). Moves the entry to the front.
    pub fn get(&mut self, key: &str, now_unix: i64) -> Option<Option<Lines>> {
        let i = self.entries.iter().position(|(k, _, _)| k == key)?;
        let entry = self.entries.remove(i)?;
        if entry.1.is_none() && !(0..NEGATIVE_TTL_SECS).contains(&(now_unix - entry.2)) {
            return None;
        }
        let lines = entry.1.clone();
        self.entries.push_front(entry);
        Some(lines)
    }

    pub fn put(&mut self, key: String, lines: Option<Lines>, now_unix: i64) {
        self.entries.retain(|(k, _, _)| *k != key);
        self.entries.push_front((key, lines, now_unix));
        self.entries.truncate(self.cap);
    }

    #[cfg(test)]
    fn keys(&self) -> Vec<&str> {
        self.entries.iter().map(|(k, _, _)| k.as_str()).collect()
    }
}

/// What the disk cache knows of a key.
#[derive(Debug, PartialEq)]
pub enum Cached {
    Lrc(String),
    /// A fresh "not found" marker, written at this Unix time.
    Nothing(i64),
    Miss,
}

/// `<dir>\<hash>.lrc2` holds a track's LRC (the text shown: shifted when the
/// playing version has a longer intro); `<hash>.none4` holds the Unix time a
/// lookup found nothing, trusted for `NEGATIVE_TTL_SECS`. Files of the older
/// lookups (`.lrc`, `.none` .. `.none3`) are never read.
pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `%APPDATA%\Vale\lyrics-cache`.
    pub fn default_dir() -> PathBuf {
        crate::data_dir::dir().join("lyrics-cache")
    }

    fn file(&self, key: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{}.{ext}", key_hash(key)))
    }

    /// An `.lrc2` over `MAX_BODY_BYTES` (never written by `write_lrc`) is
    /// left unread, as if it were not there.
    pub fn read(&self, key: &str, now_unix: i64) -> Cached {
        let lrc = self.file(key, LRC_EXT);
        let fits = std::fs::metadata(&lrc).is_ok_and(|m| m.len() <= MAX_BODY_BYTES as u64);
        if fits && let Ok(lrc) = std::fs::read_to_string(&lrc) {
            return Cached::Lrc(lrc);
        }
        let marker = self.file(key, NONE_EXT);
        match std::fs::read_to_string(&marker) {
            Ok(stamp) => match stamp.trim().parse::<i64>() {
                Ok(at) if (0..NEGATIVE_TTL_SECS).contains(&(now_unix - at)) => Cached::Nothing(at),
                _ => {
                    let _ = std::fs::remove_file(&marker);
                    Cached::Miss
                }
            },
            Err(_) => Cached::Miss,
        }
    }

    /// Write errors only cost a lookup next time; they are ignored. LRC
    /// that `storable` refuses is not written.
    pub fn write_lrc(&self, key: &str, lrc: &str) {
        if !storable(lrc) {
            return;
        }
        let _ = write_atomic(&self.file(key, LRC_EXT), lrc);
        let _ = std::fs::remove_file(self.file(key, NONE_EXT));
    }

    pub fn write_nothing(&self, key: &str, now_unix: i64) {
        let _ = write_atomic(&self.file(key, NONE_EXT), &now_unix.to_string());
    }
}

/// LRC fit to cache and show: within `MAX_BODY_BYTES` and with at least one
/// timed line.
fn storable(lrc: &str) -> bool {
    lrc.len() <= MAX_BODY_BYTES && !parse_lrc(lrc).is_empty()
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// The island's side of the lyrics: the track asked for, the lines of the
/// current track, and the line on screen. Caches live in the worker.
#[derive(Default)]
pub struct LyricsState {
    /// The key last sent to the worker; not asked again while it is current.
    /// A lookup that failed on the way clears it, so it is asked again once
    /// `retry_due` says the back-off is over.
    asked: Option<String>,
    /// The track whose failed lookup waits for `retry_due`.
    held: Option<String>,
    /// The track retried last and how many retries it has had.
    retries: Option<(String, u32)>,
    /// The key of the playing track as last seen; only its answer is shown.
    wanted: Option<String>,
    /// Artist and title (normalised) and the length last seen with them.
    seen: Option<(String, u64)>,
    /// A length that may belong to another track than the title shown with
    /// it; cleared once a different length is reported.
    stale_end: Option<u64>,
    current: Option<(String, Lines)>,
    shown: Option<Shown>,
}

/// The line on screen, and the one fading out since `since`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    line: Option<usize>,
    prev: Option<Option<usize>>,
    since: Instant,
}

/// A lyric line to draw: the text (empty = instrumental), the one fading
/// out, and the crossfade's progress (1 = settled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShownLine<'a> {
    pub text: &'a str,
    pub prev: Option<&'a str>,
    pub fade: f32,
}

impl LyricsState {
    /// Notes `q` (the playing track) and returns the lookup to send to the
    /// worker when it has not been asked yet and `fetch` allows it. The
    /// worker settles for `SETTLE` before answering, so a title reported
    /// before its timeline is superseded by the query with the right length.
    pub fn want(&mut self, q: Option<&Query>, fetch: bool) -> Option<Query> {
        let q = q?;
        let provisional = self.observe(q);
        self.wanted = Some(q.key.clone());
        if self.current.as_ref().is_some_and(|(k, _)| *k == q.key) {
            return None;
        }
        if fetch
            && self.asked.as_deref() != Some(q.key.as_str())
            && self.held.as_deref() != Some(q.key.as_str())
        {
            if self.retries.as_ref().is_some_and(|(key, _)| *key != q.key) {
                self.retries = None;
            }
            self.asked = Some(q.key.clone());
            return Some(Query {
                provisional,
                ..q.clone()
            });
        }
        None
    }

    /// Whether `q`'s length may still be another track's: the title changed
    /// but the length is the one seen before it (title reported first), or
    /// the length jumped under the same title (timeline reported first).
    /// Either way it holds until a different length is reported.
    fn observe(&mut self, q: &Query) -> bool {
        let identity = q.key.rsplit_once('|').map_or(q.key.as_str(), |(id, _)| id);
        let end = q.duration_ms;
        match &self.seen {
            Some((prev, prev_end)) if prev != identity => {
                self.stale_end = (*prev_end == end).then_some(end);
            }
            Some((_, prev_end)) => match self.stale_end {
                Some(stale) if stale != end => self.stale_end = None,
                Some(_) => {}
                None if prev_end.abs_diff(end) > LENGTH_JUMP_MS => self.stale_end = Some(end),
                None => {}
            },
            None => {}
        }
        self.seen = Some((identity.to_owned(), end));
        self.stale_end.is_some()
    }

    /// A worker answer; it becomes the current lyrics only if it is for the
    /// track playing now. A failure on the way (`transient`) for the track
    /// last asked lets it be asked again, up to `RETRIES` times: true when
    /// the caller should call `retry_due` after `RETRY_AFTER`.
    pub fn found(&mut self, f: LyricsFound) -> bool {
        if f.transient {
            if self.asked.as_deref() != Some(f.key.as_str()) {
                return false;
            }
            let tries = match &self.retries {
                Some((key, n)) if *key == f.key => *n,
                _ => 0,
            };
            if tries >= RETRIES {
                return false;
            }
            self.asked = None;
            self.held = Some(f.key.clone());
            self.retries = Some((f.key, tries + 1));
            return true;
        }
        if let Some(lines) = f.lines
            && self.wanted.as_deref() == Some(f.key.as_str())
        {
            self.set_current(f.key, lines);
        }
        false
    }

    /// The back-off after a failed lookup is over: the track may be asked
    /// again (the next `want` while it plays).
    pub fn retry_due(&mut self) {
        self.held = None;
    }

    fn set_current(&mut self, key: String, lines: Lines) {
        self.current = Some((key, lines));
        self.shown = None;
    }

    /// Synced lyrics with words exist for `q`.
    pub fn ready(&self, q: Option<&Query>) -> bool {
        match (q, &self.current) {
            (Some(q), Some((key, lines))) => *key == q.key && has_words(lines),
            _ => false,
        }
    }

    fn lines(&self) -> &[LrcLine] {
        self.current.as_ref().map_or(&[], |(_, l)| l.as_slice())
    }

    /// Moves the shown line to the one at `pos_ms`. True when the text on
    /// screen changes; it then crossfades from the old one, except for the
    /// first line shown. A line repeating the same text changes nothing.
    pub fn advance(&mut self, pos_ms: u64, now: Instant) -> bool {
        let line = current_line(self.lines(), pos_ms).map(|(i, _)| i);
        let Some(shown) = self.shown else {
            self.shown = Some(Shown {
                line,
                prev: None,
                since: now,
            });
            return true;
        };
        if shown.line == line {
            return false;
        }
        if self.text(shown.line) == self.text(line) {
            self.shown = Some(Shown { line, ..shown });
            return false;
        }
        self.shown = Some(Shown {
            line,
            prev: Some(shown.line),
            since: now,
        });
        true
    }

    fn text(&self, line: Option<usize>) -> &str {
        line.and_then(|i| self.lines().get(i))
            .map_or("", |l| l.text.as_str())
    }

    /// The lyric view went away: the next one starts without a crossfade.
    pub fn hide(&mut self) {
        self.shown = None;
    }

    /// A crossfade is still running at `now`.
    pub fn fading(&self, now: Instant) -> bool {
        self.shown
            .is_some_and(|s| s.prev.is_some() && now.saturating_duration_since(s.since) < FADE)
    }

    /// What to draw at `now`; `None` before `advance` placed a line.
    pub fn shown(&self, now: Instant) -> Option<ShownLine<'_>> {
        let s = self.shown?;
        let fade = match s.prev {
            Some(_) => {
                (now.saturating_duration_since(s.since).as_secs_f32() / FADE.as_secs_f32()).min(1.0)
            }
            None => 1.0,
        };
        Some(ShownLine {
            text: self.text(s.line),
            prev: s.prev.filter(|_| fade < 1.0).map(|p| self.text(p)),
            fade,
        })
    }

    /// Milliseconds of wall clock from `pos_ms` until the line next changes
    /// at playback `rate` (a faster song gets there sooner), plus 1 so the
    /// change has happened when a timer fires. `None` after the last line,
    /// and when the next line lies past `reach_ms` (`lyric_reach`): the
    /// position stops at the track's end, so a timer for it would fire and
    /// re-arm for ever.
    pub fn wait_ms(&self, pos_ms: u64, rate: f32, reach_ms: Option<u64>) -> Option<u64> {
        let rate = usable_rate(rate);
        next_change(self.lines(), pos_ms)
            .filter(|&at| reach_ms.is_none_or(|reach| at <= reach))
            .map(|at| (((at - pos_ms) as f64 / rate).ceil() as u64).saturating_add(1))
    }
}

/// Sends lookups to the lyrics thread.
pub struct LyricsWorker {
    queries: Sender<Query>,
}

impl LyricsWorker {
    /// False once the thread has gone.
    pub fn ask(&self, q: Query) -> bool {
        self.queries.send(q).is_ok()
    }
}

/// Starts the lyrics thread: it answers each query from its memory LRU, the
/// disk cache or LRCLIB with an `Event::Lyrics`, then `wake`s the island. A
/// query is looked up only once no newer one came for `SETTLE`; older ones
/// are dropped unanswered. It blocks between queries.
pub fn spawn_worker(
    tx: Sender<Event>,
    wake: impl Fn() + Send + 'static,
    cache: DiskCache,
) -> LyricsWorker {
    let (queries, rx) = mpsc::channel();
    std::thread::spawn(move || {
        unsafe {
            use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut http = None;
        let mut memory = MemoryCache::new(MEMORY_SLOTS);
        while let Some(q) = settled(&rx, SETTLE) {
            let found = lookup(&q, &cache, &mut memory, &mut http);
            if tx.send(Event::Lyrics(found)).is_err() {
                return;
            }
            wake();
        }
    });
    LyricsWorker { queries }
}

/// Blocks for a query, then waits until none newer came for `settle`;
/// returns the newest.
fn settled(rx: &Receiver<Query>, settle: Duration) -> Option<Query> {
    let mut q = rx.recv().ok()?;
    while let Ok(newer) = rx.recv_timeout(settle) {
        q = newer;
    }
    Some(q)
}

fn found(q: &Query, lines: Option<Lines>, transient: bool) -> LyricsFound {
    LyricsFound {
        key: q.key.clone(),
        lines,
        transient,
    }
}

fn lookup(
    q: &Query,
    cache: &DiskCache,
    memory: &mut MemoryCache,
    http: &mut Option<Http>,
) -> LyricsFound {
    let now = chrono::Local::now().timestamp();
    if let Some(known) = remembered(q, cache, memory, now) {
        return known;
    }
    if http.is_none() {
        // A request that stalls fails (and is retried later) instead of
        // holding the thread and every lookup behind it.
        *http = Http::new().ok().map(|h| h.with_timeout(REQUEST_TIMEOUT));
    }
    let Some(client) = http.as_ref() else {
        return found(q, None, true);
    };
    let answer = lookup_chain(q, &mut HttpNet(client));
    keep_answer(q, answer, cache, memory, now)
}

/// The answer from memory, else from disk (then remembered in memory).
fn remembered(
    q: &Query,
    cache: &DiskCache,
    memory: &mut MemoryCache,
    now: i64,
) -> Option<LyricsFound> {
    if let Some(lines) = memory.get(&q.key, now) {
        return Some(found(q, lines, false));
    }
    let (lines, at) = match cache.read(&q.key, now) {
        Cached::Lrc(lrc) => (Some(Arc::new(parse_lrc(&lrc))), now),
        Cached::Nothing(at) => (None, at),
        Cached::Miss => return None,
    };
    memory.put(q.key.clone(), lines.clone(), at);
    Some(found(q, lines, false))
}

/// Caches what LRCLIB said. A "not found" for a provisional query is not
/// kept anywhere: its length may have been the previous track's.
fn keep_answer(
    q: &Query,
    answer: Answer,
    cache: &DiskCache,
    memory: &mut MemoryCache,
    now: i64,
) -> LyricsFound {
    match answer {
        // Never anything else: `candidate_of` checks it.
        Answer::Synced(lrc) if !storable(&lrc) => found(q, None, true),
        Answer::Synced(lrc) => {
            cache.write_lrc(&q.key, &lrc);
            let lines = Some(Arc::new(parse_lrc(&lrc)));
            memory.put(q.key.clone(), lines.clone(), now);
            found(q, lines, false)
        }
        Answer::None if q.provisional => found(q, None, true),
        Answer::None => {
            cache.write_nothing(&q.key, now);
            memory.put(q.key.clone(), None, now);
            found(q, None, false)
        }
        Answer::Failed => found(q, None, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn l(at_ms: u64, text: &str) -> LrcLine {
        LrcLine {
            at_ms,
            text: text.into(),
        }
    }

    #[test]
    fn parses_plain_timed_lines() {
        let lrc = "[00:12.34] Hello there\n[01:02.5]Second\r\n[02:03.456] Third  \n[03:04] Fourth";
        assert_eq!(
            parse_lrc(lrc),
            vec![
                l(12_340, "Hello there"),
                l(62_500, "Second"),
                l(123_456, "Third"),
                l(184_000, "Fourth"),
            ]
        );
    }

    #[test]
    fn several_timestamps_on_one_line_repeat_it() {
        let lrc = "[00:30.00][00:10.00] Chorus\n[00:20.00] Verse";
        assert_eq!(
            parse_lrc(lrc),
            vec![l(10_000, "Chorus"), l(20_000, "Verse"), l(30_000, "Chorus")]
        );
    }

    #[test]
    fn offset_tag_shifts_every_line() {
        // Positive: lyrics come earlier; never before 0.
        let lrc = "[offset:+500]\n[00:00.20] a\n[00:10.00] b";
        assert_eq!(parse_lrc(lrc), vec![l(0, "a"), l(9_500, "b")]);
        let lrc = "[00:10.00] b\n[offset:-250]";
        assert_eq!(parse_lrc(lrc), vec![l(10_250, "b")]);
        // An unreadable offset is ignored.
        assert_eq!(
            parse_lrc("[offset:soon]\n[00:01.00] x"),
            vec![l(1_000, "x")]
        );
    }

    #[test]
    fn metadata_tags_are_ignored() {
        let lrc = "\u{feff}[ar:Demo]\n[ti:Jalan Pulang]\n[length: 05:55]\n[by:someone]\n[00:01.00] Lampu kota menyala";
        assert_eq!(parse_lrc(lrc), vec![l(1_000, "Lampu kota menyala")]);
    }

    #[test]
    fn blank_timed_lines_are_instrumental_gaps() {
        let lrc = "[00:01.00] Words\n[00:05.00]\n[00:09.00]   \n\n[00:12.00] More";
        let lines = parse_lrc(lrc);
        assert_eq!(
            lines,
            vec![
                l(1_000, "Words"),
                l(5_000, ""),
                l(9_000, ""),
                l(12_000, "More")
            ]
        );
        assert!(has_words(&lines));
        assert!(!has_words(&parse_lrc("[00:01.00]\n[00:02.00]")));
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let lrc = "no tag here\n[xx:yy] bad\n[00:61.00] bad seconds\n[1:02.3456] too precise\n[00:03.00 unclosed\n[] empty\n[00:04.00] good";
        assert_eq!(parse_lrc(lrc), vec![l(4_000, "good")]);
        assert!(parse_lrc("").is_empty());
    }

    #[test]
    fn output_is_sorted_and_stable() {
        let lrc = "[00:09.00] c\n[00:01.00] a\n[00:05.00] b1\n[00:05.00] b2";
        assert_eq!(
            parse_lrc(lrc),
            vec![l(1_000, "a"), l(5_000, "b1"), l(5_000, "b2"), l(9_000, "c")]
        );
    }

    #[test]
    fn extreme_values_never_overflow() {
        // Minutes too many for u64, minutes whose total overflows by the
        // seconds, and offsets at both ends of i64.
        let lrc = "[99999999999999999999:00.00] a\n[307445734561825:59.999] b\n[00:01.00] c";
        assert_eq!(parse_lrc(lrc), vec![l(1_000, "c")]);
        let big = "[307445734561825:00.00] far\n[00:02.00] near";
        // Past i64::MAX ms (292 million years) a time is clamped there.
        assert_eq!(parse_lrc(big)[1].at_ms, i64::MAX as u64);
        let earlier = "[offset:9223372036854775807]\n[00:02.00] x\n[307445734561825:00.00] y";
        assert!(parse_lrc(earlier).iter().all(|l| l.at_ms == 0));
        let later = "[offset:-9223372036854775808]\n[00:02.00] x\n[307445734561825:00.00] y";
        assert!(parse_lrc(later).iter().all(|l| l.at_ms == i64::MAX as u64));
    }

    #[test]
    fn long_minutes_and_colon_fractions() {
        assert_eq!(parse_lrc("[123:04:50] x"), vec![l(7_384_500, "x")]);
    }

    #[test]
    fn parsing_untrusted_lrc_stays_bounded() {
        // 100k tags on one line: only the first few become lines, the rest
        // are skipped without a copy each, and it stays quick.
        let flood = format!("{} x", "[00:01.00]".repeat(100_000));
        let start = Instant::now();
        let lines = parse_lrc(&flood);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(lines.len(), MAX_TAGS_PER_LINE);
        assert!(lines.iter().all(|line| line.text == "x"));
        // A long text is cut by characters, not bytes.
        let long = format!("[00:01.00] {}", "é".repeat(2_000));
        assert_eq!(parse_lrc(&long)[0].text.chars().count(), MAX_LINE_CHARS);
        // At most `MAX_LINES` lines, however they come.
        let many: String = (0..MAX_LINES + 100)
            .map(|i| format!("[00:{:02}.00] l{i}\n", i % 60))
            .collect();
        assert_eq!(parse_lrc(&many).len(), MAX_LINES);
        let tagged = format!("{} x\n", "[00:01.00]".repeat(MAX_TAGS_PER_LINE)).repeat(MAX_LINES);
        assert_eq!(parse_lrc(&tagged).len(), MAX_LINES);
    }

    #[test]
    fn oversized_bodies_are_refused_unparsed() {
        let pad = "x".repeat(MAX_BODY_BYTES);
        let big = format!(r#"{{"syncedLyrics":"[00:01.00] hi","pad":"{pad}"}}"#);
        assert_eq!(read_reply(200, &big, false), Reading::Transient);
        let list =
            format!(r#"[{{"duration":180.0,"syncedLyrics":"[00:01.00] hi","pad":"{pad}"}}]"#);
        assert_eq!(read_reply(200, &list, true), Reading::Transient);
        assert!(candidates(&list).is_empty());
        assert!(storable("[00:01.00] hi"));
        assert!(!storable(&format!("[00:01.00] {pad}")));
        assert!(!storable("no tags"));
    }

    fn song() -> Vec<LrcLine> {
        vec![
            l(1_000, "one"),
            l(5_000, ""),
            l(8_000, "three"),
            l(8_000, "three b"),
        ]
    }

    #[test]
    fn current_line_follows_position() {
        let s = song();
        assert_eq!(current_line(&s, 0), None, "before the first line");
        assert_eq!(current_line(&s, 999), None);
        assert_eq!(current_line(&s, 1_000), Some((0, "one")));
        assert_eq!(current_line(&s, 4_999), Some((0, "one")));
        assert_eq!(current_line(&s, 5_000), Some((1, "")));
        // Equal times: the later line wins.
        assert_eq!(current_line(&s, 8_000), Some((3, "three b")));
        assert_eq!(current_line(&s, 999_999), Some((3, "three b")));
        assert_eq!(current_line(&[], 5), None);
    }

    #[test]
    fn current_line_after_seeking_backwards() {
        let s = song();
        assert_eq!(current_line(&s, 9_000), Some((3, "three b")));
        assert_eq!(current_line(&s, 2_000), Some((0, "one")));
        assert_eq!(next_change(&s, 2_000), Some(5_000));
        assert_eq!(current_line(&s, 500), None);
        assert_eq!(next_change(&s, 500), Some(1_000));
    }

    #[test]
    fn next_change_is_the_next_distinct_time() {
        let s = song();
        assert_eq!(next_change(&s, 0), Some(1_000));
        assert_eq!(next_change(&s, 1_000), Some(5_000));
        assert_eq!(next_change(&s, 5_500), Some(8_000));
        assert_eq!(next_change(&s, 8_000), None, "after the last line");
        assert_eq!(next_change(&[], 0), None);
    }

    fn tl(position_ms: u64, end_ms: u64, updated: Instant) -> Timeline {
        Timeline {
            position_ms,
            end_ms,
            updated,
            rate: 1.0,
        }
    }

    #[test]
    fn position_interpolates_while_playing_only() {
        let t0 = Instant::now();
        let t = tl(10_000, 200_000, t0);
        let later = t0 + Duration::from_millis(2_500);
        assert_eq!(position_at(&t, true, later), 12_500);
        assert_eq!(position_at(&t, false, later), 10_000);
        // Never past the end; an unknown end does not clamp.
        let far = t0 + Duration::from_secs(1_000);
        assert_eq!(position_at(&t, true, far), 200_000);
        assert_eq!(position_at(&tl(10_000, 0, t0), true, far), 1_010_000);
        // A `now` before `updated` (clock skew) does not go backwards.
        assert_eq!(position_at(&tl(10_000, 0, later), true, t0), 10_000);
    }

    #[test]
    fn rebase_freezes_a_pause_and_resumes_from_it() {
        let t0 = Instant::now();
        let t = tl(10_000, 200_000, t0);
        let paused_at = t0 + Duration::from_secs(3);
        let paused = rebase(&t, true, paused_at);
        assert_eq!(paused, tl(13_000, 200_000, paused_at));
        // Paused for a minute: no time passes.
        let resumed_at = paused_at + Duration::from_secs(60);
        let resumed = rebase(&paused, false, resumed_at);
        assert_eq!(resumed, tl(13_000, 200_000, resumed_at));
        assert_eq!(
            position_at(&resumed, true, resumed_at + Duration::from_secs(2)),
            15_000
        );
    }

    #[test]
    fn timeline_moved_ignores_steady_playback_but_sees_seeks() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(5);
        let last = tl(10_000, 200_000, t0);
        // The player re-reported where playback is, within the tolerance:
        // not a move. Past it (a stutter, a drift) is one.
        let same = tl(15_080, 200_000, now);
        assert!(!timeline_moved(Some(&last), Some(&same), true, now));
        assert!(timeline_moved(
            Some(&last),
            Some(&tl(15_200, 200_000, now)),
            true,
            now
        ));
        // Another playback rate is a move on its own.
        let faster = Timeline {
            rate: 1.25,
            ..tl(15_000, 200_000, now)
        };
        assert!(timeline_moved(Some(&last), Some(&faster), true, now));
        // A seek back, a seek forward, a new length.
        assert!(timeline_moved(
            Some(&last),
            Some(&tl(2_000, 200_000, now)),
            true,
            now
        ));
        assert!(timeline_moved(
            Some(&last),
            Some(&tl(60_000, 200_000, now)),
            true,
            now
        ));
        assert!(timeline_moved(
            Some(&last),
            Some(&tl(15_000, 180_000, now)),
            true,
            now
        ));
        // Paused: positions compare as they are.
        assert!(timeline_moved(Some(&last), Some(&same), false, now));
        assert!(!timeline_moved(
            Some(&last),
            Some(&tl(10_060, 200_000, now)),
            false,
            now
        ));
        assert!(timeline_moved(None, Some(&last), true, now));
        assert!(timeline_moved(Some(&last), None, true, now));
        assert!(!timeline_moved(None, None, true, now));
    }

    #[test]
    fn the_lyric_position_is_the_position_minus_the_output_lag_and_never_goes_below_zero() {
        let lag = OUTPUT_LAG_MS as u64;
        assert_eq!(lyric_position(10_000), 10_000 - lag);
        assert_eq!(lyric_position(0), 0);
        assert_eq!(lyric_position(50), 0);
        assert_eq!(lyric_position(u64::MAX), i64::MAX as u64 - lag);
    }

    #[test]
    fn a_line_never_shows_before_its_time() {
        let lines = vec![l(1_000, "one"), l(5_000, "two"), l(9_000, "three")];
        // Walking the player's position in small steps: the line chosen is
        // never one whose time has not come, and each shows `OUTPUT_LAG_MS`
        // after its time.
        let mut raw = 0;
        let mut first_seen = [None; 3];
        while raw < 12_000 {
            let chosen = current_line(&lines, lyric_position(raw)).map(|(i, _)| i);
            if let Some(i) = chosen {
                assert!(raw >= lines[i].at_ms, "line {i} early at {raw}");
                first_seen[i].get_or_insert(raw);
            }
            raw += 7;
        }
        for (i, seen) in first_seen.iter().enumerate() {
            let seen = seen.expect("every line shows");
            let due = lines[i].at_ms + OUTPUT_LAG_MS as u64;
            assert!((due..due + 7).contains(&seen), "line {i}: {seen} vs {due}");
        }
    }

    #[test]
    fn the_timer_waits_wall_clock_time_at_the_playback_rate() {
        let mut st = LyricsState::default();
        let a = query("a");
        st.want(Some(&a), true);
        st.found(found(
            &a.key,
            Some(vec![l(1_000, "a"), l(5_000, "b")]),
            false,
        ));
        assert_eq!(st.wait_ms(2_000, 1.0, None), Some(3_001));
        assert_eq!(st.wait_ms(2_000, 2.0, None), Some(1_501), "twice as fast");
        assert_eq!(st.wait_ms(2_000, 0.5, None), Some(6_001), "half speed");
        assert_eq!(st.wait_ms(6_000, 1.0, None), None, "after the last line");
        // An odd rate never makes it spin or sleep for ever: a rate that is
        // not a number (or infinite) counts as 1 (a NaN used to give 1 ms).
        assert!(st.wait_ms(2_000, 0.0, None).is_some_and(|ms| ms <= 30_002));
        assert_eq!(st.wait_ms(2_000, f32::NAN, None), Some(3_001));
        assert_eq!(st.wait_ms(2_000, f32::INFINITY, None), Some(3_001));
        assert_eq!(st.wait_ms(2_000, f32::NEG_INFINITY, None), Some(3_001));
    }

    #[test]
    fn a_rate_that_is_not_a_number_plays_at_normal_speed() {
        let t0 = Instant::now();
        let mut t = tl(1_000, 0, t0);
        t.rate = f32::NAN;
        let later = t0 + Duration::from_millis(500);
        assert_eq!(position_at(&t, true, later), 1_500, "it used to freeze");
        t.rate = f32::INFINITY;
        assert_eq!(position_at(&t, true, later), 1_500);
    }

    #[test]
    fn the_timer_is_not_armed_for_a_line_the_track_end_cuts_off() {
        let mut st = LyricsState::default();
        let a = query("a");
        st.want(Some(&a), true);
        // The last line sits past the reported end (a long outro tag, or a
        // length the player rounds down).
        st.found(found(
            &a.key,
            Some(vec![l(1_000, "a"), l(5_000, "b"), l(9_500, "c")]),
            false,
        ));
        let reach = lyric_reach(9_000);
        assert_eq!(reach, Some(8_900), "the end minus the output lag");
        assert_eq!(st.wait_ms(2_000, 1.0, reach), Some(3_001), "b is reachable");
        assert_eq!(st.wait_ms(6_000, 1.0, reach), None, "c never comes");
        // Stopped at the end: position_at holds end_ms, nothing is armed
        // (it used to re-arm every few hundred ms until the next track).
        let t0 = Instant::now();
        let t = tl(8_990, 9_000, t0);
        let pos = lyric_position(position_at(&t, true, t0 + Duration::from_secs(5)));
        assert_eq!(pos, 8_900);
        assert_eq!(st.wait_ms(pos, 1.0, reach), None);
        // No reported length: nothing to cut off.
        assert_eq!(lyric_reach(0), None);
        assert_eq!(st.wait_ms(6_000, 1.0, None), Some(3_501));
    }

    #[test]
    fn streaming_decoration_is_cut_from_titles_and_artists() {
        assert_eq!(
            clean_title("Blinding Lights (Official Video)"),
            "Blinding Lights"
        );
        assert_eq!(clean_title("Song [Lyrics] (Audio)"), "Song");
        assert_eq!(clean_title("Yesterday - Remastered 2009"), "Yesterday");
        // A real part of the name stays.
        assert_eq!(clean_title("Song (feat. Someone)"), "Song (feat. Someone)");
        assert_eq!(clean_title("Plain Title"), "Plain Title");
        // Never empties the title.
        assert_eq!(clean_title("(Official Video)"), "(Official Video)");
        assert_eq!(clean_artist("The Weeknd - Topic"), "The Weeknd");
        assert_eq!(clean_artist("ArianaGrandeVEVO"), "ArianaGrande");
        assert_eq!(clean_artist("Queen"), "Queen");
        assert_eq!(clean_artist("VEVO"), "VEVO");
    }

    fn best(body: &str, duration_ms: u64) -> Option<String> {
        pick_best(candidates(body), duration_ms)
    }

    #[test]
    fn the_search_fallback_picks_the_closest_length_with_synced_lyrics() {
        let body = r#"[
            {"duration": 200.0, "syncedLyrics": null},
            {"duration": 190.0, "syncedLyrics": "[00:01.00] far"},
            {"duration": 181.0, "syncedLyrics": "[00:01.00] near"},
            {"duration": 181.5, "syncedLyrics": "not a lrc"}
        ]"#;
        assert_eq!(best(body, 180_000).as_deref(), Some("[00:01.00] near"));
        // Nothing within the tolerance, a non-list, junk: none.
        assert_eq!(best(body, 400_000), None);
        assert_eq!(best("{}", 180_000), None);
        assert_eq!(best("junk", 180_000), None);
        let url = lrclib_search_url("Hello World", "A&B");
        assert!(
            url.starts_with(SEARCH_API) && !url.contains("duration"),
            "{url}"
        );
    }

    #[test]
    fn track_key_is_normalised() {
        assert_eq!(
            track_key("  Queen ", "Bohemian   RHAPSODY", 354_499),
            "queen|bohemian rhapsody|354"
        );
        assert_eq!(
            track_key("queen", "bohemian rhapsody", 354_500),
            "queen|bohemian rhapsody|355"
        );
        assert_ne!(key_hash("a|b|1"), key_hash("a|b|2"));
        assert_eq!(key_hash("a|b|1").len(), 16);
        assert_eq!(key_hash(""), "cbf29ce484222325");
    }

    fn media(title: &str, artist: &str) -> MediaInfo {
        MediaInfo {
            title: title.into(),
            artist: artist.into(),
            playing: true,
            art: None,
            source_app: String::new(),
        }
    }

    #[test]
    fn query_needs_title_artist_and_length() {
        let t = tl(0, 215_000, Instant::now());
        let q = track_query(Some(&media("Song", "Band")), Some(&t)).unwrap();
        assert_eq!(
            q,
            Query {
                key: "band|song|215".into(),
                artist: "Band".into(),
                title: "Song".into(),
                duration_ms: 215_000,
                provisional: false,
            }
        );
        assert_eq!(track_query(Some(&media("Song", " ")), Some(&t)), None);
        assert_eq!(track_query(Some(&media("", "Band")), Some(&t)), None);
        assert_eq!(track_query(Some(&media("Song", "Band")), None), None);
        let no_end = tl(0, 0, Instant::now());
        assert_eq!(
            track_query(Some(&media("Song", "Band")), Some(&no_end)),
            None
        );
        assert_eq!(track_query(None, Some(&t)), None);
    }

    #[test]
    fn url_carries_title_artist_and_seconds() {
        let q = Query {
            key: String::new(),
            artist: "Sigur Rós".into(),
            title: "Hoppípolla & more".into(),
            duration_ms: 268_400,
            provisional: false,
        };
        assert_eq!(
            lrclib_url(&q),
            "https://lrclib.net/api/get?track_name=Hopp%C3%ADpolla%20%26%20more&artist_name=Sigur%20R%C3%B3s&duration=268"
        );
        let short = Query {
            duration_ms: 200,
            ..q
        };
        assert!(lrclib_url(&short).ends_with("&duration=1"));
        assert!(USER_AGENT.starts_with("Vale/"));
    }

    #[test]
    fn answers_map_to_found_none_or_failed() {
        let ok = r#"{"id":1,"trackName":"t","artistName":"a","albumName":"x","duration":200,"instrumental":false,"plainLyrics":"hi","syncedLyrics":"[00:01.00] hi"}"#;
        let Reading::Found(got) = read_reply(200, ok, false) else {
            panic!("not found");
        };
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].lrc, "[00:01.00] hi");
        assert_eq!(got[0].duration_ms, Some(200_000));
        let plain_only = r#"{"id":1,"plainLyrics":"hi","syncedLyrics":null}"#;
        assert_eq!(read_reply(200, plain_only, false), Reading::Empty);
        let instrumental = r#"{"id":1,"instrumental":true,"plainLyrics":null,"syncedLyrics":null}"#;
        assert_eq!(read_reply(200, instrumental, false), Reading::Empty);
        assert_eq!(
            read_reply(200, r#"{"syncedLyrics":"no tags"}"#, false),
            Reading::Empty
        );
        assert_eq!(read_reply(404, r#"{"code":404}"#, false), Reading::Empty);
        assert_eq!(read_reply(400, "", true), Reading::Empty);
        assert_eq!(read_reply(200, "[]", true), Reading::Empty);
        assert_eq!(read_reply(200, "<html>", false), Reading::Transient);
        assert_eq!(read_reply(200, "{}", true), Reading::Transient);
        assert_eq!(read_reply(500, "", false), Reading::Transient);
        assert_eq!(read_reply(503, "", true), Reading::Transient);
        assert_eq!(read_reply(429, "", false), Reading::Transient);
    }

    #[test]
    fn memory_cache_keeps_the_most_recent() {
        let now = 1_800_000_000;
        let mut c = MemoryCache::new(3);
        let lines = Arc::new(vec![l(1, "x")]);
        c.put("a".into(), Some(lines.clone()), now);
        c.put("b".into(), None, now);
        c.put("c".into(), None, now);
        assert_eq!(c.get("a", now), Some(Some(lines.clone())));
        assert_eq!(c.get("b", now), Some(None), "a known miss");
        assert_eq!(c.get("zzz", now), None, "never asked");
        // "c" is now the least recently used.
        c.put("d".into(), None, now);
        assert_eq!(c.keys(), ["d", "b", "a"]);
        c.put("a".into(), None, now);
        assert_eq!(c.keys(), ["a", "d", "b"], "re-put replaces");
        assert_eq!(c.get("a", now), Some(None));
        assert_eq!(MemoryCache::new(MEMORY_SLOTS).cap, 50);
    }

    #[test]
    fn memory_negatives_expire_but_lyrics_do_not() {
        let now = 1_800_000_000;
        let mut c = MemoryCache::new(5);
        c.put("none".into(), None, now);
        c.put("lyrics".into(), Some(Arc::new(vec![l(1, "x")])), now);
        let day = now + NEGATIVE_TTL_SECS;
        assert_eq!(c.get("none", day - 1), Some(None));
        assert_eq!(c.get("none", day), None, "expired");
        assert_eq!(c.keys(), ["lyrics"], "and dropped");
        assert!(matches!(c.get("lyrics", day * 2), Some(Some(_))));
        // A stamp from the future (clock moved back) is not trusted.
        c.put("skew".into(), None, now + 100);
        assert_eq!(c.get("skew", now), None);
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("vale-lyrics-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn disk_cache_round_trip_and_negative_ttl() {
        let tmp = TempDir::new("disk");
        let cache = DiskCache::new(tmp.0.join("lyrics-cache"));
        let now = 1_800_000_000;
        assert_eq!(cache.read("k", now), Cached::Miss, "missing dir is a miss");
        cache.write_lrc("k", "[00:01.00] hi");
        assert_eq!(cache.read("k", now), Cached::Lrc("[00:01.00] hi".into()));
        let file = tmp
            .0
            .join("lyrics-cache")
            .join(format!("{}.{LRC_EXT}", key_hash("k")));
        assert!(file.exists());

        cache.write_nothing("gone", now);
        let marker = tmp
            .0
            .join("lyrics-cache")
            .join(format!("{}.{NONE_EXT}", key_hash("gone")));
        assert!(marker.exists(), "the marker is where the test looks");
        assert_eq!(cache.read("gone", now), Cached::Nothing(now));
        assert_eq!(
            cache.read("gone", now + NEGATIVE_TTL_SECS - 1),
            Cached::Nothing(now)
        );
        assert!(marker.exists());
        // Expired: a miss, and the marker is removed.
        assert_eq!(cache.read("gone", now + NEGATIVE_TTL_SECS), Cached::Miss);
        assert!(!marker.exists());
        // A marker from the future (clock moved back) is not trusted.
        cache.write_nothing("skew", now + 100);
        assert_eq!(cache.read("skew", now), Cached::Miss);
        // Found lyrics replace an earlier "not found".
        cache.write_nothing("later", now);
        cache.write_lrc("later", "[00:02.00] yes");
        assert_eq!(
            cache.read("later", now),
            Cached::Lrc("[00:02.00] yes".into())
        );
    }

    #[test]
    fn oversized_lrc_is_neither_written_nor_read() {
        let tmp = TempDir::new("oversized");
        let cache = DiskCache::new(tmp.0.clone());
        let now = 1_800_000_000;
        let huge = format!("[00:01.00] {}", "x".repeat(MAX_BODY_BYTES));
        let q = query("big");
        let file = tmp.0.join(format!("{}.{LRC_EXT}", key_hash(&q.key)));
        let mut memory = MemoryCache::new(5);
        let f = keep_answer(&q, Answer::Synced(huge.clone()), &cache, &mut memory, now);
        assert!(f.transient && f.lines.is_none());
        cache.write_lrc(&q.key, &huge);
        assert!(!file.exists(), "never written");
        // One put there by hand is skipped.
        std::fs::create_dir_all(&tmp.0).unwrap();
        std::fs::write(&file, &huge).unwrap();
        assert_eq!(cache.read(&q.key, now), Cached::Miss);
        std::fs::write(&file, "[00:01.00] hi").unwrap();
        assert_eq!(cache.read(&q.key, now), Cached::Lrc("[00:01.00] hi".into()));
    }

    #[test]
    fn worker_answers_from_disk_cache() {
        let tmp = TempDir::new("worker");
        let cache = DiskCache::new(tmp.0.clone());
        cache.write_lrc("band|song|200", "[00:01.00] one\n[00:02.00] two");
        let (tx, rx) = mpsc::channel();
        let worker = spawn_worker(tx, || {}, DiskCache::new(tmp.0.clone()));
        assert!(worker.ask(Query {
            key: "band|song|200".into(),
            artist: "Band".into(),
            title: "Song".into(),
            duration_ms: 200_000,
            provisional: false,
        }));
        let event = rx.recv_timeout(SETTLE + Duration::from_secs(5)).unwrap();
        assert_eq!(
            event,
            Event::Lyrics(LyricsFound {
                key: "band|song|200".into(),
                lines: Some(Arc::new(vec![l(1_000, "one"), l(2_000, "two")])),
                transient: false,
            })
        );
    }

    #[test]
    fn queries_settle_to_the_newest() {
        let (tx, rx) = mpsc::channel();
        for key in ["a", "b", "c"] {
            tx.send(query(key)).unwrap();
        }
        // One more arrives while it settles: it wins too.
        let late = tx.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            late.send(query("d")).unwrap();
        });
        let t0 = Instant::now();
        assert_eq!(
            settled(&rx, Duration::from_millis(150)).unwrap().key,
            query("d").key
        );
        assert!(
            t0.elapsed() >= Duration::from_millis(150),
            "waited to settle"
        );
        sender.join().unwrap();
        drop(tx);
        assert!(settled(&rx, Duration::from_millis(10)).is_none());
        assert_eq!(SETTLE, Duration::from_secs(1));
    }

    #[test]
    fn worker_remembers_and_skips_provisional_misses() {
        let tmp = TempDir::new("keep");
        let cache = DiskCache::new(tmp.0.clone());
        let mut memory = MemoryCache::new(MEMORY_SLOTS);
        let now = 1_800_000_000;
        let shaky = Query {
            provisional: true,
            ..query("b")
        };
        // A provisional "not found" is kept nowhere and counts as transient.
        let f = keep_answer(&shaky, Answer::None, &cache, &mut memory, now);
        assert!(f.transient && f.lines.is_none());
        assert_eq!(cache.read(&shaky.key, now), Cached::Miss);
        assert_eq!(remembered(&shaky, &cache, &mut memory, now), None);
        // A settled one is kept on disk and in memory.
        let c = query("c");
        let f = keep_answer(&c, Answer::None, &cache, &mut memory, now);
        assert!(!f.transient);
        assert_eq!(cache.read(&c.key, now), Cached::Nothing(now));
        assert_eq!(memory.get(&c.key, now), Some(None));
        // Lyrics found for a provisional query are fine to keep.
        let f = keep_answer(
            &shaky,
            Answer::Synced("[00:01.00] x".into()),
            &cache,
            &mut memory,
            now,
        );
        assert_eq!(f.lines, Some(Arc::new(vec![l(1_000, "x")])));
        // From disk into memory, keeping the marker's age.
        let mut fresh = MemoryCache::new(MEMORY_SLOTS);
        let later = now + 10;
        let f = remembered(&c, &cache, &mut fresh, later).unwrap();
        assert!(f.lines.is_none() && !f.transient);
        assert_eq!(fresh.get(&c.key, now + NEGATIVE_TTL_SECS), None, "same TTL");
        // Transient failures are never kept.
        let d = query("d");
        let f = keep_answer(&d, Answer::Failed, &cache, &mut memory, now);
        assert!(f.transient);
        assert_eq!(memory.get(&d.key, now), None);
    }

    /// A track keyed like `track_key`: `key` is artist|title, length 200 s.
    fn query(key: &str) -> Query {
        query_len(key, 200_000)
    }

    fn query_len(id: &str, duration_ms: u64) -> Query {
        Query {
            key: format!("{id}|{}", duration_ms / 1000),
            artist: "Band".into(),
            title: id.into(),
            duration_ms,
            provisional: false,
        }
    }

    fn found(key: &str, lines: Option<Vec<LrcLine>>, transient: bool) -> LyricsFound {
        LyricsFound {
            key: key.into(),
            lines: lines.map(Arc::new),
            transient,
        }
    }

    #[test]
    fn state_asks_each_track_once() {
        let mut st = LyricsState::default();
        let a = query("a");
        assert_eq!(st.want(Some(&a), false), None, "paused: no network");
        assert_eq!(st.want(Some(&a), true), Some(a.clone()));
        assert_eq!(st.want(Some(&a), true), None, "asked already");
        assert!(!st.ready(Some(&a)));
        st.found(found(&a.key, Some(song()), false));
        assert!(st.ready(Some(&a)));
        assert_eq!(st.want(Some(&a), true), None, "current already");
        // Another track, then back: each is asked again (the worker's
        // memory answers the second time).
        let b = query_len("b", 245_000);
        assert_eq!(st.want(Some(&b), true), Some(b.clone()));
        st.found(found(&b.key, None, false));
        assert!(!st.ready(Some(&b)));
        assert_eq!(st.want(Some(&b), true), None);
        assert_eq!(
            st.want(Some(&a), true),
            None,
            "its lyrics are still current"
        );
        assert!(st.ready(Some(&a)));
        assert!(!st.ready(None));
        assert_eq!(st.want(None, true), None);
    }

    #[test]
    fn a_failure_is_retried_after_a_back_off_a_few_times() {
        let mut st = LyricsState::default();
        let a = query("a");
        assert!(st.want(Some(&a), true).is_some());
        for _ in 0..RETRIES {
            // Failed on the way: the caller arms the back-off.
            assert!(st.found(found(&a.key, None, true)));
            assert_eq!(st.want(Some(&a), true), None, "not before the back-off");
            st.retry_due();
            assert_eq!(st.want(Some(&a), false), None, "paused: still no network");
            assert_eq!(st.want(Some(&a), true), Some(a.clone()), "asked again");
        }
        // Out of retries: kept until another track was asked.
        assert!(!st.found(found(&a.key, None, true)));
        assert_eq!(st.want(Some(&a), true), None);
        let b = query_len("b", 245_000);
        assert!(st.want(Some(&b), true).is_some());
        st.found(found(&b.key, Some(song()), false));
        assert_eq!(st.want(Some(&a), true), Some(a.clone()));
        // And then it has its retries again.
        assert!(st.found(found(&a.key, None, true)));
        // A late failure for a track no longer asked arms nothing.
        assert!(!st.found(found(&b.key, None, true)));
        // A real "not found" is no failure.
        let c = query("c");
        st.retry_due();
        assert!(st.want(Some(&c), true).is_some());
        assert!(!st.found(found(&c.key, None, false)));
        assert_eq!(st.want(Some(&c), true), None);
    }

    #[test]
    fn only_the_playing_tracks_answer_is_taken() {
        let mut st = LyricsState::default();
        let (a, b) = (query("a"), query("b"));
        st.want(Some(&a), true);
        st.want(Some(&b), true);
        // A late answer for "a" does not replace anything.
        st.found(found(&a.key, Some(song()), false));
        assert!(!st.ready(Some(&a)) && !st.ready(Some(&b)));
        st.found(found(&b.key, Some(vec![l(0, "bee")]), false));
        assert!(st.ready(Some(&b)));
        st.found(found(&a.key, Some(song()), false));
        assert!(st.ready(Some(&b)), "b stays");
        // Wanted while paused (not asked): its answer is still taken.
        let c = query("c");
        assert_eq!(st.want(Some(&c), false), None);
        st.found(found(&c.key, Some(song()), false));
        assert!(st.ready(Some(&c)));
    }

    #[test]
    fn title_before_timeline_waits_for_the_new_length() {
        let mut st = LyricsState::default();
        let a = query_len("a", 200_000);
        st.want(Some(&a), true);
        // "b" shows up with "a"'s length: provisional.
        let early = query_len("b", 200_000);
        let asked = st.want(Some(&early), true).unwrap();
        assert!(asked.provisional);
        // Its own timeline arrives: asked again, settled.
        let b = query_len("b", 245_000);
        let asked = st.want(Some(&b), true).unwrap();
        assert!(!asked.provisional);
        // A seek or a pause (same length) changes nothing.
        assert_eq!(st.want(Some(&b), true), None);
        assert!(!st.observe(&b));
        // The provisional answer is not shown for the settled track.
        st.found(found(&early.key, Some(song()), false));
        assert!(!st.ready(Some(&b)));
    }

    #[test]
    fn timeline_before_title_is_provisional_until_a_new_length() {
        let mut st = LyricsState::default();
        let a = query_len("a", 200_000);
        assert!(!st.want(Some(&a), true).unwrap().provisional);
        // "b"'s length arrives under "a"'s title.
        let mixed = query_len("a", 245_000);
        assert!(st.want(Some(&mixed), true).unwrap().provisional);
        // Then "b"'s title, with the same length: still provisional, so a
        // "not found" is not remembered, but lyrics found are shown.
        let b = query_len("b", 245_000);
        let asked = st.want(Some(&b), true).unwrap();
        assert!(asked.provisional);
        st.found(found(&b.key, Some(song()), false));
        assert!(st.ready(Some(&b)));
        // The next track reported whole is settled again.
        let c = query_len("c", 180_000);
        assert!(!st.want(Some(&c), true).unwrap().provisional);
    }

    #[test]
    fn title_and_timeline_together_are_settled() {
        let mut st = LyricsState::default();
        let a = query_len("a", 200_000);
        assert!(!st.want(Some(&a), true).unwrap().provisional, "first track");
        let b = query_len("b", 245_000);
        assert!(!st.want(Some(&b), true).unwrap().provisional);
        // A small length correction under the same title is not a jump.
        let b2 = query_len("b", 246_000);
        assert!(!st.want(Some(&b2), true).unwrap().provisional);
    }

    #[test]
    fn lyrics_without_words_are_not_ready() {
        let mut st = LyricsState::default();
        let a = query("a");
        st.want(Some(&a), true);
        st.found(found(&a.key, Some(vec![l(1_000, ""), l(2_000, "")]), false));
        assert!(!st.ready(Some(&a)));
    }

    fn playing_state() -> LyricsState {
        let mut st = LyricsState::default();
        let a = query("a");
        st.want(Some(&a), true);
        st.found(found(&a.key, Some(song()), false));
        st
    }

    #[test]
    fn advance_crossfades_between_lines() {
        let t0 = Instant::now();
        let mut st = playing_state();
        assert_eq!(st.shown(t0), None);
        // Before the first line: the rest sign, shown at once.
        assert!(st.advance(0, t0));
        assert_eq!(
            st.shown(t0),
            Some(ShownLine {
                text: "",
                prev: None,
                fade: 1.0
            })
        );
        assert!(!st.fading(t0));
        assert_eq!(st.wait_ms(0, 1.0, None), Some(1_001));
        assert!(!st.advance(500, t0), "same line");
        let t1 = t0 + Duration::from_millis(1_000);
        assert!(st.advance(1_000, t1));
        assert!(st.fading(t1));
        let mid = st.shown(t1 + FADE / 2).unwrap();
        assert_eq!((mid.text, mid.prev), ("one", Some("")));
        assert!((mid.fade - 0.5).abs() < 0.01);
        let done = st.shown(t1 + FADE).unwrap();
        assert_eq!((done.text, done.prev, done.fade), ("one", None, 1.0));
        assert!(!st.fading(t1 + FADE));
        // Seeking backwards fades back.
        let t2 = t1 + Duration::from_secs(1);
        assert!(st.advance(200, t2));
        assert_eq!(st.shown(t2).unwrap().prev, Some("one"));
        assert_eq!(st.wait_ms(200, 1.0, None), Some(801));
        assert_eq!(st.wait_ms(9_000, 1.0, None), None, "after the last line");
    }

    #[test]
    fn same_text_moves_without_a_fade_and_hide_resets() {
        let t0 = Instant::now();
        let mut st = LyricsState::default();
        let a = query("a");
        st.want(Some(&a), true);
        let lines = vec![l(1_000, "la"), l(2_000, "la"), l(3_000, "")];
        st.found(found(&a.key, Some(lines), false));
        st.advance(1_500, t0);
        assert!(!st.advance(2_500, t0), "repeated text: nothing to redraw");
        assert_eq!(st.wait_ms(2_500, 1.0, None), Some(501));
        assert!(st.advance(3_000, t0));
        st.hide();
        assert_eq!(st.shown(t0), None);
        assert!(st.advance(3_000, t0));
        assert_eq!(st.shown(t0).unwrap().prev, None, "no fade after hide");
    }

    #[test]
    fn new_lyrics_reset_the_shown_line() {
        let t0 = Instant::now();
        let mut st = playing_state();
        st.advance(9_000, t0);
        let b = query("b");
        st.want(Some(&b), true);
        st.found(found(&b.key, Some(vec![l(0, "bee")]), false));
        assert_eq!(st.shown(t0), None);
        st.advance(0, t0);
        assert_eq!(st.shown(t0).unwrap().text, "bee");
    }

    /// One real request (run by hand: `cargo test lrclib_live -- --ignored`).
    #[test]
    #[ignore]
    fn lrclib_live() {
        let q = Query {
            key: track_key("Queen", "Bohemian Rhapsody", 354_000),
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            duration_ms: 354_000,
            provisional: false,
        };
        let http = Http::new().unwrap();
        let (status, body) = http
            .get_with_headers_capped(
                &lrclib_url(&q),
                &[("User-Agent", USER_AGENT)],
                MAX_BODY_BYTES,
            )
            .unwrap();
        println!("status {status}, {} bytes", body.len());
        let Reading::Found(found) = read_reply(status, &body, false) else {
            panic!("no synced lyrics: status {status}");
        };
        let lrc = found[0].lrc.clone();
        let lines = parse_lrc(&lrc);
        // Timing only: the lyrics themselves are not printed.
        println!(
            "{} lines; first at {:?} ms",
            lines.len(),
            lines.first().map(|l| l.at_ms)
        );
        assert!(lines.len() > 20 && has_words(&lines));
        assert!(lines.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));
        assert!(current_line(&lines, 60_000).is_some());
    }
}

#[cfg(test)]
mod lookup_tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};

    /// LRC of `n` lines three seconds apart, all shifted by `shift` ms.
    fn lrc(shift: i64, n: u64) -> String {
        (0..n)
            .map(|k| {
                let at = (1_000 + k as i64 * 3_000 + shift) as u64;
                let (m, s, cs) = (at / 60_000, at / 1000 % 60, at % 1000 / 10);
                format!("[{m:02}:{s:02}.{cs:02}] la la {k}\n")
            })
            .collect()
    }

    /// The LRC text of an entry; `tag` tells copies apart in the output.
    fn tagged(tag: &str, shift: i64) -> String {
        format!("[ar:{tag}]\n{}", lrc(shift, 12))
    }

    /// An entry as LRCLIB lists it.
    fn entry(tag: &str, shift: i64, secs: f64) -> serde_json::Value {
        serde_json::json!({"duration": secs, "syncedLyrics": tagged(tag, shift)})
    }

    fn list(items: &[serde_json::Value]) -> String {
        serde_json::Value::Array(items.to_vec()).to_string()
    }

    fn cand(tag: &str, shift: i64, millis: Option<u64>) -> Candidate {
        let lrc = tagged(tag, shift);
        Candidate {
            lines: parse_lrc(&lrc),
            lrc,
            duration_ms: millis,
        }
    }

    fn query(title: &str, artist: &str, secs: u64) -> Query {
        Query {
            key: track_key(artist, title, secs * 1000),
            artist: artist.into(),
            title: title.into(),
            duration_ms: secs * 1000,
            provisional: false,
        }
    }

    type Scripted = Result<Reply, ()>;

    /// A network that answers from a script, with a clock that only moves
    /// when told: every request costs `cost`, every sleep its length.
    struct Fake {
        replies: HashMap<String, VecDeque<Scripted>>,
        fallback: Scripted,
        asked: Vec<String>,
        sleeps: Vec<Duration>,
        base: Instant,
        elapsed: Duration,
        cost: Duration,
    }

    impl Fake {
        fn new(fallback: Scripted) -> Self {
            Fake {
                replies: HashMap::new(),
                fallback,
                asked: Vec::new(),
                sleeps: Vec::new(),
                base: Instant::now(),
                elapsed: Duration::ZERO,
                cost: Duration::ZERO,
            }
        }

        /// Answers to `url`, in order; the last one repeats.
        fn on(mut self, url: &str, replies: Vec<Scripted>) -> Self {
            self.replies.insert(url.to_owned(), replies.into());
            self
        }

        fn times_asked(&self, url: &str) -> usize {
            self.asked.iter().filter(|u| *u == url).count()
        }
    }

    impl Net for Fake {
        fn get(&mut self, url: &str) -> Result<Reply, ()> {
            self.asked.push(url.to_owned());
            self.elapsed += self.cost;
            match self.replies.get_mut(url) {
                Some(queue) if queue.len() > 1 => queue.pop_front().unwrap(),
                Some(queue) => queue.front().cloned().unwrap(),
                None => self.fallback.clone(),
            }
        }

        fn sleep(&mut self, how_long: Duration) {
            self.sleeps.push(how_long);
            self.elapsed += how_long;
        }

        fn now(&self) -> Instant {
            self.base + self.elapsed
        }
    }

    fn ok(body: String) -> Scripted {
        Ok((200, body, None))
    }

    fn status(code: u16, retry_after: Option<&str>) -> Scripted {
        Ok((code, "{}".into(), retry_after.map(str::to_owned)))
    }

    #[test]
    fn retry_delay_reads_whole_seconds_within_one_to_three() {
        let s = Duration::from_secs;
        assert_eq!(retry_delay(None), s(1));
        assert_eq!(retry_delay(Some("1")), s(1));
        assert_eq!(retry_delay(Some("2")), s(2));
        assert_eq!(retry_delay(Some("10")), s(3));
        assert_eq!(retry_delay(Some("0")), s(1));
        assert_eq!(retry_delay(Some("soon")), s(1));
        assert_eq!(retry_delay(Some("Wed, 21 Oct 2026 07:28:00 GMT")), s(1));
        assert_eq!(retry_delay(Some(" 2 ")), s(2));
        assert_eq!(retry_delay(Some("-5")), s(1));
    }

    #[test]
    fn a_busy_server_is_asked_again_after_its_retry_after() {
        let q = query("Song", "Band", 200);
        let url = lrclib_search_url("Song", "Band");
        let hit = list(&[entry("a", 0, 200.0)]);
        let mut net = Fake::new(status(404, None)).on(
            &url,
            vec![status(503, Some("2")), status(503, Some("1")), ok(hit)],
        );
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("a", 0)));
        assert_eq!(net.times_asked(&url), 3);
        assert_eq!(
            net.sleeps,
            vec![Duration::from_secs(2), Duration::from_secs(1)]
        );
    }

    #[test]
    fn a_url_is_retried_twice_at_most() {
        let q = query("Song", "Band", 200);
        let mut net = Fake::new(status(503, Some("1")));
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);
        let mut urls = net.asked.clone();
        urls.sort();
        urls.dedup();
        assert_eq!(urls.len(), 3, "every step still ran: {urls:?}");
        for url in &urls {
            assert_eq!(net.times_asked(url), 3, "one try and two retries: {url}");
        }
        assert_eq!(net.sleeps.len(), 2 * urls.len());
        assert!(net.sleeps.iter().all(|d| *d == Duration::from_secs(1)));
    }

    #[test]
    fn a_429_is_retried_like_a_503() {
        let q = query("Song", "Band", 200);
        let url = lrclib_search_url("Song", "Band");
        let hit = list(&[entry("a", 0, 200.0)]);
        let mut net = Fake::new(status(404, None)).on(&url, vec![status(429, None), ok(hit)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("a", 0)));
        assert_eq!(net.sleeps, vec![Duration::from_secs(1)]);
    }

    #[test]
    fn a_network_error_is_asked_once_more() {
        let q = query("Song", "Band", 200);
        let url = lrclib_search_url("Song", "Band");
        let hit = list(&[entry("a", 0, 200.0)]);
        let mut net = Fake::new(status(404, None)).on(&url, vec![Err(()), ok(hit)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("a", 0)));
        assert_eq!(net.times_asked(&url), 2);

        let mut net = Fake::new(Err(()));
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);
        assert_eq!(net.times_asked(&url), 2, "not asked a third time");
    }

    #[test]
    fn a_network_error_counts_toward_the_retry_cap() {
        let q = query("Song", "Band", 200);
        let url = lrclib_search_url("Song", "Band");
        let mut net = Fake::new(status(404, None)).on(&url, vec![Err(()), status(503, Some("1"))]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);
        assert_eq!(net.times_asked(&url), 3, "one try and two retries in all");
        assert_eq!(net.sleeps.len(), 2);
    }

    #[test]
    fn a_503_on_the_first_step_still_runs_the_others() {
        let q = query("Song", "Band", 200);
        let search = lrclib_search_url("Song", "Band");
        let get = lrclib_url(&q);
        let free = lrclib_query_url("Song", "Band");
        let hit = list(&[entry("q", 0, 200.0)]);
        let mut net = Fake::new(status(404, None))
            .on(&search, vec![status(503, Some("1"))])
            .on(&free, vec![ok(hit)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("q", 0)));
        assert!(net.asked.contains(&get), "the get step ran");
        assert!(net.asked.contains(&free));
    }

    #[test]
    fn a_get_hit_without_a_length_is_taken() {
        let q = query("Song", "Band", 200);
        let get = lrclib_url(&q);
        let body = serde_json::json!({"syncedLyrics": tagged("g", 0)}).to_string();
        let mut net = Fake::new(status(404, None)).on(&get, vec![ok(body)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("g", 0)));
    }

    #[test]
    fn answered_but_empty_everywhere_is_none() {
        let q = query("Song", "Band", 200);
        let mut net = Fake::new(status(404, None));
        assert_eq!(lookup_chain(&q, &mut net), Answer::None);

        let mut net = Fake::new(ok("[]".into())).on(&lrclib_url(&q), vec![status(404, None)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::None);

        // Entries far off the length (more than a longest intro shift, 60 s)
        // are not the track.
        let far = list(&[entry("a", 0, 100.0), entry("b", 0, 110.0)]);
        let mut net = Fake::new(ok(far)).on(&lrclib_url(&q), vec![status(404, None)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::None);

        // A refused request (400) is a definitive "nothing" too.
        let mut net = Fake::new(status(400, None));
        assert_eq!(lookup_chain(&q, &mut net), Answer::None);
    }

    #[test]
    fn trouble_that_never_clears_with_nothing_found_is_failed_not_none() {
        let q = query("Song", "Band", 200);
        let mut net = Fake::new(status(503, Some("1")));
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);

        // One transient request among definitive ones is enough.
        let free = lrclib_query_url("Song", "Band");
        let mut net = Fake::new(status(404, None)).on(&free, vec![status(503, None)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);

        // An unreadable 200 is not an answer either.
        let mut net = Fake::new(ok("<html>".into()));
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);
    }

    #[test]
    fn the_budget_stops_the_chain_and_that_is_a_failure() {
        let q = query("Song", "Band", 200);
        let mut net = Fake::new(status(404, None));
        net.cost = Duration::from_secs(20);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);
        assert_eq!(net.asked.len(), 2, "the third request is past 30 s");

        // Waiting counts: busy answers whose sleeps pass the budget.
        let mut net = Fake::new(status(503, Some("3")));
        net.cost = Duration::from_secs(5);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Failed);
        assert!(net.elapsed <= LOOKUP_BUDGET + Duration::from_secs(5 + 3));
    }

    #[test]
    fn the_budget_does_not_cut_a_found_answer() {
        let q = query("Song", "Band", 200);
        let url = lrclib_search_url("Song", "Band");
        let hit = list(&[entry("a", 0, 200.0)]);
        let mut net = Fake::new(status(404, None)).on(&url, vec![ok(hit)]);
        net.cost = Duration::from_secs(29);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("a", 0)));
    }

    #[test]
    fn artist_and_title_are_split_out_of_a_video_title() {
        let pair = |a: &str, t: &str| Some((a.to_owned(), t.to_owned()));
        assert_eq!(
            split_artist_title(
                "Avril Lavigne - When You're Gone (Official Video)",
                "AvrilLavigneVEVO"
            ),
            pair("Avril Lavigne", "When You're Gone")
        );
        // The artist's own name in front, whatever the channel.
        assert_eq!(
            split_artist_title("Queen - Bohemian Rhapsody", "Queen"),
            pair("Queen", "Bohemian Rhapsody")
        );
        // A Topic channel.
        assert_eq!(
            split_artist_title("Foo - Bar", "Baz - Topic"),
            pair("Foo", "Bar")
        );
        // No " - ", or a first part that is not the artist.
        assert_eq!(
            split_artist_title("Red (Taylor Swift)", "Against The Current"),
            None
        );
        assert_eq!(split_artist_title("Foo - Bar", "Someone"), None);
        // Decoration is cut as `clean_title` does; nothing is left to split.
        assert_eq!(split_artist_title("Some Song - Live", "Band"), None);
        assert_eq!(
            search_names("Some Song - Live", "Band"),
            ("Band".into(), "Some Song".into())
        );
        assert_eq!(
            search_names("Blinding Lights (Official Video)", "The Weeknd - Topic"),
            ("The Weeknd".into(), "Blinding Lights".into())
        );
    }

    #[test]
    fn a_split_title_is_what_the_steps_search_for() {
        let q = query(
            "Avril Lavigne - When You're Gone (Official Video)",
            "AvrilLavigneVEVO",
            239,
        );
        let hit = list(&[entry("v", 0, 239.0)]);
        let free = lrclib_query_url("When You're Gone", "Avril Lavigne");
        let mut net = Fake::new(status(404, None)).on(&free, vec![ok(hit)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("v", 0)));
        let search = lrclib_search_url("When You're Gone", "Avril Lavigne");
        assert!(net.asked.contains(&search));
        assert!(net.asked.contains(&lrclib_url(&q)), "the raw names too");
        let cleaned = lrclib_url_for("When You're Gone", "Avril Lavigne", 239_000);
        assert!(net.asked.contains(&cleaned));
    }

    #[test]
    fn a_cover_is_found_by_the_free_text_search_without_the_trailing_group() {
        let q = query("Red (Taylor Swift)", "Against The Current", 197);
        let only = lrclib_query_url("Red", "Against The Current");
        let hit = list(&[entry("c", 0, 197.0)]);
        let mut net = Fake::new(ok("[]".into()))
            .on(&only, vec![ok(hit)])
            .on(&lrclib_url(&q), vec![status(404, None)]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("c", 0)));
        assert_eq!(
            only,
            format!("{SEARCH_API}?q=Against%20The%20Current%20Red")
        );
        // The whole title was tried first, and nothing is asked twice.
        let whole = lrclib_query_url("Red (Taylor Swift)", "Against The Current");
        assert!(net.asked.contains(&whole));
        let mut seen = net.asked.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), net.asked.len());
    }

    #[test]
    fn the_trailing_group_is_cut_only_when_there_is_one() {
        let cut = |t: &str| without_trailing_group(t);
        assert_eq!(cut("Red (Taylor Swift)").as_deref(), Some("Red"));
        assert_eq!(cut("Red [Live]").as_deref(), Some("Red"));
        assert_eq!(cut("Red"), None);
        assert_eq!(cut("(Red)"), None);
        assert_eq!(cut("Red (a) b"), None);
    }

    #[test]
    fn identical_copies_give_the_closest_length() {
        let cands = vec![
            cand("far", 0, Some(199_000)),
            cand("near", 0, Some(197_400)),
            cand("mid", 0, Some(195_000)),
        ];
        assert_eq!(pick_best(cands, 197_000), Some(tagged("near", 0)));
    }

    #[test]
    fn a_shifted_version_is_never_chosen_even_with_the_closest_length() {
        let cands = vec![
            cand("a", 0, Some(196_000)),
            cand("b", 0, Some(198_500)),
            cand("c", 0, Some(195_500)),
            cand("odd", 2_000, Some(197_000)),
        ];
        // Of the agreeing three, the closest length (1.0 s, 1.5 s, 1.5 s).
        assert_eq!(pick_best(cands, 197_000), Some(tagged("a", 0)));
    }

    #[test]
    fn the_majority_timing_wins_over_a_shifted_one() {
        let cands = vec![
            cand("late", 1_500, Some(197_000)),
            cand("x", 0, Some(198_000)),
            cand("y", 0, Some(198_500)),
        ];
        assert_eq!(pick_best(cands, 197_000), Some(tagged("x", 0)));
    }

    #[test]
    fn candidates_more_than_three_seconds_off_are_ignored() {
        let cands = vec![
            cand("off", 0, Some(190_000)),
            cand("edge", 0, Some(200_100)),
            cand("ok", 0, Some(199_000)),
        ];
        assert_eq!(pick_best(cands, 197_000), Some(tagged("ok", 0)));
        // Far shorter than the playing length (97 s: over the longest intro
        // shift) and far longer: none.
        let far = vec![cand("off", 0, Some(100_000))];
        assert_eq!(pick_best(far, 197_000), None);
        let long = vec![cand("off", 0, Some(300_000))];
        assert_eq!(pick_best(long, 197_000), None);
        assert_eq!(pick_best(Vec::new(), 197_000), None);
    }

    #[test]
    fn a_single_candidate_is_returned_even_without_a_length() {
        let one = cand("only", 0, None);
        assert_eq!(pick_best(vec![one], 197_000), Some(tagged("only", 0)));
    }

    #[test]
    fn versions_with_nothing_in_common_fall_back_to_the_closer_length() {
        let make = |text: &str, millis: u64| Candidate {
            lrc: text.into(),
            lines: parse_lrc(text),
            duration_ms: Some(millis),
        };
        let a = make("[00:01.00] one\n", 196_000);
        let b = make("[00:01.00] two\n", 197_100);
        assert_eq!(
            pick_best(vec![a, b], 197_000),
            Some("[00:01.00] two\n".into())
        );
    }

    #[test]
    fn repeated_lines_are_matched_in_order() {
        // A chorus sung twice: each occurrence is compared with its own.
        let text = |shift: i64| {
            format!(
                "[00:{:02}.00] verse\n[00:{:02}.00] chorus\n[00:{:02}.00] bridge\n[00:{:02}.00] chorus\n",
                10 + shift,
                20 + shift,
                30 + shift,
                40 + shift
            )
        };
        let a = times_by_text(&parse_lrc(&text(0)));
        let b = times_by_text(&parse_lrc(&text(1)));
        assert_eq!(shift_between(&a, &b), Some(-1000));
        assert_eq!(shift_between(&a, &a), Some(0));
        // Under three shared lines there is no value.
        let few = times_by_text(&parse_lrc("[00:01.00] verse\n[00:02.00] chorus\n"));
        assert_eq!(shift_between(&a, &few), None);
    }

    #[test]
    fn candidates_skip_entries_without_synced_lyrics() {
        let body = list(&[
            serde_json::json!({"duration": 1.0, "syncedLyrics": null}),
            serde_json::json!({"duration": 1.0, "syncedLyrics": "no tags"}),
            entry("a", 0, 197.0),
        ]);
        let got = candidates(&body);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].duration_ms, Some(197_000));
        assert!(candidates("junk").is_empty());
    }

    #[test]
    fn markers_of_the_older_lookups_are_ignored() {
        assert_eq!(NONE_EXT, "none4");
        assert_eq!(LRC_EXT, "lrc2");
        let dir = std::env::temp_dir().join(format!("vale-lookup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = DiskCache::new(dir.clone());
        let now = 1_800_000_000;
        for (key, ext) in [("old", "none2"), ("old3", "none3")] {
            let old = dir.join(format!("{}.{ext}", key_hash(key)));
            std::fs::write(old, now.to_string()).unwrap();
            assert_eq!(cache.read(key, now), Cached::Miss, ".{ext} is ignored");
        }
        // An LRC written by the older pick is not read either.
        let old_lrc = dir.join(format!("{}.lrc", key_hash("oldlrc")));
        std::fs::write(old_lrc, "[00:01.00] hi").unwrap();
        assert_eq!(cache.read("oldlrc", now), Cached::Miss);
        cache.write_nothing("new", now);
        assert_eq!(cache.read("new", now), Cached::Nothing(now));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real `/api/search` replies (trimmed to ten lines per entry).
    const PERFECT: &str = include_str!("testdata/lrclib-perfect-ed-sheeran.json");
    const SUPERMAN: &str = include_str!("testdata/lrclib-superman-five-for-fighting.json");

    fn first_worded(lrc: &str) -> LrcLine {
        parse_lrc(lrc)
            .into_iter()
            .find(|l| !alnum_key(&l.text).is_empty())
            .expect("a worded line")
    }

    /// The `syncedLyrics` of every entry, as uploaded.
    fn uploads(body: &str) -> Vec<(u64, String)> {
        candidates(body)
            .into_iter()
            .map(|c| (c.duration_ms.unwrap_or(0), c.lrc))
            .collect()
    }

    #[test]
    fn the_fixtures_are_whole() {
        assert_eq!(candidates(PERFECT).len(), 20);
        assert_eq!(
            candidates(SUPERMAN).len(),
            19,
            "one entry has no synced lyrics"
        );
        let lengths: Vec<u64> = uploads(SUPERMAN).iter().map(|u| u.0).collect();
        assert!(lengths.contains(&222_198) && lengths.contains(&220_816));
    }

    #[test]
    fn a_video_with_a_longer_intro_gets_the_lyrics_shifted_later() {
        // Perfect: the official video is 282 s, the album timing's family has
        // a lower median length of 263 s: every line comes 19 s later.
        let got = pick_best(candidates(PERFECT), 282_000).expect("lyrics");
        let first = first_worded(&got);
        assert_eq!(first.text, "I found a love, for me");
        assert_eq!(first.at_ms, 2_980 + 19_000);
        let original = parse_lrc(&uploads(PERFECT)[0].1);
        let shifted = parse_lrc(&got);
        assert_eq!(shifted.len(), original.len());
        for (s, o) in shifted.iter().zip(&original) {
            assert_eq!(s.at_ms, o.at_ms + 19_000);
            assert_eq!(s.text, o.text);
        }
    }

    #[test]
    fn the_album_length_is_not_shifted() {
        let got = pick_best(candidates(PERFECT), 263_000).expect("lyrics");
        assert_eq!(first_worded(&got).at_ms, 2_980);
        assert!(
            uploads(PERFECT).iter().any(|u| u.1 == got),
            "an upload as it is"
        );
        // Exactly 3 s longer than the family's reference: not over it.
        let edge = pick_best(candidates(PERFECT), 266_000).expect("lyrics");
        assert_eq!(first_worded(&edge).at_ms, 2_980);
        // A shorter playing version is never shifted.
        let shorter = pick_best(candidates(PERFECT), 259_500).expect("lyrics");
        assert_eq!(first_worded(&shorter).at_ms, 2_980);
    }

    #[test]
    fn the_intro_shift_stops_at_sixty_seconds() {
        // No entry within 3 s: the shift is the only way to an answer.
        let at = |ms| pick_best(candidates(PERFECT), ms);
        assert_eq!(first_worded(&at(323_000).unwrap()).at_ms, 2_980 + 60_000);
        assert_eq!(at(323_001), None);
        assert_eq!(at(345_000), None);
        // Over 3 s and up to 60 s longer than every length: shifted.
        assert_eq!(first_worded(&at(310_000).unwrap()).at_ms, 2_980 + 47_000);
    }

    #[test]
    fn the_exact_length_entries_win_the_tight_window() {
        // Superman at 219 s: only the three 219-s uploads are within 1.5 s;
        // they time "I can't stand to fly" at 18.41 and are right as they are.
        let got = pick_best(candidates(SUPERMAN), 219_000).expect("lyrics");
        let first = first_worded(&got);
        assert_eq!(first.text, "I can't stand to fly");
        assert_eq!(first.at_ms, 18_410);
        let exact: Vec<String> = uploads(SUPERMAN)
            .into_iter()
            .filter(|u| u.0 == 219_000)
            .map(|u| u.1)
            .collect();
        assert!(exact.contains(&got), "an upload as it is, not shifted");
    }

    #[test]
    fn a_longer_superman_is_not_shifted() {
        // At 222 s the entries around it (221-223 s) agree within a second:
        // no shift, and not the 18.41 timing of the 219-s ones.
        let got = pick_best(candidates(SUPERMAN), 222_000).expect("lyrics");
        // Copies of one timing share their text: one of them is 220.5-223.5 s.
        let lengths: Vec<u64> = uploads(SUPERMAN)
            .into_iter()
            .filter(|u| u.1 == got)
            .map(|u| u.0)
            .collect();
        assert!(!lengths.is_empty(), "returned exactly as an entry uploaded it");
        assert!(
            lengths.iter().any(|l| (220_500..=223_500).contains(l)),
            "{lengths:?}"
        );
        assert_ne!(first_worded(&got).at_ms, 18_410);
    }

    #[test]
    fn a_shorter_playing_version_keeps_a_close_upload_as_it_is() {
        // Three long copies of one timing and one 188-s upload of nearly the
        // same timing; playing 189 s: the family's reference is 200 s, which is
        // longer, so nothing is shifted.
        let cands = vec![
            cand("long1", 0, Some(200_000)),
            cand("long2", 0, Some(200_000)),
            cand("long3", 0, Some(200_000)),
            cand("short", 100, Some(188_000)),
        ];
        assert_eq!(pick_best(cands, 189_000), Some(tagged("short", 100)));
    }

    #[test]
    fn nothing_within_three_seconds_and_a_shorter_playing_version_is_none() {
        let cands = vec![
            cand("a", 0, Some(200_000)),
            cand("b", 0, Some(200_000)),
            cand("c", 0, Some(201_000)),
        ];
        assert_eq!(pick_best(cands.clone(), 190_000), None);
        // One within 3 s: it is returned, and 2.9 s over the reference is no
        // shift.
        assert_eq!(pick_best(cands.clone(), 202_900), Some(tagged("c", 0)));
        // Longer by over 3 s: shifted by the extra length.
        let got = pick_best(cands, 210_000).expect("shifted");
        assert_eq!(first_worded(&got).at_ms, 1_000 + 10_000);
    }

    #[test]
    fn a_single_get_reply_is_never_shifted() {
        let one = |millis| vec![cand("g", 0, millis)];
        assert_eq!(
            pick_exact(one(Some(200_000)), 202_900),
            Some(tagged("g", 0))
        );
        assert_eq!(pick_exact(one(Some(200_000)), 203_001), None);
        assert_eq!(
            pick_exact(one(Some(200_000)), 197_000),
            Some(tagged("g", 0))
        );
        assert_eq!(pick_exact(one(Some(200_000)), 120_000), None);
        assert_eq!(pick_exact(one(None), 120_000), Some(tagged("g", 0)));
        assert_eq!(pick_exact(Vec::new(), 120_000), None);
    }

    #[test]
    fn a_get_reply_for_a_shorter_version_is_not_shifted_in_the_chain() {
        let q = query("Song", "Band", 282);
        let get = serde_json::json!({"duration": 263.0, "syncedLyrics": tagged("g", 0)});
        let mut net = Fake::new(status(404, None)).on(&lrclib_url(&q), vec![ok(get.to_string())]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::None);
        let near = serde_json::json!({"duration": 281.0, "syncedLyrics": tagged("g", 0)});
        let mut net = Fake::new(status(404, None)).on(&lrclib_url(&q), vec![ok(near.to_string())]);
        assert_eq!(lookup_chain(&q, &mut net), Answer::Synced(tagged("g", 0)));
    }

    #[test]
    fn format_lrc_reads_back_as_it_was() {
        let lines = vec![
            LrcLine {
                at_ms: 0,
                text: "start".into(),
            },
            LrcLine {
                at_ms: 12_345,
                text: "a line".into(),
            },
            LrcLine {
                at_ms: 20_007,
                text: String::new(),
            },
            LrcLine {
                at_ms: 20_007,
                text: "same time".into(),
            },
            LrcLine {
                at_ms: 3_725_999,
                text: "after an hour".into(),
            },
        ];
        let text = format_lrc(&lines);
        assert!(text.contains("[00:12.345]a line\n"), "{text}");
        assert!(
            text.contains("[00:20.007]\n"),
            "a gap is only its tag: {text}"
        );
        assert!(text.contains("[62:05.999]after an hour"), "{text}");
        assert_eq!(parse_lrc(&text), lines);
        assert_eq!(format_lrc(&[]), "");
    }

    #[test]
    fn a_longer_video_is_found_shifted_by_the_whole_lookup() {
        let q = query("Perfect", "Ed Sheeran", 282);
        let url = lrclib_search_url("Perfect", "Ed Sheeran");
        let mut net = Fake::new(status(404, None)).on(&url, vec![ok(PERFECT.to_string())]);
        let Answer::Synced(text) = lookup_chain(&q, &mut net) else {
            panic!("no lyrics");
        };
        assert_eq!(first_worded(&text).at_ms, 21_980);
        assert_eq!(net.asked, vec![url], "the first step was enough");
        // What is cached and shown is that same text.
        let dir = std::env::temp_dir().join(format!("vale-shifted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = DiskCache::new(dir.clone());
        let mut memory = MemoryCache::new(5);
        let f = keep_answer(
            &q,
            Answer::Synced(text.clone()),
            &cache,
            &mut memory,
            1_800_000_000,
        );
        assert_eq!(f.lines.as_deref(), Some(&parse_lrc(&text)));
        assert_eq!(cache.read(&q.key, 1_800_000_000), Cached::Lrc(text));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
