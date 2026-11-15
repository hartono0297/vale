# Copy the Phone Link Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A copy icon on the phone QR view's network card puts the live phone link on the clipboard (so a laptop can open it), and the phone page accepts files dropped onto it.

**Architecture:** Notch side: a new `Hit::PhoneCopyLink` → `Effect::PhoneCopyLink`; the app keeps the live URL in `app_phone::Ready.url` (filled from `PhoneEvent::Ready.url` and `NewLink.url`), writes it with `win::write` like `app_search::copy_search_result`, and shows `Icon::Check` instead of `Icon::Copy` for 2 s using the existing 1 s `TIMER_PHONE` redraw. Page side: document-level drag and drop feeding the existing `pickFiles`.

**Tech Stack:** Rust 2024, Win32 (existing `clipboard::win`), Direct2D painter, ES5 page in `src/phone/page.html`.

**Spec:** `docs/superpowers/specs/2026-11-15-phone-copy-link-design.md`

## Global Constraints

- Session rules unchanged: token TTL 300 s, renewal, `vs` cookie, peer checks.
- No new crate, `windows` feature, timer id or thread. `TIMER_PHONE` (48) stays the only phone timer.
- Clipboard write: plain `CF_UNICODETEXT` (format 13) only, no exclusion formats; record `self.clip.own_sequence` so Vale's history skips it; no notice.
- No test or demo calls `win::write`/`win::read` ([[clipboard-privacy]]).
- New strings: `PhoneStep1Sub` becomes `"Atau salin link-nya ke laptop"` / `"Or copy the link to a laptop"`; page key `drop`: `"Lepas untuk kirim"` / `"Drop to send"`. No em dash in new text.
- Page: ES5 (`var`, no arrow functions), text via `h()` text nodes, no new `.innerHTML`, no `url(`, stays under 30 KB, `id`/`en` keys identical.
- Every phone string fits its box in DM Sans and JetBrains Mono, both styles, ID and EN (`every_phone_string_holds_its_box_in_both_fonts`).
- Antislop skills (`antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-human`) apply to the painter and page work; mode is `during` (global preference).

## Review Focus

1. Copy clicked while the code is dead, still starting, or on the warning card: nothing is written, no check. (Task 2 test `copy_target_is_none_without_a_live_code`.)
2. Token renewed while the check shows: the check goes, the next copy gives the new link. (Task 2 test `a_renewal_clears_the_check_and_moves_the_link`.)
3. Wi-Fi ↔ Tailscale switch: the copied link is the new session's address, never the old one. (Task 2 test `ready_keeps_the_link_it_was_given`; `forget_session` wipes `copied_at`.)
4. A long network name or a 15-character IPv4 address in the narrower text box: still fits. (Task 1 fit test uses `phone_net_text` and the longest demo strings.)
5. A drop on a lost page, a folder, or a drop on the "Dari laptop" tab: the browser must never navigate to the dropped file; lost = ignored, folders skipped, other tab = queued. (Task 3 browser check.)

---

### Task 1: Copy hit, layout rect, painter, strings, demo

**Files:**
- Modify: `src/model.rs` (Hit ~813-820, Effect ~1046-1052)
- Modify: `src/state.rs` (~2242-2251; tests ~10412, ~10447, ~10461)
- Modify: `src/ui/panel_layout.rs` (`PhoneQrLayout` :3247, `phone_qr_layout` :3296, hits :4926-4935, tests :7564-7642)
- Modify: `src/ui/phone_view.rs` (`CodeView` :22, test :142)
- Modify: `src/ui/render_panel.rs` (`paint_phone_qr` :4125-4183, fit test :7067-7219)
- Modify: `src/i18n.rs` (`PhoneStep1Sub` :~555)
- Modify: `src/ui/demo.rs` (`SCENES` :1583, phone scenes :1928-1960, `PhoneDemo`, `DemoPhone::view` :5670)
- Modify: `src/app_phone.rs` (only the `CodeView` literal :300, `copied: false` for now)

**Interfaces:**
- Produces: `Hit::PhoneCopyLink`, `Effect::PhoneCopyLink`; `PhoneQrLayout.copy: Option<Rect>` (Some iff `expired == false`): a square `rect(net.right() - 38.0, net.y, 38.0, 38.0)`; `pub fn phone_net_text(l: &PhoneQrLayout) -> Rect` = `Rect { x: l.net.x + 32.0, w: l.net.w - 72.0, ..l.net }` used by the painter and the fit test for both text lines; `CodeView.copied: bool`.

