---
tags: [decision, screenshot, capture, editor, uia, power, privacy]
date: 2026-11-02
---

# Vale's own screenshot: snap capture and editor

The Pintasan Screenshot tile runs Vale's own capture (`src/capture/`) and then
the editor (`src/shot_edit/`) unless Settings → Clipboard → *Alat screenshot*
is *Snipping Tool* (`ShotTool::Windows`, then `ms-screenclip:` as before). The
choice is made in `App::queue_shortcut`; `Action::Capture` waits `ANIM_MS + 60`
like the screen clip, so the collapsing panel is not in the picture. Config
`screenshot.tool` reads leniently: missing, a bad name or any non-string is
Vale.

- **Frozen frame.** `win::grab_virtual_screen` (GetDC + BitBlt
  `SRCCOPY|CAPTUREBLT` + GetDIBits, 32 bpp top-down) in physical px. It lives
  in memory only and is freed when the overlay closes; the crop is freed when
  the editor closes. Nothing touches disk without Save; the clipboard gets the
  picture on Copy and by itself ("Salin otomatis", see "Auto-copy" below).
- **Display affinity.** Only the island is kept out of the frame: visible
  windows of our process whose class is in `win::KEPT_OUT` (`ValeWindow`; the
  halo margin and "Di belakang jendela" are the same window, not helpers of
  their own) get `WDA_EXCLUDEFROMCAPTURE` just for the BitBlt and their
  previous affinity back right after (`DwmFlush` in between). A window that
  refuses is hidden and shown again. Settings, the calendar/notes editor and
  a screenshot editor stay in the picture, so they can be captured (a bug
  report). Windows of our process are still never snap targets.
- **Snapping** (`capture::pick`, pure): the frontmost top-level window under
  the pointer (lowest z, depth 0), then the smallest candidate of that window
  that holds the pointer, skipping boxes under 12 × 12. Elements of windows
  behind it never win. Click = press and release within 4 px (`is_click`);
  further is a drag (`drag_rect`, clamped to the virtual screen).
- **UI Automation budget** (`capture::uia`). One short-lived worker (COM MTA),
  never waited for: the overlay snaps to windows at once, finer boxes join
  when a window's result arrives. Frontmost 6 windows, 400 usable boxes and
  800 elements looked at per window, 600 ms in all. Each window is walked
  breadth first, one `FindAllBuildCache(TreeScope_Children)` per opened
  element (only BoundingRectangle cached, off-screen filtered out, boxes under
  12 px clipped to the window are neither kept nor opened). Not
  `TreeScope_Descendants`: that fetches the whole subtree in one go, so the
  caps only cut the list afterwards and the pre-order cut-off dropped the
  right and bottom panes of a big window. Before every call (`uia::Clock`)
  the budget and the stop flag are checked and `IUIAutomation2`'s connection
  and transaction timeouts (`CUIAutomation8`) are set to what is left, so the
  600 ms holds across all windows, not only between them. A window's result
  is dropped, not posted, when the stop flag went up meanwhile. No timer, no
  thread at idle.
- **UIA results by ticket.** `WM_APP_UIA` carries the capture number in
  `wparam` and a ticket in `lparam`; the boxes wait in a static `INBOX`
  (`deposit`, `withdraw`, `take_result`). Not a boxed pointer: a stray or
  forged `WM_APP_UIA` (another process can post one) would make the overlay
  free whatever its `lparam` points at, while an unknown ticket just finds
  nothing. A post that fails withdraws its own ticket; the overlay takes
  every ticket it receives, even for a stale capture, and drains the queue
  at `WM_DESTROY` (only a post racing the destroy itself can stay behind).
- **Windows list.** `EnumWindows` in z-order; skipped: invisible, minimised,
  cloaked, zero-size, `WS_EX_TRANSPARENT`, Progman/WorkerW (the desktop would
  swallow every click), own process. Rect from `DWMWA_EXTENDED_FRAME_BOUNDS`.
