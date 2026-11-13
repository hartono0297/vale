//! What the island's tooltips say: the closed notch (the notice's ✕, the REC
//! view's level groups) and the open panel (the page dock, the Settings
//! button, the icon-only controls of every page). Pure; `App` feeds the hit under the pointer and where the last
//! frame drew it (`Renderer::region_of`).

use crate::hotkey::HotkeyId;
use crate::i18n::{self, Key};
use crate::model::{CalView, Hit, Page};
use crate::recorder::Source;
use crate::ui::tooltip::{KeyHint, Side, Tip};

/// The app state a tip reads, built only while a hit with a tip is under the
/// pointer (`has_tip`).
#[derive(Clone, Debug, PartialEq)]
pub struct TipCtx {
    /// The running recording's mic, and its PC audio, are muted.
    pub rec_muted_mic: bool,
    pub rec_muted_system: bool,
    /// The running recording is paused.
    pub rec_paused: bool,
    /// The Rekam page shows its Layar tab.
    pub screen_tab: bool,
    /// The calendar's view: what Prev and Next move by.
    pub cal_view: CalView,
    /// The pomodoro's timer runs, or is paused part way.
    pub pomo_running: bool,
    pub pomo_paused: bool,
    /// The hovered note card's note is stuck on the desktop.
    pub note_on_desktop: bool,
    /// The song is playing.
    pub playing: bool,
    /// The floating lyrics card is open.
    pub lyrics_card_open: bool,
    /// The hovered Mixer row: the name the page shows and whether it is muted.
    pub mixer: Option<(String, bool)>,
    /// The hovered Clipboard card is pinned.
    pub clip_pinned: bool,
    /// The CLI page is enlarged.
    pub cli_tall: bool,
    /// A recording runs: the Rekam page's round button is stop.
    pub recording: bool,
    /// The Rekam page's trash was clicked once: "Buang video ini?" asks, and
    /// `Hit::ScreenDiscard` is its labelled "Buang".
    pub screen_discard_armed: bool,
}

impl Default for TipCtx {
    fn default() -> Self {
        TipCtx {
            rec_muted_mic: false,
            rec_muted_system: false,
            rec_paused: false,
            screen_tab: false,
            cal_view: CalView::Month,
            pomo_running: false,
            pomo_paused: false,
            note_on_desktop: false,
            playing: false,
            lyrics_card_open: false,
            mixer: None,
            clip_pinned: false,
            cli_tall: false,
            recording: false,
            screen_discard_armed: false,
        }
    }
}

/// Whether `hit` may have a tip: the gate before `TipCtx` is built, so a move
/// over a hit without one reads no state. Lists every hit `tip_for` answers.
pub fn has_tip(hit: &Hit) -> bool {
    matches!(
        hit,
        Hit::NoticeClose
            | Hit::RecMuteCompact(_)
            | Hit::PanelPage(_)
            | Hit::OpenSettings
            | Hit::CalPrev
            | Hit::CalNext
            | Hit::PomoStartPause
            | Hit::PomoReset
            | Hit::PomoSkip
            | Hit::NoteSticky(_)
            | Hit::Prev
            | Hit::PlayPause
            | Hit::Next
            | Hit::LyricsCard
            | Hit::MixerMute(_)
            | Hit::ClipPin(_)
            | Hit::ClipPlain(_)
            | Hit::ClipDelete(_)
            | Hit::NotifRemove(_)
            | Hit::ShelfRemove(_)
            | Hit::PhoneBack
            | Hit::PhoneShowQr
            | Hit::ArrangePlay(_)
            | Hit::RecPause
            | Hit::RecMute(_)
            | Hit::RecToggle
            | Hit::RecPickFile
            | Hit::RecFolder
            | Hit::ScreenDiscard
            | Hit::DeviceOpen(_)
            | Hit::DeviceDetail(_)
            | Hit::CliExpand
            | Hit::CliFolderStar { .. }
            | Hit::SearchBack
            | Hit::CleanBack
    )
}

/// What a pointer move gives the tooltip.
#[derive(Debug, PartialEq)]
pub enum Feed {
    /// This hit's tip (it has one, by `has_tip`).
    Tip(Hit),
    /// Nothing with a tip: the tip goes, its wait stops.
    Nothing,
    /// Not told anything: a button is held (a drag, a slider), the frame is in
    /// motion (its regions are empty or about to move), or the pointer is
    /// still on the target it pressed, which stays quiet until it is left.
    LeaveAlone,
}

/// What the move over `hit` gives the tooltip. `held`: a button is down;
/// `in_flux`: the island or the panel's page is moving; `pressed`: the hit a
/// press was on, forgotten once a settled move finds the pointer elsewhere.
pub fn feed(hit: Option<Hit>, held: bool, in_flux: bool, pressed: &mut Option<Hit>) -> Feed {
    if held || in_flux {
        return Feed::LeaveAlone;
    }
    if let (Some(hit), Some(was)) = (&hit, pressed.as_ref())
        && same_control(hit, was)
    {
        return Feed::LeaveAlone;
    }
    *pressed = None;
    match hit {
        Some(hit) if has_tip(&hit) => Feed::Tip(hit),
        _ => Feed::Nothing,
    }
}

