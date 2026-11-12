//! The one-time move of old recordings into the `Audio` and `Video`
//! subfolders, and the matching rewrite of the transcript queue.
//! Rename only: nothing is overwritten, copied or deleted.

use std::path::{Path, PathBuf};

use super::prefs::{Move, SplitPlan};

/// Moves every recording `<stem>.<ext>` directly in `m.from` into `m.to`, with
/// the files that share its stem (sidecar, transcript, cursor log). An
/// unfinished `<stem>.<ext>.part` counts as a recording too. Returns the
/// `(old, new)` paths of the finished recordings moved (the queue never holds
/// a `.part`). A name already taken in `m.to` or a failed rename leaves that
/// file where it is. In a folder the user picked (`m.vale_only`) only a
/// recording with a Vale stamp or sidecar is touched.
pub fn move_recordings(m: &Move) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(&m.from) else {
        return Vec::new();
    };
    let suffix = format!(".{}", m.ext).to_ascii_lowercase();
    let part_suffix = format!("{suffix}.part");
    let files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.file_name().to_str()?.to_owned(), e.path())))
        .collect();
    let is_main = |name: &str| name.to_ascii_lowercase().ends_with(&suffix);
    let part_stem = |name: &str| {
        let lower = name.to_ascii_lowercase();
        lower
            .ends_with(&part_suffix)
            .then(|| name[..name.len() - part_suffix.len()].to_owned())
    };
    let mut stems: Vec<String> = Vec::new();
    for (name, _) in &files {
        let stem = if is_main(name) {
            name[..name.len() - suffix.len()].to_owned()
        } else if let Some(stem) = part_stem(name) {
            stem
        } else {
            continue;
        };
        if !stem.is_empty() && !stems.iter().any(|t| t.eq_ignore_ascii_case(&stem)) {
            stems.push(stem);
        }
    }
    let mut moved = Vec::new();
    for stem in &stems {
        let prefix = format!("{stem}.").to_ascii_lowercase();
        let finished = format!("{stem}{suffix}").to_ascii_lowercase();
        let main = files.iter().find(|(n, _)| n.to_ascii_lowercase() == finished);
        let own_part = |n: &str| part_stem(n).is_some_and(|t| t.eq_ignore_ascii_case(stem));
        let part = files.iter().find(|(n, _)| own_part(n));
        if m.vale_only {
            let sidecar = format!("{prefix}vale.json");
            let has_sidecar = files.iter().any(|(n, _)| n.to_ascii_lowercase() == sidecar);
            if !has_sidecar && !has_vale_stamp(stem) {
                continue;
            }
        }
        let Some((name, path)) = main.or(part) else {
            continue;
        };
        if !rename_new(path, &m.to.join(name), &m.to) {
            continue;
        }
        if main.is_some() {
            moved.push((path.clone(), m.to.join(name)));
        }
        for (cname, cpath) in &files {
            // Another recording whose stem starts with this one (`a.b.m4a`
            // next to `a.m4a`) is not a companion.
            let other = is_main(cname) || (part_stem(cname).is_some() && !own_part(cname));
            let shares_stem = cname.to_ascii_lowercase().starts_with(&prefix);
            if cname == name || other || !shares_stem {
                continue;
            }
            rename_new(cpath, &m.to.join(cname), &m.to);
        }
    }
    moved
}

/// The stem starts like `naming::file_stem`: `YYYY-MM-DD HHmm`.
fn has_vale_stamp(stem: &str) -> bool {
    let b = stem.as_bytes();
    b.len() >= 15
        && b[..15].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b' ',
            _ => c.is_ascii_digit(),
        })
        && (b.len() == 15 || b[15] == b' ')
}

/// Renames `from` to `to` inside `dir`, creating `dir` first. False when the
/// target is taken or anything fails; the error is printed. The OS refuses to
/// replace an existing file, so a name taken at any moment is never lost.
fn rename_new(from: &Path, to: &Path, dir: &Path) -> bool {
    let result = std::fs::create_dir_all(dir).and_then(|()| move_no_replace(from, to));
    if let Err(e) = &result {
        eprintln!("vale: could not move {}: {e}", from.display());
    }
    result.is_ok()
}

/// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`: it fails when `to`
/// exists, where `std::fs::rename` would replace it.
fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    use windows::core::PCWSTR;
    let wide = |p: &Path| p.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let (f, t) = (wide(from), wide(to));
    unsafe { MoveFileExW(PCWSTR(f.as_ptr()), PCWSTR(t.as_ptr()), MOVEFILE_WRITE_THROUGH) }
        .map_err(std::io::Error::from)
}

/// Runs every move, then removes the old video folder when it is empty.
pub fn apply(plan: &SplitPlan) -> Vec<(PathBuf, PathBuf)> {
    use crate::transcribe::queue::same_audio;
    let mut moved = Vec::new();
    for m in &plan.moves {
        moved.extend(move_recordings(m));
        // Never the root (the audio folder, or the one folder picked for both
        // kinds); `remove_dir` fails on a folder that still holds anything.
        let is_root = m.to.parent().is_some_and(|r| same_audio(&m.from, r))
            || plan.root.as_deref().is_some_and(|r| same_audio(&m.from, r));
        if m.ext == "mp4" && !is_root {
            let _ = std::fs::remove_dir(&m.from);
        }
    }
    moved
}

