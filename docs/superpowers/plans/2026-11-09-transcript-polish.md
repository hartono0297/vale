---
tags: [plan, recording, transcript]
date: 2026-11-09
spec: owner approval of 2026-11-09 (quoted below)
---

# Transcript polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four small fixes to the transcript feature: a stale "Bikin transkrip" notice is dropped, deleting a model takes two clicks, a recording without a sidecar shows its real length, and a live "Saya" line never shows and then vanishes.

**Architecture:** Each fix stays in the layer that owns it: the notice queue rule in the pure reducer (`state.rs`), the two-click Hapus in the Settings window (same pattern and timer as "Hapus riwayat"), the length probe on the `rec-scan` thread (`app_record.rs` + `decode.rs`), and the hold-back as a watermark the runner sends with each progress word (`chunk.rs`, `runner.rs`, `echo.rs`) that the live view applies (`app_transcript_win.rs`).

**Tech Stack:** Rust 2024, `windows` 0.62 (Win32, Media Foundation via `crate::mf`), `chrono`, `serde_json`. No new crate, no new `windows` feature.

**Spec:** the owner's approval of 2026-11-09 below. It builds on `docs/superpowers/specs/2026-11-08-transcript-phase2-design.md` and `docs/superpowers/plans/2026-11-08-transcript-phase2.md`. Read the memory notes `docs/memory/transcript.md`, `notice-close.md`, `toast-while-busy.md`, `light-accent-contrast.md`, `bata-idle-power-budget.md` before a task.

> 1. Notice "Bikin transkrip" yang telat: kalau transkripnya udah selesai sebelum notice itu sempat tampil, notice-nya dibuang dari antrean. Jadi buat rekaman pendek kamu lihat "Rekaman tersimpan" lalu langsung "Transkrip siap". Rekaman panjang tetap seperti sekarang.
> 2. Hapus model tanpa konfirmasi: klik pertama ubah tombol jadi "Yakin?" (warna merah), klik kedua dalam 4 detik baru menghapus. Kalau tidak diklik lagi, balik jadi "Hapus". Tidak ada dialog terpisah.
> 3. "0 s" di file tanpa sidecar: durasinya dibaca dari file audionya sendiri (di thread yang sudah membaca daftar rekaman), jadi baris dan judul jendela transkrip menunjukkan panjang aslinya.
> 4. Baris "Saya" yang muncul lalu hilang: selama transkrip jalan, baris "Saya" ditahan sampai bagian "Peserta" di waktu yang sama sudah dibaca, baru ditampilkan (atau dibuang kalau ternyata gema). Tidak ada baris yang muncul lalu hilang.

**Branch:** `transcript-polish`, worktree `.claude/worktrees/transcript-polish`, made from `main` (653f06b) in Task 1 Step 0. This plan never merges into `main` and never pushes: that is the controller's step after the final whole-branch review (most capable model).

**Model tiers** (for the controller): *cheap* = mechanical, *standard* = integration, *most capable* = design-heavy. Each task names one.

## Global Constraints

