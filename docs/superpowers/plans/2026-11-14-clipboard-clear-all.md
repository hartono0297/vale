# Clipboard "Hapus semua" button Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A two-click "Hapus semua" button in the title row of the panel's Clipboard page that clears every unpinned entry.

**Architecture:** Copy the Notifikasi page's armed button: a `State` field with a deadline, a `Hit` and an `Effect`, a header button measured by the renderer and placed by the layout. The clearing itself is the code Settings' "Hapus riwayat" already runs, moved into one `App` method used by both.

**Tech Stack:** Rust 2024, Win32 + Direct2D, unit tests inside `src/**`.

**Spec:** `docs/superpowers/specs/2026-11-14-clipboard-clear-all-design.md`

## Global Constraints

- Pinned entries are never cleared; memory and `clipboard-history.bin` are both cleared (`write_clip_file_now`).
- First click arms ("Yakin?"), second within `CLEAN_CONFIRM` (4 s) clears; any other click, the deadline, a page change or the panel closing disarms.
- No new timer or thread: the deadline joins `State::next_deadline` / `expire_confirm`.
- Text: reuse `Key::NotifClearAll` and `Key::NotifSure` (Indonesian and English already exist). No new i18n key.
- The button shows only when history is on, the search field is empty and at least one entry is unpinned.
- Tests never touch the real clipboard or `%APPDATA%` ([[clipboard-privacy]]). Run `cargo test --locked`.
- Match existing style; touch nothing else (Settings behaviour, the lock clear, persistence stay as they are).

## Review Focus