/// The tip of `hit`; `None` for a hit with none (the body, the busy toast's
/// logo, a compact view).
pub fn tip_for(hit: &Hit, ctx: &TipCtx) -> Option<Tip> {
    Some(match hit {
        Hit::NoticeClose => Tip::named(Key::TipNoticeClose).about(Key::TipNoticeCloseAbout),
        // The name says what a click does now. Only the mic has a hotkey. The
        // Rekam page's mute buttons say the same as the notch's level groups.
        Hit::RecMuteCompact(Source::Mic) | Hit::RecMute(Source::Mic) => {
            Tip::named(if ctx.rec_muted_mic {
                Key::TrayMicUnmute
            } else {
                Key::TrayMicMute
            })
            .key(KeyHint::Hotkey(HotkeyId::RecordMute))
        }
        Hit::RecMuteCompact(Source::System) | Hit::RecMute(Source::System) => {
            Tip::named(if ctx.rec_muted_system {
                Key::TipPcAudioUnmute
            } else {
                Key::TipPcAudioMute
            })
        }
        Hit::PanelPage(page) => page_tip(*page),
        Hit::OpenSettings => Tip::named(Key::TipSettings).about(Key::TipSettingsAbout),

        Hit::CalPrev | Hit::CalNext => {
            let prev = *hit == Hit::CalPrev;
            Tip::named(match (ctx.cal_view, prev) {
                (CalView::Month, true) => Key::TipCalPrevMonth,
                (CalView::Month, false) => Key::TipCalNextMonth,
                (CalView::Week, true) => Key::TipCalPrevWeek,
                (CalView::Week, false) => Key::TipCalNextWeek,
                (CalView::Day, true) => Key::TipCalPrevDay,
                (CalView::Day, false) => Key::TipCalNextDay,
            })
        }
        Hit::PomoStartPause => Tip::named(if ctx.pomo_running {
            Key::TipPause
        } else if ctx.pomo_paused {
            Key::TipPomoResume
        } else {
            Key::Start
        }),
        // `reset()` goes back to the first focus session, not to this phase's
        // start; `skip()` ends the phase and waits for a start.
        Hit::PomoReset => Tip::named(Key::TipPomoReset).about(Key::TipPomoResetAbout),
        Hit::PomoSkip => Tip::named(Key::TipPomoSkip).about(Key::TipPomoSkipAbout),
        Hit::NoteSticky(_) if ctx.note_on_desktop => {
            Tip::named(Key::StickyOffDesktopTip).about(Key::TipNoteUnstickAbout)
        }
        Hit::NoteSticky(_) => Tip::named(Key::StickyOnDesktopTip).about(Key::TipNoteStickAbout),

        Hit::Prev => Tip::named(Key::TipMediaPrev).about(Key::TipMediaPrevNextAbout),
        Hit::Next => Tip::named(Key::TipMediaNext).about(Key::TipMediaPrevNextAbout),
        Hit::PlayPause => Tip::named(if ctx.playing {
            Key::TipPause
        } else {
            Key::TipPlay
        })
        .about(Key::TipMediaPlayPauseAbout),
        Hit::LyricsCard => Tip::named(if ctx.lyrics_card_open {
            Key::TipLyricsCardClose
        } else {
            Key::TipLyricsCard
        })
        .about(Key::TipLyricsCardAbout)
        .key(KeyHint::Hotkey(HotkeyId::LyricsCard)),
        Hit::MixerMute(_) => {
            let (name, muted) = ctx.mixer.clone().unwrap_or_default();
            Tip::name_str(i18n::mute_app(i18n::lang(), &name, muted))
        }

        // Pinning keeps a card through "Hapus riwayat", the lock clear and the
        // item cap (`clear_unpinned`, `oldest_unpinned`).
        Hit::ClipPin(_) if ctx.clip_pinned => {
            Tip::named(Key::TipClipUnpin).about(Key::TipClipUnpinAbout)
        }
        Hit::ClipPin(_) => Tip::named(Key::TipClipPin).about(Key::TipClipPinAbout),
        Hit::ClipPlain(_) => Tip::named(Key::TipClipPlain).about(Key::TipClipPlainAbout),
        Hit::ClipDelete(_) => Tip::named(Key::Delete).about(Key::TipClipDeleteAbout),
        Hit::NotifRemove(_) => Tip::named(Key::TipRemove).about(Key::TipNotifRemoveAbout),
        Hit::ShelfRemove(_) => Tip::named(Key::TipShelfRemove).about(Key::TipShelfRemoveAbout),

        // Esc closes the search; the phone link and the cleaner have no key.
        Hit::SearchBack => Tip::named(Key::TipBack).key(KeyHint::Raw("Esc")),
        Hit::PhoneBack => Tip::named(Key::TipBack),
        Hit::PhoneShowQr => Tip::named(Key::TipPhoneShowQr).about(Key::TipPhoneShowQrAbout),
        Hit::CleanBack => Tip::named(Key::TipCleanBack),
        Hit::ArrangePlay(_) => Tip::named(Key::TipArrangePlay).about(Key::TipArrangePlayAbout),

        Hit::RecPause => Tip::named(if ctx.rec_paused {
            Key::TipRecResume
        } else {
            Key::TipRecPause
        }),
        // The round button acts on the tab's kind, as its hotkey does. Idle
        // it shows "Rekam" (but under the waiting strip): only what it
        // starts. Recording, it is the stop square, with no label.
        Hit::RecToggle => {
            let tip = if ctx.recording {
                Tip::named(Key::TipRecStop).about(Key::TipRecStopAbout)
            } else if ctx.screen_tab {
                Tip::short(Key::TipRecStartScreenAbout)
            } else {
                Tip::short(Key::TipRecStartAudioAbout)
            };
            tip.key(KeyHint::Hotkey(if ctx.screen_tab {
                HotkeyId::RecordScreen
            } else {
                HotkeyId::Record
            }))
        }
        // The header's two icons. The picker lists recordings and videos on
        // either tab (`RECORDING_FILTER`); the folder is the tab's.
        Hit::RecPickFile => Tip::named(Key::TipRecPickFile).about(Key::TipRecPickFileAbout),
        Hit::RecFolder => Tip::named(Key::RecOpenFolder).about(if ctx.screen_tab {
            Key::TipRecFolderScreenAbout
        } else {
            Key::TipRecFolderAudioAbout
        }),
        // The trash beside a running screen recording asks first; once it
        // asks, the hit is the labelled "Buang" under the question.
        Hit::ScreenDiscard if ctx.screen_discard_armed => return None,
        Hit::ScreenDiscard => Tip::named(Key::TipScreenDiscard).about(Key::TipScreenDiscardAbout),

        Hit::DeviceOpen(_) => Tip::named(Key::TipDeviceOpen),
        Hit::DeviceDetail(_) => Tip::named(Key::TipDeviceDetail).about(Key::TipDeviceDetailAbout),
        Hit::CliExpand => Tip::named(if ctx.cli_tall {
            Key::TipCliShrink
        } else {
            Key::TipCliEnlarge
        }),
        Hit::CliFolderStar { on: true, .. } => Tip::named(Key::TipCliStar),
        Hit::CliFolderStar { on: false, .. } => Tip::named(Key::TipCliUnstar),
        _ => return None,
    })
}

