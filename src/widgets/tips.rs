//! What the sticky note's tooltips say: the header's buttons, the stack's
//! arrows, the image's remove button, the format bar, the colour popover's
//! swatches and the reminder popover's chevrons. Pure; the hover feed (where
//! each target is on screen) is `host`, `pop` and `remind_pop`.

use crate::hotkey::HotkeyId;
use crate::i18n::Key;
use crate::ui::tooltip::{KeyHint, Tip};
use crate::widgets::paint::Btn;
use crate::widgets::remind_pop;
use crate::widgets::sticky::Paper;
use crate::widgets::toolbar::{self, BarHit, Tool};

/// The tip of a header button. `pinned` is whether the note is kept on top
/// (the pin says what a click does now); `at_cap` is whether the note cap is
/// reached (the (+) says why it does nothing).
pub fn header_tip(btn: Btn, pinned: bool, at_cap: bool) -> Tip {
    match btn {
        Btn::List => Tip::named(Key::StickyList)
            .about(Key::TipStickyListAbout)
            .key(KeyHint::Hotkey(HotkeyId::StickyList)),
        Btn::Pin if pinned => Tip::named(Key::TipStickyPinOn).about(Key::TipStickyPinOnAbout),
        Btn::Pin => Tip::named(Key::TipStickyPinOff).about(Key::TipStickyPinOffAbout),
        Btn::Plus if at_cap => Tip::named(Key::TipStickyNewNote).about(Key::StickyMax),
        Btn::Plus => Tip::named(Key::TipStickyNewNote).about(Key::TipStickyNewAbout),
        Btn::Check => Tip::named(Key::StickyChecklist)
            .about(Key::TipStickyCheckAbout)
            .key(KeyHint::Raw(toolbar::shortcut(Tool::Check))),
        Btn::Colour => Tip::named(Key::TipStickyStyle).about(Key::TipStickyStyleAbout),
        Btn::More => Tip::named(Key::FmtMore)
            .about(Key::TipStickyMoreAbout)
            .key(KeyHint::Fixed(Key::TipKeyRightClick)),
        Btn::Close => Tip::named(Key::TipStickyClose).about(Key::TipStickyCloseAbout),
        Btn::Prev => Tip::named(Key::TipStickyPrev).about(Key::TipStickyStackAbout),
        Btn::Next => Tip::named(Key::TipStickyNext).about(Key::TipStickyStackAbout),
    }
}

/// The tip of a format bar button; `None` on the bar's own background.
pub fn bar_tip(hit: BarHit) -> Option<Tip> {
    match hit {
        BarHit::Tool(Tool::Image) => {
            Some(Tip::named(Tool::Image.label_key()).about(Key::TipImageAbout))
        }
        BarHit::Tool(t) => {
            let tip = Tip::named(t.label_key());
            let keys = toolbar::shortcut(t);
            Some(if keys.is_empty() { tip } else { tip.key(KeyHint::Raw(keys)) })
        }
        BarHit::MarkerMenu => Some(Tip::named(Key::TipMarkerMenu).about(Key::TipMarkerMenuAbout)),
        BarHit::More => Some(Tip::named(Key::FmtMore).about(Key::TipBarMoreAbout)),
        BarHit::Bar => None,
    }
}

/// The tip of an image's round remove button.
pub fn image_remove_tip() -> Tip {
    Tip::named(Key::FmtRemoveImage).about(Key::TipImageRemoveAbout)
}

/// The tip of a paper swatch of the colour popover: the paper's name.
pub fn swatch_tip(paper: Paper) -> Tip {
    Tip::named(paper.label_key())
}