- **Overlay.** One `WS_POPUP` topmost over the virtual screen; Direct2D at
  96 dpi (a unit is a pixel); redraws only when the lit rect or the monitor
  under the pointer changes. One at a time (a second trigger is ignored).
  Display change cancels. If it cannot take the foreground (plain
  `SetForegroundWindow`, then a brief `AttachThreadInput` to the foreground
  thread) it cancels, so Esc/Enter never reach the app underneath. Once it
  has the foreground (`armed`), losing it (`WM_ACTIVATE` `WA_INACTIVE`,
  `WM_ACTIVATEAPP` false: Alt+Tab, Win, another app) cancels too; the frozen
  screen would otherwise stay up while the keys went elsewhere. Not armed
  before that, because the input join can move the activation around.
- **Overlay close and destroy.** The state leaves its slot in one place
  (`take_owned`), so `finish` is idempotent: the `WM_ACTIVATE` that its own
  `DestroyWindow` sends, or an Alt+F4 after a click, finds nothing.
  `WM_CLOSE` is handled as `finish(None)` (no DefWindowProc, which would
  destroy a second time); `finish` calls `DestroyWindow` only while
  `IsWindow`. `WM_DESTROY` takes and drops the state still stored for its
  own hwnd (a close from outside), so `active()` goes false and the frame
  (over 100 MB on a big desktop) is freed. `STARTING` is cleared by a drop
  guard on every way out of `start`.
- **Island accent.** The highlight and the size label use the island's
  accent (`theme::current().accent.rgb()`), not `current_window()`'s, whose
  light-mode shade is darker for text on a light window.
- **Draw errors** (overlay and editor): only `D2DERR_RECREATE_TARGET` is
  retried, at most 3 in a row; anything else stops (overlay cancels; the
  editor shows the canvas without a picture too big for the device and says
  so in the footer) instead of repainting in a loop.
