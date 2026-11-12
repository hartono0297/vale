---
tags: [decision, robot, notch]
date: 2026-11-02
---

# The notch assistant: which face, and where it shows

Since v3.1 (Run 13) the assistant is Vale's cat, not the robot Bata.
`src/ui/robot.rs` draws it from variant D1 in
`docs/design/vale-kucing-minimal.html` (64-unit grid, fixed brand colours
#E2683C head / #17120F eyes / #F4EDE4 extras, never the accent): a head
with a flat base (a dome on top) and two ears, and plain espresso pill eyes
(no shine, no lashes, no bow, no mouth, nose or whiskers). The cat floats in
the island and is not cut by its lower edge; the grid has room under the
base. (v3.1 had a round head with lashes and a plum bow; the owner asked for
this simpler one on 2026-10-28.) The cat has no name yet
(the owner picks one); the UI says "kucing Vale" and notices come from
"Vale". Code and demo steps still say `robot` / `bata` (no rename churn).
`choose_expr` picks the face (unchanged from Bata); its priority, highest
first:

1. Panel or hidden: no robot. The panel stays clean.
2. A notice decides by kind: reminder, Claude waiting → Alert; pomodoro
   phase done, Claude done, a finished clean ([[cleaner]], or a Mole one, [[mole-mode]]),
   "Disalin" (`NoticeKind::Copied`, see [[clipboard-history]]) → Happy;
   low battery, low disk/RAM, a failed Mole clean → Sad; greeting → Wave.
   With the robot off, "Disalin" shows the Clipboard icon badge.
   `NoticeKind::App` → no robot: other apps keep their badge. The island's
   own messages (config invalid, DND hint) also use `App { app_id: None }`
   and so keep their badge too; they cannot be told apart.
3. Pointer over a compact island → Happy (+ 2 px lift), before the dwell
   opens the panel. It also wakes a sleeping robot. The lift only applies
   where the hover sets the face (`Spot::compact`): a notice keeps its face
   still, and no frame clock runs for a robot that is not drawn.
4. Pomodoro → Focus, media → Music (the robot replaces the album art),
   lyric line → Sing.
5. Idle only: CLI just done/failed (4 s) → Happy/Sad; Claude working, a
   CLI command or a Mole clean running → **Typing** (since 2026-11-01, the
   cat types on a laptop, see [[cat-types-while-working]]; it was Think, which
   never looked at the pointer); battery at or below the threshold and
   discharging → Sad; asleep → Sleep; else Idle. **Think** stays in `Expr` for
   the demo face sheet and the `bata-claude` notice scene, but `choose_expr`
   never returns it.

Asleep (2026-11-02) = N minutes ("Tidur setelah", default 10) without mouse
or keyboard input anywhere on the PC (`GetLastInputInfo`) and without island
activity. A sleeping cat wakes within 2 s of any input; hovering the island
still wakes it at once. Motion Off never sleeps. Vale's own synthetic input
(media keys, volume wheel) counts as input too. How it is polled, and why
nothing runs while awake: [[bata-idle-power-budget]].

"Claude working" = a session with `ClaudeActivity` in the last 2 minutes
that has not sent Done or Waiting since (`CLAUDE_WORKING_FOR`).

Geometry: compact shapes widen by `robot_extra` (media +2, lyric +20,
pomodoro +26; idle holds robot + clock centred as a group). Notices are
unchanged: the 40 px robot sits exactly in the badge's square. With the
robot off every width is as before (`shape_with_robot(.., false)`).

Blink only shows on open eyes (Idle, Focus, Alert, Think, Sad, Typing); the
design's `blink` eyes (two level lines) replace the open eyes. The ears
carry the mood (`Face::ears`): up 4 for Alert, turned outward 12 degrees
asleep and 24 degrees when low. The ear tips stay
inside every compact island shape (`robot_and_content_share_every_compact_shape`).
The painter builds each path once, on first use.

**v3.8: coats and accessories.** The cat is no longer always terracotta: four
coats (Terracotta, Calico, White, Black) and seven accessories, chosen in
Settings → Kucing, drawn by the same painter with a `CatLook`. The paint
order, calico patches and rejected mockups are in [[cat-accessories]], the
colours and the island surface that follows them in
[[cat-colours-and-surfaces]]. The faces, the ear moods and the pointer rules
above are unchanged; the logo, tray and the notch-peek eyes stay terracotta.
The label "Asisten di notch" is now "Kucing di notch".

Related: [[bata-idle-power-budget]], [[bata-morning-greeting]],
[[studio-bata-logo-sources]], [[cat-accessories]], [[cat-colours-and-surfaces]],
[[cat-types-while-working]]

Part of [[v3-studio-overview]]. Listed in [[index]].

**2026-11-13: three mascots.** The same faces and `choose_expr` drive the
falk (cockatiel; its crest carries the ears' moods) and the sitting octopus
(its arms do), chosen in Settings → Maskot: [[maskot-falk-gurita]].
