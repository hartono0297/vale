# Claude notices stop covering the lyric line

Date: 2026-11-19. Small change, written after a short talk with the owner.

## Intent

While music plays and the lyric line shows in the notch, a hook notice from
Claude ("Claude selesai", "Claude butuh respon") takes the whole notch and hides
the line. The owner wants to keep reading the lyric.

## Root cause

[[toast-while-busy]] already tucks a real Windows toast into a small logo slot
beside a compact view ("Ringkas"). It only does so for
`NoticeKind::App { toast: Some(_) }` (`state::is_real_toast`). The Claude
notices are not that kind, so:

- `ClaudeDone` is a timed notice in `current`: `view()` returns `View::Notice`,
  which replaces the lyric for its whole time.
- `ClaudeWaiting` is sticky: `base_view_with` returns `View::Notice(sticky)`
  ahead of the media, so the lyric stays hidden until the session answers.

## Goals

- With the Ringkas setting (`ToastStyle::Compact`, the default), both Claude
  notices tuck into the existing slot beside the lyric line (and beside the
  compact music view). The line keeps playing; the slot shows a small Claude
  badge (letter tile "C", as for a toast without a logo).
- Peek (pointer on the badge) shows the full notice; click does what the
  notice does today (`Effect::OpenClaudeSession`); the ✕ rules are unchanged.
- `ClaudeWaiting` stays as the badge until the session shows activity or
  finishes (same clearing as today), with no expiry and no unread dot.
- With the Penuh setting, or over any other view, nothing changes.

## Non-goals

- No change to the Strip form: an open panel keeps today's behaviour for both
  Claude notices. No change to Idle, REC, pomodoro, caption, fullscreen.
- No new setting, timer or window. No change to real-toast behaviour.
- No edits to `cat_style.rs`, `render.rs`, `geometry.rs` unless a test shows
  they are needed (another session works there).

## Approaches

1. **Reuse the toast slot machinery** (chosen): widen "tuckable" from "real
   toast" to "real toast, or a Claude notice over a music view". The slot,
   peek, geometry, lyric fit and renderer already exist and are tested.
2. A separate Claude-only badge with its own state and drawing: duplicates the
   slot, peek and morph code, and the two badges would fight for the same spot.
3. Shorten the Claude notice and draw the lyric under it: both texts would
   still overlap, which is the bug.

## Design

- `state.rs`: a predicate `tuckable(notice)` (real toast, or `ClaudeDone` /
  `ClaudeWaiting`) replaces `is_real_toast` where it decides tucking
  (`toast_candidate`, `toast_form`, peek and hold). Claude notices only count
  when the base view is `MediaLyric` or `MediaCompact`; a real toast keeps its
  full table.
- Sticky `ClaudeWaiting`: while the setting is Compact and the view without it
  is a music view, `base_view_with` skips it and `toast_slot()` returns it.
  A peek shows it whole (`view()`), a click on the slot dismisses it with
  `OpenClaudeSession`, exactly like the notice's body click.
- `ClaudeDone` keeps its queue and its 5 s; only its form is derived from
  context, like a toast. Over a Full form it shows as today.
- `app.rs` / `notif_page::toast_badge_of`: a Claude notice gives a letter-tile
  badge ("C") with source "Claude".
- Tests first: reducer tests in `state.rs` (a `ClaudeDone` and a
  `ClaudeWaiting` over `MediaLyric` give `View::MediaLyric` with
  `toast_slot()` set; peek, click, clearing, Penuh, panel and idle unchanged).

## Left to the builder

Exact fn names, the demo scene for `--demo` (lyric plus Claude slot), and
whether the strip form of `notice-busy-lyric` scenes needs a twin for Claude.
