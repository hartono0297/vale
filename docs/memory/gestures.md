---
tags: [decision, gestures, media, v3.2]
date: 2026-11-02
---

# Gestures on the island (v3.2 Run 8)

`src/gestures.rs` (pure: `Swipe`, `HWheel`, `Dir`); `App` feeds them from plain
window messages, no global hook (the plan's "hook + evaluator" was not
needed): `WM_LBUTTONDOWN/UP` for a swipe, `WM_MOUSEHWHEEL` for the sideways
wheel, `WM_MBUTTONDOWN/UP` for the middle button.

- Count only while `App::gestures_active`: `config.gestures` (default on,
  hand-edited, no Settings row) **and** a media session **and** the view is
  `MediaCompact`, `MediaLyric` or `Panel(Media)`. Elsewhere nothing happens.
- Swipe: ≥ 56 logical px sideways (scaled by the window's dpi), at least
  twice as far across as up/down, within 700 ms. Left = next, right = prev. A
  swipe clears the pending click so it never opens the panel in Click mode.
- Sideways wheel (reworked 2026-10-30, see below): a gesture is a run of
  deltas the same way with gaps of at most 150 ms (`HWHEEL_NEW_GESTURE_GAP`);
  deltas add up to one notch (120), the gesture fires once and is spent, so
  its whole momentum tail, however long, is ignored. The 500 ms quiet time is
  counted from the flick only and never extended. Right = next.
- Middle button over the drawn island = play/pause, only when the press
  (`WM_MBUTTONDOWN`) was on the island too (`middle_click`); a press that
  began elsewhere, or a pointer that left in between, does nothing.
- All three go through `Event::Click(Hit::Prev/Next/PlayPause)`, so the
  existing media guard and the media log see them like button clicks.

The plan's "clipboard peek" already exists from v3.1: Settings → Clipboard →
"Disalin untuk semua salinan" (`copied_feedback_all`) shows the "Disalin"
notice with a one-line preview (`app_clip::copied_notice`); nothing added.

**Not verified live:** a real touchpad's horizontal scroll (does it reach the
island window?), and whether a swipe feels right on a touch screen.

## Sideways swipe turns panel pages (2026-10-30)

`WM_MOUSEHWHEEL` is routed by the pure `gestures::hwheel_route(view,
cli_menu_open, gestures_media_ok, over_island)` (`App::on_hwheel`):

| Where the flick lands | Result |
|---|---|
| Open panel, any page incl. Music, pointer on the drawn island | one page: right = next, left = previous (`Event::PageFlick { next }`) |
| Panel on `Page::Search`, or the CLI folder menu open | nothing |
| Panel, pointer on the faint margin | nothing |
| `MediaCompact` / `MediaLyric` with `gestures_active` | song skip, as before (`Click(Prev/Next)`; the margin was never excluded here) |
| Notices, idle, anything else | nothing |

- **Not behind `config.gestures`:** turning a page is navigation like the
  vertical wheel; the setting only guards the music gestures.
- One flick = one page: the same `HWheel` rules (2026-10-30). A gesture = same
  sign, gaps <= 150 ms; the other sign or a longer gap starts a new gesture
  (sum reset, not spent). Accumulate to 120, fire once, then the gesture is
  spent (momentum tail returns `None`). `quiet_until` = flick + 500 ms, set
  only by a flick, never extended by later deltas (the old rule extended it
  on every delta, so a continuous touchpad tail blocked paging until the
  cursor moved: the stutter). A new same-direction gesture reaching 120
  inside the quiet window keeps its sum and fires on its first delta at or
  after `quiet_until`; the opposite direction fires at once. A touchpad burst
  of 30 deltas turns exactly one page (tested), as does a 1.5 s tail. `PageFlick` steps `pages.step(page, ±1)`, so order, skips
  and clamping equal the vertical wheel; it zeroes the vertical residue, keeps
  the sticky-reminder peek rule, stays off the search and arms the linger as a
  `PageTurn`. `last_paging` is set so the scroll-volume cover does not grab the
  next vertical notch.
- No element of the panel scrolls sideways (only hidden, owner-drawn EDITs
  have `ES_AUTOHSCROLL`), so nothing else claims the flick.
- Left-button swipe, middle click and the vertical wheel are unchanged.

## The cleaner holds the wheel (2026-11-02)

While the cleaner shows over the System page (`cleaner_shown()`), the vertical
wheel (`Event::Wheel`), the sideways flick (`Event::PageFlick`) and the dock
(`Hit::PanelPage`, `go_to_page`) turn no page: `can_turn_page()` is false and
a locked notch zeroes the saved-up residue. Back leaves it. The sticky
reminder peek branch is unchanged, and the plain System page still turns
pages. Over a Mole log longer than its box the wheel scrolls the log
(`Event::MoleLogScroll`, the CLI output's `WHEEL_LINES` rule), see
[[cleaner]] and [[mole-mode]].