- Armed, then the user types in the search: the button hides (no hit region), so no click can clear; it is back as "Yakin?" if the field is emptied before the deadline. Test: layout with no button has no `Hit::ClipClear`.
- A wide font or language: the button's width must cover "Yakin?" and "Hapus semua" so arming moves nothing, and the search keeps room. Test: layout stays valid for widths 60, 120 and 200.
- History on but only pinned entries: no button. Test on `clip_clearable`.
- A card selected and the list scrolled when the clear runs: selection and scroll reset (as Settings' clear does).
- History off: no button.

---

### Task 1: Logic: hit, action, armed state, the clear

**Files:**
- Modify: `src/model.rs` (`Hit` near `NotifClear` ~line 940; `ClipAction` ~line 1214)
- Modify: `src/state.rs` (field near `notif_armed` line 344 and its init line 512; disarm at ~1159; `next_deadline` ~1427; `expire_confirm` ~1464; `click` ~2014 and ~2662; accessor near line 1962)
- Modify: `src/app_clip.rs` (`perform_clip` ~625; `on_clip_setting` `ClipSetting::Clear` ~1106)

**Interfaces:**
- Produces: `Hit::ClipClear`; `ClipAction::ClearUnpinned`; `State::clip_armed(&self) -> bool`; `App::clip_clear_unpinned(&mut self)` in `app_clip.rs` (private to the module tree like its neighbours).

- [ ] **Step 1: Write the failing tests in `src/state.rs` tests module**, next to the `hapus_semua_*` tests (copy their shape with `st()`, `open_on(&mut s, Page::Clipboard, t0)`):
  - `clip_hapus_semua_arms_then_clears_on_a_second_click_inside_the_confirm_time`: first `Event::Click(Hit::ClipClear)` returns `vec![]`, `s.clip_armed()` true, `s.next_deadline() == Some(t0 + CLEAN_CONFIRM)`; second at `t0 + 1000 ms` returns `vec![Effect::Clip(ClipAction::ClearUnpinned)]` and disarms; armed and ticked past `CLEAN_CONFIRM` it disarms and the next click only arms.
  - `clip_hapus_semua_disarms_on_any_other_click_a_page_change_and_the_panel_closing`: arm; `Event::Click(Hit::ClipSearch)` returns `vec![Effect::FocusClipSearch]` and disarms; arm again; `Event::Wheel(-120)` changes page and disarms; return to the page, arm, `Event::Hover(false)` then a `Tick` after `ACTIVITY_LINGER + 1 ms` gives `View::Idle` and disarmed.
  - Extend `clipboard_hits_act_only_on_their_page`: add `Hit::ClipClear` to the list of hits that return `vec![]` on `Page::Notes`, and assert `!s.clip_armed()` after.

- [ ] **Step 2: Run** `cargo test --locked clip_hapus_semua` — expect a compile error (`ClipClear` not defined).

- [ ] **Step 3: Implement.**
  - `model.rs`: add `Hit::ClipClear` (doc: "Hapus semua" on the Clipboard page) and `ClipAction::ClearUnpinned` (doc: every entry not pinned).
  - `state.rs`: add `clip_armed: Option<Instant>` beside `notif_armed` (same doc style, initialised `None`); `pub fn clip_armed(&self) -> bool`. Disarm exactly where `notif_armed` is: page/panel check (`self.page != Page::Clipboard || !self.panel_open()`), `next_deadline` list, `expire_confirm`, and the "any click but its own" block (`!matches!(hit, Hit::ClipClear)`). In `click`, add `Hit::ClipClear` to the `Clip*` arm that does nothing off the page, and a `Hit::ClipClear` arm that mirrors `Hit::NotifClear` (arm with `now + CLEAN_CONFIRM`, or on a live second click push `Effect::Clip(ClipAction::ClearUnpinned)`). Update the `expire_confirm` doc comment to mention the Clipboard page.
  - `app_clip.rs`: extract the `if self.clip.history.clear_unpinned() { … }` block of `ClipSetting::Clear` (packing, selection None, scroll 0, `write_clip_file_now`, `clip_drag_forget_missing`) into `fn clip_clear_unpinned(&mut self)`; `ClipSetting::Clear` calls it then keeps its `redraw_clip(); return;`. In `perform_clip` add `ClipAction::ClearUnpinned => self.clip_clear_unpinned()` (the trailing `redraw_clip()` already runs). Let the compiler list any other exhaustive `Hit` / `ClipAction` matches and add the obvious arm (no region, no tip).

- [ ] **Step 4: Run** `cargo test --locked` — expect all pass (including the three state tests).

- [ ] **Step 5: Commit**

```bash
git add src/model.rs src/state.rs src/app_clip.rs
git commit -m "feat(clipboard): armed Hapus semua action and state"
```

### Task 2: The button: view, layout, regions, paint, demo

**Files:**
- Modify: `src/app_clip.rs` (`clip_view` ~225; add `clip_clearable`)
- Modify: `src/app.rs` (the `clip::clip_view(...)` call ~7536)
- Modify: `src/ui/render.rs` (`ClipView` ~852, `ClipView::empty` ~875)
- Modify: `src/ui/panel_layout.rs` (`ClipLayout` ~845, `clip_frame` ~2596, `clipboard_layout` ~2671, `regions` Clipboard arm ~4827, tests ~8990-9128, call at ~5346)
- Modify: `src/ui/render_panel.rs` (`Page::Clipboard` layout call ~835, `paint_clipboard` ~3228, a width helper next to `notif_head` ~1734)
- Modify: `src/ui/demo.rs` (`clip_view` ~3542 and the `ClipDemo` scene list near line 737)

**Interfaces:**
- Consumes: Task 1's `Hit::ClipClear`, `State::clip_armed()`.
- Produces:
  - `fn clip_clearable(enabled: bool, query: &str, items: &[Clip]) -> bool` in `app_clip.rs`.
  - `ClipView { clearable: bool, armed: bool, .. }` (both false in `empty`).
  - `ClipLayout.clear: Option<Rect>`.
  - `clipboard_layout(s, strip, ids, selected, scroll, clear_w: Option<f32>, image)`: `clear_w` is inserted before `image`; `None` = no button.
  - `const CLIP_CLEAR_GAP: f32 = 8.0` (button to search).

- [ ] **Step 1: Write the failing tests.**
  - `app_clip.rs` tests, `clip_clearable_needs_history_on_no_search_and_an_unpinned_entry`: build `History::new(50)`, `push(ClipKind::Text, ClipData::Text("a".into()), 1)`; assert true with `("", enabled)`, false with enabled false, false with query `"a"`; after `set_pinned(id, true)` (id from `items()[0].id`) false; empty history false.
  - `panel_layout.rs`: `clipboard_clear_button_sits_left_of_the_search_in_the_title_row`: with `Some(100.0)`: `l.clear` is `Some`, width 100, `clear.right() + CLIP_CLEAR_GAP == search.x`, vertical centre equals the search's, `header.right() + 12.0 <= clear.x`. For `clear_w` in `[60.0, 120.0, 200.0]`: `search.w > 100.0` and `header.w >= 107.9`. With `None`: `l.clear` is `None` and `l.search.right() == header_right(s)` and `search.w == CLIP_SEARCH_MAX_W` (unchanged from today).
  - Extend `clipboard_regions_and_empty_state`: with `Some(100.0)` `Hit::ClipClear` is in `regions`; without it, absent.
  - Update every existing `clipboard_layout(...)` test call: insert `None` before the image closure.

- [ ] **Step 2: Run** `cargo test --locked clipboard_ clip_clearable` — expect compile errors.

- [ ] **Step 3: Implement.**
  - `app_clip.rs`: `clip_clearable` as specified (history on, `query.is_empty()`, `items.iter().any(|c| !c.pinned)`); `clip_view` gets a new `armed: bool` parameter and fills `clearable: clip_clearable(enabled, query, clip.history.items())`, `armed`. `app.rs`: pass `state.clip_armed()`.
  - `render.rs`: add the two fields with doc comments; `empty` sets both false.
  - `panel_layout.rs`: `clip_frame` takes `clear_w: Option<f32>` and returns also `Option<Rect>` for the button: search width becomes `CLIP_SEARCH_MAX_W.min(hr - l - 120.0 - clear_w.map_or(0.0, |w| w + CLIP_CLEAR_GAP))`; the button is `rect(search.x - CLIP_CLEAR_GAP - w, search.y + (CLIP_SEARCH_H - DEV_HEAD_H) / 2.0, w, DEV_HEAD_H)`; `header` ends 12 px left of the button (or of the search when none). Thread the new argument through `clip_rows` callers (they pass `None`: the card area does not depend on it) and `clipboard_layout`. Add `clear` to `ClipLayout`. In `regions` Clipboard arm add `if let Some(r) = l.clear { add(r, Hit::ClipClear); }` right after the search.
  - `render_panel.rs`: a helper `clip_clear_w(&self) -> f32` computed like `NotifHead.clear_w` (max of `Key::NotifClearAll` and `Key::NotifSure` widths in `fonts.button`, plus `2 * DEV_BTN_PAD_X + DEV_BTN_ICON + DEV_BTN_ICON_GAP`); the `Page::Clipboard` layout call passes `clip.clearable.then(|| self.clip_clear_w())`. In `paint_clipboard`, after the search, paint `l.clear` like the Notifikasi button: armed (`clip.armed`) a `SOFT_RED` 0.22 pill with `Key::NotifSure` centred in `SOFT_RED`; otherwise `paint_labelled_button(r, Some(Icon::Trash), text(lang, Key::NotifClearAll), (p.raised, p.text), 1.0)`.
  - `demo.rs`: set `clearable: true` in `ClipDemo::Design` and `ClipDemo::Long`; add a scene `clipboard-armed` (same cards as Design, `clearable: true, armed: true`) following how `clipboard-long` is registered.

- [ ] **Step 4: Run** `cargo test --locked` — expect all pass; then `cargo build --release --locked` — expect success.

- [ ] **Step 5: Look at it.** Stop any installed copy (`Stop-Process -Name vale, dynamic-island`), run the demo scenes `clipboard` (Design), `clipboard-long` and `clipboard-armed` with `cargo run --release -- --demo` (see `src/main.rs` and `src/ui/demo.rs` for how a scene is chosen) and check by eye: title, button and search fit on one row at 560 px, the armed pill is the same size as the plain one, the search scene shows no button, the empty and off scenes show none. Repeat once with the JetBrains Mono font (`DI_FONT`, see [[ui-font-option]]) and with English.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat(clipboard): Hapus semua button in the panel's title row"
```

## After the tasks (project flow, not tasks)

Whole-branch review, merge locally into `main`, `cargo test --locked` on `main`, then the docs commit ("docs: update memory and README": a section in `docs/memory/clipboard-history.md`, the line in `docs/memory/index.md`, one README line if the clipboard features are listed there), push, and clean up this worktree per AGENTS.md.