- [ ] **Step 1: Failing tests.**
  - state.rs `each_phone_button_counts_only_at_its_own_stage`: add `Hit::PhoneCopyLink` to the hits array; at stage Qr it yields `[Effect::PhoneCopyLink]`, at every other stage and with the page hidden (`a_phone_hit_with_the_page_not_showing_does_nothing`) nothing.
  - panel_layout.rs hit-list asserts: `phone_qr_layout(s, strip, false)` hits == `[PhoneBack, PhoneNet(Wifi), PhoneNet(Tailscale), PhoneCopyLink]`; with `true` == `[PhoneBack, PhoneNet(Wifi), PhoneNet(Tailscale), PhoneNewQr]` (no copy).
  - panel_layout.rs `the_phone_views_fit_the_panel_and_end_their_header_at_the_edge`: `copy` lies inside `net`, `copy.h == net.h`, and `phone_net_text(&q).right() <= copy.x`.
  - i18n.rs: `text(Id, PhoneStep1Sub) == "Atau salin link-nya ke laptop"`, `text(En, PhoneStep1Sub) == "Or copy the link to a laptop"`.
- [ ] **Step 2:** `cargo test --locked phone` → the new asserts FAIL (unknown variant / field).
- [ ] **Step 3: Implement.** Variants and reducer arm with the same `phone_shown() == Some(PhoneStage::Qr)` guard as `PhoneNet`; `app.rs` effect dispatch `Effect::PhoneCopyLink => self.phone_copy_link(now)` calling a stub in `app_phone.rs` that does nothing yet (Task 2 fills it). Layout rect and `phone_net_text`; register `add(copy, Hit::PhoneCopyLink)` after the segment items. Painter, in the `QrView::Code(c)` arm only when `c.remaining.is_some()`: `icon_colored(if c.copied { Icon::Check } else { Icon::Copy }, copy, 14.0, p.secondary, 1.0, None)` (check in the palette's ok/accent ink if one exists for confirmations; follow `widgets/list_win.rs:1129-1134`); hover look like the other QR-view buttons if the painter has one. Both net text lines use `phone_net_text`. `CodeView.copied` field (update the literal at phone_view.rs:142, app_phone.rs:300, demo.rs builder). Demo: `PhoneDemo::QrCopied`, scene `"phone-qr-copied"` (live code, `copied: true`), `SCENES` length +1.
- [ ] **Step 4:** In the fit test switch both net-line rects to `phone_net_text(&qr)`, and add the longest address `"192.168.100.200:47390"` (mono) and `"Kenzie_5G · publik"` if not already listed. `cargo test --locked` → all PASS.
- [ ] **Step 5:** Render `--demo` with `DI_DEMO_STEP=phone-qr` and `phone-qr-copied` (debug build) in DM Sans and JetBrains Mono, look at the icon and the step-1 line; then commit `feat(phone): copy icon on the QR view's network card`.

### Task 2: Copy the live link

**Files:**
- Modify: `src/app_phone.rs` (`Ready` :44-54, `PhoneState` :117, Ready handler :576-599, `phone_new_qr` :468-480, `phone_keep_fresh` :548-561, the stub from Task 1)
- Modify: `src/app_search.rs` (`text_formats` :66 → `pub(super)`)

**Interfaces:**
- Consumes: `Effect::PhoneCopyLink`, `CodeView.copied` (Task 1).
- Produces: `Ready.url: String`; `PhoneState.copied_at: Option<Instant>`; `const COPIED_FOR: Duration = Duration::from_secs(2)`; `fn copy_target(&self, now: Instant) -> Option<&str>` (the url when `ready` is some, `refusal` none and `remaining(now).is_some()`); `fn copied(&self, now: Instant) -> bool` (`copied_at` younger than `COPIED_FOR` and `remaining(now).is_some()`); `fn link_renewed(&mut self, link: &NewLink)` (sets `qr`, `made`, `url`, clears `copied_at`), used by both renewal sites.

- [ ] **Step 1: Failing tests** in `app_phone.rs`'s test module (pure `PhoneState`, injected `Instant`s, no clipboard):
  - `copy_target_is_none_without_a_live_code`: no `ready` → None; `ready` with `made` older than `TOKEN_TTL` → None; `renew_failed` → None; `refusal` set → None; fresh `ready` with url `"http://192.168.18.42:47390/s/AbCdEfGhIjKlMnOpQrStUv"` → `Some` that exact string.
  - `the_check_lasts_two_seconds`: `copied_at = Some(t)`; `copied(t + 1.9 s)` true, `copied(t + 2 s)` false.
  - `a_renewal_clears_the_check_and_moves_the_link`: after `link_renewed(&NewLink { url: "http://…/s/NEW", .. })`, `copied(now)` false and `copy_target(now) == Some("http://…/s/NEW")`.
  - `ready_keeps_the_link_it_was_given`: the Ready handler's pure part (extract `fn ready_from(event fields…) -> Ready` if the handler is not testable as is) keeps `url`; `forget_session` leaves `copied_at` None.
- [ ] **Step 2:** `cargo test --locked app_phone` → FAIL.
- [ ] **Step 3: Implement.** Fields and helpers above; Ready handler stores `url` instead of dropping it with `..`; both renewal sites call `link_renewed`. `phone_copy_link(&mut self, now)`: `let Some(url) = self.phone.copy_target(now).map(str::to_owned) else { return };` `win::write(self.hwnd, &text_formats(&url))` → on `Some(seq)`: `self.clip.own_sequence = Some(seq); self.phone.copied_at = Some(now); self.draw(now)`; on `None` nothing. `phone_view` sets `copied: st.copied(now)`. The 1 s `on_phone_timer` redraw already turns the check back; add nothing to it.
- [ ] **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Commit `feat(phone): copy the live phone link`.

### Task 3: Drop files onto the page (parallel with Tasks 1-2, own worktree)

**Files:**
- Modify: `src/phone/page.html` (dicts :73-93, after `show()` setup; new overlay outside `#app`; CSS near :14-64)
- Modify: `src/phone/server.rs` (tests near :1287-1371)

**Interfaces:**
- Consumes: `pickFiles(list)` (:321), `state`, `t(k)`, `h(...)`.
- Produces: page key `drop`; a document-level handler.

- [ ] **Step 1: Failing test** `the_page_takes_dropped_files` in server.rs: `PAGE` contains `addEventListener("drop"` and `addEventListener("dragover"`, and the `id` dict has `drop:"Lepas untuk kirim"`, the `en` dict `drop:"Drop to send"`.
- [ ] **Step 2:** `cargo test --locked the_page` → FAIL.
- [ ] **Step 3: Implement.** On `document`: `dragenter`/`dragover` with `"Files"` in `dataTransfer.types` → `preventDefault()` always (so the browser never opens the file); show the overlay only when `state === "approved"`; set `dropEffect` to `"copy"` or `"none"` when lost. `dragleave` hides it when the pointer leaves the window (`relatedTarget` null, or an enter/leave counter). `drop` → `preventDefault()`, hide; if `state !== "approved"` stop; build the list from `dataTransfer.items` skipping entries whose `webkitGetAsEntry && webkitGetAsEntry().isDirectory`, falling back to `dataTransfer.files`; `pickFiles(list)` (works on either tab; the list repaints when the send tab shows). Overlay: one element made once with `h()`, appended to `body` (not `#app`, which `show()` clears), `position:fixed; inset:0`, `hidden` toggled, uses `--bg`/`--acc`/`--line` tokens, a dashed frame like `.pick`, the `drop` text centred; text and frame meet 4.5:1 on its background (check with antislop-human's `check_contrast`).
- [ ] **Step 4:** `cargo test --locked` → PASS (incl. 30 KB cap, key parity, no `innerHTML`/`url(`).
- [ ] **Step 5: Browser check.** Debug build `--phone-debug`, open the printed link in the built-in browser at desktop size; in the page run a synthetic drop (`var dt = new DataTransfer(); dt.items.add(new File(["hi"], "drop-test.txt")); document.dispatchEvent(new DragEvent("drop", {dataTransfer: dt, bubbles: true}))`) and see the row upload and the file land in the debug receive folder; `dragover` shows the overlay; after stopping the server (lost) a drop does nothing and the page stays. Commit `feat(phone-page): drop files onto the page`.

### Task 4: Docs

- [ ] README privacy section: one line that the link can be copied and opened in a laptop browser, and that anyone holding a fresh link can join (same as the QR). `docs/guide.md` phone section: same, plus drag and drop.
- [ ] After merge (finishing step): `docs/memory/shelf-phone.md` gets a "Copy link and drop (2026-11-15)" section; index line updated.
