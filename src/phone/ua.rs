//! What the phone says it is: the User-Agent read into a device kind and a
//! browser name, only to tell the owner which phone asks. Pure.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Android,
    IPhone,
    IPad,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Safari,
    Firefox,
    SamsungInternet,
    Edge,
    Other,
}

impl Browser {
    /// The name shown in the notch; `None` when it is not known.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Browser::Chrome => Some("Chrome"),
            Browser::Safari => Some("Safari"),
            Browser::Firefox => Some("Firefox"),
            Browser::SamsungInternet => Some("Samsung Internet"),
            Browser::Edge => Some("Edge"),
            Browser::Other => None,
        }
    }
}

/// The kind and the browser for a User-Agent. An iPad that asks for the
/// desktop site (iPadOS says "Macintosh") is `Other`.
pub fn parse(ua: &str) -> (DeviceKind, Browser) {
    let has = |needle: &str| ua.contains(needle);
    let kind = if has("iPhone") || has("iPod") {
        DeviceKind::IPhone
    } else if has("iPad") {
        DeviceKind::IPad
    } else if has("Android") {
        DeviceKind::Android
    } else {
        DeviceKind::Other
    };
    // The most specific first: Chrome's string is inside Edge's and Samsung's,
    // Safari's inside all of them.
    let browser = if has("SamsungBrowser") {
        Browser::SamsungInternet
    } else if has("EdgA") || has("EdgiOS") || has("Edg/") {
        Browser::Edge
    } else if has("FxiOS") || has("Firefox") {
        Browser::Firefox
    } else if has("CriOS") || has("Chrome") {
        Browser::Chrome
    } else if has("Safari") {
        Browser::Safari
    } else {
        Browser::Other
    };
    (kind, browser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_phones_are_told_apart() {
        let cases = [
            (
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36",
                DeviceKind::Android,
                Browser::Chrome,
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
                DeviceKind::IPhone,
                Browser::Safari,
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/126.0.0.0 Mobile/15E148 Safari/604.1",
                DeviceKind::IPhone,
                Browser::Chrome,
            ),
            (
                "Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
                DeviceKind::IPad,
                Browser::Safari,
            ),
            (
                "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/24.0 Chrome/117.0.0.0 Mobile Safari/537.36",
                DeviceKind::Android,
                Browser::SamsungInternet,
            ),
            (
                "Mozilla/5.0 (Android 14; Mobile; rv:127.0) Gecko/127.0 Firefox/127.0",
                DeviceKind::Android,
                Browser::Firefox,
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36 EdgA/126.0.0.0",
                DeviceKind::Android,
                Browser::Edge,
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
                DeviceKind::Other,
                Browser::Safari,
            ),
            ("", DeviceKind::Other, Browser::Other),
            ("curl/8.0", DeviceKind::Other, Browser::Other),
        ];
        for (ua, kind, browser) in cases {
            assert_eq!(parse(ua), (kind, browser), "{ua}");
        }
    }

    #[test]
    fn an_unknown_browser_has_no_label() {
        assert_eq!(Browser::Other.label(), None);
        assert_eq!(Browser::SamsungInternet.label(), Some("Samsung Internet"));
    }
}
