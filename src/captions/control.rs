//! What state the caption card is in and whether the worker should run.

use crate::transcribe::prefs::TrLang;

/// Everything the card's state depends on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ctl {
    pub on: bool,
    pub user_paused: bool,
    pub on_battery: bool,
    /// The setting that lets captions run on battery.
    pub battery_ok: bool,
    /// The "run anyway" override for this recording only.
    pub session_battery_ok: bool,
    pub transcript_on: bool,
    pub engine_ok: bool,
    pub model_ok: bool,
    pub downloading: Option<u8>,
    pub failed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardStatus {
    Listening,
    Running,
    Paused,
    Battery,
    NeedsTranskrip,
    NeedsModel,
    Downloading(u8),
    Failed(String),
}

/// `None` when captions are off.
pub fn status(c: &Ctl, has_lines: bool) -> Option<CardStatus> {
    if !c.on {
        return None;
    }
    if !c.transcript_on {
        return Some(CardStatus::NeedsTranskrip);
    }
    if let Some(p) = c.downloading {
        return Some(CardStatus::Downloading(p));
    }
    if !c.engine_ok || !c.model_ok {
        return Some(CardStatus::NeedsModel);
    }
    if let Some(f) = &c.failed {
        return Some(CardStatus::Failed(f.clone()));
    }
    if c.user_paused {
        return Some(CardStatus::Paused);
    }
    if c.on_battery && !c.battery_ok && !c.session_battery_ok {
        return Some(CardStatus::Battery);
    }
    Some(if has_lines {
        CardStatus::Running
    } else {
        CardStatus::Listening
    })
}

pub fn should_run(c: &Ctl) -> bool {
    matches!(
        status(c, false),
        Some(CardStatus::Running | CardStatus::Listening)
    )
}

/// The transcript queue waits while captions are on.
pub fn holds_queue(c: &Ctl) -> bool {
    c.on
}

pub fn next_lang(l: TrLang) -> TrLang {
    match l {
        TrLang::Id => TrLang::En,
        TrLang::En => TrLang::Auto,
        TrLang::Auto => TrLang::Id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Ctl {
        Ctl {
            on: true,
            transcript_on: true,
            engine_ok: true,
            model_ok: true,
            ..Ctl::default()
        }
    }

    #[test]
    fn status_order() {
        let mut c = ready();
        assert_eq!(status(&c, false), Some(CardStatus::Listening));
        assert_eq!(status(&c, true), Some(CardStatus::Running));

        c.on = false;
        assert_eq!(status(&c, true), None);

        let mut c = ready();
        c.transcript_on = false;
        c.downloading = Some(10);
        assert_eq!(status(&c, true), Some(CardStatus::NeedsTranskrip));

        let mut c = ready();
        c.downloading = Some(40);
        c.model_ok = false;
        assert_eq!(status(&c, true), Some(CardStatus::Downloading(40)));

        let mut c = ready();
        c.engine_ok = false;
        c.failed = Some("x".into());
        assert_eq!(status(&c, true), Some(CardStatus::NeedsModel));
        let mut c = ready();
        c.model_ok = false;
        assert_eq!(status(&c, true), Some(CardStatus::NeedsModel));

        let mut c = ready();
        c.failed = Some("boom".into());
        c.user_paused = true;
        assert_eq!(status(&c, true), Some(CardStatus::Failed("boom".into())));

        let mut c = ready();
        c.user_paused = true;
        c.on_battery = true;
        assert_eq!(status(&c, true), Some(CardStatus::Paused));

        let mut c = ready();
        c.on_battery = true;
        assert_eq!(status(&c, true), Some(CardStatus::Battery));
    }

    #[test]
    fn battery_stops_only_without_either_override() {
        let mut c = ready();
        c.on_battery = true;
        assert!(!should_run(&c));
        c.battery_ok = true;
        assert!(should_run(&c));
        c.battery_ok = false;
        c.session_battery_ok = true;
        assert!(should_run(&c));
        c.on_battery = false;
        c.session_battery_ok = false;
        assert!(should_run(&c));
    }

    #[test]
    fn nothing_runs_while_paused_or_blocked() {
        assert!(should_run(&ready()));
        assert!(holds_queue(&ready()));
        let blocked: [fn(&mut Ctl); 6] = [
            |c| c.on = false,
            |c| c.user_paused = true,
            |c| c.transcript_on = false,
            |c| c.model_ok = false,
            |c| c.downloading = Some(1),
            |c| c.failed = Some("x".into()),
        ];
        for f in blocked {
            let mut c = ready();
            f(&mut c);
            assert!(!should_run(&c));
        }
        let mut off = ready();
        off.on = false;
        assert!(!holds_queue(&off));
    }

    #[test]
    fn lang_cycles_id_en_auto() {
        assert_eq!(next_lang(TrLang::Id), TrLang::En);
        assert_eq!(next_lang(TrLang::En), TrLang::Auto);
        assert_eq!(next_lang(TrLang::Auto), TrLang::Id);
    }
}
