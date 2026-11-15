# Name a Laptop on the Phone Link Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The notch calls a joined laptop "Laptop", an unknown device "perangkat", a mixed group "2 perangkat", and uses neutral words for the receiving title and the shelf card tag.

**Architecture:** One new `DeviceKind::Laptop` in the pure UA parser; the pure wording functions in `i18n.rs` choose the words; two existing keys change text.

**Tech Stack:** Rust 2024.

**Spec:** `docs/superpowers/specs/2026-11-15-phone-laptop-name-design.md`

## Global Constraints

- `ua::parse`: "Windows NT", "CrOS", or ("X11" or "Linux") without "Android" → `Laptop`; iPhone/iPod, iPad, Android rules unchanged and checked first; Macintosh and unknown → `Other`.
- Words: Laptop → "Laptop" / "Laptop"; Other → "perangkat" / "device"; several, all Android/iPhone/iPad → "{n} HP" / "{n} phones"; several with any Laptop or Other → "{n} perangkat" / "{n} devices".
- `PhoneReceiving` → "Menerima berkas" / "Receiving"; `ShelfFromPhone` → "masuk" / "received" (EN may be "in" only if "received" fails the fit test).
- Shelf pill "HP", QR steps, page texts, "Kirim ke HP" unchanged. No change to joining, counting or the server.
- Every changed string fits its box in DM Sans and JetBrains Mono, both styles, ID and EN (existing fit tests in `render_panel.rs`, incl. `every_phone_string_holds_its_box_in_both_fonts` and the shelf tag check near :3957 / :7059).
- No em dash in new text.

## Review Focus

1. Android UAs contain "Linux": must stay Android (test `an_android_says_linux_but_is_not_a_laptop`).
2. Samsung DeX / Android desktop mode sends "X11; Linux" without "Android" on some builds: becomes Laptop; acceptable, pinned by no test (owner check).
3. iPad Safari desktop mode ("Macintosh"): Other → "perangkat", never Laptop (test in `ua.rs`).
4. Mixed counts after a phone left: the list is per join, unchanged; the wording only looks at kinds.
5. A long EN tag "received" on a narrow card in Mono: fit test decides; fall back to "in".

---

### Task 1: Laptop kind and neutral words

**Files:**
- Modify: `src/phone/ua.rs` (enum, `parse`, tests)
- Modify: `src/i18n.rs` (keys :548, :586, :593-596 plus a new `PhoneKindLaptop`; `phone_kind` :2233-2245; `phone_devices_chip` :2249-2258; tests near :5230-5290)
- Modify (only if a fit test needs it): `src/ui/render_panel.rs`
- Modify (only to keep a match exhaustive or a demo honest): any `DeviceKind` match the compiler names; `docs/guide.md` / `README.md` if they quote "dari HP" / "Menerima dari HP".

**Interfaces:**
- Produces: `DeviceKind::Laptop`; `Key::PhoneKindLaptop`.

- [ ] **Step 1: Failing tests.**
  - `ua.rs`: `desktop_browsers_are_laptops` with Windows Chrome (`Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36`), Windows Edge, Windows Firefox (`Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:127.0) Gecko/20100101 Firefox/127.0`), ChromeOS (`... (X11; CrOS x86_64 14541.0.0) ...`), Linux Firefox (`Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0`) → `Laptop`; `an_android_says_linux_but_is_not_a_laptop` (the existing Android UAs stay `Android`); the existing Macintosh case stays `Other`.
  - `i18n.rs`: `phone_kind(Id, Laptop) == "Laptop"`, `phone_kind(Id, Other) == "perangkat"`, `phone_kind(En, Other) == "device"`; `phone_connected(Id, &[Laptop]) == "Tersambung: Laptop"`; `phone_devices_chip(Id, &[Android, Laptop]) == "2 perangkat"`, `(En, ..) == "2 devices"`; `phone_devices_chip(Id, &[Android, IPhone]) == "2 HP"`, `(En, ..) == "2 phones"`; `phone_devices_chip(Id, &[Android, Other]) == "2 perangkat"`; `text(Id, PhoneReceiving) == "Menerima berkas"`, `text(Id, ShelfFromPhone) == "masuk"`. Update existing tests that assert the old words.
- [ ] **Step 2:** `cargo test --locked ua` and `cargo test --locked phone` → FAIL.
- [ ] **Step 3: Implement** the variant, the parse rule order (iPhone, iPad, Android, then Laptop, else Other), the words and keys above; doc comments on `parse`, `phone_kind`, `phone_devices_chip` updated to the new words.
- [ ] **Step 4:** `cargo test --locked` → PASS (fit tests included; if EN "received" fails a fit test, use "in" and say so).
- [ ] **Step 5:** Commit `feat(phone): name a laptop, neutral words for received files`.
