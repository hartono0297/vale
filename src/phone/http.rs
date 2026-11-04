//! The little HTTP/1.1 the phone link speaks: a request head parsed from
//! bytes with hard limits, and a response writer. No sockets here, so every
//! limit is tested without a network, and nothing on any input panics.

use std::io::{self, Write};
use std::time::SystemTime;

use super::share::RangeAns;

/// The longest request head accepted (request line and headers), in bytes.
pub const MAX_HEAD: usize = 16 * 1024;
/// More headers than this is refused (431) too.
const MAX_HEADERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    /// The target's path, without the query.
    pub path: String,
    /// Names lower-cased, values trimmed.
    pub headers: Vec<(String, String)>,
    /// `Content-Length`, when the request has one.
    pub content_length: Option<u64>,
}

impl Request {
    /// The first header called `name` (give it in lower case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The value of cookie `name` in the `Cookie` header(s).
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k == "cookie")
            .flat_map(|(_, v)| v.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v)
    }
}

/// Why a request is refused before it is looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// 400: not HTTP, or something refused outright (Transfer-Encoding).
    Bad,
    /// 411: a POST without `Content-Length`.
    LengthRequired,
    /// 431: the head is over the cap.
    HeadTooLarge,
}

impl HttpError {
    pub fn status(self) -> u16 {
        match self {
            HttpError::Bad => 400,
            HttpError::LengthRequired => 411,
            HttpError::HeadTooLarge => 431,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// The head is not complete yet: read more.
    Need,
    /// A request, and how many bytes of the buffer its head took (what
    /// follows is the start of the body).
    Done(Request, usize),
}

fn find_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Reads the request head from the start of `buf`.
pub fn parse_head(buf: &[u8]) -> Result<Parsed, HttpError> {
    let Some(end) = find_end(buf) else {
        return if buf.len() >= MAX_HEAD {
            Err(HttpError::HeadTooLarge)
        } else {
            Ok(Parsed::Need)
        };
    };
    let head_len = end + 4;
    if head_len > MAX_HEAD {
        return Err(HttpError::HeadTooLarge);
    }
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| HttpError::Bad)?;
    let mut lines = head.split("\r\n");
    let line = lines.next().ok_or(HttpError::Bad)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(HttpError::Bad);
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") || !target.starts_with('/') {
        return Err(HttpError::Bad);
    }
    let method = match method {
        "GET" => Method::Get,
        "HEAD" => Method::Head,
        "POST" => Method::Post,
        _ => Method::Other,
    };
    let path = target.split(['?', '#']).next().unwrap_or("/").to_owned();
    let mut headers = Vec::new();
    let mut content_length: Option<u64> = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(HttpError::Bad)?;
        // A header name is a token: no spaces, no control characters.
        if name.is_empty() || name.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(HttpError::Bad);
        }
        if headers.len() >= MAX_HEADERS {
            return Err(HttpError::HeadTooLarge);
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim().to_owned();
        match name.as_str() {
            "transfer-encoding" => return Err(HttpError::Bad),
            "content-length" => {
                // Digits only, and a second one must agree with the first.
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(HttpError::Bad);
                }
                let n: u64 = value.parse().map_err(|_| HttpError::Bad)?;
                if content_length.is_some_and(|old| old != n) {
                    return Err(HttpError::Bad);
                }
                content_length = Some(n);
            }
            _ => {}
        }
        headers.push((name, value));
    }
    if method == Method::Post && content_length.is_none() {
        return Err(HttpError::LengthRequired);
    }
    Ok(Parsed::Done(
        Request {
            method,
            path,
            headers,
            content_length,
        },
        head_len,
    ))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Payload Too Large",
        416 => "Range Not Satisfiable",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        507 => "Insufficient Storage",
        _ => "Error",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// Whole `Set-Cookie` values.
    pub cookies: Vec<String>,
    /// The `Cache-Control` value: `no-store` unless a thumbnail says more.
    pub cache: &'static str,
}