- **Editor tools.** Eight, in this order: Select, Text, Rectangle, **Line**
  (key L, an arrow without its head: `Annotation::Line`, hit within
  `tol + width / 2`, two handles like the arrow's), Arrow, Pen, Marker,
  Pixelate (`Tool::ALL`, the toolbar test checks the first eight items).
- **Editor.** Annotations in image px (`shot_edit::Session`: whole-document
  snapshots for undo/redo, `dirty` = anything changed since the last save or
  copy). `paint::draw_annotations` is used by the canvas and the export alike
  (D2D on a WIC bitmap), so a saved file equals what was seen. Text is a
  native EDIT (IME, paste); its subclass posts commit/cancel to the window so
  the EDIT is never destroyed inside its own proc. The post carries the
  EDIT's HWND (see the stale-message note below). Closing while dirty shows an
  inline bar, never a system dialog: Enter = Discard, Esc = Keep, every other
  key is ignored while it is up. The X button and Alt+F4 go through
  `WM_CLOSE` -> `request_close` too (no `WM_SYSCOMMAND` handler). Paint order
  matters here: [[shot-editor-paint-order]].
- **Text click rule** (`text_click_action`, pure). A press on the canvas with
  the Text tool while an EDIT is open only commits it: no new EDIT, and the
  focus goes back to the editor window, so Ctrl+C, Ctrl+S, Delete, Esc and
  V/T/R reach the editor and not a fresh empty EDIT. With no EDIT open a
  click places one. The decision uses what `commit_text` returned (true when
  an EDIT was open), not the tool alone.
- **Stale EDIT message.** `WM_APP_TEXT` carries the posting EDIT's HWND in
  `lparam`; `text_msg_is_current` accepts it only when that is the EDIT open
  now. The subclass is removed before `DestroyWindow` and the post is
  ignored on a mismatch, so the WM_KILLFOCUS of a dying EDIT never closes
  the next text box.
- **Save** runs on a worker (inbox + a doorbell message, so a closed window
  leaks nothing). It writes a snapshot; `Session::revision` is taken with it
  and `mark_clean_at` only cleans when nothing changed since. PNG encoding and `save_name` are the preview's own
  (`screenshot::win::encode_png`, `write_at`; create-new, never overwrites). A
  blocked write says "mungkin diblokir Controlled folder access" (see the
  gotcha in [[screenshot-preview]]).
- **Own-copy skip.** Every editor write (Copy, an auto-copy, the flush at
  close) goes through the clipboard module and is recorded in a registry of
  the last 32 writes (`shot_edit::record_own_copy(seq, capture, managed)`,
  `own_copy(seq)`; 0 never matches). `App::shot_from_image` returns early for
  any recorded sequence, so the notch never previews Vale's own copy. What the
  island does with the write itself depends on `managed`: see Auto-copy.
- **Demos** (debug builds, in `opens_windows`): `--demo-snap` (a desktop drawn
  in code, fixed candidates, no grab, no EnumWindows, no UIA) and
  `--demo-shot-editor [empty]`. In demos Copy is inert (the footer says so),
  auto-copy is inert too (`enabled` false: no worker, no timer, no clipboard)
  and Save writes to `target/demo-shots/`; the process ends when the window
  closes. Screenshots of them: `PrintWindow` with flag 2 on the demo's main
  window. `--demo-snap inline` is the inline mode, see "Inline mode".

## Inline mode (2026-11-02, "Setelah capture" = "Langsung di layar")

The owner's ask: a Lightshot-style capture. After the pick the overlay does not
open the editor: the frozen screen stays, the region is drawn on in place, then
Copy, Save or Open in editor. Pure rules: `src/capture/inline.rs` (tested); the
window glue: `capture/overlay.rs` (Pick phase, `finish`, `finish_inline`) and
`capture/overlay/edit.rs` (the drawing phase). Editor mode is untouched.

- **Setting.** Settings → Clipboard → Screenshot → *Setelah capture*
  (`screenshot.after_capture`, `config::AfterCapture`: `Editor` default,
  `Inline`; read case-insensitively, a missing value, an unknown name, a number
  or null reads as Editor and the rest of the file stays). Row 2 of 6, a
  segmented control, never faded ([[settings-structure]]). `App` stores it with
  `capture::set_after_capture` in `clip_start` and `clip_follow_config`
  (every Settings change goes `on_shot_setting` -> `save_config` ->
  `adopt_config` -> `clip_follow_config`, like auto-copy). The overlay latches
  it and "Salin otomatis" in `overlay::start` / `start_demo`, so a change
  meanwhile applies to the next capture. Applies to the tile and to PrtSc
  (both start `overlay::start`); the Snipping Tool path (`queue_shortcut`) is
  unchanged.
- **Phases.** `Phase::Pick` is today's behaviour (snap by click, drag a
  rectangle, Enter takes the highlight, Esc and a right click cancel). In
  inline mode a pick calls `edit::enter` instead of `finish(Some(r))`: the
  overlay stays, the pick hint pill goes, the UIA answers that come later are
  ignored (still taken, so their boxes are freed). `Phase::Edit` holds the
  selection (virtual-screen px), a `Session` (undo, redo, tool, colour, stroke;
  Select is the tool), the drag, the open text, hover and pressed button, the
  confirm flag and the mosaic cache.
