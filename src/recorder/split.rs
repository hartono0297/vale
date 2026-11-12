//! The one-time move of old recordings into the `Audio` and `Video`
//! subfolders, and the matching rewrite of the transcript queue.
//! Rename only: nothing is overwritten, copied or deleted.

use std::path::{Path, PathBuf};

use super::prefs::{Move, SplitPlan};

/// Moves every recording `<stem>.<ext>` directly in `m.from` into `m.to`, with
/// the files that share its stem (sidecar, transcript, cursor log). Returns the
/// `(old, new)` paths of the recordings moved. A name already taken in `m.to`
/// or a failed rename leaves that file where it is.
pub fn move_recordings(m: &Move) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(&m.from) else {
        return Vec::new();
    };
    let suffix = format!(".{}", m.ext).to_ascii_lowercase();
    let files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| Some((e.file_name().to_str()?.to_owned(), e.path())))
        .collect();
    let is_main = |name: &str| name.to_ascii_lowercase().ends_with(&suffix);
    let mut moved = Vec::new();
    for (name, path) in files.iter().filter(|(n, _)| is_main(n)) {
        let stem = &name[..name.len() - suffix.len()];
        let to = m.to.join(name);
        if !rename_new(path, &to, &m.to) {
            continue;
        }
        moved.push((path.clone(), to));
        let prefix = format!("{stem}.").to_ascii_lowercase();
        for (cname, cpath) in &files {
            if is_main(cname) || !cname.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }
            rename_new(cpath, &m.to.join(cname), &m.to);
        }
    }
    moved
}

/// Renames `from` to `to` inside `dir`, creating `dir` first. False when the
/// target is taken or anything fails; the error is printed.
fn rename_new(from: &Path, to: &Path, dir: &Path) -> bool {
    if to.exists() {
        eprintln!("vale: {} is taken, left in place", to.display());
        return false;
    }
    let result = std::fs::create_dir_all(dir).and_then(|()| std::fs::rename(from, to));
    if let Err(e) = &result {
        eprintln!("vale: could not move {}: {e}", from.display());
    }
    result.is_ok()
}

/// Runs every move, then removes the old video folder when it is empty.
pub fn apply(plan: &SplitPlan) -> Vec<(PathBuf, PathBuf)> {
    let mut moved = Vec::new();
    for m in &plan.moves {
        moved.extend(move_recordings(m));
        // Only a video folder that is not the root; `remove_dir` fails on a
        // folder that still holds anything.
        if m.ext == "mp4" && Some(m.from.as_path()) != m.to.parent() {
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
        Move { from: from.to_path_buf(), to: to.to_path_buf(), ext }
    }

    #[test]
    fn moves_a_recording_with_its_companions() {
        let d = Dir::new("companions");
        for n in ["a.m4a", "a.vale.json", "a.md", "a.srt", "a.transcript.md", "notes.txt", "b.m4a.part"] {
            d.put(n, n);
        }
        let to = d.0.join("Audio");
        let moved = move_recordings(&mv(&d.0, &to, "m4a"));
        assert_eq!(moved, vec![(d.0.join("a.m4a"), to.join("a.m4a"))]);
        for n in ["a.m4a", "a.vale.json", "a.md", "a.srt", "a.transcript.md"] {
            assert!(to.join(n).is_file(), "{n} moved");
            assert!(!d.0.join(n).exists(), "{n} left the root");
        }
        assert!(d.0.join("notes.txt").is_file() && d.0.join("b.m4a.part").is_file());
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
        let text = r#"{"v":1,"jobs":[{"audio":"C:\\Rec\\A.m4a","model":"small","tries":2},{"audio":"C:\\Rec\\b.m4a","model":"base","tries":0}]}"#;
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