impl Response {
    /// A 404 with nothing in it: what every request that has no business
    /// here gets, so a probe learns nothing.
    pub fn not_found() -> Self {
        Self {
            status: 404,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
            cookies: Vec::new(),
            cache: "no-store",
        }
    }

    /// A 410 with nothing in it: a shared file that is gone.
    pub fn gone() -> Self {
        Self {
            status: 410,
            ..Self::not_found()
        }
    }

    pub fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
            ..Self::not_found()
        }
    }

    pub fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.into_bytes(),
            ..Self::not_found()
        }
    }

    /// A thumbnail: the phone keeps it for an hour, privately.
    pub fn jpeg_thumb(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: "image/jpeg",
            body,
            cache: "private, max-age=3600",
            ..Self::not_found()
        }
    }

    /// `{"error":"<code>"}` with `status`.
    pub fn error(status: u16, code: &str) -> Self {
        Self::json(status, format!("{{\"error\":\"{code}\"}}"))
    }

    /// The bare answer for a refused head.
    pub fn for_error(e: HttpError) -> Self {
        Self {
            status: e.status(),
            ..Self::not_found()
        }
    }

    pub fn with_cookie(mut self, cookie: String) -> Self {
        self.cookies.push(cookie);
        self
    }

    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: {}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len(),
            self.cache
        );
        for c in &self.cookies {
            head.push_str("Set-Cookie: ");
            head.push_str(c);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        w.write_all(head.as_bytes())?;
        w.write_all(&self.body)?;
        w.flush()
    }
}

/// What a `Range` header asks of a file of `len` bytes (RFC 9110 14.1.2).
/// `bytes=a-b` (b clamped to the last byte), `bytes=a-` (to the end) and
/// `bytes=-n` (the last n bytes, n clamped to the length) are answered with
/// a part; a start past the end, `a > b`, `bytes=-0`, any range on an empty
/// file, another unit, non-digits, empty parts and overflow are 416. A
/// multi-range (a comma anywhere) is ignored and the whole file is sent: a
/// server may do that, and it spares a multipart body nobody here needs.
pub fn parse_range(value: &str, len: u64) -> RangeAns {
    let value = value.trim();
    let Some((unit, spec)) = value.split_once('=') else {
        return RangeAns::Unsatisfiable;
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") {
        return RangeAns::Unsatisfiable;
    }
    if spec.contains(',') {
        return RangeAns::Full;
    }
    let Some((a, b)) = spec.trim().split_once('-') else {
        return RangeAns::Unsatisfiable;
    };
    let (a, b) = (a.trim(), b.trim());
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    if len == 0 {
        return RangeAns::Unsatisfiable;
    }
    if a.is_empty() {
        // A suffix: the last n bytes.
        if !digits(b) {
            return RangeAns::Unsatisfiable;
        }
        let Ok(n) = b.parse::<u64>() else {
            return RangeAns::Unsatisfiable;
        };
        if n == 0 {
            return RangeAns::Unsatisfiable;
        }
        let n = n.min(len);
        return RangeAns::Partial {
            start: len - n,
            end_incl: len - 1,
            open_ended: false,
        };
    }
    if !digits(a) {
        return RangeAns::Unsatisfiable;
    }
    let Ok(start) = a.parse::<u64>() else {
        return RangeAns::Unsatisfiable;
    };
    if start >= len {
        return RangeAns::Unsatisfiable;
    }
    if b.is_empty() {
        return RangeAns::Partial {
            start,
            end_incl: len - 1,
            open_ended: true,
        };
    }
    if !digits(b) {
        return RangeAns::Unsatisfiable;
    }
    let Ok(end) = b.parse::<u64>() else {
        return RangeAns::Unsatisfiable;
    };
    if start > end {
        return RangeAns::Unsatisfiable;
    }
    RangeAns::Partial {
        start,
        end_incl: end.min(len - 1),
        open_ended: false,
    }
}