/// The queue JSON with the `audio` paths in `moved` replaced; `None` when
/// nothing matched or the text is not a queue.
pub fn rewrite_queue(text: &str, moved: &[(PathBuf, PathBuf)]) -> Option<String> {
    use crate::transcribe::queue::same_audio;
    let mut v: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut changed = false;
    for job in v.get_mut("jobs")?.as_array_mut()? {
        let Some(audio) = job.get("audio").and_then(|a| a.as_str()) else {
            continue;
        };
        if let Some((_, new)) = moved.iter().find(|(old, _)| same_audio(old, Path::new(audio))) {
            job["audio"] = serde_json::Value::String(new.to_string_lossy().into_owned());
            changed = true;
        }
    }
    changed.then(|| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Dir(PathBuf);
    impl Dir {
        fn new(name: &str) -> Dir {
            let p = std::env::temp_dir().join(format!("vale-split-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn put(&self, rel: &str, bytes: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, bytes).unwrap();
            p
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mv(from: &Path, to: &Path, ext: &'static str) -> Move {
        Move { from: from.to_path_buf(), to: to.to_path_buf(), ext, vale_only: false }
    }

    fn picked(from: &Path, to: &Path, ext: &'static str) -> Move {
        Move { vale_only: true, ..mv(from, to, ext) }
    }

    #[test]
    fn moves_a_recording_with_its_companions() {
        let d = Dir::new("companions");
        let names = ["a.m4a", "a.vale.json", "a.md", "a.srt", "a.transcript.md", "notes.txt"];
        for n in names.into_iter().chain(["b.m4a.part"]) {
            d.put(n, n);
        }
        let to = d.0.join("Audio");
        let moved = move_recordings(&mv(&d.0, &to, "m4a"));
        assert_eq!(moved, vec![(d.0.join("a.m4a"), to.join("a.m4a"))]);
        for n in ["a.m4a", "a.vale.json", "a.md", "a.srt", "a.transcript.md"] {
            assert!(to.join(n).is_file(), "{n} moved");
            assert!(!d.0.join(n).exists(), "{n} left the root");
        }
        assert!(d.0.join("notes.txt").is_file());
        // A lone unfinished recording moves too, but is not a returned pair.
        assert!(to.join("b.m4a.part").is_file() && !d.0.join("b.m4a.part").exists());
    }

    #[test]
    fn an_unfinished_recording_moves_with_its_companions_once() {
        let d = Dir::new("part");
        for n in ["u.m4a.part", "u.vale.json", "w.m4a", "w.m4a.part", "w.md", "w.x.m4a.part"] {
            d.put(n, n);
        }
        let to = d.0.join("Audio");
        let moved = move_recordings(&mv(&d.0, &to, "m4a"));
        assert_eq!(moved, vec![(d.0.join("w.m4a"), to.join("w.m4a"))]);
        for n in ["u.m4a.part", "u.vale.json", "w.m4a", "w.m4a.part", "w.md", "w.x.m4a.part"] {
            assert!(to.join(n).is_file(), "{n} moved");
        }
        assert!(!d.0.join("u.m4a.part").exists());
    }

    #[test]
    fn a_picked_folder_moves_only_what_vale_made() {
        let d = Dir::new("picked");
        for n in [
            "Song.m4a",
            "Song.jpg",
            "2026-11-12 0930.m4a",
            "2026-11-12 0930.md",
            "Rapat.m4a",
            "Rapat.vale.json",
            "Lone.m4a.part",
            ".m4a",
            "Draft.m4a.part",
            "Draft.vale.json",
        ] {
            d.put(n, n);
        }
        let to = d.0.join("Audio");
        let mut moved = move_recordings(&picked(&d.0, &to, "m4a"));
        moved.sort();
        assert_eq!(
            moved,
            vec![
                (d.0.join("2026-11-12 0930.m4a"), to.join("2026-11-12 0930.m4a")),
                (d.0.join("Rapat.m4a"), to.join("Rapat.m4a")),
            ]
        );
        for n in ["Song.m4a", "Song.jpg", "Lone.m4a.part", ".m4a"] {
            assert!(d.0.join(n).is_file(), "{n} stays");
        }
        for n in ["2026-11-12 0930.md", "Rapat.vale.json", "Draft.m4a.part", "Draft.vale.json"] {
            assert!(to.join(n).is_file(), "{n} moved");
        }
    }

    #[test]
    fn vale_own_folder_moves_any_name_but_an_empty_stem_stays() {
        let d = Dir::new("own");
        d.put("Song.m4a", "s");
        d.put(".m4a", "x");
        let to = d.0.join("Audio");
        let moved = move_recordings(&mv(&d.0, &to, "m4a"));
        assert_eq!(moved, vec![(d.0.join("Song.m4a"), to.join("Song.m4a"))]);
        assert!(d.0.join(".m4a").is_file());
    }

    #[test]
    fn one_picked_folder_for_both_kinds_splits_into_two_subfolders() {
        let d = Dir::new("shared");
        let root = d.0.join("R");
        d.put("R/a.m4a", "a");
        d.put("R/v.mp4", "v");
        let plan = SplitPlan {
            root: Some(root.clone()),
            moves: vec![
                mv(&root, &root.join("Audio"), "m4a"),
                mv(&root, &root.join("Video"), "mp4"),
            ],
        };
        let moved = apply(&plan);
        assert_eq!(moved.len(), 2);
        assert!(root.join("Audio").join("a.m4a").is_file());
        assert!(root.join("Video").join("v.mp4").is_file());
        assert!(root.is_dir());
        let upper = PathBuf::from(root.to_string_lossy().to_uppercase());
        let plan = SplitPlan {
            root: Some(root.clone()),
            moves: vec![mv(&upper, &root.join("Video"), "mp4")],
        };
        apply(&plan);
        assert!(root.is_dir(), "a case-different root is still never removed");
    }

    #[test]
    fn video_moves_the_cursor_log_too() {
        let d = Dir::new("video");
        let log = format!("x.{}", crate::screenrec::cursor_log::EXTENSION);
        for n in ["x.mp4", "x.vale.json", log.as_str()] {
            d.put(n, n);
        }
        let to = d.0.join("Video");
        move_recordings(&mv(&d.0, &to, "mp4"));
        for n in ["x.mp4", "x.vale.json", log.as_str()] {
            assert!(to.join(n).is_file(), "{n} moved");
        }
    }

    #[test]
    fn a_taken_name_is_never_overwritten() {
        let d = Dir::new("taken");
        d.put("a.m4a", "new");
        d.put("Audio/a.m4a", "old");
        let to = d.0.join("Audio");
        let moved = move_recordings(&mv(&d.0, &to, "m4a"));
        assert!(moved.is_empty());
        assert_eq!(fs::read_to_string(d.0.join("a.m4a")).unwrap(), "new");
        assert_eq!(fs::read_to_string(to.join("a.m4a")).unwrap(), "old");
    }

    #[test]
    fn a_failed_rename_leaves_that_file_and_moves_the_rest() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = Dir::new("locked");
        let a = d.put("a.m4a", "a");
        d.put("b.m4a", "b");
        let _hold = fs::OpenOptions::new().read(true).share_mode(0).open(&a).unwrap();
        let to = d.0.join("Audio");
        let moved = move_recordings(&mv(&d.0, &to, "m4a"));
        assert!(a.is_file(), "the locked file stays");
        assert!(to.join("b.m4a").is_file());
        assert_eq!(moved, vec![(d.0.join("b.m4a"), to.join("b.m4a"))]);
    }

    #[test]
    fn nothing_to_move_creates_no_folder() {
        let d = Dir::new("empty");
        let to = d.0.join("Audio");
        assert!(move_recordings(&mv(&d.0, &to, "m4a")).is_empty());
        assert!(!to.exists());
    }

    #[test]
    fn owner_layout_moves_both_and_drops_the_empty_video_folder() {
        for keep in [false, true] {
            let d = Dir::new(if keep { "owner-keep" } else { "owner" });
            let root = d.0.join("R");
            let v = d.0.join("V");
            d.put("R/a.m4a", "a");
            d.put("V/v.mp4", "v");
            if keep {
                d.put("V/keep.txt", "k");
            }
            let plan = SplitPlan {
                root: Some(root.clone()),
                moves: vec![
                    mv(&root, &root.join("Audio"), "m4a"),
                    mv(&v, &root.join("Video"), "mp4"),
                ],
            };
            let moved = apply(&plan);
            assert_eq!(moved.len(), 2);
            assert!(root.join("Audio").join("a.m4a").is_file());
            assert!(root.join("Video").join("v.mp4").is_file());
            assert_eq!(v.exists(), keep, "the video folder goes only when empty");
            assert!(root.is_dir(), "the audio folder is never removed");
        }
    }

    #[test]
    fn queue_paths_follow_the_move() {
        let text = concat!(
            r#"{"v":1,"jobs":[{"audio":"C:\\Rec\\A.m4a","model":"small","tries":2},"#,
            r#"{"audio":"C:\\Rec\\b.m4a","model":"base","tries":0}]}"#
        );
        let moved = vec![(PathBuf::from(r"c:\rec\a.m4a"), PathBuf::from(r"C:\Rec\Audio\a.m4a"))];
        let out = rewrite_queue(text, &moved).expect("one job matched");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["jobs"][0]["audio"], r"C:\Rec\Audio\a.m4a");
        assert_eq!(v["jobs"][0]["model"], "small");
        assert_eq!(v["jobs"][0]["tries"], 2);
        assert_eq!(v["jobs"][1]["audio"], r"C:\Rec\b.m4a");
        assert_eq!(v["jobs"][1]["model"], "base");
        let other = vec![(PathBuf::from(r"C:\Rec\z.m4a"), PathBuf::from(r"C:\Rec\Audio\z.m4a"))];
        assert_eq!(rewrite_queue(text, &other), None);
        assert_eq!(rewrite_queue("not json", &moved), None);
    }
}
