//! The little HTTP/1.1 the phone link speaks: a request head parsed from
//! bytes with hard limits, and a response writer. No sockets here, so every
//! limit is tested without a network, and nothing on any input panics.

use std::io::{self, Write};

/// The longest request head accepted (request line and headers), in bytes.
pub const MAX_HEAD: usize = 16 * 1024;
/// More headers than this is refused (431) too.
const MAX_HEADERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
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
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
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
        }
    }

    pub fn json(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
            cookies: Vec::new(),
        }
    }

    pub fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.into_bytes(),
            cookies: Vec::new(),
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
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len()
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

/// A cookie the page cannot read and no other site can send.
pub fn session_cookie(name: &str, value: &str) -> String {
    format!("{name}={value}; HttpOnly; SameSite=Strict; Path=/")
}

/// The same cookie, told to go.
pub fn clear_cookie(name: &str) -> String {
    format!("{name}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
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
            .with_cookie(clear_cookie("vp"));
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
            "Set-Cookie: vp=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
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