- Windows 11 x64, Rust 2024, one `vale.exe`. No new crate, no new `windows` feature.
- Nothing at idle: no island timer, thread or poll is added. The only new timer use is the Settings window's existing `TIMER_CONFIRM` (id 1, `CONFIRM_MS = 4000`), armed only while "Yakin?" shows.
- Media Foundation is only loaded through `crate::mf` (`MfSession::start`), never on the island's UI thread ([[media-foundation-dynamic-load]]).
- Every UI string goes through `src/i18n.rs` with Indonesian and English; `no_key_leaks_the_other_language` and `no_formatted_text_leaks_the_other_language` stay green. No em dash or en dash in any UI string. This plan adds no new key: "Yakin?" / "Sure?" is the existing `Key::ConfirmShort`.
- Task 2 is UI work: it starts by loading `antislop:antislop`, `antislop:antislop-ui` and `antislop:antislop-human` with the Skill tool, checks every colour pair it draws with antislop-human's `check_contrast` in light, warm dark and Terminal dark (`theme::window_for`), puts the numbers in its report, and runs each loaded skill's checklist before its last commit.
- Tests open no window and spawn no console ([[test-spawns-no-window]]); test files go under `target/test-tmp/` (`store::tests::temp_store` or the module's `scratch`).
- Every task ends with `cargo test --locked` green and `cargo build --locked` with 0 warnings, then a commit on `transcript-polish`. Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Labels stay `output::LABELS_ID` "Saya"/"Peserta", `LABELS_EN` "Me"/"Others".

## Review Focus

1. **Two recordings back to back:** "Transkrip siap" of A must drop only A's waiting "Bikin transkrip", never B's, and a start notice already on screen stays (Task 1 `a_waiting_start_notice_goes_when_its_transcript_ends`).
2. **Hapus armed while the row changes under it** (a job starts on that model, so it is in use; the language changes; another row's Hapus is clicked): "Yakin?" never shows on a disabled button, the button never moves, and a click on another row arms that row instead of deleting (Task 2 `the_armed_delete_says_sure_in_red`, `transcript_clicks_map_to_actions`).
3. **A file Media Foundation cannot read or must download first** (a zero-byte or broken `.m4a`, a OneDrive cloud-only file): the scan does not hang, does not fetch the file, and the row keeps today's "0 s" (Task 3 `the_scan_probes_only_rows_without_a_length`, `a_cloud_only_file_is_not_probed`, `probe_of_a_missing_file_is_none`).
4. **The GPU fails mid-job and the job starts over on the CPU:** the hold starts again from zero, so no "Saya" line of the new run shows before its "Peserta" time is read (Task 4 `a_restart_holds_me_lines_again`).
5. **Others talks without a pause for a long time** (one 30 s chunk), or the last progress word is throttled away: a "Saya" line waits at most until that chunk is read, and at the end the lines come with the finished file, never shown then removed (Task 4 `progress_says_how_far_others_is_read`, `a_me_line_waits_until_others_is_read_at_that_time`).

---

### Task 1: Drop a waiting "Bikin transkrip" when its transcript ends (cheap)

**Files:**
- Modify: `src/model.rs` (`NoticeKind`), `src/state.rs` (the `Event::Notice` arm near line 637, tests near line 10280), `src/app_transcribe.rs:570-580` (`transcribe_start`), `src/ui/geometry.rs:1334` and its badge test near line 2930.

**Interfaces:**
- Produces: `NoticeKind::TranscriptStarting { audio: PathBuf }`: the "Bikin transkrip" notice of `audio`. Like `TranscriptInfo` (accent `Icon::Note` badge, a click dismisses, closable, no action), but it carries the audio so the reducer can find it.

- [ ] **Step 0: Worktree.** From the main checkout (never switch its branch, never stash or reset, leave the untracked `docs/design/` files alone): `git worktree add .claude/worktrees/transcript-polish -b transcript-polish main`. Work there for every task.

- [ ] **Step 1: Write the failing test** in `state.rs` tests:

```rust
#[test]
fn a_waiting_start_notice_goes_when_its_transcript_ends() {
    let t0 = Instant::now();
    let a = PathBuf::from(REC_FILE);
    let b = PathBuf::from(r"C:\r\b.m4a");
    let start = |p: &Path| rec_notice(NoticeKind::TranscriptStarting { audio: p.into() });
    let ready = |p: &Path| rec_notice(NoticeKind::TranscriptReady { audio: p.into() });
    let failed = |p: &Path| rec_notice(NoticeKind::TranscriptFailed { audio: p.into() });
    let later = t0 + NOTICE_DURATION + secs(1);
    // A short recording: "Rekaman tersimpan" shows, the start waits, the
    // transcript is ready first: the start never shows.
    for end in [ready(&a), failed(&a), ready(Path::new(r"c:\R\A.M4A"))] {
        let mut s = st();
        s.apply(Event::Notice(rec_saved(TranscriptOffer::None)), t0);
        s.apply(Event::Notice(start(&a)), t0 + secs(1));
        s.apply(Event::Notice(end.clone()), t0 + secs(2));
        s.apply(Event::Tick, later);
        assert_eq!(s.view(), View::Notice(end), "the start is gone");
    }
    // A long recording: the start already shows; it stays, the end waits.
    let mut s = st();
    s.apply(Event::Notice(start(&a)), t0);
    s.apply(Event::Notice(ready(&a)), t0 + secs(1));
    assert_eq!(s.view(), View::Notice(start(&a)));
    s.apply(Event::Tick, later);
    assert_eq!(s.view(), View::Notice(ready(&a)));
    // Another recording's start stays in the queue.
    let mut s = st();
    s.apply(Event::Notice(rec_saved(TranscriptOffer::None)), t0);
    s.apply(Event::Notice(start(&b)), t0 + secs(1));
    s.apply(Event::Notice(ready(&a)), t0 + secs(2));
    s.apply(Event::Tick, later);
    assert_eq!(s.view(), View::Notice(start(&b)));
    // It behaves as a plain word: closable, no action, a click dismisses.
    assert!(notice_closable(&start(&a)));
    assert!(!notice_has_action(&start(&a)));
}
```

Add `NoticeKind::TranscriptStarting { audio: r"C:\r\a.m4a".into() }` to the kinds of the geometry badge test (same badge as `TranscriptInfo`).

- [ ] **Step 2: Run it to verify it fails.** Run: `cargo test --locked a_waiting_start_notice_goes_when_its_transcript_ends`. Expected: compile error, no variant `TranscriptStarting`.

- [ ] **Step 3: Implement.** Add the variant in `model.rs` after `TranscriptInfo` with a doc comment saying the above. In `state.rs`, `TranscriptStarting { .. }` queues like `TranscriptInfo`; `TranscriptReady { audio }` and `TranscriptFailed { audio }` first remove from `self.queue` (never from `self.current`) every `TranscriptStarting` whose audio is `transcribe::queue::same_audio` with theirs, then queue as now. `geometry.rs`: the new kind joins the `TranscriptInfo | TranscriptSettings` badge arm. `transcribe_start` posts `NoticeKind::TranscriptStarting { audio: job.audio.clone() }` instead of `TranscriptInfo` (key, title and body unchanged). Any other exhaustive match the compiler names gets the `TranscriptInfo` behaviour.

- [ ] **Step 4: Run the tests.** Run: `cargo test --locked` then `cargo build --locked`. Expected: all pass, 0 warnings.

- [ ] **Step 5: Commit** `fix(transcript): drop a start notice whose transcript already ended`.

### Task 2: Two clicks to delete a model (standard; UI: load antislop first)

**Files:**
- Modify: `src/ui/settings.rs` (`TrClick`, `transcript_click`, the window struct near line 800, `on_click` near lines 2420 and 2512, the layout state near line 1710, `TIMER_CONFIRM` near line 3858, test `transcript_clicks_map_to_actions`), `src/ui/settings_layout.rs` (`State` near line 1454, `tr_cols` near line 4655, the model row near line 4945, tests near line 9325).

**Interfaces:**
- Produces: `TrClick::ArmDelete(usize)` (a model row index into `catalog::MODELS`); `fn transcript_click(view: &TranscriptView, hit: Hit, armed: Option<usize>) -> Option<TrClick>`; window field `transcript_confirm: Option<usize>` ("Hapus" of that row waits for its second click); `settings_layout::State::transcript_confirm: Option<usize>`.

- [ ] **Step 0:** Load `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-human` with the Skill tool.

- [ ] **Step 1: Write the failing tests.** In `transcript_clicks_map_to_actions`, every existing call gets `None` as the third argument, and the Hapus lines become:

```rust
// Hapus: the first click arms the row, the second deletes it.
assert_eq!(transcript_click(&on, Hit::TranscriptDelete(0), None), Some(TrClick::ArmDelete(0)));
assert_eq!(
    transcript_click(&on, Hit::TranscriptDelete(0), Some(0)),
    post(Action::TranscriptDelete("whisper-large-v3-turbo"))
);
// Armed on another row: this one is armed instead, nothing is deleted.
assert_eq!(transcript_click(&on, Hit::TranscriptDelete(1), Some(0)), Some(TrClick::ArmDelete(1)));
assert_eq!(transcript_click(&on, Hit::TranscriptDelete(7), None), None);
```

In `settings_layout.rs` tests:

```rust
#[test]
fn the_armed_delete_says_sure_in_red() {
    use Hit::TranscriptDelete;
    for lang in [Lang::Id, Lang::En] {
        let both = TranscriptView { installed: [true, true], engine_installed: true, ..tr_on() };
        let at = |view: TranscriptView, armed: Option<usize>| {
            let s = State { lang, transcript: view, transcript_confirm: armed, ..states()[0].clone() };
            layout(Section::Features, &s, &Estimate)
        };
        let calm = at(both.clone(), None);
        let armed = at(both.clone(), Some(0));
        let sure = tr_button(&armed, TranscriptDelete(0)).unwrap();
        assert_eq!(sure.label, text(lang, Key::ConfirmShort)); // "Yakin?" / "Sure?"
        assert_eq!(sure.kind, ButtonKind::Confirm);
        assert!(sure.enabled);
        // Same place and width as "Hapus": nothing moves.
        assert_eq!(sure.rect, tr_button(&calm, TranscriptDelete(0)).unwrap().rect);
        let other = tr_button(&armed, TranscriptDelete(1)).unwrap();
        assert_eq!((other.label.as_str(), other.kind), (text(lang, Key::TrDelete), ButtonKind::Secondary));
        // A model in use is never armed.
        let busy = at(TranscriptView { in_use: Some(0), ..both }, Some(0));
        let off = tr_button(&busy, TranscriptDelete(0)).unwrap();
        assert_eq!((off.label.as_str(), off.kind, off.enabled), (text(lang, Key::TrDelete), ButtonKind::Secondary, false));
    }
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test --locked -- transcript_clicks_map_to_actions the_armed_delete_says_sure_in_red`. Expected: compile errors (`ArmDelete`, the third argument, `transcript_confirm`).

- [ ] **Step 3: Implement.**
  - `transcript_click`: `TranscriptDelete(i)` posts the delete when `armed == Some(i)`, else `ArmDelete(i)` (`None` for an index outside `MODELS`).
  - `on_click`: like `clip_confirm`, any hit that is not `Hit::TranscriptDelete(_)` clears `transcript_confirm` and kills `TIMER_CONFIRM`. `ArmDelete(i)` sets `transcript_confirm = Some(i)` and `SetTimer(TIMER_CONFIRM, CONFIRM_MS)`; a posted `TranscriptDelete` sets it to `None` and kills the timer. The `TIMER_CONFIRM` arm also sets `transcript_confirm = None`. Pass `self.transcript_confirm` to `transcript_click` and into the layout state.
  - Layout: `tr_cols` measures `Key::ConfirmShort` with the other three labels. The installed row draws `(Hit::TranscriptDelete(i), Key::ConfirmShort, ButtonKind::Confirm, true)` when `s.transcript_confirm == Some(i) && !in_use`, else today's button.
  - No new i18n key: `ConfirmShort` is "Yakin?" in Indonesian and "Sure?" in English, already covered by `no_key_leaks_the_other_language`.

- [ ] **Step 4: Contrast.** With `check_contrast`, check the Confirm label (`p.error`) on the quiet fill over the card in light, warm dark and Terminal dark; report the three ratios (4.5:1 for 13 px text). If one fails, report it to the controller; do not change the palette in this task.

- [ ] **Step 5: Run the tests.** Run: `cargo test --locked`, then `cargo build --locked`. Expected: all pass, 0 warnings. Run the antislop checklists.

- [ ] **Step 6: Commit** `fix(transcript): Hapus asks "Yakin?" before deleting a model`.

### Task 3: The real length of a recording without a sidecar (standard)

**Files:**
- Modify: `src/transcribe/decode.rs` (new fn, tests), `src/app_record.rs` (`scan_recent`, `entry_of`, `rec_rescan`, tests), `src/app_transcript_win.rs` (`transcript_view`, tests).

**Interfaces:**
- Produces:
  - `decode::probe_duration_ms(path: &Path) -> Option<u64>`: starts an `MfSession`, opens a source reader and reads `MF_PD_DURATION` only (no media type set, nothing decoded). Share the duration read with `Decoder::open` through one private `fn duration_of(reader: &IMFSourceReader) -> Option<u64>`.
  - `scan_recent(dir, lang, today, limit, skip, probe: &dyn Fn(&Path) -> Option<u64>) -> Vec<RecEntry>`; `rec_rescan` passes `&decode::probe_duration_ms`, so the probe runs on the `rec-scan` thread only.
  - `app_record::may_probe(attributes: u32) -> bool`: false when `FILE_ATTRIBUTE_OFFLINE` (0x1000), `FILE_ATTRIBUTE_RECALL_ON_OPEN` (0x40000) or `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` (0x400000) is set (a cloud-only file is never fetched for its length). Attributes come from the `DirEntry` metadata the scan already reads (`std::os::windows::fs::MetadataExt::file_attributes`).
  - `app_transcript_win::known_length(sidecar_ms: Option<u64>, job_total_ms: Option<u64>, scanned: Option<Duration>) -> Option<u64>`: the first non-zero of the three.

- [ ] **Step 1: Write the failing tests.**

In `decode.rs` tests:

```rust
#[test]
fn probe_reads_the_length_of_a_recording() {
    let store = temp_store("decode-probe");
    let path = store.root().join("2026-11-09 0900 Probe.m4a");
    write_m4a(&path, &[sine(440.0, 48_000, 2.0, 0.4)]);
    let ms = probe_duration_ms(&path).unwrap();
    assert!((1990..=2100).contains(&ms), "{ms}");
}

#[test]
fn probe_of_a_missing_file_is_none() {
    let store = temp_store("decode-probe-missing");
    assert_eq!(probe_duration_ms(&store.root().join("nope.m4a")), None);
    let junk = store.root().join("junk.m4a");
    std::fs::write(&junk, b"x").unwrap();
    assert_eq!(probe_duration_ms(&junk), None);
}
```

In `app_record.rs` tests (existing calls of `scan_recent` pass `&|_| None`, and `the_recent_list_is_the_newest_five_with_sidecar_details` keeps its "no sidecar: the length is unknown" assertion with that probe):

```rust
#[test]
fn the_scan_probes_only_rows_without_a_length() {
    let dir = scratch("probe");
    for name in ["bare", "titled-only", "timed"] {
        fs::write(dir.join(format!("{name}.m4a")), b"x").unwrap();
    }
    fs::write(dir.join("cut.m4a.part"), b"x").unwrap();
    fs::write(dir.join("titled-only.vale.json"), r#"{"v":1,"title":"Rapat"}"#).unwrap();
    fs::write(dir.join("timed.vale.json"), r#"{"v":1,"duration_ms":65000}"#).unwrap();
    let asked = std::cell::RefCell::new(Vec::new());
    let probe = |p: &Path| {
        asked.borrow_mut().push(p.file_name().unwrap().to_string_lossy().into_owned());
        Some(42_000)
    };
    let list = scan_recent(&dir, Lang::Id, today(), 20, None, &probe);
    let len = |n: &str| list.iter().find(|e| e.path.file_name().unwrap().to_string_lossy() == n).unwrap().duration;
    assert_eq!(len("bare.m4a"), Duration::from_secs(42), "no sidecar: from the audio");
    assert_eq!(len("titled-only.m4a"), Duration::from_secs(42), "a sidecar without a length");
    assert_eq!(len("timed.m4a"), Duration::from_secs(65), "the sidecar's length wins");
    let mut asked = asked.into_inner();
    asked.sort();
    assert_eq!(asked, ["bare.m4a", "titled-only.m4a"], "never the sidecar's or the unfinished one");
    // A failed probe keeps the length unknown.
    let none = scan_recent(&dir, Lang::Id, today(), 20, None, &|_| None);
    assert!(none.iter().find(|e| e.path.ends_with("bare.m4a")).unwrap().duration.is_zero());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_cloud_only_file_is_not_probed() {
    assert!(may_probe(0x20)); // FILE_ATTRIBUTE_ARCHIVE
    assert!(may_probe(0x20 | 0x80000)); // pinned (always on this device)
    for cloud in [0x1000, 0x40000, 0x400000] {
        assert!(!may_probe(0x20 | cloud), "{cloud:#x}");
    }
}
```

In `app_transcript_win.rs` tests:

```rust
#[test]
fn the_window_takes_the_length_the_scan_or_the_job_found() {
    use std::time::Duration;
    let s = |n| Some(Duration::from_millis(n));
    assert_eq!(known_length(Some(5_000), Some(9_000), s(7_000)), Some(5_000), "the sidecar first");
    assert_eq!(known_length(None, Some(9_000), s(7_000)), Some(9_000), "then the running job");
    assert_eq!(known_length(Some(0), None, s(7_000)), Some(7_000), "a zero is unknown");
    assert_eq!(known_length(None, None, s(0)), None);
    assert_eq!(known_length(None, None, None), None);
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test --locked -- probe_ the_scan_probes_only a_cloud_only the_window_takes_the_length`. Expected: compile errors (missing fns, the new argument).

- [ ] **Step 3: Implement.** `entry_of` probes only when the row is not unfinished, the sidecar's `duration_ms` is 0 (no sidecar, or none in it) and `may_probe(attributes)`. `transcript_view` sets `doc.duration_ms = known_length(doc.duration_ms, <the worker's progress total>, <the duration of the `self.rec.recent` row with the same audio>)` for both the live and the loaded doc: no Media Foundation on the UI thread. No extra refresh: the window reads the last scan when it is built.

- [ ] **Step 4: Run the tests.** Run: `cargo test --locked`, then `cargo build --locked`. Expected: all pass, 0 warnings.

- [ ] **Step 5: Commit** `fix(rec): read the length of a recording without a sidecar from its audio`.

### Task 4: Hold a live "Saya" line until "Peserta" is read at that time (standard)

**The runner's real order (`runner::transcribe`):** each loop reads about one second of both channels, pushes it through both chunkers, recognizes every chunk that closed (sending its `Segment`s), then sends one `Progress`. A chunk closes after 600 ms of silence or at 30 s, so a "Saya" chunk can be sent while the "Peserta" chunk over the same time is still open. `echo::drop_echo` calls a "Saya" segment an echo using every "Peserta" segment with `o.start_ms <= me.end_ms + SLACK_MS` (1 000 ms) and `o.end_ms + SLACK_MS >= me.start_ms`.

**The rule:** every `Progress` carries `others_settled_ms`: the Others channel chunker's `settled_ms()` after that loop's segments were sent (no "Peserta" segment sent later starts before it), `u64::MAX` on the last `Progress` (after every chunker's `finish`) and always `u64::MAX` when no channel is Others. The live view shows a "Saya" segment only when `me.end_ms + SLACK_MS < others_settled_ms`; from then on no later "Peserta" segment can overlap it, so `drop_echo`'s verdict on it is final. "Peserta" and unlabelled segments are never held. The island starts each run (and a CPU restart after a failed GPU) at `others_settled_ms = 0`.

**Files:**
- Modify: `src/transcribe/chunk.rs`, `src/transcribe/runner.rs` (`Event::Progress`, `transcribe`, tests), `src/transcribe/echo.rs`, `src/transcribe/debug.rs:141`, `src/app_transcribe.rs` (`Worker`, `restart_live`, `on_transcribe`, the throttle in `transcribe_start`), `src/app_transcript_win.rs` (`live_lines`, `transcript_view`, tests).

**Interfaces:**
- Produces:
  - `Chunker::settled_ms(&self) -> u64`: no chunk this chunker emits later starts before it. The open chunk's first frame, else `(seen - before.len()) * FRAME_MS` (the leading pad can still join a chunk).
  - `runner::Event::Progress { done_ms: u64, total_ms: Option<u64>, others_settled_ms: u64 }`.
  - `echo::settled(me: &Segment, others_settled_ms: u64) -> bool`: `me.end_ms.saturating_add(SLACK_MS) < others_settled_ms`.
  - `Worker::others_settled_ms: u64`; `fn restart_live(segments: &mut Vec<Segment>, progress: &mut Option<(u64, Option<u64>)>, others_settled_ms: &mut u64)`.
  - `app_transcript_win::live_lines(segments: &[Segment], others_settled_ms: u64) -> Vec<Segment>`: `merge`, then `drop_echo`, then keep only "Saya" segments that are `echo::settled`.

- [ ] **Step 1: Write the failing tests.**

`chunk.rs`:

```rust
#[test]
fn settled_is_the_open_chunks_start_or_the_pad_before_now() {
    let mut c = Chunker::new(0, ChunkParams::default());
    let (quiet, loud) = (vec![0.0; FRAME], vec![0.5; FRAME]);
    let mut marks = Vec::new();
    let mut chunks = Vec::new();
    for i in 0..400u64 {
        let speech = (50..120).contains(&i) || (200..260).contains(&i);
        let f = if speech { &loud } else { &quiet };
        chunks.extend(c.push(f, if speech { 1.0 } else { 0.0 }).map(|ch| (i, ch)));
        marks.push((i, c.settled_ms()));
        if i == 49 {
            assert_eq!(c.settled_ms(), (50 - 6) * FRAME_MS, "the 192 ms pad may still join");
        }
    }
    assert!(marks.windows(2).all(|w| w[0].1 <= w[1].1), "never goes back");
    // No chunk emitted at frame i starts before a mark read before i.
    for (at, ch) in &chunks {
        for (i, m) in &marks {
            if i < at {
                assert!(ch.start_ms >= *m, "chunk at {} ms emitted at frame {at}, mark {m} at {i}", ch.start_ms);
            }
        }
    }
    assert_eq!(chunks.len(), 2);
}
```

`runner.rs` (fix the one `Event::Progress { done_ms, .. }` pattern already there):

```rust
#[test]
fn progress_says_how_far_others_is_read() {
    // Me talks 1-3 s; Others talks over it, 1.5-6.5 s, and its chunk closes later.
    let mut source = Memory {
        channels: vec![channel(10.0, &[(1.0, 3.0)]), channel(10.0, &[(1.5, 6.5)])],
        at: 0,
    };
    let mut events = Vec::new();
    run(&mut source, &mut Fake { timed: false, calls: 0 }, &AtomicBool::new(false), &mut events).unwrap();
    let mut settled = 0;
    let mut marks = Vec::new();
    for e in &events {
        match e {
            Event::Progress { others_settled_ms, .. } => {
                marks.push(*others_settled_ms);
                settled = *others_settled_ms;
            }
            Event::Segment(s) if s.speaker == Some(Speaker::Others) => {
                assert!(s.start_ms >= settled, "{s:?} came after the mark {settled}");
            }
            _ => {}
        }
    }
    assert!(marks.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(marks.last(), Some(&u64::MAX));
    // The Me line was sent before Others was read past it.
    let me_at = events.iter().position(|e| matches!(e, Event::Segment(s) if s.speaker == Some(Speaker::Me))).unwrap();
    let first_mark_after = events[me_at..].iter().find_map(|e| match e {
        Event::Progress { others_settled_ms, .. } => Some(*others_settled_ms),
        _ => None,
    });
    assert!(first_mark_after.unwrap() < 4_000, "it is held at first");
}

#[test]
fn one_channel_never_holds() {
    let mut source = Memory { channels: vec![channel(5.0, &[(1.0, 2.0)])], at: 0 };
    let mut events = Vec::new();
    run(&mut source, &mut Fake { timed: false, calls: 0 }, &AtomicBool::new(false), &mut events).unwrap();
    assert!(events.iter().all(|e| !matches!(e, Event::Progress { others_settled_ms, .. } if *others_settled_ms != u64::MAX)));
}
```

`echo.rs`:

```rust
#[test]
fn a_me_segment_is_settled_a_slack_after_its_end() {
    let me = Segment { start_ms: 1_000, end_ms: 3_000, speaker: Some(Speaker::Me), text: "halo".into() };
    assert!(!settled(&me, 4_000));
    assert!(settled(&me, 4_001));
    assert!(settled(&me, u64::MAX));
    assert!(!settled(&Segment { end_ms: u64::MAX - 10, ..me.clone() }, u64::MAX));
}
```

`app_transcript_win.rs` (`the_live_view_drops_echo` passes `u64::MAX`):

```rust
#[test]
fn a_me_line_waits_until_others_is_read_at_that_time() {
    let me = seg(1_000, 3_000, Me, "oke kita mulai agenda hari ini");
    let meeting = seg(900, 3_100, Others, "Oke, kita mulai. Agenda hari ini.");
    // Others is read only to 3.5 s: the line waits.
    assert!(live_lines(&[me.clone()], 3_500).is_empty());
    // Others is read past it and it is the echo: it never shows.
    assert_eq!(live_lines(&[me.clone(), meeting.clone()], 4_001), [meeting.clone()]);
    // A real reply shows once its time is read.
    let reply = seg(5_000, 6_000, Me, "Boleh, deadline Jumat ya");
    assert!(live_lines(&[reply.clone()], 7_000).is_empty());
    assert_eq!(live_lines(&[reply.clone()], 7_001), [reply]);
    // Others and unlabelled lines are never held.
    let plain = Segment { speaker: None, ..meeting.clone() };
    assert_eq!(live_lines(&[meeting.clone()], 0), [meeting]);
    assert_eq!(live_lines(&[plain.clone()], 0), [plain]);
}
```

`app_transcribe.rs`:

```rust
#[test]
fn a_restart_holds_me_lines_again() {
    let mut segments = vec![Segment { start_ms: 0, end_ms: 1, speaker: None, text: "x".into() }];
    let (mut progress, mut settled) = (Some((5_000, Some(9_000))), 6_000);
    restart_live(&mut segments, &mut progress, &mut settled);
    assert!(segments.is_empty());
    assert_eq!((progress, settled), (None, 0));
}
```

- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test --locked -- settled progress_says one_channel_never a_me_line_waits a_restart_holds`. Expected: compile errors (no `settled_ms`, no field `others_settled_ms`, wrong arity).

- [ ] **Step 3: Implement** to the rule above. `transcribe` takes the Others channel from `speakers` (`Some(Speaker::Others)`); `transcript_view` passes `w.others_settled_ms`; `on_transcribe` stores the field on every `Progress`. The island's throttle stays as it is: a dropped `Progress` only delays a line, and the finished file replaces the live lines anyway.

- [ ] **Step 4: Run the tests.** Run: `cargo test --locked`, then `cargo build --locked`. Expected: all pass, 0 warnings.

- [ ] **Step 5: Commit** `fix(transcript): hold a live "Saya" line until "Peserta" is read at that time`.

## Live checks for the controller (owner's PC, after the final review, before merging)

1. A 10 s recording with Transkrip on and automatic: "Rekaman tersimpan", then "Transkrip siap", no "Bikin transkrip". A 5 min one: all three, as before.
2. Settings → Fitur → Transkrip, an installed model not in use: Hapus turns into a red "Yakin?"; after 4 s it is "Hapus" again; two clicks within 4 s delete it; in English it says "Sure?".
3. An `.m4a` copied into the recordings folder without a `.vale.json`: its Rekam row and its transcript window header show its real length.
4. A two-source meeting with the speakers on, window open while it runs: no "Saya" line appears and then disappears.

## Decisions this plan made where the approval left room

- **Item 1:** "selesai" covers a failed job too: "Transkrip gagal" also drops a waiting "Bikin transkrip" of the same recording. The notice gets its own kind (`TranscriptStarting { audio }`) so the reducer matches by audio, not by text.
- **Item 2:** reuses `Key::ConfirmShort` ("Yakin?" / "Sure?"), `ButtonKind::Confirm` (error red text on the quiet fill, as "Putuskan" and "Hapus riwayat") and the Settings window's `TIMER_CONFIRM` / `CONFIRM_MS = 4000`; no island timer. Any other click in Settings disarms it, as the other confirms do. A model in use is never armed.
- **Item 3:** probed only for finished rows with no sidecar length, and not for cloud-only files (so the scan never downloads a OneDrive file). A failed probe keeps "0 s". The window fills a missing length from the running job's total, then from the last scan's row; a picked file outside the recordings folder that is not running still gets its length from the player, as today.
- **Item 4:** the hold rides on the `Progress` word (no new event), and lasts at most until the Others chunk over that time closes (30 s at most). Lines still held when the job ends come with the finished file.
