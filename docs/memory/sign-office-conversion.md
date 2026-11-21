---
tags: [decision, sign, office, com]
date: 2026-11-21
---

# Sign: Office to PDF conversion

Order: Microsoft Office, then WPS (same COM code, its own ProgIDs, best effort), then LibreOffice (`soffice --headless`). None: a message to save it as PDF from the app. Part of [[tanda-tangan]].

- **Private copy:** the source is copied to `%TEMP%\Vale\sign\<job>\vale-source.<ext>`; Office opens the copy and Close or SaveAs happen only when `FullName` equals it. The user's file is never opened.
- **Never quit or kill the user's instance:** a pid snapshot decides what Vale started (PowerPoint also checks `Presentations.Count`). On a user's instance only AutomationSecurity and DisplayAlerts are touched, and restored.
- **Detached worker** (`sign-office-com`) with a 90 s deadline and cancel; a stuck worker is detached, only a process Vale started is killed.
- **LibreOffice:** in a kill-on-close job object (`cli::kill_on_close_job`), private profile as a percent-encoded URL, `CREATE_NO_WINDOW`. A child spawned before the job assignment could escape (std cannot spawn suspended).
- **JobEnd rule:** whoever ends second (window or worker) sweeps the job dir; a startup sweep clears leftovers.
- **This PC:** Microsoft Office 16, unlicensed, so a visible Word shows a product-key modal that rejects COM; tests use a hidden Word, live tests are `#[ignore]` (Word, Excel, PowerPoint pass). WPS and LibreOffice are not installed: those paths are unit-tested detection only, not verified live.
- Password files: Word gives `Locked` (matched by error text, heuristic); Excel and PowerPoint not live-tested.