/// The tip of a reminder popover control; `None` for the two buttons that
/// already say what they do (Save, Cancel).
pub fn remind_tip(item: remind_pop::Item) -> Option<Tip> {
    use remind_pop::Item;
    let name = match item {
        Item::DayPrev => Key::TipRemindDayPrev,
        Item::DayNext => Key::TipRemindDayNext,
        Item::HourPrev => Key::TipRemindHourPrev,
        Item::HourNext => Key::TipRemindHourNext,
        Item::MinPrev => Key::TipRemindMinPrev,
        Item::MinNext => Key::TipRemindMinNext,
        Item::Save | Item::Cancel => return None,
    };
    Some(Tip::named(name).about(Key::TipRemindWheelAbout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Lang, tr};
    use crate::widgets::remind_pop::Item;

    const LANGS: [Lang; 2] = [Lang::Id, Lang::En];

    fn every_tip() -> Vec<Tip> {
        let mut all = Vec::new();
        for b in Btn::ALL.into_iter().chain([Btn::Prev, Btn::Next]) {
            for pinned in [false, true] {
                for cap in [false, true] {
                    all.push(header_tip(b, pinned, cap));
                }
            }
        }
        for t in Tool::ALL {
            all.extend(bar_tip(BarHit::Tool(t)));
        }
        all.extend(bar_tip(BarHit::MarkerMenu));
        all.extend(bar_tip(BarHit::More));
        all.push(image_remove_tip());
        for p in Paper::ALL {
            all.push(swatch_tip(p));
        }
        for i in [
            Item::DayPrev,
            Item::DayNext,
            Item::HourPrev,
            Item::HourNext,
            Item::MinPrev,
            Item::MinNext,
        ] {
            all.extend(remind_tip(i));
        }
        all
    }

    #[test]
    fn every_header_button_has_a_name_in_both_languages() {
        for lang in LANGS {
            i18n::set_lang(lang);
            for b in Btn::ALL.into_iter().chain([Btn::Prev, Btn::Next]) {
                let name = header_tip(b, false, false).name;
                assert!(name.is_some_and(|n| !n.is_empty()), "{lang:?} {b:?}");
            }
        }
    }

    #[test]
    fn the_pin_names_what_a_click_does_now() {
        for lang in LANGS {
            i18n::set_lang(lang);
            assert_ne!(
                header_tip(Btn::Pin, false, false).name,
                header_tip(Btn::Pin, true, false).name,
                "{lang:?}"
            );
        }
    }

    #[test]
    fn the_plus_at_the_cap_says_why() {
        for lang in LANGS {
            i18n::set_lang(lang);
            assert_eq!(
                header_tip(Btn::Plus, false, true).about.as_deref(),
                Some(tr(Key::StickyMax))
            );
            assert_ne!(
                header_tip(Btn::Plus, false, false).about.as_deref(),
                Some(tr(Key::StickyMax))
            );
        }
    }

    #[test]
    fn the_bar_keeps_its_names_and_shortcuts() {
        let bold = bar_tip(BarHit::Tool(Tool::Bold)).unwrap();
        assert_eq!(bold.keys, [KeyHint::Raw("Ctrl+B")]);
        assert!(bold.about.is_none());
        let image = bar_tip(BarHit::Tool(Tool::Image)).unwrap();
        assert!(image.about.is_some());
        assert!(image.keys.is_empty());
        assert!(bar_tip(BarHit::Bar).is_none());
    }

    #[test]
    fn the_header_keys_are_the_documented_ones() {
        assert!(
            header_tip(Btn::More, false, false)
                .keys
                .contains(&KeyHint::Fixed(Key::TipKeyRightClick))
        );
        assert_eq!(
            header_tip(Btn::List, false, false).keys,
            [KeyHint::Hotkey(HotkeyId::StickyList)]
        );
        assert_eq!(header_tip(Btn::Check, false, false).keys, [KeyHint::Raw("Ctrl+Shift+9")]);
    }

    #[test]
    fn only_the_six_chevrons_have_a_remind_tip() {
        assert!(remind_tip(Item::DayPrev).is_some());
        assert!(remind_tip(Item::MinNext).is_some());
        assert!(remind_tip(Item::Save).is_none());
        assert!(remind_tip(Item::Cancel).is_none());
    }

    #[test]
    fn no_tip_text_has_an_em_dash() {
        for lang in LANGS {
            i18n::set_lang(lang);
            for tip in every_tip() {
                for s in tip.name.iter().chain(tip.about.iter()) {
                    assert!(!s.contains('\u{2014}'), "{lang:?}: {s}");
                    assert!(!s.is_empty(), "{lang:?}");
                }
            }
        }
    }
}