/// Which part of a file to send: the `Range` header, unless an `If-Range`
/// that does not match the file's `ETag` (or `Last-Modified` text) says the
/// phone's copy is stale, which gets the whole file again.
pub fn answer_range(
    range: Option<&str>,
    if_range: Option<&str>,
    len: u64,
    etag: &str,
    last_modified: &str,
) -> RangeAns {
    let Some(range) = range else {
        return RangeAns::Full;
    };
    if let Some(cond) = if_range {
        let cond = cond.trim();
        let by_date = !last_modified.is_empty() && cond == last_modified;
        if cond != etag && !by_date {
            return RangeAns::Full;
        }
    }
    parse_range(range, len)
}

/// A strong `ETag` from the size and the modified time.
pub fn etag(len: u64, mtime: Option<SystemTime>) -> String {
    let nanos = mtime
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    format!("\"{len:x}-{nanos:x}\"")
}

/// An HTTP-date ("Tue, 03 Nov 2026 08:49:37 GMT"), or nothing when the file
/// has no modified time.
pub fn http_date(mtime: Option<SystemTime>) -> Option<String> {
    let t = mtime?;
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    Some(dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string())
}

/// `Content-Disposition` for `name`: an ASCII fallback and the real name
/// percent-encoded (RFC 6266 and 5987). `kind` is "attachment" or "inline".
/// Non-ASCII, control characters, quotes and backslashes cannot reach the
/// header raw, so no CR or LF ever can.
pub fn content_disposition(kind: &str, name: &str) -> String {
    let fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for b in name.bytes() {
        let attr_char = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            );
        if attr_char {
            encoded.push(b as char);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

/// The content type for a file name, by its extension. Anything unknown is
/// `application/octet-stream`. SVG is deliberately not here: it can carry
/// script, so it is never served as something a page could render.
pub fn mime_for(name: &str) -> &'static str {
    let ext = match name.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => return "application/octet-stream",
    };
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "m4v" => "video/x-m4v",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// An image the page may show inline (`/api/view`, `/api/thumb`): never SVG.
pub fn is_viewable(name: &str) -> bool {
    mime_for(name).starts_with("image/")
}

/// The head of a streamed file answer: the safety headers every answer has,
/// the caching rule, and `extra` (Content-Disposition, Accept-Ranges,
/// ETag, Last-Modified, Content-Range). The body (none for a HEAD or a 416)
/// is written by the caller after it. `content_length` is the body's.
pub struct FileHead<'a> {
    pub status: u16,
    pub content_type: &'a str,
    pub content_length: u64,
    pub cache_control: &'a str,
    pub extra: Vec<(&'static str, String)>,
}

/// Writes `head`. No header value may hold a CR or LF (a name is encoded
/// before it gets here); one that does is dropped rather than sent.
pub fn write_file_head(w: &mut impl Write, head: &FileHead) -> io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: {}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
        head.status,
        reason(head.status),
        head.content_type,
        head.content_length,
        head.cache_control
    );
    for (name, value) in &head.extra {
        if value.bytes().any(|b| b == b'\r' || b == b'\n') {
            continue;
        }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    w.write_all(out.as_bytes())?;
    w.flush()
}

