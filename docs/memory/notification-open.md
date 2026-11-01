---
tags: [decision, notifications, win32, uia, focus, browser]
date: 2026-11-01
---

# Opening a caught toast: go to where it came from

One routine, `sources::notif_open::open(OpenTarget)`, serves both the click on
the notch toast (`Effect::OpenTarget`, from `state::dismiss_visible_notice`)
and the Notifikasi page's Open (row click and Enter, `app_notif::perform_notif`,
[[notification-history]]). It replaced `explorer shell:AppsFolder\<AUMID>`,
which opens Chrome's profile picker and does nothing for an app hidden in the
tray. `OpenTarget { app_id, source, site, title, body }` is built from the
notice or the history entry; a toast with **no AUMID still opens** (searched by
display name). Only an island-owned App notice with neither a toast origin nor
an AUMID stays a no-op. The pure rules are `notif::open` (unit tested), the
Win32 side is `sources::notif_open`.

## Why not the window's AUMID alone

`SHGetPropertyStoreForWindow` + `PKEY_AppUserModel_ID` returns an **empty**
string for Chrome and Discord windows. It only helps packaged apps (including
their `ApplicationFrameHost` windows). So the desktop search is: window AUMID,
else the process image name against the Start entry's target
(`PKEY_Link_TargetParsingPath`, ignored when it is an updater or launcher such
as `Update.exe`, or a host of many apps such as explorer.exe, cmd.exe,
rundll32.exe, javaw.exe, python.exe, node.exe, powershell.exe: `HOST_STEMS`;
Explorer's own toasts still find explorer.exe by the stems of its AUMID), else the image stem against `exe_stems_for` (display name,
every dotted piece of the AUMID; generic stems like update, app, desktop,
electron are dropped so `Update.exe` never stands for Discord).

## Installed web apps (PWAs)

An AUMID containing `_crx_` (`Chrome._crx_<id>`, `MSEdge._crx_<id>`) is an
installed web app (`is_web_app`): `decide` sends it to the **desktop** rule even
though `is_browser` sees the word chrome in it. Its window has no tab strip and
the browser rule would bring a normal browser window forward instead. Its
AUMID gives no exe stems and its Start entry target is not used (both name the
browser, whose every window would match); its window declares the AUMID, so
the window-AUMID match finds it, and with no window it launches by AUMID.

## Browser rule (`decide` = Browser by `is_browser`, not a web app)

1. The browser's windows by image name (`browser_exe_names`, all profiles), in
   Z order, `Chrome_WidgetWin_1` / `MozillaWindowClass`, visible, titled.
2. Per window the tab strip is read through UI Automation: breadth first, one
   level per call, descending only into containers (never the page document),
   stopping at the first depth with `TabItem`s. Cached: control type, name,
   `SelectionItem.IsSelected`.
3. `pick_tab`: names are stripped of browser decorations (`strip_tab_suffix`:
   "- Memory usage - 120 MB", "- Audio playing", "- Google Chrome", ...);
   `score_tab` ranks the site's registrable label (`site_label`: web.telegram.org
   -> telegram) in the tab name above the whole notification title above its
   words of 4+ letters. The title and its words match only whole words of
   the tab name (`WordRuns`): a short sender name like "Ibu" must not hit
   "Distribution", nor "pesan" hit "Pesanan Saya", or a toast with no site and
   no chat tab open would switch to an unrelated tab. The body is not used.
   A meaningful subdomain label (`site_sub_label`:
   calendar.google.com -> calendar, mail.google.com -> mail; www, web, app, m
   and other generic or short labels are skipped) counts as a site match on
   its own, in three strengths: the subdomain alone as the tab's **first or
   last whole word** ("Meet - abc-defg-hij", "Mail - Budi - Outlook",
   "Calendar - Outlook"; `WordRuns::is_first_or_last_word`), never as a raw
   substring or a middle word, so "ChatGPT" (chat), "Meeting notes - Notion"
   (meet), "Academic calendar 2026 - UI" (calendar), "How to use Gmail -
   YouTube" and "Sending email with Python - Stack Overflow" (mail) are not
   matches (the owner's rule: never switch to an unrelated tab); ending the
   tab's **last word**, where pages name their site ("Inbox -
   user@gmail.com - Gmail", "Mail - Budi - Outlook" for outlook.live.com),
   so for a Gmail toast "Mail - Budi - Outlook" or "Mailchimp - Campaigns"
   never beats the Gmail tab even when active; and the label right in front
   of the subdomain ("googlechat", "googlecalendar", "googledocs"), the
   strongest. A tab whose **last whole-word** label is followed by another
   word and by no subdomain names another page of the site and is no site
   match at all: "gmail - Google Search", "Email newsletter - Google Docs"
   and "Gmail: Private and Secure Email | Google Workspace" score 0 for a
   Gmail toast, while "Security alert for your Google Account -
   user@gmail.com - Gmail" and "user@google.com - Gmail" still match (the
   subdomain comes after the label). The label must be a whole word: "live"
   inside "Oliver", "delivery" or "olive" blocks nothing (a raw substring
   test did, and lost the Outlook tab). Known gaps: "Hotmail" also ends in
   "mail" and ties with Gmail; with no Gmail tab open, "Mail - Budi -
   Outlook" still matches a Gmail toast on the bare subdomain as its first
   word (the Meet call tab needs that rule). The first-word half also
   matches unrelated tabs that open with the subdomain, picked when no
   stronger tab exists: "Mail merge in Word - Microsoft Support"
   (mail.google.com), "Chat with PDF - X" (chat), "Meet the team - Acme"
   (meet), "Calendar of events - City" (calendar). And a leading unread
   badge hides it: "(2) Chat - Budi" no longer matches the lone subdomain,
   its first word being "2". Owner awareness (no code change): a toast whose
   **title** is "Google" (for example a Google security alert) can still
   match a label-only Google tab (for example "cats - Google Search")
   through the title tier. With no match the window comes forward on its
   current tab. A site with no meaningful subdomain (web.telegram.org,
   google.com) matches on the registrable label alone. Matches come in tiers
   (label and subdomain; subdomain ending the last word; subdomain as first
   or last whole word, or site label; title; words only): the strongest tier
   anywhere decides, so a site match in an older window beats a words-only
   match in the newest. The most recent window with a match of that tier
   wins; inside it the active tab if its match is of that tier too, else the
   first best. A weak words-only hit on the active tab never beats a site
   match on another tab.