/// A dock icon's tip: the page's name, what it holds and the hotkey that
/// opens it, where it has one.
fn page_tip(page: Page) -> Tip {
    let (name, about, hotkey) = match page {
        Page::Calendar => (Key::PageCalendar, Key::TipPageCalendar, None),
        Page::Pomodoro => (Key::PagePomodoro, Key::TipPagePomodoro, None),
        Page::Record => (Key::PageRecord, Key::TipPageRecord, None),
        Page::Reminders => (Key::PageReminders, Key::TipPageReminders, None),
        Page::Notes => (Key::PageNotes, Key::TipPageNotes, None),
        Page::Media => (Key::PageMedia, Key::TipPageMedia, None),
        Page::Mixer => (Key::PageMixer, Key::TipPageMixer, None),
        Page::Clipboard => (
            Key::PageClipboard,
            Key::TipPageClipboard,
            Some(HotkeyId::Clipboard),
        ),
        Page::Notifications => (Key::PageNotifications, Key::TipPageNotifications, None),
        Page::Shelf => (Key::PageShelf, Key::TipPageShelf, None),
        Page::System => (Key::PageSystem, Key::TipPageSystem, None),
        Page::Devices => (
            Key::PageDevices,
            Key::TipPageDevices,
            Some(HotkeyId::Devices),
        ),
        Page::Cli => (Key::PageCli, Key::TipPageCli, None),
        Page::Arrange => (
            Key::PageArrange,
            Key::TipPageArrange,
            Some(HotkeyId::Arrange),
        ),
        Page::Shortcuts => (Key::PageShortcuts, Key::TipPageShortcuts, None),
        // Never in the dock (not one of `Page::ALL`, and its layout draws no
        // dots); named for completeness, with the only way to open it.
        Page::Search => {
            return Tip::named(Key::SearchTitle).key(KeyHint::Hotkey(HotkeyId::Search));
        }
    };
    let tip = Tip::named(name).about(about);
    match hotkey {
        Some(id) => tip.key(KeyHint::Hotkey(id)),
        None => tip,
    }
}

/// Where the tip of `hit` goes: below for the closed notch, which sits at the
/// top edge of the screen.
pub fn side_for(hit: &Hit) -> Side {
    match hit {
        Hit::NoticeClose | Hit::RecMuteCompact(_) => Side::Below,
        _ => Side::Auto,
    }
}

/// A stable id for the control `hit` is on (`Hit` holds strings and paths and
/// is not `Hash`). It never carries the state: a click that flips play into
/// pause or stars a folder is on the same control, which stays quiet until
/// the pointer leaves it; a flip while its tip is up repaints the tip.
pub fn tip_id(hit: &Hit) -> u64 {
    match hit {
        Hit::CliFolderStar { path, .. } => crate::ui::tooltip::id_of(&("CliFolderStar", path)),
        _ => crate::ui::tooltip::id_of(&format!("{hit:?}")),
    }
}