/// A cookie the page cannot read and no other site can send.
pub fn session_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}; HttpOnly; SameSite=Strict; Path=/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Parsed, HttpError> {
        parse_head(s.as_bytes())
    }

    fn req(s: &str) -> (Request, usize) {
        match parse(s) {
            Ok(Parsed::Done(r, n)) => (r, n),
            other => panic!("{other:?}"),
        }
    }

    fn part(start: u64, end_incl: u64, open_ended: bool) -> RangeAns {
        RangeAns::Partial {
            start,
            end_incl,
            open_ended,
        }
    }

    #[test]
    fn a_head_request_parses_as_head() {
        let (r, _) = req("HEAD /api/file/x HTTP/1.1\r\nHost: a\r\n\r\n");
        assert_eq!(r.method, Method::Head);
    }

    #[test]
    fn a_range_is_a_closed_an_open_or_a_suffix_part_and_clamped() {
        assert_eq!(parse_range("bytes=10-19", 100), part(10, 19, false));
        assert_eq!(parse_range("bytes=0-0", 100), part(0, 0, false));
        // The end is clamped to the last byte.
        assert_eq!(parse_range("bytes=90-5000", 100), part(90, 99, false));
        // Open ended: to the end.
        assert_eq!(parse_range("bytes=40-", 100), part(40, 99, true));
        assert_eq!(parse_range("bytes=99-", 100), part(99, 99, true));
        // A suffix: the last n bytes, clamped to the length.
        assert_eq!(parse_range("bytes=-10", 100), part(90, 99, false));
        assert_eq!(parse_range("bytes=-500", 100), part(0, 99, false));
        assert_eq!(parse_range("bytes=-100", 100), part(0, 99, false));
        // Case of the unit and spaces round it do not matter.
        assert_eq!(parse_range(" Bytes = 1-2 ", 100), part(1, 2, false));
    }

    #[test]
    fn a_bad_or_unsatisfiable_range_is_416() {
        let bad = [
            "bytes=100-",
            "bytes=100-200",
            "bytes=999999-",
            "bytes=20-10",
            "bytes=-0",
            "bytes=-",
            "bytes=",
            "bytes=-x",
            "bytes=a-b",
            "bytes=1-b",
            "bytes=1x-2",
            "bytes=+1-2",
            "bytes=1--2",
            "bytes=18446744073709551616-",
            "bytes=0-18446744073709551616",
            "bytes=-18446744073709551616",
            "items=0-5",
            "0-5",
            "bytes 0-5",
            "bytes",
            "",
        ];
        for b in bad {
            assert_eq!(parse_range(b, 100), RangeAns::Unsatisfiable, "{b:?}");
        }
        // Any range on an empty file.
        for b in ["bytes=0-", "bytes=0-0", "bytes=-1"] {
            assert_eq!(parse_range(b, 0), RangeAns::Unsatisfiable, "{b:?}");
        }
        // The very last byte of a huge file is fine.
        assert_eq!(
            parse_range("bytes=18446744073709551614-", u64::MAX),
            part(u64::MAX - 1, u64::MAX - 1, true)
        );
    }

    #[test]
    fn a_multi_range_is_ignored_and_the_whole_file_goes() {
        assert_eq!(parse_range("bytes=0-1,5-6", 100), RangeAns::Full);
        assert_eq!(parse_range("bytes=0-1, 5-6", 100), RangeAns::Full);
        assert_eq!(parse_range("bytes=0-1,", 100), RangeAns::Full);
    }

    #[test]
    fn an_if_range_that_does_not_match_gets_the_whole_file() {
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);
        let tag = etag(100, Some(at));
        let date = http_date(Some(at)).expect("date");
        let ask = |range, cond| answer_range(range, cond, 100, &tag, &date);
        assert_eq!(ask(None, None), RangeAns::Full);
        assert_eq!(ask(Some("bytes=10-"), None), part(10, 99, true));
        assert_eq!(ask(Some("bytes=10-"), Some(&tag)), part(10, 99, true));
        assert_eq!(ask(Some("bytes=10-"), Some(&date)), part(10, 99, true));
        assert_eq!(ask(Some("bytes=10-"), Some("\"other\"")), RangeAns::Full);
        // A stale condition beats even an unsatisfiable range: 200, not 416.
        assert_eq!(ask(Some("bytes=500-"), Some("\"other\"")), RangeAns::Full);
        assert_eq!(ask(Some("bytes=500-"), None), RangeAns::Unsatisfiable);
    }

    #[test]
    fn the_etag_holds_size_and_time_and_the_date_is_an_http_date() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        assert_eq!(etag(255, None), "\"ff-0\"");
        assert_ne!(etag(255, Some(t)), etag(255, None));
        assert_ne!(etag(256, Some(t)), etag(255, Some(t)));
        assert_eq!(
            http_date(Some(t)).as_deref(),
            Some("Tue, 14 Nov 2023 22:13:20 GMT")
        );
        assert_eq!(http_date(None), None);
        assert!(etag(1, Some(t)).starts_with('"') && etag(1, Some(t)).ends_with('"'));
    }

    #[test]
    fn a_disposition_keeps_every_name_inside_one_header_line() {
        assert_eq!(
            content_disposition("attachment", "denah-lantai-2.pdf"),
            "attachment; filename=\"denah-lantai-2.pdf\"; filename*=UTF-8''denah-lantai-2.pdf"
        );
        assert_eq!(
            content_disposition("inline", "foto lokasi.png"),
            "inline; filename=\"foto lokasi.png\"; filename*=UTF-8''foto%20lokasi.png"
        );
        let nasty = [
            "a\r\nSet-Cookie: x=1.txt",
            "quote\"and\\slash.txt",
            "emoji \u{1F600}.png",
            "catatan ruang \u{e9}.txt",
            "\u{30c6}\u{30b9}\u{30c8}\u{30d5}\u{30a1}\u{30a4}\u{30eb}.pdf",
            "tab\there\u{7f}.txt",
            "50% off; filename=evil.txt",
        ];
        for name in nasty {
            let d = content_disposition("attachment", name);
            assert!(d.is_ascii(), "{d}");
            assert!(!d.contains('\r') && !d.contains('\n'), "{d:?}");
            assert!(d.starts_with("attachment; filename=\""));
            // Between the quotes there is no quote or backslash of the name.
            let inner = &d["attachment; filename=\"".len()..];
            let end = inner.find("\"; filename*=").expect("fallback ends");
            assert!(!inner[..end].contains('\\') && !inner[..end].contains('"'), "{d}");
            // The encoded part holds only attr-chars and %XX.
            let enc = &d[d.find("UTF-8''").expect("star") + 7..];
            assert!(
                enc.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~%".contains(&b)),
                "{enc}"
            );
        }
        // The encoding is the name's UTF-8 bytes.
        let d = content_disposition("attachment", "catatan ruang \u{e9}.txt");
        assert!(d.ends_with("UTF-8''catatan%20ruang%20%C3%A9.txt"), "{d}");
        assert!(d.contains("filename=\"catatan ruang _.txt\""), "{d}");
        let d = content_disposition("attachment", "a\r\nb.txt");
        assert!(d.contains("a%0D%0Ab.txt"), "{d}");
        assert!(d.contains("filename=\"a__b.txt\""), "{d}");
    }

    #[test]
    fn a_file_is_typed_by_its_extension_and_svg_is_never_a_page() {
        for (name, mime) in [
            ("a.PNG", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.bmp", "image/bmp"),
            ("a.heic", "image/heic"),
            ("a.avif", "image/avif"),
            ("a.tif", "image/tiff"),
            ("a.tiff", "image/tiff"),
            ("a.ico", "image/x-icon"),
            ("a.mp4", "video/mp4"),
            ("a.mov", "video/quicktime"),
            ("a.m4v", "video/x-m4v"),
            ("a.webm", "video/webm"),
            ("a.mp3", "audio/mpeg"),
            ("a.m4a", "audio/mp4"),
            ("a.pdf", "application/pdf"),
        ] {
            assert_eq!(mime_for(name), mime, "{name}");
        }
        for other in ["a.svg", "a.html", "a.js", "a.xlsx", "noext", "a.", ".png.exe"] {
            assert_eq!(mime_for(other), "application/octet-stream", "{other}");
        }
        assert!(is_viewable("a.png") && is_viewable("A.JPG"));
        assert!(!is_viewable("a.svg"), "SVG can carry script");
        assert!(!is_viewable("a.pdf") && !is_viewable("a.mp4") && !is_viewable("noext"));
    }

    #[test]
    fn a_file_head_has_the_safety_headers_and_drops_a_header_with_a_line_break() {
        let mut out = Vec::new();
        write_file_head(
            &mut out,
            &FileHead {
                status: 206,
                content_type: "video/mp4",
                content_length: 10,
                cache_control: "no-store",
                extra: vec![
                    ("Accept-Ranges", "bytes".into()),
                    ("Content-Range", "bytes 0-9/100".into()),
                    ("X-Bad", "a\r\nSet-Cookie: x".into()),
                ],
            },
        )
        .expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        for h in [
            "Content-Type: video/mp4",
            "Content-Length: 10",
            "Connection: close",
            "Cache-Control: no-store",
            "X-Content-Type-Options: nosniff",
            "Referrer-Policy: no-referrer",
            "Accept-Ranges: bytes",
            "Content-Range: bytes 0-9/100",
        ] {
            assert!(text.contains(h), "{h}\n{text}");
        }
        assert!(!text.contains("Set-Cookie") && !text.contains("X-Bad"));
        assert!(text.ends_with("\r\n\r\n"));
        for (status, word) in [(410, "Gone"), (416, "Range Not Satisfiable")] {
            let mut out = Vec::new();
            write_file_head(
                &mut out,
                &FileHead {
                    status,
                    content_type: "text/plain",
                    content_length: 0,
                    cache_control: "no-store",
                    extra: Vec::new(),
                },
            )
            .expect("write");
            let text = String::from_utf8(out).expect("utf8");
            assert!(text.starts_with(&format!("HTTP/1.1 {status} {word}\r\n")), "{text}");
        }
    }

    #[test]
    fn a_get_is_parsed_with_case_insensitive_headers_and_cookies() {
        let text = "GET /s/abc?x=1 HTTP/1.1\r\nHost: 192.168.1.2\r\nCOOKIE: vp=aaa; vs=bbb\r\nUser-Agent:  Test \r\n\r\nrest";
        let (r, n) = req(text);
        assert_eq!(r.method, Method::Get);
        assert_eq!(r.path, "/s/abc");
        assert_eq!(r.header("user-agent"), Some("Test"));
        assert_eq!(r.cookie("vp"), Some("aaa"));
        assert_eq!(r.cookie("vs"), Some("bbb"));
        assert_eq!(r.cookie("x"), None);
        assert_eq!(&text[n..], "rest");
    }

    #[test]
    fn a_partial_head_asks_for_more() {
        assert_eq!(parse("GET / HTTP/1.1\r\nHost: x\r\n"), Ok(Parsed::Need));
        assert_eq!(parse(""), Ok(Parsed::Need));
    }

    #[test]
    fn a_head_over_the_cap_is_431_with_or_without_its_end() {
        let big = "a".repeat(MAX_HEAD);
        assert_eq!(
            parse(&format!("GET / HTTP/1.1\r\nX: {big}\r\n\r\n")),
            Err(HttpError::HeadTooLarge)
        );
        // Never ends: the cap still stops it.
        assert_eq!(
            parse(&format!("GET / HTTP/1.1\r\nX: {big}")),
            Err(HttpError::HeadTooLarge)
        );
        // Just under the cap is fine.
        let ok = "a".repeat(MAX_HEAD - 40);
        assert!(matches!(
            parse(&format!("GET / HTTP/1.1\r\nX: {ok}\r\n\r\n")),
            Ok(Parsed::Done(..))
        ));
        assert_eq!(HttpError::HeadTooLarge.status(), 431);
    }

    #[test]
    fn too_many_headers_are_431() {
        let mut s = String::from("GET / HTTP/1.1\r\n");
        for i in 0..100 {
            s.push_str(&format!("H{i}: v\r\n"));
        }
        s.push_str("\r\n");
        assert_eq!(parse(&s), Err(HttpError::HeadTooLarge));
    }

    #[test]
    fn a_post_needs_content_length() {
        assert_eq!(
            parse("POST /api/upload HTTP/1.1\r\nHost: x\r\n\r\n"),
            Err(HttpError::LengthRequired)
        );
        let (r, _) = req("POST /api/upload HTTP/1.1\r\ncontent-length: 12\r\n\r\n");
        assert_eq!(r.content_length, Some(12));
        assert_eq!(HttpError::LengthRequired.status(), 411);
    }

    #[test]
    fn transfer_encoding_and_a_bad_length_are_400() {
        assert_eq!(
            parse("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 3\r\n\r\n"),
            Err(HttpError::Bad)
        );
        for bad in ["-1", "1e3", "", "0x10", "99999999999999999999999", "1 2"] {
            assert_eq!(
                parse(&format!("POST / HTTP/1.1\r\nContent-Length: {bad}\r\n\r\n")),
                Err(HttpError::Bad),
                "{bad:?}"
            );
        }
        // Two lengths that disagree: smuggling, refused.
        assert_eq!(
            parse("POST / HTTP/1.1\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\n"),
            Err(HttpError::Bad)
        );
    }

    #[test]
    fn garbage_is_refused_without_a_panic() {
        let cases: [&[u8]; 10] = [
            b"\r\n\r\n",
            b"GET\r\n\r\n",
            b"GET / \r\n\r\n",
            b"GET / HTTP/9\r\n\r\n",
            b"GET x HTTP/1.1\r\n\r\n",
            b"GET / HTTP/1.1\r\nno colon here\r\n\r\n",
            b"GET / HTTP/1.1\r\n: empty\r\n\r\n",
            b"GET / HTTP/1.1\r\nbad name: v\r\n\r\n",
            b"\xff\xfe\x00\x01\r\n\r\n",
            b"GET / HTTP/1.1 extra\r\n\r\n",
        ];
        for c in cases {
            assert_eq!(parse_head(c), Err(HttpError::Bad), "{c:?}");
        }
    }

    #[test]
    fn every_prefix_and_a_pile_of_noise_never_panic() {
        let full = b"POST /api/upload?x=1 HTTP/1.1\r\nHost: a\r\nCookie: vs=1\r\nContent-Length: 5\r\n\r\nhello";
        for i in 0..=full.len() {
            let _ = parse_head(&full[..i]);
        }
        let mut seed = 12345u32;
        for _ in 0..500 {
            let len = {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 20) as usize % 300
            };
            let buf: Vec<u8> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 24) as u8
                })
                .collect();
            let _ = parse_head(&buf);
            let mut with_end = buf.clone();
            with_end.extend_from_slice(b"\r\n\r\n");
            let _ = parse_head(&with_end);
        }
        // A huge buffer is stopped at once.
        assert_eq!(parse_head(&vec![b'a'; 1 << 20]), Err(HttpError::HeadTooLarge));
    }

    #[test]
    fn a_response_carries_the_safety_headers_and_the_cookies() {
        let r = Response::json(200, "{\"ok\":true}".into())
            .with_cookie(session_cookie("vs", "abc"))
            .with_cookie(session_cookie("other", "x"));
        let mut out = Vec::new();
        r.write_to(&mut out).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        for h in [
            "Connection: close",
            "Cache-Control: no-store",
            "X-Content-Type-Options: nosniff",
            "Referrer-Policy: no-referrer",
            "Content-Length: 11",
            "Set-Cookie: vs=abc; HttpOnly; SameSite=Strict; Path=/",
            "Set-Cookie: other=x; HttpOnly; SameSite=Strict; Path=/",
        ] {
            assert!(text.contains(h), "{h}\n{text}");
        }
        assert!(text.ends_with("\r\n\r\n{\"ok\":true}"));
    }

    #[test]
    fn a_not_found_is_bare() {
        let mut out = Vec::new();
        Response::not_found().write_to(&mut out).expect("write");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(text.contains("Content-Length: 0"));
        assert!(text.ends_with("\r\n\r\n"));
    }
}
