//! The mic hears the speakers: when the PC audio plays a meeting, the "Me"
//! channel repeats it. A "Me" segment whose content words mostly appear in
//! what "Others" said around the same time is that echo and is dropped.
//! Segments of everyone else, and unlabelled ones, are never touched.

use super::output::{Segment, Speaker};

/// How far "Others" may sit outside the "Me" segment and still count.
const SLACK_MS: u64 = 1_000;
/// The share of content words that must match to call a segment an echo.
const MATCH_SHARE: f64 = 0.5;

const STOP: &[&str] = &[
    "jadi", "yang", "itu", "ini", "kalau", "juga", "udah", "sudah", "tapi", "dan", "atau", "gitu",
    "nah", "kan", "aja", "saja", "gue", "kita", "kami", "saya", "kamu", "mereka", "ada", "untuk",
    "dengan", "dari", "karena", "lagi", "tuh", "nih", "sih", "deh", "dong", "bang", "pada", "akan",
    "bisa", "mau", "nya", "the", "and", "that", "this", "is", "to", "of", "yes", "okay", "oke",
];

/// Whether no "Others" segment sent later can overlap this one (`others_settled_ms`: the
/// Others channel is read up to there), so `drop_echo`'s verdict on it is final.
pub fn settled(me: &Segment, others_settled_ms: u64) -> bool {
    me.end_ms.saturating_add(SLACK_MS) < others_settled_ms
}

/// Drops the "Me" segments that echo "Others"; the order is kept.
pub fn drop_echo(segments: Vec<Segment>) -> Vec<Segment> {
    let words: Vec<Vec<String>> = segments.iter().map(|s| words(&s.text)).collect();
    let keep: Vec<bool> = segments
        .iter()
        .enumerate()
        .map(|(i, me)| me.speaker != Some(Speaker::Me) || !is_echo(me, &words[i], &segments, &words))
        .collect();
    segments
        .into_iter()
        .zip(keep)
        .filter_map(|(s, keep)| keep.then_some(s))
        .collect()
}

