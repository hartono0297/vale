//! What the island's tooltips say: the closed notch (the notice's ✕, the REC
//! view's level groups) and the open panel (the page dock, the Settings
//! button). Pure; `App` feeds the hit under the pointer and where the last
//! frame drew it (`Renderer::region_of`).

use crate::capture::Rect;
use crate::hotkey::HotkeyId;
use crate::i18n::Key;
use crate::model::{Hit, Page};
use crate::recorder::Source;
use crate::ui::geometry::Rect as FRect;
use crate::ui::tooltip::{KeyHint, Side, Tip};

/// The app state a tip reads, built only while a hit other than the body is
/// under the pointer.
#[derive(Clone, Debug, Default)]
pub struct TipCtx {
    /// The running recording's mic, and its PC audio, are muted.
    pub rec_muted_mic: bool,
    pub rec_muted_system: bool,
}

/// The tip of `hit`; `None` for a hit with none (the body, the busy toast's
/// logo, a compact view).
pub fn tip_for(hit: &Hit, ctx: &TipCtx) -> Option<Tip> {
    Some(match hit {
        Hit::NoticeClose => Tip::named(Key::TipNoticeClose).about(Key::TipNoticeCloseAbout),
        // The name says what a click does now. Only the mic has a hotkey.
        Hit::RecMuteCompact(Source::Mic) => Tip::named(if ctx.rec_muted_mic {
            Key::TrayMicUnmute
        } else {
            Key::TrayMicMute
        })
        .key(KeyHint::Hotkey(HotkeyId::RecordMute)),
        Hit::RecMuteCompact(Source::System) => Tip::named(if ctx.rec_muted_system {
            Key::TipPcAudioUnmute
        } else {
            Key::TipPcAudioMute
        }),
        Hit::PanelPage(page) => page_tip(*page),
        Hit::OpenSettings => Tip::named(Key::TipSettings).about(Key::TipSettingsAbout),
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

/// A stable id for `hit` (`Hit` holds strings and paths and is not `Hash`).
pub fn tip_id(hit: &Hit) -> u64 {
    crate::ui::tooltip::id_of(&format!("{hit:?}"))
}

/// `r` (dip from the island window's top-left) on screen, px, for a window
/// at `origin` (px) and `scale`.
pub fn anchor_px(origin: (i32, i32), r: FRect, scale: f32) -> Rect {
    Rect::new(
        origin.0 + (r.x * scale).round() as i32,
        origin.1 + (r.y * scale).round() as i32,
        (r.w * scale).round() as i32,
        (r.h * scale).round() as i32,
    )
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
        }
    }

    fn every_tip() -> Vec<Tip> {
        let mut all = Vec::new();
        for muted in [false, true] {
            let c = ctx(muted);
            let hits = Page::ALL
                .into_iter()
                .chain([Page::Search])
                .map(Hit::PanelPage)
                .chain([
                    Hit::NoticeClose,
                    Hit::OpenSettings,
                    Hit::RecMuteCompact(Source::Mic),
                    Hit::RecMuteCompact(Source::System),
                ]);
            all.extend(hits.filter_map(|h| tip_for(&h, &c)));
        }
        all
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
            Hit::PlayPause,
            Hit::CalPrev,
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
    fn the_anchor_is_the_region_on_screen() {
        let r = FRect {
            x: 10.0,
            y: 4.0,
            w: 20.0,
            h: 8.0,
        };
        assert_eq!(anchor_px((100, 0), r, 1.5), Rect::new(115, 6, 30, 12));
        assert_eq!(anchor_px((-1920, 0), r, 1.0), Rect::new(-1910, 4, 20, 8));
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
    }
}
