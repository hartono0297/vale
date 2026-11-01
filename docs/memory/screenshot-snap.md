---
tags: [decision, screenshot, capture, editor, uia, power, privacy]
date: 2026-10-29
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
  the editor closes. Nothing touches disk or clipboard without Save or Copy.
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
- **Own-copy skip.** Copy writes PNG + CF_DIB through the clipboard module and
  records the sequence number (`shot_edit::record_own_copy`);
  `App::shot_from_image` returns early for that one image
  (`is_own_copy(seq, own)`), so the notch does not preview Vale's own copy.
  The clipboard history may still record it.
- **Demos** (debug builds, in `opens_windows`): `--demo-snap` (a desktop drawn
  in code, fixed candidates, no grab, no EnumWindows, no UIA) and
  `--demo-shot-editor [empty]`. In demos Copy is inert (the footer says so) and
  Save writes to `target/demo-shots/`; the process ends when the window closes.
  Screenshots of them: `PrintWindow` with flag 2 on the demo's main window.

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
  is now four rows: tool, preview, on-ignore, seconds.
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