- **Frame px.** `Frozen` is now an origin plus an `Arc<Shot>`: the same pixels
  are the painter's `&Shot` (no 100+ MB copy) and the source of the crop.
  Annotations are in frame px: (0, 0) is the frame's top-left, also the
  overlay's client (0, 0); the view is `View { scale: 1, ox: 0, oy: 0 }`. The
  pointer is virtual px (the frame's origin added); `Env::local` turns it into
  a frame point. Because the annotations are in frame px, moving the selection
  moves only the crop, never what is drawn on screen.
- **Drawing.** The frame, dimmed outside the selection; the annotations inside
  `PushAxisAlignedClip(selection)` with `draw_annotations_clipped(.., clip =
  selection)`; the selected annotation's handles (`paint::draw_selection`);
  the selection's border (a 1 px black line, a white dashed one on top,
  `Gfx::dash`); a size label in a dark pill above the top-left (just inside
  when there is no room above on the monitor); eight handle squares; the tool
  strip and the action bar; a tooltip; the demo's notice. The order is
  `inline::PAINT_ORDER` (tested): a panel's background before its buttons, the
  same rule as [[shot-editor-paint-order]]. Redraws only when something
  changed; no timer, no worker at idle. Strip: Select, Pen, Line, Arrow, Rect,
  Marker, Text, Pixelate | six colours | three strokes | Undo, Redo (icons from
  `shot_edit::icons`, the editor's own). Bar: Salin, Simpan, Buka di editor,
  Tutup; the confirm bar ("Buang perubahan?", Buang, Batal) takes its place.
  Sizes are dip times `ui`, `ui` = the DPI of the monitor holding the
  selection's centre over 96 (`win::monitor_at`), measured again when the
  selection moves to another monitor.
- **Placement** (`inline`, pure, tested). `place_side(sel, size, monitor, side,
  gap)`: the strip goes Right (from the selection's top, else Left, else
  inside the right edge), the bar Below (right edge on the selection's, else
  Above, else inside the bottom edge); always clamped fully onto the monitor
  of the selection, so a selection on a second monitor with negative
  coordinates keeps its panels there. `separate` moves the bar clear of the
  strip when they would overlap (a narrow selection near the edge).
  `strip_layout(ui, max_h)` makes two (three, ...) columns when one is taller
  than the monitor allows. `label_rect`, `tooltip_rect` likewise.
- **Selection maths** (pure). Eight `SelHandle`s (`handle_at`, corners win a
  tie); `resize_sel(start, handle, to, bounds)` moves the grabbed edges of the
  rectangle **as it was at the press**, flips over the opposite edge, is at
  least 1 × 1 and stays in the frame; `move_sel` keeps the size.
  `select_press` orders what a Select press grabs: the selected annotation's
  handle, the topmost annotation under the pointer (only inside the selection,
  `Session::hit` with 6 × `ui` px), a selection handle (7 × `ui`), inside:
  move the selection, outside: a new selection while nothing is drawn (else
  nothing).
- **Mouse.** A press on a button is that button and acts on release over the
  same one, like the editor; while the confirm bar is up only Buang and Batal
  answer, and a press on a panel's background does nothing. Drawing tools
  (Pen, Line, Arrow, Rect, Marker, Pixelate) start only on a press inside the
  selection; the stroke may leave it and is clipped. Same `Session::begin` /
  `end` and degenerate-drop rules as the editor's window. Text: the shared
  EDIT (see below), placed inside the selection, a double click on a text with
  Select edits it. Cursors: arrow over the panels, size cursors over handles,
  move over the selection, an I beam for Text, a cross to draw.
  `WM_CAPTURECHANGED` mid-drag ends the drag as a release. The overlay's class
  has `CS_DBLCLKS` and `WS_CLIPCHILDREN`.
- **Keys** (`inline_key`, pure). A text EDIT or a drag running takes every key.
  The confirm bar: Enter = Discard (close, copy nothing), Esc = Keep, the rest
  ignored. Otherwise Enter or Ctrl+C copy and close, Ctrl+S save and close,
  Esc asks (the confirm bar) when something is drawn else cancels, Ctrl+Z,
  Ctrl+Y / Ctrl+Shift+Z, Delete, and V T R L A P M B pick a tool. "Tutup" and a
  right click act as Esc (first the text being typed, then the confirm bar's
  Keep). Assumption: `WM_CLOSE` / Alt+F4 in the drawing phase is Esc too (the
  bar when something is drawn); in the Pick phase it cancels as before. Losing
  the foreground, `WM_DISPLAYCHANGE` and a failed draw cancel at once and copy
  nothing.
- **Finishing** (`finish_inline`, `inline::finish_copies`). Copy always copies,
  Save only with "Salin otomatis" (latched), Cancel / Close / Discard /
  OpenEditor never. "Salin otomatis" never copies at the pick or on a debounce
  here. The open text is committed first, then the state leaves its slot
  (idempotent like `finish`), the selection is cropped from the frame, a clone
  of the document is moved by -(selection - frame origin) (`doc_in_crop`;
  annotations partly outside stay as they are) and the frame is freed. Copy:
  `export::copy(overlay, crop, doc, next_capture_id(), true)` on the UI thread
  before `DestroyWindow`; it records the write as **managed**, so the island
  shows no notch preview and no copy notice and keeps one history card per
  capture ([[clipboard-notice]]). Save (after that copy when auto-copy is
  latched): `export::save_detached(crop, doc, dir)` on its own worker (the
  same render, `write_new` naming and Screenshots folder as the editor's
  Simpan, never over a file). OpenEditor: `shot_edit::window::open_with(crop,
  selection centre, demo, doc)`; the editor then follows its own rules,
  auto-copy included.
- **Mosaic = export.** `paint::pixel_region(r, w, h, clip)`: without a clip a
  Pixelate is `r` cut to the picture with `pixel_block(w, h)` (editor and
  export); with a clip (the selection) it is `r` cut to the clip, then to the
  picture, with `pixel_block(clip.w, clip.h)`. The cells are aligned to the
  region's corner, so what the overlay shows (frame base, clip = selection) is
  pixel for pixel the export of the crop (crop base, no clip); a test compares
  the two `pixelate()` outputs. The `Pix` cache key now includes the block.
- **Shared pieces** (no behaviour change for the editor). `shot_edit/text.rs`:
  `TextEdit`, `create` (EDIT, font, brush, margins, subclass, caret at the
  end), `destroy` (unsubclass before `DestroyWindow`), `fit`, `contrast_back`,
  `edit_proc` (posts `WM_APP_TEXT` = `WM_APP + 50` to its parent with the EDIT in
  `lparam`: 1 commit, 2 cancel), `TextClick`, `text_click_action`,
  `text_msg_is_current` (the stale-message rule above, in both windows).
  `shot_edit/icons.rs`: `Glyph` and `glyph` (taking `ui::geometry::Rect`), so
  the strip draws the editor's icons. `export::save_detached` shares the save
  worker's body (`save_blocking`) with `spawn_save`.
- **Lifetime.** One overlay at a time; armed / deactivation cancel; display
  change cancels; the frame is freed on close; `WM_DESTROY` takes the state, and
  `Edit`'s `Drop` destroys an open EDIT and frees its GDI objects; a lost D2D
  target is retried up to 3 times (the mosaic cache cleared too); no timers.
- **Demo.** `--demo-snap inline` (debug builds): the same made-up desktop, the
  pick keeps the overlay. Copy / Enter touch no clipboard: the top pill says
  `EditDemoNoClipboard` and the overlay stays. Save writes to
  `target/demo-shots/` on a worker whose `JoinHandle` waits in a thread-local;
  `run_snap` joins it after the message loop. Buka di editor opens the editor
  demo. A plain `--demo-snap` is as before. Driven in this session by posting
  window messages to the demo's window and reading it with `PrintWindow` (flag
  2): pick, rectangle / arrow / pen / line / mosaic, the tooltip, the confirm
  bar, resize and move of the selection, an open text EDIT, Copy's notice,
  Save (the file matched the selection with the mosaic), Buka di editor.
