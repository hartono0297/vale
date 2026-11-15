---
tags: [spec, shelf, phone, i18n]
date: 2026-11-15
---

# Name a laptop on the phone link

## Why

Since the copy-link change (`2026-11-15-phone-copy-link-design.md`) a laptop can join the
shelf's phone link, but the notch still calls it a phone: "Tersambung: HP", "2 HP",
"Menerima dari HP", and the card tag "dari HP". Owner: "Ya, benerin", then "u atur yang
terbaiknya gimana" (the builder's calls below are the session's, agreed in principle).

## Goals

- `phone::ua::parse` gets a `DeviceKind::Laptop` for User-Agents that are never a phone or
  tablet: contains "Windows NT", "CrOS", or "X11" / "Linux" without "Android". Called
  "Laptop" / "Laptop".
- `DeviceKind::Other` (Macintosh, which iPadOS Safari also sends by default, and anything
  unknown) is called "perangkat" / "device" instead of "HP" / "Phone": no guess.
- Several devices: all of them phones (Android, iPhone; iPad counts as a phone-like
  tablet) keep "2 HP" / "2 phones"; any Laptop or Other among them gives "2 perangkat" /
  "2 devices".
- The receiving view's title "Menerima dari HP" / "From phone" becomes neutral
  ("Menerima berkas" / "Receiving" or a shorter word that fits), and the shelf card tag
  "dari HP" / "phone" becomes neutral ("masuk" / "received", or "in" if "received" does
  not fit): the tag cannot know which device sent a file.

## Non-goals

- The shelf pill stays "HP"; QR view steps, page texts, "Kirim ke HP" (sticky notes) and
  the page's own wording stay as they are.
- No change to joining, counting or the server.

## Rules

- All new strings fit their boxes in DM Sans and JetBrains Mono, both styles, ID and EN
  (`every_phone_string_holds_its_box_in_both_fonts`, and the shelf tag's own fit check).
- `ua.rs` tests: Windows Chrome/Edge/Firefox, ChromeOS, desktop Linux Firefox → Laptop;
  Android (which says "Linux") stays Android; Macintosh Safari stays Other; iPhone/iPad
  unchanged.
- i18n tests: one Laptop → "Tersambung: Laptop"; Android + Laptop → "2 perangkat" /
  "2 devices"; two Androids → "2 HP" / "2 phones"; Other → "perangkat" / "device".

## Approaches

One sensible way: extend the existing pure parser and wording functions. Rejected:
detecting touch on the page and reporting it back (a new route or header for a label);
calling Macintosh "Mac" (an iPad would be named a Mac).

## Done

Merged to `main`, `cargo test --locked` green, memory (`shelf-phone`) updated, `main`
pushed. Owner checks: a laptop joining shows "Tersambung: Laptop"; a phone plus a laptop
shows "2 perangkat"; a file sent from either shows the neutral title and tag.