4. Match: restore if minimized, `SelectionItemPattern.Select()` on that tab,
   bring the window forward. The select runs through the same UIA client with
   its timeouts reset to 1 s first: the budget checks leave it with whatever
   was over (down to 1 ms), and a Select cut short by `UIA_E_TIMEOUT` would
   bring the window forward on the wrong tab. No match: the most recent window
   whose tab strip read any tabs (a PWA, DevTools or picture-in-picture window
   has none), else the most recent window; tab unchanged.
5. **Never a new window, never the profile picker** while a window exists. The
   browser not running: the site known and the exe resolved (`App Paths`, else
   the Start entry's target) -> start it with `https://<site>` and no profile
   flag; else launch by AUMID as before. The exe must be a **full path** that
   exists (`browser_launch_path`): `Command` searches the exe dir, System32,
   Windows and PATH but never `App Paths`, so a bare `vivaldi.exe` never
   starts. If the spawn still fails, launch by AUMID. Opera is `opera.exe`
   only; its `launcher.exe` (a generic name) is accepted as a launch path only
   inside a folder named Opera..., never as a window's process.

## Desktop rule

`pick_main_window`: unowned, titled, not a tool window, not empty when visible;
visible beats minimized beats hidden. A **hidden** candidate must also not be a
helper (`is_helper_window`: GDI+ hook windows, `*MessageWindow*` and
`*TrayIcon*` classes or titles, WPF's "Hidden Window", ATL's "AXWIN Frame
Window", a title equal to its class) and, unless iconic, at least 64x64 (message
windows sit at 136x39, GDI+ ones at 1x1). Most apps own several such hidden,
titled, unowned windows; showing one pops up a blank frame. No WS_CAPTION check:
a frameless main window may lack it. Visible or minimized: restore, bring
forward. Only a hidden main window (tray) with an AUMID: launch by AUMID (a
single-instance app shows itself), poll up to 2 s (every 100 ms) for a visible
window of that app, else `SW_SHOW` the hidden one. No window: launch by AUMID.
No AUMID and nothing found: nothing, no error, never a launch
(`desktop_action`).

## Foreground rule

`AllowSetForegroundWindow(ASFW_ANY)`, `SetForegroundWindow`; if refused,
`BringWindowToTop` + `SwitchToThisWindow(hwnd, TRUE)`. No `AttachThreadInput`,
no synthetic keys. The island stays `MA_NOACTIVATE`; when the Notifikasi page
holds the keyboard (hidden EDIT, [[cli-panel-keyboard-focus]]) the island is the
foreground process, so the call is allowed and the existing `WA_INACTIVE` /
Blur path lets go. No new code there.

## Worker and budget

`open` only sends the job (a few ms on the UI thread). One long-lived thread
`notif-open` with its own MTA apartment does everything: window listing (also
Z-order `EnumWindows`, image names cached per pid), UIA with connection and
transaction timeouts set to what is left of the 1 s search budget before every
call (the pattern of `capture/uia.rs`), the launch, the wake poll. A generation
counter makes a newer click supersede an older one (the worker takes only the
newest queued job and every step checks it).

## Dry run

`notif_open::plan(&OpenTarget) -> String` runs the same search and does
nothing (no select, no foreground, no launch). Exposed as
`vale.exe --probe-open <app name> [AUMID|-] [site|-] [title]` and as `#[ignore]`
tests (`cargo test plan_for -- --ignored --nocapture --test-threads=1`, run
with `rtk bypass` or the output is condensed away). Seen on the author's
desktop: Chrome tab strip read (5 tabs, `WhatsApp` found as tab 4 for a
web.whatsapp.com toast), Discord hidden in the tray planned as "relaunch".

## Still unverified

- Whether Chrome's site line really arrives in `GetTextElements` (without a
  site only the title scores a tab; see [[notification-history]], run
  `--probe-notifications`).
- Actual foreground behaviour of each launch and wake path with real toasts
  (only the plans were run, nothing was clicked); a minimized Chrome window's
  tab strip may read empty (then the window just comes forward).
- Firefox's and Opera's tab strips; Edge vertical tabs.