- **Limits and not verified.** A Save that fails after the overlay is gone is
  only `eprintln!`-ed (no window is left to say it). Not verified live: the
  real clipboard (managed own-copy, history card, no notch preview), a text
  typed through an IME inside the overlay, several monitors with different DPI,
  and whether an IME or a system popup deactivates the overlay (that would
  cancel).

## Auto-copy (2026-11-02, "Salin otomatis")

The owner's ask: a screenshot taken with Vale's capture is already on the
clipboard, without pressing Copy in the editor. Settings → Clipboard →
Screenshot → *Salin otomatis* (`screenshot.auto_copy`, default on, a missing
or non-bool value reads as on). Vale's own capture only; Snipping Tool copies
by itself. Pure rules: `src/shot_edit/auto.rs` (`AutoCopy<P>`, tested);
window: `shot_edit/window.rs`; export: `shot_edit/export.rs`.

- **Latched.** An editor reads the setting once when it opens
  (`shot_edit::auto_copy_setting`, an `AtomicBool` that `App` sets in
  `clip_start` and `clip_follow_config`); a demo is always off. Off: the
  editor behaves as before (no worker, no timer, Copy as today).
- **Copy at pick.** The window shows at once; a worker for revision 0 starts
  right after. Worker = a named thread `shot-edit-copy` (COM MTA) that runs
  `export::prepare` (render + PNG + DIB), pushes `CopyReady { rev, prepared }`
  into an `Arc<Mutex<Vec<..>>>` inbox and rings `WM_APP_COPIED` (`WM_APP + 52`)
  on the editor, like Save's inbox + doorbell (a closed window leaks nothing
  beyond the Arc). The UI thread then writes with `export::put`, never inside
  a `with(...)` borrow (`EmptyClipboard` can send WM_DESTROYCLIPBOARD to
  another editor on this thread).
