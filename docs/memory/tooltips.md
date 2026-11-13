---
tags: [decision, ui, tooltip, popup]
date: 2026-11-13
---

# Hover tooltips (`ui/tooltip`)

One Vale tooltip for every icon-only or unclear button in Vale's own windows
(owner's ask, 2026-11-12: "se informatif mungkin ... infokan hotkey nya juga
dan untuk apa"). Spec `docs/superpowers/specs/2026-11-12-tooltips-design.md`
(the tables are the texts), plan `docs/superpowers/plans/2026-11-12-tooltips.md`.
Related: [[menu-pop]], [[bata-idle-power-budget]], [[exclude-from-capture-in-context]],
[[widgets]], [[sticky-note-list]], [[rekam-page]], [[hotkeys]].

- **Look (owner picked "follows the window theme"):** `theme::current_window()` plate
  (`popover` fill, hairline, the dropdown's shadow through the shared
  `menu_pop::paint_plate`), name 600 ~12.5 dip, one key plate per key at the right
  of line 1, the "what a click does" line in `secondary`, wrapping, plate at most
  280 dip. Short form (button already labelled): only the line, in `text`.
- **Behaviour:** 500 ms rest (`DELAY_MS`); within 300 ms of a tip going (`WARM_MS`)
  the next one shows at once; no animation. After a click the button stays quiet
  until the pointer has left it. Below the button, flipped above when no room,
  clamped in the anchor monitor's work area at its DPI; `Side::Below` (closed
  notch) never flips.
- **Window:** one layered popup, `WS_EX_TRANSPARENT | NOACTIVATE | TOOLWINDOW |
  TOPMOST`, owned by the hovered window, created on show and destroyed on hide;
  the waits are thread timers (`SetTimer(None, ..)`) alive only while waiting or
  warm. Nothing at idle. `WM_NCDESTROY` (owner gone) resets the machine. It is a
  top-level Vale window, so the screen-recording excluder keeps it out of videos.
- **Feeding it:** each window calls `tooltip::hover(owner, Some(Target))` on every
  pointer move (`None` over nothing), `pressed(owner)` on any button down,
  `hover(owner, None)` on leave (keeps the warm window), `hide(owner)` on owner
  hide / move / resize / drag / close, `hide_all()` on theme or language change,
  any global hotkey or PrtSc, and at capture start (`run_shortcut`, `RunCapture`,
  the screen picker).
- **Ids carry no state.** A tip id is per control (`id_of` of a stateless key);
  play/pause, pin/unpin, mute/unmute keep their id, and `hover` repaints a shown
  tip in place when only its text changed. A state in the id broke the clicked
  rule (the click flips the state, the id changes, the tip comes back under the
  pointer); found in the final review, fixed everywhere.
- **Re-entrancy:** never call `hover` / `hide` / `to_screen` / `ClientToScreen`
  while a RefCell borrow that window code re-enters is held. The transcript window
  and Settings build a `tooltip::TipSpec` in the borrow and call `on_screen` after;
  the caption and lyrics cards queue `TipOp`s and run them after the `CARD` borrow.
  Anchors from a window origin + dip rect go through `tooltip::anchor_px`.
- **Texts:** pure per-language functions per surface: `widgets/tips.rs` (notes,
  format bar, popovers, note list marks), `ui/panel_tips.rs` (notch, dock, every
  page incl. the redesigned Rekam page), `transcript_win/layout.rs`,
  `caption_card/layout.rs`, `lyrics_card/layout.rs`, `settings_layout::hit_tip`,
  `editor_close_tip`, `month_nav_tip`. Hotkeys are `KeyHint::Hotkey(id)`, resolved
  from the live config at show time (`set_hotkeys`); no combo, no plate. Each text
  was checked against its handler (e.g. Check acts on the selected lines, the Bell
  mark means the reminder went off, caption Copy copies the final lines only, Esc
  does not go back on the phone view).
- **Island specifics:** `panel_tips::has_tip` must list every hit `tip_for` answers
  (a gate test checks both agree); `TipCtx` is built only for those hits.
  `panel_tips::feed` leaves the tooltip alone while a button is held, while the
  shape morphs or a page slides (`tip_in_flux`), and while the pointer stays on
  the pressed control (`tip_pressed`, compared with `same_control`). The tip of
  "open folder" on the Rekam page shows only on the header icon
  (`on_first_region`).
- **Not given tips:** text buttons and rows, steppers, accent swatches (they name
  themselves in the row text), the busy-toast logo slot (hover peeks the toast),
  the closed music notch (no sub-regions). The screenshot strip, its action bar
  and the screen-recording picker keep their own in-overlay tips; the shot editor
  keeps its footer hint.
- **Known gap:** in a sticky note, `open_overflow`, `open_popover` and
  `open_remind_entry` still call `hide` after the click, so the More, colour and
  reminder buttons can show their tip again after their popover closes under a
  resting pointer (should be `pressed`).
- **Not checked live** (the owner declined the automated run): placement on every
  surface, light / warm dark / Terminal with the real palette and the key plate's
  contrast (mockup numbers: text 13.5-16.6:1, secondary 5.07-6.64:1), a second
  monitor with another DPI, idle CPU with nothing hovered, a tip during a screen
  recording and a screenshot.
