//! What the phone says it is: the User-Agent read into a device kind, only
//! to tell the owner what joined ("Tersambung: Android"). Pure.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Android,
    IPhone,
    IPad,
    Laptop,
    Other,
}

/// The kind of device for a User-Agent. Phones and the iPad are checked first,
/// then a Windows, ChromeOS or Linux desktop browser is a `Laptop`. An iPad that
/// asks for the desktop site (iPadOS says "Macintosh"), a Mac and anything
/// unknown are `Other`.
pub fn parse(ua: &str) -> DeviceKind {
    let has = |needle: &str| ua.contains(needle);
    if has("iPhone") || has("iPod") {
        DeviceKind::IPhone
    } else if has("iPad") {
        DeviceKind::IPad
    } else if has("Android") {
        DeviceKind::Android
    } else if has("Windows NT") || has("CrOS") || has("X11") || has("Linux") {
        DeviceKind::Laptop
    } else {
        DeviceKind::Other
    }
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
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
                DeviceKind::IPhone,
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/126.0.0.0 Mobile/15E148 Safari/604.1",
                DeviceKind::IPhone,
            ),
            (
                "Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
                DeviceKind::IPad,
            ),
            (
                "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/24.0 Chrome/117.0.0.0 Mobile Safari/537.36",
                DeviceKind::Android,
            ),
            (
                "Mozilla/5.0 (Android 14; Mobile; rv:127.0) Gecko/127.0 Firefox/127.0",
                DeviceKind::Android,
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
                DeviceKind::Other,
            ),
            ("", DeviceKind::Other),
            ("curl/8.0", DeviceKind::Other),
        ];
        for (ua, kind) in cases {
            assert_eq!(parse(ua), kind, "{ua}");
        }
    }

    #[test]
    fn desktop_browsers_are_laptops() {
        let cases = [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36 Edg/126.0.0.0",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:127.0) Gecko/20100101 Firefox/127.0",
            "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
            "Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0",
        ];
        for ua in cases {
            assert_eq!(parse(ua), DeviceKind::Laptop, "{ua}");
        }
    }

    #[test]
    fn an_android_says_linux_but_is_not_a_laptop() {
        let ua = "Mozilla/5.0 (X11; Linux x86_64) Android 14 Chrome/126.0.0.0";
        assert_eq!(parse(ua), DeviceKind::Android);
        let ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
        assert_eq!(parse(ua), DeviceKind::Android);
    }
}