- **Debounce.** After each finished change (add, move, resize, delete,
  restyle, committed text, undo, redo) `note_change` sees a new
  `Session::revision` and (re)arms one one-shot `ID_AUTO_DEBOUNCE` (300 ms);
  it is killed when it fires, never repeating. Typing in an open EDIT does not
  change the revision. When the timer fires: an EDIT open or a drag running
  re-arms it (never copies mid-typing or mid-drag); a worker in flight sets
  `again`; the revision already copied does nothing; else a worker starts. A
  result for a revision that is no longer current is dropped and the current
  one rendered (`on_result`: Write / Drop / Restart).
- **Rule 3, the guard.** The first write of a capture is unconditional.
  Later writes need `GetClipboardSequenceNumber() == last_seq` (what this
  editor's last write left): else somebody else copied since, auto-copy stops
  silently (`stopped`) and the session stays dirty. `clipboard::win::write_when`
  re-checks the sequence *with the clipboard open*, before `EmptyClipboard`
  (like `empty_if_unchanged`), returning Written(seq) / NotOurs / Busy /
  Emptied(seq). Emptied: `EmptyClipboard` ran but no format could be set
  (memory); the sequence moved by the editor's own hand, so `on_emptied`
  records it as `last_seq` before the retry, else the retry would read it
  as somebody else's copy and stop silently with the clipboard empty. A
  manual Copy (`on_manual_copy`) takes the clipboard back, clears `stopped`
  and drops any pending retry or debounce. Latest capture wins: a second
  editor's first write makes the first one stop.
- **Busy clipboard.** `write_when` opens with 4 tries 8 ms apart; Busy keeps
  the prepared formats and sets a one-shot `ID_AUTO_RETRY` (150 ms), 3
  retries (`retry_step`), re-checking staleness and rule 3 each time. Then the
  footer says `EditAutoCopyFailed` ("Gagal menyalin · tekan Salin" / "Couldn't
  copy · press Copy"), Tone::Error. The next change tries again.
- **Footer and dirty.** A landed write says "Disalin ke clipboard" and calls
  `Session::mark_clean_at(rev)` (cleans only at the copied revision, never a
  newer one). So the picture on the clipboard counts as kept.
- **Flush on close.** `request_close` (Esc, the X, Alt+F4, WM_CLOSE) first
  calls `flush_on_close`: when `needs_flush(rev)` (debounce due, a worker in
  flight, a retry pending, or the current revision not yet copied) it kills
  the timers, renders and writes the current revision synchronously on the UI
  thread (like Copy), with the same guard, then `mark_clean_at`. The discard
  bar therefore shows only when the picture is neither on the clipboard nor
  saved. `Hit::Discard` never flushes; a late worker result for a destroyed
  window is never read. A drag still running at close is ended as a release.
- **Timers and the model move together.** `kill_auto_timers` also calls
  `AutoCopy::timers_cancelled` (no debounce due, no retry waiting). Without
  it a retry whose timer the close flush killed stayed in `waiting`, and the
  next debounce for that revision did nothing (`on_timer`), so the revision
  never reached the clipboard. A busy flush sets `due` again (`on_change`)
  and arms the debounce. Format bytes are borrowed (`write`/`write_when` take
  `&[(u32, B: AsRef<[u8]>)]`), never cloned out of `Prepared`.
- **Registry and the island.** Each editor takes `next_capture_id()`. Its
  writes are recorded with `managed` = the latched flag. The island's
  `read_clipboard` looks `own_copy(seq)` up right after the snapshot: a
  `managed` write gives one card per capture (`store::ingest_replacing`, see
  [[clipboard-history]]) and no ClipPreview notice, no "Disalin", no
  screenshot preview ([[clipboard-notice]]); with the history off it does
  nothing. An editor that opened with auto-copy off is `managed: false`: the
  old path.
- **Windows' own Win+V history is not ours.** It records every clipboard
  write, so each auto re-copy (debounced, finished edits only) can add a
  Win+V entry. Vale cannot replace Windows' entries; only Vale's Clipboard
  page is one card per capture. A known limitation, decided with the owner.
- **Privacy / tests.** No test and no demo reads, writes or empties the real
  clipboard, registers a listener or grabs the screen ([[clipboard-privacy]]):
  `write_when` / `put` are not unit-tested; the model, the registry, the
  history helpers, the config and the Settings card are.
- **Not verified live:** the real clipboard with several editors open, Win+V
  entries, and a large 4K capture's first-copy delay.

## PrtSc key opens Vale's capture

Code: `src/app_prtsc.rs` (a child of `app`, the hook only), `src/app_hotkeys.rs`
(which rows use it), `Event::PrintScreen` / `Effect::RunCapture` in the
reducer, the row in `settings_layout::hotkeys_page`. Generalised in
[[hotkeys]].

- **Why a hotkey failed.** Windows 11 owns the bare PrtSc key (Snipping Tool)
  while `HKCU\Control Panel\Keyboard\PrintScreenKeyForSnippingEnabled` is
  absent (the default), so `RegisterHotKey` on it fails. The first design
  (hotkey id 4, plus a Settings button that wrote that value = 0) was
  dropped: Windows applied the write only after a sign-out. **Vale now writes
  nothing to the registry** and needs no sign-out.
- **The hook.** A `WH_KEYBOARD_LL` hook (`SetWindowsHookExW`, module handle,
  thread 0) sees the key before Windows and swallows it. It runs on its **own
  thread** (`HookThread`, named `vale-prtsc-hook`), not the UI thread: a
  low-level hook is called inside its installing thread's message loop, every
  keystroke system-wide waits for it, and Windows silently removes a slow
  hook. The thread installs the hook, creates its queue (`PeekMessageW`),
  reports success or failure once over a channel (the UI waits for that one
  answer), then blocks in a plain `GetMessageW` loop. Stop =
  `PostThreadMessageW(WM_QUIT)`, the thread unhooks and ends, the UI joins it.
  Since the recordable hotkeys ([[hotkeys]]) it exists only while some row of
  `config.hotkeys` has a PrtSc combo (the screenshot row by default) and the
  hotkeys are not paused: started and stopped by `app_hotkeys::sync_hotkeys`
  and in `WM_DESTROY` (`hotkeys_shutdown`). No timer.
- **Fast-proc rules.** `hook_proc` allocates nothing, takes no lock, logs
  nothing. It reads the island HWND (`ISLAND`, `AtomicIsize`), `SWALLOWING`
  (`AtomicBool`) and a static table of five `AtomicU8` (one per `HotkeyId`:
  0xFF unassigned, else the modifier bits of that row's PrtSc combo), calls the
  pure `prtsc_hook_action(vk, key_down, mods, swallowing, table)`, and on
  `SwallowAndFire(row)` does `PostMessageW(island, WM_APP_PRTSC, row)`
  (`WM_APP + 60`, a free id; `wparam` is the `HotkeyId` index) and returns 1;
  else `CallNextHookEx`. A key-down whose modifiers equal an assigned row's:
  swallow and fire once; auto-repeat while swallowing: swallow, no second fire;
  key-up after a swallow: swallow and clear; an unassigned combo, its key-up
  and other keys: pass. Changing a row while the hook runs is an atomic store.
- **Modifier match.** Alt (`LLKHF_ALTDOWN` or `GetAsyncKeyState` VK_MENU),
  Win, Ctrl and Shift are read as before and must equal the row's modifiers
  exactly: with the default bare row, Alt+PrtSc, Win+PrtSc, Ctrl+PrtSc and
  Shift+PrtSc stay Windows'. Default row: bare PrtSc.
- **Config.** `hotkeys.screenshot` ("PrtSc", or "None" for off). The old
  `screenshot.print_screen` bool is migrated once (false: None; true, absent
  or a non-bool: PrtSc) and drops out on the next save.
- **What a press does.** `WM_APP_PRTSC` -> `on_hook_row` ->
  `on_screenshot_hotkey` (any row may carry a PrtSc combo; the screenshot row
  does this): always Vale's
  own capture, whatever "Alat screenshot" says (that choice is only for the
  tile). Returns early when the row is off or `capture::overlay::active()`
  (an overlay is up or starting); else `Event::PrintScreen`: the reducer
  collapses an open panel or the search (checked with `panel_open()`, not
  `view()`: a transient notice on top hides a panel still held under it) and
  asks `Effect::RunCapture`, which `App` queues as `Action::Capture` on
  `TIMER_SHORTCUT` (delay `ANIM_MS + 60`), so the panel is not in the picture.
  It is not mapped through the `ms-screenclip:` branch of `queue_shortcut`.
- **Settings row.** The old "Tombol PrtSc" toggle (and `PrtscRow`,
  `PrtscHookFailed`) are gone: the key is the last row of Settings → Tombol
  pintas ("Screenshot Vale"), a recordable field like the others. When the
  hook cannot be installed every PrtSc row is `failed` and its description
  turns red ("Sudah dipakai aplikasi lain"). The Screenshot card in Clipboard
  is six rows: tool, Setelah capture, Salin otomatis, preview, on-ignore,
  seconds.
- **Retry.** A failed install is tried again when the tray menu opens and
  when Settings opens (`retry_hotkeys`), and by any sync of a failed row.
- **Untouched:** `app_shot.rs` (the clipboard preview of a PrtSc image),
  `src/screenshot/`, the tile's tool choice, gestures, the search layout.
- **Demo:** `DI_HOTKEYS=failed` with `--settings hotkeys` marks every set row
  as taken (it replaced `DI_PRTSC=failed`).
- **Tests:** the pure decision over the table (exact modifiers, the row that
  fires, repeat, key-up), the table's atomic stores, the `WM_APP + 60` id
  against every other `WM_APP + n`, and a start/stop of the thread with no
  island window. The proc itself is not driven by synthetic keys.
- **Not verified live:** real hardware against Snipping Tool (that the swallow
  keeps it from opening), and that a low-level hook does not see keys while
  an elevated (admin) window has focus because of UIPI, so PrtSc then reaches
  Windows.

Related: [[screenshot-preview]], [[clipboard-privacy]], [[studio-editor-frame]],
[[media-click-guards]], [[bata-idle-power-budget]]

Listed in [[index]].