/// `a` and `b` are the same control, in whatever state (`tip_id`).
fn same_control(a: &Hit, b: &Hit) -> bool {
    match (a, b) {
        (Hit::CliFolderStar { path: x, .. }, Hit::CliFolderStar { path: y, .. }) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang, tr};

    const LANGS: [Lang; 2] = [Lang::Id, Lang::En];

    fn ctx(muted: bool) -> TipCtx {
        TipCtx {
            rec_muted_mic: muted,
            rec_muted_system: muted,
            ..TipCtx::default()
        }
    }

    /// Every state a tip reads, off and on, so a tip is built both ways.
    fn ctxs() -> Vec<TipCtx> {
        let mut all = Vec::new();
        for on in [false, true] {
            for view in [CalView::Day, CalView::Week, CalView::Month] {
                all.push(TipCtx {
                    rec_muted_mic: on,
                    rec_muted_system: on,
                    rec_paused: on,
                    screen_tab: on,
                    cal_view: view,
                    pomo_running: on,
                    pomo_paused: !on,
                    note_on_desktop: on,
                    playing: on,
                    lyrics_card_open: on,
                    mixer: Some(("Spotify".to_string(), on)),
                    clip_pinned: on,
                    cli_tall: on,
                    recording: on,
                    screen_discard_armed: on,
                });
            }
        }
        all
    }

    /// Every hit this panel gives a tip beyond the dock, the Settings button
    /// and the closed notch.
    fn page_hits() -> Vec<Hit> {
        let path = std::path::PathBuf::from("C:\\work");
        vec![
            Hit::CalPrev,
            Hit::CalNext,
            Hit::PomoStartPause,
            Hit::PomoReset,
            Hit::PomoSkip,
            Hit::NoteSticky("n1".into()),
            Hit::Prev,
            Hit::PlayPause,
            Hit::Next,
            Hit::LyricsCard,
            Hit::MixerMute(0),
            Hit::MixerMute(3),
            Hit::ClipPin(3),
            Hit::ClipPlain(3),
            Hit::ClipDelete(3),
            Hit::NotifRemove(7),
            Hit::ShelfRemove(1),
            Hit::PhoneBack,
            Hit::PhoneShowQr,
            Hit::ArrangePlay(2),
            Hit::RecPause,
            Hit::RecMute(Source::Mic),
            Hit::RecMute(Source::System),
            Hit::RecToggle,
            Hit::RecPickFile,
            Hit::RecFolder,
            Hit::ScreenDiscard,
            Hit::DeviceOpen(0),
            Hit::DeviceDetail(0),
            Hit::CliExpand,
            Hit::CliFolderStar {
                path: path.clone(),
                on: true,
            },
            Hit::CliFolderStar { path, on: false },
            Hit::SearchBack,
            Hit::CleanBack,
        ]
    }

    fn every_tip() -> Vec<Tip> {
        let mut all = Vec::new();
        for c in ctxs() {
            let hits = Page::ALL
                .into_iter()
                .chain([Page::Search])
                .map(Hit::PanelPage)
                .chain([
                    Hit::NoticeClose,
                    Hit::OpenSettings,
                    Hit::RecMuteCompact(Source::Mic),
                    Hit::RecMuteCompact(Source::System),
                ])
                .chain(page_hits());
            all.extend(hits.filter_map(|h| tip_for(&h, &c)));
        }
        all
    }

    fn name_of(hit: &Hit, c: &TipCtx) -> String {
        tip_for(hit, c).unwrap().name.unwrap_or_default()
    }

    #[test]
    fn every_page_has_a_name_and_an_about_in_both_languages() {
        for lang in LANGS {
            i18n::set_lang(lang);
            for p in Page::ALL {
                let tip = tip_for(&Hit::PanelPage(p), &TipCtx::default());
                let tip = tip.unwrap_or_else(|| panic!("{lang:?} {p:?}: no tip"));
                assert!(tip.name.is_some_and(|n| !n.is_empty()), "{lang:?} {p:?}");
                assert!(tip.about.is_some_and(|a| !a.is_empty()), "{lang:?} {p:?}");
            }
        }
    }

    #[test]
    fn a_page_is_named_as_the_dock_names_it() {
        for lang in LANGS {
            i18n::set_lang(lang);
            let tip = tip_for(&Hit::PanelPage(Page::Media), &TipCtx::default()).unwrap();
            assert_eq!(tip.name.as_deref(), Some(tr(Key::PageMedia)), "{lang:?}");
        }
    }

    #[test]
    fn the_pages_with_a_hotkey_show_it() {
        let keys = |p| {
            tip_for(&Hit::PanelPage(p), &TipCtx::default())
                .unwrap()
                .keys
        };
        assert_eq!(
            keys(Page::Clipboard),
            [KeyHint::Hotkey(HotkeyId::Clipboard)]
        );
        assert_eq!(keys(Page::Devices), [KeyHint::Hotkey(HotkeyId::Devices)]);
        assert_eq!(keys(Page::Arrange), [KeyHint::Hotkey(HotkeyId::Arrange)]);
        assert_eq!(keys(Page::Search), [KeyHint::Hotkey(HotkeyId::Search)]);
        assert!(keys(Page::Media).is_empty());
        assert!(keys(Page::Calendar).is_empty());
    }

    #[test]
    fn the_mic_group_names_what_a_click_does_now() {
        let mic = Hit::RecMuteCompact(Source::Mic);
        let off = tip_for(&mic, &ctx(false)).unwrap();
        assert_eq!(off.name.as_deref(), Some(tr(Key::TrayMicMute)));
        assert_eq!(off.keys, [KeyHint::Hotkey(HotkeyId::RecordMute)]);
        let on = tip_for(&mic, &ctx(true)).unwrap();
        assert_eq!(on.name.as_deref(), Some(tr(Key::TrayMicUnmute)));
        assert_eq!(on.keys, [KeyHint::Hotkey(HotkeyId::RecordMute)]);
    }

    #[test]
    fn the_pc_audio_group_has_its_own_names_and_no_key() {
        let system = Hit::RecMuteCompact(Source::System);
        let off = tip_for(&system, &ctx(false)).unwrap();
        let on = tip_for(&system, &ctx(true)).unwrap();
        assert_eq!(off.name.as_deref(), Some(tr(Key::TipPcAudioMute)));
        assert_eq!(on.name.as_deref(), Some(tr(Key::TipPcAudioUnmute)));
        assert!(off.keys.is_empty() && on.keys.is_empty());
        // Each group reads its own source's state.
        let mixed = TipCtx {
            rec_muted_mic: true,
            rec_muted_system: false,
            ..TipCtx::default()
        };
        assert_eq!(tip_for(&system, &mixed).unwrap().name, off.name);
    }

    #[test]
    fn the_notice_close_and_settings_have_a_name_and_an_about() {
        for hit in [Hit::NoticeClose, Hit::OpenSettings] {
            let tip = tip_for(&hit, &TipCtx::default()).unwrap();
            assert!(tip.name.is_some() && tip.about.is_some(), "{hit:?}");
            assert!(tip.keys.is_empty(), "{hit:?}");
        }
    }

    #[test]
    fn the_closed_notch_tips_go_below() {
        assert_eq!(side_for(&Hit::NoticeClose), Side::Below);
        assert_eq!(side_for(&Hit::RecMuteCompact(Source::Mic)), Side::Below);
        assert_eq!(side_for(&Hit::RecMuteCompact(Source::System)), Side::Below);
        assert_eq!(side_for(&Hit::PanelPage(Page::Media)), Side::Auto);
        assert_eq!(side_for(&Hit::OpenSettings), Side::Auto);
    }

    #[test]
    fn the_body_and_the_rest_have_no_tip_yet() {
        let c = TipCtx::default();
        for hit in [
            Hit::Body,
            Hit::ToastSlot,
            Hit::Strip,
            Hit::CalAdd,
            Hit::CalViewSel(CalView::Day),
            Hit::ClipCard(3),
            Hit::MixerSlider(0),
        ] {
            assert!(tip_for(&hit, &c).is_none(), "{hit:?}");
        }
    }

    #[test]
    fn no_tip_text_has_an_em_dash() {
        for lang in LANGS {
            i18n::set_lang(lang);
            for tip in every_tip() {
                for s in tip.name.iter().chain(tip.about.iter()) {
                    assert!(!s.contains('\u{2014}'), "{lang:?}: {s}");
                    assert!(!s.contains(" \u{2013} "), "{lang:?}: {s}");
                    assert!(!s.is_empty(), "{lang:?}");
                }
            }
        }
    }

    #[test]
    fn the_id_tells_hits_apart_and_is_stable() {
        let a = Hit::PanelPage(Page::Media);
        assert_eq!(tip_id(&a), tip_id(&a.clone()));
        assert_ne!(tip_id(&a), tip_id(&Hit::PanelPage(Page::Mixer)));
        assert_ne!(
            tip_id(&Hit::RecMuteCompact(Source::Mic)),
            tip_id(&Hit::RecMuteCompact(Source::System))
        );
        let star = |p: &str| Hit::CliFolderStar {
            path: p.into(),
            on: true,
        };
        assert_ne!(tip_id(&star("C:\\a")), tip_id(&star("C:\\b")));
        assert_ne!(tip_id(&star("C:\\a")), tip_id(&Hit::CliExpand));
    }

    #[test]
    fn a_flipped_state_keeps_the_id_and_changes_the_tip() {
        // A click flips these: the tip must stay away under the pointer that
        // clicked, and a live flip repaints the tip that is up.
        let off = TipCtx::default();
        let flips: [(Hit, TipCtx); 11] = [
            (
                Hit::PlayPause,
                TipCtx {
                    playing: true,
                    ..off.clone()
                },
            ),
            (
                Hit::LyricsCard,
                TipCtx {
                    lyrics_card_open: true,
                    ..off.clone()
                },
            ),
            (
                Hit::ClipPin(3),
                TipCtx {
                    clip_pinned: true,
                    ..off.clone()
                },
            ),
            (
                Hit::CliExpand,
                TipCtx {
                    cli_tall: true,
                    ..off.clone()
                },
            ),
            (
                Hit::RecPause,
                TipCtx {
                    rec_paused: true,
                    ..off.clone()
                },
            ),
            (
                Hit::NoteSticky("n".into()),
                TipCtx {
                    note_on_desktop: true,
                    ..off.clone()
                },
            ),
            (
                Hit::CalPrev,
                TipCtx {
                    cal_view: CalView::Day,
                    ..off.clone()
                },
            ),
            (
                Hit::RecMute(Source::Mic),
                TipCtx {
                    rec_muted_mic: true,
                    ..off.clone()
                },
            ),
            (
                Hit::PomoStartPause,
                TipCtx {
                    pomo_running: true,
                    ..off.clone()
                },
            ),
            (
                Hit::RecToggle,
                TipCtx {
                    recording: true,
                    ..off.clone()
                },
            ),
            (
                Hit::MixerMute(1),
                TipCtx {
                    mixer: Some((String::new(), true)),
                    ..off.clone()
                },
            ),
        ];
        for (hit, on) in flips {
            assert_ne!(tip_for(&hit, &off), tip_for(&hit, &on), "{hit:?}");
            assert_eq!(tip_id(&hit), tip_id(&hit.clone()), "{hit:?}");
        }
        // The CLI star carries its state in the hit itself.
        let star = |on| Hit::CliFolderStar {
            path: "C:\\work".into(),
            on,
        };
        assert_ne!(tip_for(&star(true), &off), tip_for(&star(false), &off));
        assert_eq!(tip_id(&star(true)), tip_id(&star(false)));
    }

    #[test]
    fn a_click_that_flips_the_cli_star_stays_quiet_until_left() {
        let star = |on| Hit::CliFolderStar {
            path: "C:\\work".into(),
            on,
        };
        let mut pressed = Some(star(true));
        // The click starred it: the same star, now "Unstar".
        assert_eq!(
            feed(Some(star(false)), false, false, &mut pressed),
            Feed::LeaveAlone
        );
        assert_eq!(pressed, Some(star(true)));
        // Another folder's star is another control.
        let other = Hit::CliFolderStar {
            path: "C:\\other".into(),
            on: false,
        };
        assert_eq!(
            feed(Some(other.clone()), false, false, &mut pressed),
            Feed::Tip(other)
        );
        assert_eq!(pressed, None);
    }

    /// Every hit with a tip and every neighbour without one.
    fn sample_hits() -> Vec<Hit> {
        Page::ALL
            .into_iter()
            .chain([Page::Search])
            .map(Hit::PanelPage)
            .chain([
                Hit::NoticeClose,
                Hit::OpenSettings,
                Hit::RecMuteCompact(Source::Mic),
                Hit::RecMuteCompact(Source::System),
                Hit::Body,
                Hit::ToastSlot,
                Hit::Strip,
                Hit::CalAdd,
                Hit::CalViewSel(CalView::Week),
                Hit::PomoAdjust(crate::model::PomoField::Focus, 1),
                Hit::ClipCard(3),
                Hit::ClipSearch,
                Hit::MixerSlider(0),
                Hit::NotifRow(7),
                Hit::NotifOpen(7),
                Hit::ShelfCard(1),
                Hit::ShelfClear,
                Hit::DeviceEject(0),
                Hit::CliInput,
                Hit::CliStop,
                Hit::CleanGo,
            ])
            .chain(page_hits())
            .chain(rekam_hits())
            .collect()
    }

    /// Every hit of the Rekam page (and the "Rekaman tersimpan" notice's
    /// chips), idle and recording, with a tip or not.
    fn rekam_hits() -> Vec<Hit> {
        use crate::model::{RecKind, RecSources};
        let both = RecSources {
            mic: true,
            system: true,
        };
        vec![
            Hit::RecTab(RecKind::Audio),
            Hit::RecTab(RecKind::Screen),
            Hit::RecPickFile,
            Hit::RecFolder,
            Hit::RecToggle,
            Hit::RecPause,
            Hit::ScreenDiscard,
            Hit::ScreenDiscardKeep,
            Hit::RecMute(Source::Mic),
            Hit::RecMute(Source::System),
            Hit::RecDevice(Source::Mic),
            Hit::RecDevice(Source::System),
            Hit::RecSources(both),
            Hit::RecQuality,
            Hit::RecCaptions,
            Hit::RecOpen(0),
            Hit::RecTranscript(0),
            Hit::RecTranscribeChip,
            Hit::RecTranscriptSetup,
        ]
    }

    #[test]
    fn the_gate_lets_through_exactly_the_hits_with_a_tip() {
        let all: Vec<TipCtx> = ctxs().into_iter().chain([TipCtx::default()]).collect();
        for hit in sample_hits() {
            // No tip passes the gate's back; a hit through it has one in
            // some state (the armed discard's "Buang" shows its own label).
            let some = all.iter().filter(|c| tip_for(&hit, c).is_some()).count();
            assert!(some == 0 || has_tip(&hit), "{hit:?} has a tip the gate stops");
            assert_eq!(has_tip(&hit), some > 0, "{hit:?}");
        }
    }

    #[test]
    fn the_rekam_icon_buttons_have_tips_and_its_labelled_controls_none() {
        let icons = [
            Hit::RecPickFile,
            Hit::RecFolder,
            Hit::RecToggle,
            Hit::RecPause,
            Hit::ScreenDiscard,
            Hit::RecMute(Source::Mic),
            Hit::RecMute(Source::System),
        ];
        for hit in rekam_hits() {
            assert_eq!(has_tip(&hit), icons.contains(&hit), "{hit:?}");
        }
        for lang in LANGS {
            i18n::set_lang(lang);
            for c in ctxs() {
                for hit in &icons {
                    let Some(tip) = tip_for(hit, &c) else {
                        assert!(*hit == Hit::ScreenDiscard && c.screen_discard_armed);
                        continue;
                    };
                    let said: Vec<&String> = tip.name.iter().chain(tip.about.iter()).collect();
                    assert!(!said.is_empty(), "{lang:?} {hit:?}");
                    for s in said {
                        assert!(!s.is_empty() && !s.contains('\u{2014}'), "{lang:?} {s}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_rekam_header_icons_say_what_they_open_for_the_tab() {
        let audio = TipCtx::default();
        let screen = TipCtx {
            screen_tab: true,
            ..TipCtx::default()
        };
        let pick = tip_for(&Hit::RecPickFile, &audio).unwrap();
        assert_eq!(pick.name.as_deref(), Some(tr(Key::TipRecPickFile)));
        assert_eq!(pick.about.as_deref(), Some(tr(Key::TipRecPickFileAbout)));
        // The picker lists recordings and videos on both tabs.
        assert_eq!(tip_for(&Hit::RecPickFile, &screen).unwrap(), pick);
        let folder = tip_for(&Hit::RecFolder, &audio).unwrap();
        assert_eq!(folder.name.as_deref(), Some(tr(Key::RecOpenFolder)));
        assert_eq!(folder.about.as_deref(), Some(tr(Key::TipRecFolderAudioAbout)));
        let videos = tip_for(&Hit::RecFolder, &screen).unwrap();
        assert_eq!(videos.about.as_deref(), Some(tr(Key::TipRecFolderScreenAbout)));
    }

    #[test]
    fn the_trash_asks_first_and_its_question_has_no_tip() {
        let live = TipCtx::default();
        let trash = tip_for(&Hit::ScreenDiscard, &live).unwrap();
        assert_eq!(trash.name.as_deref(), Some(tr(Key::TipScreenDiscard)));
        assert_eq!(trash.about.as_deref(), Some(tr(Key::TipScreenDiscardAbout)));
        let armed = TipCtx {
            screen_discard_armed: true,
            ..TipCtx::default()
        };
        assert!(tip_for(&Hit::ScreenDiscard, &armed).is_none());
    }

    #[test]
    fn every_page_control_has_a_name_or_an_about_in_both_languages() {
        for lang in LANGS {
            i18n::set_lang(lang);
            for c in ctxs() {
                for hit in page_hits() {
                    // The armed discard's "Buang" says what it does itself.
                    if hit == Hit::ScreenDiscard && c.screen_discard_armed {
                        continue;
                    }
                    let tip = tip_for(&hit, &c).unwrap_or_else(|| panic!("{hit:?}: no tip"));
                    let said = tip.name.iter().chain(tip.about.iter());
                    assert!(said.clone().count() > 0, "{lang:?} {hit:?}");
                    assert!(said.into_iter().all(|s| !s.is_empty()), "{lang:?} {hit:?}");
                }
            }
        }
    }

    #[test]
    fn the_calendar_arrows_follow_the_view() {
        let of = |view, hit: &Hit| {
            name_of(
                hit,
                &TipCtx {
                    cal_view: view,
                    ..TipCtx::default()
                },
            )
        };
        for lang in LANGS {
            i18n::set_lang(lang);
            let (prev, next) = (Hit::CalPrev, Hit::CalNext);
            let table = [
                (CalView::Month, Key::TipCalPrevMonth, Key::TipCalNextMonth),
                (CalView::Week, Key::TipCalPrevWeek, Key::TipCalNextWeek),
                (CalView::Day, Key::TipCalPrevDay, Key::TipCalNextDay),
            ];
            for (view, p, n) in table {
                assert_eq!(of(view, &prev), tr(p), "{lang:?} {view:?}");
                assert_eq!(of(view, &next), tr(n), "{lang:?} {view:?}");
            }
            assert_ne!(of(CalView::Month, &prev), of(CalView::Day, &prev));
        }
    }

    #[test]
    fn the_pomodoro_button_follows_the_timer() {
        let hit = Hit::PomoStartPause;
        let at = |running, paused| {
            name_of(
                &hit,
                &TipCtx {
                    pomo_running: running,
                    pomo_paused: paused,
                    ..TipCtx::default()
                },
            )
        };
        for lang in LANGS {
            i18n::set_lang(lang);
            assert_eq!(at(false, false), tr(Key::Start), "{lang:?}");
            assert_eq!(at(true, false), tr(Key::TipPause), "{lang:?}");
            assert_eq!(at(false, true), tr(Key::TipPomoResume), "{lang:?}");
        }
    }

    #[test]
    fn the_pomodoro_reset_and_skip_say_what_the_handlers_do() {
        let c = TipCtx::default();
        let reset = tip_for(&Hit::PomoReset, &c).unwrap();
        assert_eq!(reset.name.as_deref(), Some(tr(Key::TipPomoReset)));
        assert_eq!(reset.about.as_deref(), Some(tr(Key::TipPomoResetAbout)));
        let skip = tip_for(&Hit::PomoSkip, &c).unwrap();
        assert_eq!(skip.name.as_deref(), Some(tr(Key::TipPomoSkip)));
        assert_eq!(skip.about.as_deref(), Some(tr(Key::TipPomoSkipAbout)));
    }

    #[test]
    fn the_note_pin_follows_the_desktop() {
        let hit = Hit::NoteSticky("n1".into());
        let off = tip_for(&hit, &TipCtx::default()).unwrap();
        assert_eq!(off.name.as_deref(), Some(tr(Key::StickyOnDesktopTip)));
        assert_eq!(off.about.as_deref(), Some(tr(Key::TipNoteStickAbout)));
        let on = TipCtx {
            note_on_desktop: true,
            ..TipCtx::default()
        };
        let on = tip_for(&hit, &on).unwrap();
        assert_eq!(on.name.as_deref(), Some(tr(Key::StickyOffDesktopTip)));
        assert_eq!(on.about.as_deref(), Some(tr(Key::TipNoteUnstickAbout)));
    }

    #[test]
    fn the_music_buttons_name_what_a_click_does() {
        let c = TipCtx::default();
        let prev = tip_for(&Hit::Prev, &c).unwrap();
        assert_eq!(prev.name.as_deref(), Some(tr(Key::TipMediaPrev)));
        assert_eq!(prev.about.as_deref(), Some(tr(Key::TipMediaPrevNextAbout)));
        let next = tip_for(&Hit::Next, &c).unwrap();
        assert_eq!(next.name.as_deref(), Some(tr(Key::TipMediaNext)));
        let paused = tip_for(&Hit::PlayPause, &c).unwrap();
        assert_eq!(paused.name.as_deref(), Some(tr(Key::TipPlay)));
        assert_eq!(
            paused.about.as_deref(),
            Some(tr(Key::TipMediaPlayPauseAbout))
        );
        let playing = TipCtx {
            playing: true,
            ..TipCtx::default()
        };
        let playing = tip_for(&Hit::PlayPause, &playing).unwrap();
        assert_eq!(playing.name.as_deref(), Some(tr(Key::TipPause)));
    }

    #[test]
    fn the_lyrics_card_button_follows_the_card_and_shows_its_hotkey() {
        let closed = tip_for(&Hit::LyricsCard, &TipCtx::default()).unwrap();
        assert_eq!(closed.name.as_deref(), Some(tr(Key::TipLyricsCard)));
        assert_eq!(closed.keys, [KeyHint::Hotkey(HotkeyId::LyricsCard)]);
        let open = TipCtx {
            lyrics_card_open: true,
            ..TipCtx::default()
        };
        let open = tip_for(&Hit::LyricsCard, &open).unwrap();
        assert_eq!(open.name.as_deref(), Some(tr(Key::TipLyricsCardClose)));
        assert_eq!(open.keys, [KeyHint::Hotkey(HotkeyId::LyricsCard)]);
    }

    #[test]
    fn a_mixer_speaker_names_its_app_and_what_a_click_does() {
        for lang in LANGS {
            i18n::set_lang(lang);
            for muted in [false, true] {
                let c = TipCtx {
                    mixer: Some(("Spotify".into(), muted)),
                    ..TipCtx::default()
                };
                let name = name_of(&Hit::MixerMute(0), &c);
                assert!(name.contains("Spotify"), "{name}");
                assert_eq!(name, i18n::mute_app(lang, "Spotify", muted));
            }
            let (off, on) = (
                i18n::mute_app(lang, "Spotify", false),
                i18n::mute_app(lang, "Spotify", true),
            );
            assert_ne!(off, on, "{lang:?}");
        }
    }

    #[test]
    fn mute_app_keeps_a_long_name_whole_and_has_no_em_dash() {
        let long = "A very long application name that the tooltip has to cut";
        for lang in LANGS {
            for muted in [false, true] {
                let s = i18n::mute_app(lang, long, muted);
                assert!(s.ends_with(long), "{s}");
                assert!(!s.contains('\u{2014}') && !s.contains(" \u{2013} "), "{s}");
            }
        }
        i18n::set_lang(Lang::Id);
        assert_eq!(i18n::mute_app(Lang::Id, "Chrome", false), "Bisukan Chrome");
        assert_eq!(i18n::mute_app(Lang::Id, "Chrome", true), "Nyalakan Chrome");
        assert_eq!(i18n::mute_app(Lang::En, "Chrome", false), "Mute Chrome");
        assert_eq!(i18n::mute_app(Lang::En, "Chrome", true), "Unmute Chrome");
    }

    #[test]
    fn the_clipboard_pin_follows_the_card() {
        let pin = Hit::ClipPin(3);
        let off = tip_for(&pin, &TipCtx::default()).unwrap();
        assert_eq!(off.name.as_deref(), Some(tr(Key::TipClipPin)));
        assert_eq!(off.about.as_deref(), Some(tr(Key::TipClipPinAbout)));
        let c = TipCtx {
            clip_pinned: true,
            ..TipCtx::default()
        };
        let on = tip_for(&pin, &c).unwrap();
        assert_eq!(on.name.as_deref(), Some(tr(Key::TipClipUnpin)));
        assert_eq!(on.about.as_deref(), Some(tr(Key::TipClipUnpinAbout)));
        let plain = tip_for(&Hit::ClipPlain(3), &c).unwrap();
        assert_eq!(plain.name.as_deref(), Some(tr(Key::TipClipPlain)));
    }

    #[test]
    fn the_back_buttons_carry_esc_only_where_esc_goes_back() {
        let c = TipCtx::default();
        let search = tip_for(&Hit::SearchBack, &c).unwrap();
        assert_eq!(search.name.as_deref(), Some(tr(Key::TipBack)));
        assert_eq!(search.keys, [KeyHint::Raw("Esc")]);
        // Esc does nothing on the phone link or the cleaner (the handlers).
        assert!(tip_for(&Hit::PhoneBack, &c).unwrap().keys.is_empty());
        let clean = tip_for(&Hit::CleanBack, &c).unwrap();
        assert_eq!(clean.name.as_deref(), Some(tr(Key::TipCleanBack)));
        assert!(clean.keys.is_empty());
    }

    #[test]
    fn the_rekam_buttons_follow_the_recording_and_the_tab() {
        let c = TipCtx::default();
        let pause = tip_for(&Hit::RecPause, &c).unwrap();
        assert_eq!(pause.name.as_deref(), Some(tr(Key::TipRecPause)));
        let paused = TipCtx {
            rec_paused: true,
            ..TipCtx::default()
        };
        let resume = tip_for(&Hit::RecPause, &paused).unwrap();
        assert_eq!(resume.name.as_deref(), Some(tr(Key::TipRecResume)));

        // Idle, the round button shows "Rekam" (but under the waiting
        // strip): the short form, what it starts on this tab.
        let audio = tip_for(&Hit::RecToggle, &c).unwrap();
        assert!(audio.name.is_none());
        assert_eq!(audio.about.as_deref(), Some(tr(Key::TipRecStartAudioAbout)));
        assert_eq!(audio.keys, [KeyHint::Hotkey(HotkeyId::Record)]);
        let screen = TipCtx {
            screen_tab: true,
            ..TipCtx::default()
        };
        let screen = tip_for(&Hit::RecToggle, &screen).unwrap();
        assert_eq!(screen.about.as_deref(), Some(tr(Key::TipRecStartScreenAbout)));
        assert_eq!(screen.keys, [KeyHint::Hotkey(HotkeyId::RecordScreen)]);
        // Recording, it is the stop square with no label: named, with the
        // tab's key (the tab is the running recording's).
        for (screen_tab, key) in [(false, HotkeyId::Record), (true, HotkeyId::RecordScreen)] {
            let live = TipCtx {
                recording: true,
                screen_tab,
                ..TipCtx::default()
            };
            let stop = tip_for(&Hit::RecToggle, &live).unwrap();
            assert_eq!(stop.name.as_deref(), Some(tr(Key::TipRecStop)));
            assert_eq!(stop.about.as_deref(), Some(tr(Key::TipRecStopAbout)));
            assert_eq!(stop.keys, [KeyHint::Hotkey(key)]);
        }

        // The page's mute buttons say what the notch's level groups say.
        for (src, muted) in [
            (Source::Mic, false),
            (Source::Mic, true),
            (Source::System, true),
        ] {
            let c = TipCtx {
                rec_muted_mic: muted,
                rec_muted_system: muted,
                ..TipCtx::default()
            };
            let page = tip_for(&Hit::RecMute(src), &c).unwrap();
            let notch = tip_for(&Hit::RecMuteCompact(src), &c).unwrap();
            assert_eq!(page.name, notch.name, "{src:?} {muted}");
            assert_eq!(page.keys, notch.keys, "{src:?} {muted}");
        }
        let mic = tip_for(&Hit::RecMute(Source::Mic), &c).unwrap();
        assert_eq!(mic.keys, [KeyHint::Hotkey(HotkeyId::RecordMute)]);
    }

    #[test]
    fn the_cli_and_the_folder_star_follow_their_state() {
        let tall = TipCtx {
            cli_tall: true,
            ..TipCtx::default()
        };
        assert_eq!(
            name_of(&Hit::CliExpand, &TipCtx::default()),
            tr(Key::TipCliEnlarge)
        );
        assert_eq!(name_of(&Hit::CliExpand, &tall), tr(Key::TipCliShrink));
        let path = std::path::PathBuf::from("C:\\work");
        let star = |on| Hit::CliFolderStar {
            path: path.clone(),
            on,
        };
        let c = TipCtx::default();
        assert_eq!(name_of(&star(true), &c), tr(Key::TipCliStar));
        assert_eq!(name_of(&star(false), &c), tr(Key::TipCliUnstar));
    }

    #[test]
    fn the_other_controls_say_what_the_handlers_do() {
        let c = TipCtx::default();
        let tip = |h: Hit| tip_for(&h, &c).unwrap();
        assert_eq!(
            tip(Hit::ClipDelete(1)).about.as_deref(),
            Some(tr(Key::TipClipDeleteAbout))
        );
        assert_eq!(
            tip(Hit::NotifRemove(1)).about.as_deref(),
            Some(tr(Key::TipNotifRemoveAbout))
        );
        assert_eq!(
            tip(Hit::ShelfRemove(0)).name.as_deref(),
            Some(tr(Key::TipShelfRemove))
        );
        assert_eq!(
            tip(Hit::PhoneShowQr).name.as_deref(),
            Some(tr(Key::TipPhoneShowQr))
        );
        assert_eq!(
            tip(Hit::ArrangePlay(0)).name.as_deref(),
            Some(tr(Key::TipArrangePlay))
        );
        assert_eq!(
            tip(Hit::DeviceOpen(0)).name.as_deref(),
            Some(tr(Key::TipDeviceOpen))
        );
        let detail = tip(Hit::DeviceDetail(0));
        assert_eq!(detail.name.as_deref(), Some(tr(Key::TipDeviceDetail)));
        assert_eq!(detail.about.as_deref(), Some(tr(Key::TipDeviceDetailAbout)));
    }

    #[test]
    fn a_settled_move_over_a_hit_with_a_tip_feeds_it() {
        let dock = Hit::PanelPage(Page::Media);
        let mut pressed = None;
        assert_eq!(
            feed(Some(dock.clone()), false, false, &mut pressed),
            Feed::Tip(dock)
        );
        assert_eq!(
            feed(Some(Hit::Body), false, false, &mut pressed),
            Feed::Nothing
        );
        assert_eq!(
            feed(Some(Hit::CalAdd), false, false, &mut pressed),
            Feed::Nothing
        );
        assert_eq!(
            feed(Some(Hit::CalPrev), false, false, &mut pressed),
            Feed::Tip(Hit::CalPrev)
        );
        assert_eq!(feed(None, false, false, &mut pressed), Feed::Nothing);
    }

    #[test]
    fn a_held_button_or_a_moving_frame_leaves_the_tooltip_alone() {
        let dock = Hit::PanelPage(Page::Media);
        let mut pressed = None;
        assert_eq!(
            feed(Some(dock.clone()), true, false, &mut pressed),
            Feed::LeaveAlone
        );
        assert_eq!(
            feed(Some(dock), false, true, &mut pressed),
            Feed::LeaveAlone
        );
        // A sliding page draws no regions: the body under the pointer is not
        // a move off the target.
        assert_eq!(
            feed(Some(Hit::Body), false, true, &mut pressed),
            Feed::LeaveAlone
        );
    }

    #[test]
    fn a_pressed_target_stays_quiet_through_its_page_turn_until_left() {
        let dock = Hit::PanelPage(Page::Media);
        let mut pressed = Some(dock.clone());
        // The page slides in under the still pointer.
        assert_eq!(
            feed(Some(Hit::Body), false, true, &mut pressed),
            Feed::LeaveAlone
        );
        assert_eq!(pressed, Some(dock.clone()));
        // Settled again, the same icon under the pointer: still quiet.
        assert_eq!(
            feed(Some(dock.clone()), false, false, &mut pressed),
            Feed::LeaveAlone
        );
        assert_eq!(pressed, Some(dock.clone()));
        // Off it, then back: its tip comes again.
        assert_eq!(
            feed(Some(Hit::Body), false, false, &mut pressed),
            Feed::Nothing
        );
        assert_eq!(pressed, None);
        assert_eq!(
            feed(Some(dock.clone()), false, false, &mut pressed),
            Feed::Tip(dock)
        );
    }

    #[test]
    fn a_pressed_target_is_forgotten_on_another_target() {
        let mut pressed = Some(Hit::PanelPage(Page::Media));
        let other = Hit::PanelPage(Page::Mixer);
        assert_eq!(
            feed(Some(other.clone()), false, false, &mut pressed),
            Feed::Tip(other)
        );
        assert_eq!(pressed, None);
    }
}