fn is_echo(me: &Segment, mine: &[String], all: &[Segment], all_words: &[Vec<String>]) -> bool {
    let pool: Vec<&String> = all
        .iter()
        .zip(all_words)
        .filter(|(o, _)| {
            o.speaker == Some(Speaker::Others)
                && o.start_ms <= me.end_ms + SLACK_MS
                && o.end_ms + SLACK_MS >= me.start_ms
        })
        .flat_map(|(_, w)| w.iter())
        .collect();
    let matches = |w: &String| pool.iter().any(|p| close(w, p));
    let content: Vec<&String> = mine
        .iter()
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str()))
        .collect();
    if content.is_empty() {
        return !mine.is_empty() && mine.iter().all(matches);
    }
    let hits = content.iter().filter(|w| matches(w)).count();
    hits as f64 / content.len() as f64 >= MATCH_SHARE
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Equal, or a near spelling of the same word (both at least 3 chars).
fn close(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (la, lb) = (a.chars().count(), b.chars().count());
    la >= 3 && lb >= 3 && levenshtein(a, b) * 3 <= la.max(lb)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (diag + usize::from(ca != *cb)).min(row[j] + 1).min(row[j + 1] + 1);
            diag = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TURBO: &str = include_str!("testdata/run2254-turbo.txt");
    const SMALL: &str = include_str!("testdata/run2254-small.txt");

    fn clock(s: &str) -> u64 {
        let (m, s) = s.split_once(':').unwrap();
        (m.parse::<u64>().unwrap() * 60 + s.parse::<u64>().unwrap()) * 1000
    }

    /// Parses `[MM:SS - MM:SS] Saya|Peserta text`.
    fn fixture(text: &str) -> Vec<Segment> {
        text.lines()
            .map(|line| {
                let (times, rest) = line[1..].split_once("] ").unwrap();
                let (a, b) = times.split_once(" - ").unwrap();
                let (who, said) = rest.split_once(' ').unwrap();
                Segment {
                    start_ms: clock(a),
                    end_ms: clock(b),
                    speaker: Some(if who == "Saya" { Speaker::Me } else { Speaker::Others }),
                    text: said.trim().to_string(),
                }
            })
            .collect()
    }

    fn me(s: &[Segment]) -> usize {
        s.iter().filter(|s| s.speaker == Some(Speaker::Me)).count()
    }

    fn others(s: &[Segment]) -> Vec<Segment> {
        s.iter().filter(|s| s.speaker == Some(Speaker::Others)).cloned().collect()
    }

    fn seg(start_ms: u64, end_ms: u64, speaker: Option<Speaker>, text: &str) -> Segment {
        Segment { start_ms, end_ms, speaker, text: text.into() }
    }

    #[test]
    fn run2254_turbo_echo_is_dropped() {
        let input = fixture(TURBO);
        assert_eq!(me(&input), 12);
        let out = drop_echo(input.clone());
        assert_eq!(me(&out), 0, "{out:?}");
        assert_eq!(out, others(&input));
        assert_eq!(out.len(), 7);
    }

    #[test]
    fn run2254_small_echo_is_dropped() {
        let input = fixture(SMALL);
        assert_eq!(me(&input), 7);
        let out = drop_echo(input.clone());
        assert_eq!(me(&out), 0, "{out:?}");
        assert_eq!(out, others(&input));
        assert_eq!(out.len(), 7);
    }

    #[test]
    fn a_real_reply_over_someone_talking_stays() {
        let mut input = fixture(TURBO);
        let reply = seg(10_000, 13_000, Some(Speaker::Me), "Boleh, jadi deadline-nya Jumat ya");
        input.push(reply.clone());
        let out = drop_echo(input);
        assert_eq!(me(&out), 1);
        assert!(out.contains(&reply));
    }

    #[test]
    fn me_without_an_overlapping_other_stays() {
        let mut input = fixture(TURBO);
        let late = seg(40_000, 42_000, Some(Speaker::Me), "Coba adres pertanyaan");
        input.push(late.clone());
        let out = drop_echo(input);
        assert!(out.contains(&late));
    }

    #[test]
    fn filler_alone_drops_only_when_every_word_matches() {
        let other = seg(4_000, 7_000, Some(Speaker::Others), "Coba adres pertanyaan gue ya, pertanyaan");
        let ya = seg(5_000, 5_500, Some(Speaker::Me), "ya");
        let oke = seg(5_000, 5_500, Some(Speaker::Me), "oke");
        assert_eq!(drop_echo(vec![other.clone(), ya]), vec![other.clone()]);
        assert_eq!(drop_echo(vec![other.clone(), oke.clone()]), vec![other, oke]);
    }

    #[test]
    fn unlabelled_segments_are_untouched() {
        let input = vec![
            seg(1_000, 3_000, None, "Coba adres pertanyaan"),
            seg(1_000, 3_000, Some(Speaker::Others), "Coba adres pertanyaan"),
            seg(1_000, 3_000, None, "Coba adres pertanyaan"),
        ];
        assert_eq!(drop_echo(input.clone()), input);
    }

    #[test]
    fn near_spellings_match() {
        assert!(close("adres", "address"));
        assert!(close("dimilai", "dinilai"));
        assert!(!close("gw", "gue"));
        assert!(!close("jumat", "jual"));
    }

    #[test]
    fn a_me_segment_is_settled_a_slack_after_its_end() {
        let me = Segment { start_ms: 1_000, end_ms: 3_000, speaker: Some(Speaker::Me), text: "halo".into() };
        assert!(!settled(&me, 4_000));
        assert!(settled(&me, 4_001));
        assert!(settled(&me, u64::MAX));
        assert!(!settled(&Segment { end_ms: u64::MAX - 10, ..me.clone() }, u64::MAX));
    }
}
