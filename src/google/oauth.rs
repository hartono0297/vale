//! Google OAuth for an installed app: PKCE (S256) with a loopback redirect,
//! token refresh, and a DPAPI-encrypted token file. Tokens, codes and the
//! client secret never appear in logs or error strings.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows::Win32::Security::Cryptography::{
    BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, BCryptHash,
};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

use super::client::{self, Client};
use super::http::{Http, form_encode, url_decode};
use crate::config::{self, Config};
use crate::dpapi;
use crate::i18n::{self, Key};

/// Read the calendar list (to know which calendars are `selected`) and
/// list/insert/patch events. Source: Google Calendar API v3 reference,
/// calendarList.list and events.insert "Authorization" sections.
pub const SCOPES: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events";

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// A stray connection (browser preconnect) that sends nothing is dropped after this.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);
const REFRESH_MARGIN_SECS: i64 = 60;
/// `parse_redirect` error for a request that is not the OAuth redirect (e.g. `/favicon.ico`).
const NO_CODE: &str = "no_code";

/// Google's own messages are cut to this many characters before they are shown.
const MESSAGE_MAX: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// No client ID in the config, or Google rejected the client credentials.
    NotConfigured,
    /// No stored tokens (or the user cancelled the consent screen).
    NeedsLogin,
    /// The refresh token was revoked or expired (`invalid_grant`), or the
    /// token lacks a scope a Calendar call needs.
    Reconnect,
    /// The consent screen came back without every scope in `SCOPES` ticked.
    /// The token is not kept.
    MissingScopes,
    /// A Calendar API call answered 401. The sync thread refreshes the token
    /// and retries once; a second 401 counts as `Reconnect`.
    Unauthorized,
    /// Google answered and refused for a reason a retry will not fix (the
    /// Calendar API not enabled, a bad request): its short message, in a form
    /// fit for the island.
    Google(String),
    /// The request never got an answer from Google (or a 5xx / rate limit).
    Network(String),
    /// The stored tokens came from the other client (Vale's built-in one or
    /// the user's own): a new login is needed.
    ClientChanged,
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
    /// Space-separated scopes this token carries.
    pub scope: Option<String>,
}

/// At most `MESSAGE_MAX` characters of `message`.
pub fn short(message: &str) -> String {
    message.chars().take(MESSAGE_MAX).collect()
}

/// `MissingScopes` when `granted` (the token response's `scope`) lacks one of
/// `SCOPES`. Google lets the user untick each scope on the consent screen.
/// An absent field cannot be judged and passes; a Calendar call would then
/// answer 403 and read as `Reconnect`.
pub fn check_scopes(granted: Option<&str>) -> Result<(), AuthError> {
    /// Covers both requested scopes.
    const FULL_CALENDAR: &str = "https://www.googleapis.com/auth/calendar";
    let Some(granted) = granted else {
        return Ok(());
    };
    let granted: Vec<&str> = granted.split_whitespace().collect();
    let all = granted.contains(&FULL_CALENDAR)
        || SCOPES
            .split_whitespace()
            .all(|needed| granted.contains(&needed));
    if all {
        Ok(())
    } else {
        Err(AuthError::MissingScopes)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at_unix: i64,
    /// The client ID that issued them; empty in token files from before it
    /// was recorded (always the user's own client then).
    #[serde(default)]
    pub client: String,
}

/// Redacted, so a stray `{:?}` cannot leak a token.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("access", &"<redacted>")
            .field("refresh", &"<redacted>")
            .field("expires_at_unix", &self.expires_at_unix)
            .field("client", &self.client)
            .finish()
    }
}

/// Base64 with the URL-safe alphabet and no padding (RFC 4648 section 5).
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &b)| acc | u32::from(b) << (16 - 8 * i));
        // 1, 2 or 3 input bytes give 2, 3 or 4 output characters.
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// SHA-256 of `bytes` (BCrypt).
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut digest = [0u8; 32];
    // The SHA-256 pseudo-handle needs no setup and does not fail on Windows 10+.
    // If it ever did, Google would reject the code exchange (PKCE) and the
    // updater would report a checksum mismatch: both surface as errors.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut digest) };
    debug_assert!(status.is_ok(), "BCryptHash failed: {status:?}");
    digest
}

/// PKCE S256: `base64url(SHA-256(verifier))`.
pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&sha256(verifier.as_bytes()))
}

pub fn auth_url(client_id: &str, redirect: &str, challenge: &str, state: &str) -> String {
    let query = form_encode(&[
        ("client_id", client_id),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        // A refresh token on every consent, so a re-login always replaces a revoked one.
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", state),
    ]);
    format!("{AUTH_ENDPOINT}?{query}")
}

/// Parses the loopback request line, e.g. `GET /?state=..&code=.. HTTP/1.1`.
/// `Err` carries Google's `error` value and the request's `state` (to be
/// checked like a code's), or `no_code` for any other request.
pub fn parse_redirect(request_line: &str) -> Result<(String, String), (String, Option<String>)> {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Err((NO_CODE.into(), None));
    };
    let Some((_, query)) = target.split_once('?') else {
        return Err((NO_CODE.into(), None));
    };
    let (mut code, mut state, mut error) = (None, None, None);
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let slot = match key {
            "code" => &mut code,
            "state" => &mut state,
            "error" => &mut error,
            _ => continue,
        };
        *slot = Some(url_decode(value));
    }
    if let Some(error) = error {
        return Err((error, state));
    }
    match (code, state) {
        (Some(code), Some(state)) => Ok((code, state)),
        _ => Err((NO_CODE.into(), None)),
    }
}

/// Google omits `refresh_token` on refresh responses, so the old one is kept.
/// `None` when neither side has a refresh token.
pub fn merge_tokens(old: Option<&Tokens>, resp: TokenResponse, now: i64) -> Option<Tokens> {
    let refresh = resp
        .refresh_token
        .filter(|r| !r.is_empty())
        .or_else(|| old.map(|t| t.refresh.clone()))?;
    Some(Tokens {
        access: resp.access_token,
        refresh,
        expires_at_unix: now.saturating_add(i64::try_from(resp.expires_in).unwrap_or(i64::MAX)),
        client: old.map(|t| t.client.clone()).unwrap_or_default(),
    })
}

/// The stored tokens, if `client` issued them: `NeedsLogin` without any,
/// `ClientChanged` when the other client did.
fn usable(tokens: Option<Tokens>, client: &Client) -> Result<Tokens, AuthError> {
    let tokens = tokens.ok_or(AuthError::NeedsLogin)?;
    if client::issued_to(&tokens.client, client) {
        Ok(tokens)
    } else {
        Err(AuthError::ClientChanged)
    }
}

/// Maps a token-endpoint failure using only its `error` / `error_description`
/// fields, so nothing else from the body ends up in a message.
pub fn classify_token_error(status: u16, body: &str) -> AuthError {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct ErrorBody {
        error: String,
        error_description: String,
    }
    let parsed: ErrorBody = serde_json::from_str(body).unwrap_or_default();
    let message = if parsed.error_description.is_empty() {
        parsed.error.clone()
    } else {
        format!("{} ({})", parsed.error, parsed.error_description)
    };
    match parsed.error.as_str() {
        "invalid_grant" => AuthError::Reconnect,
        "invalid_client" | "unauthorized_client" => AuthError::NotConfigured,
        "" => AuthError::Network(format!("HTTP {status}")),
        // A refusal (bad request, wrong redirect): retrying will not help.
        _ if (400..500).contains(&status) && status != 429 => AuthError::Google(short(&message)),
        _ => AuthError::Network(format!("HTTP {status}: {}", short(&message))),
    }
}

/// True when the access token expires within the refresh margin.
fn needs_refresh(tokens: &Tokens, now: i64) -> bool {
    tokens.expires_at_unix - now <= REFRESH_MARGIN_SECS
}

/// `n_bytes` from the system RNG, base64url encoded; `None` if the RNG fails.
fn try_random_urlsafe(n_bytes: usize) -> Option<String> {
    let mut buf = vec![0u8; n_bytes];
    let status = unsafe { BCryptGenRandom(None, &mut buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    status.is_ok().then(|| base64url(&buf))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn network(e: windows::core::Error) -> AuthError {
    AuthError::Network(e.message())
}

fn io_error(e: io::Error) -> AuthError {
    AuthError::Network(e.to_string())
}

/// Interactive login: opens the consent page in the browser, waits up to five
/// minutes for the loopback redirect, exchanges the code, checks the granted
/// scopes and saves the tokens, and only then tells the browser how it went.
/// Blocks; run it on a worker thread.
pub fn login(http: &Http, client: &Client) -> Result<Tokens, AuthError> {
    let (client_id, client_secret) = (client.id.as_str(), client.secret.as_str());
    if client_id.is_empty() {
        return Err(AuthError::NotConfigured);
    }
    let rng_failed = || AuthError::Network("system RNG failed".into());
    // 32 bytes give a 43-character verifier, the RFC 7636 minimum length.
    let verifier = try_random_urlsafe(32).ok_or_else(rng_failed)?;
    let state = try_random_urlsafe(16).ok_or_else(rng_failed)?;

    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
    let port = listener.local_addr().map_err(io_error)?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    open_browser(&auth_url(
        client_id,
        &redirect,
        &pkce_challenge(&verifier),
        &state,
    ))?;
    wait_for_code(&listener, &state, client.own, |code| {
        let (status, body) = http
            .post_form(
                TOKEN_ENDPOINT,
                &[
                    ("code", &code),
                    ("client_id", client_id),
                    ("client_secret", client_secret),
                    ("redirect_uri", &redirect),
                    ("grant_type", "authorization_code"),
                    ("code_verifier", &verifier),
                ],
            )
            .map_err(network)?;
        let tokens = Tokens {
            client: client_id.to_owned(),
            ..token_result(status, &body, None)?
        };
        save_tokens(&tokens)
            .map_err(|e| AuthError::Network(format!("could not save the token: {}", e.kind())))?;
        Ok(tokens)
    })
}

pub fn refresh(
    http: &Http,
    client_id: &str,
    client_secret: &str,
    tokens: &Tokens,
) -> Result<Tokens, AuthError> {
    let (status, body) = http
        .post_form(
            TOKEN_ENDPOINT,
            &[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("refresh_token", &tokens.refresh),
                ("grant_type", "refresh_token"),
            ],
        )
        .map_err(network)?;
    token_result(status, &body, Some(tokens))
}

/// The tokens in a token-endpoint answer. A token without every scope in
/// `SCOPES` is refused (`MissingScopes`).
fn token_result(status: u16, body: &str, old: Option<&Tokens>) -> Result<Tokens, AuthError> {
    if !(200..300).contains(&status) {
        return Err(classify_token_error(status, body));
    }
    let resp: TokenResponse = serde_json::from_str(body)
        .map_err(|_| AuthError::Network("unreadable token response".into()))?;
    check_scopes(resp.scope.as_deref())?;
    merge_tokens(old, resp, now_unix())
        .ok_or_else(|| AuthError::Network("Google sent no refresh token".into()))
}

/// A valid access token, refreshed (and saved) when it expires within a minute.
/// `Reconnect` and `MissingScopes` also delete the stored tokens.
pub fn access_token(http: &Http, cfg: &Config) -> Result<String, AuthError> {
    let client = client::for_config(cfg).ok_or(AuthError::NotConfigured)?;
    let tokens = usable(load_tokens(), &client)?;
    if !needs_refresh(&tokens, now_unix()) {
        return Ok(tokens.access);
    }
    refresh_and_store(http, &client, &tokens)
}

/// A new access token even though the stored one has not expired: Google
/// answered 401 to it (revoked, or the clock is off).
pub fn force_refresh(http: &Http, cfg: &Config) -> Result<String, AuthError> {
    let client = client::for_config(cfg).ok_or(AuthError::NotConfigured)?;
    let tokens = usable(load_tokens(), &client)?;
    refresh_and_store(http, &client, &tokens)
}

fn refresh_and_store(http: &Http, client: &Client, tokens: &Tokens) -> Result<String, AuthError> {
    match refresh(http, &client.id, &client.secret, tokens) {
        Ok(fresh) => {
            // A failed save only costs another refresh next time.
            let _ = save_tokens(&fresh);
            Ok(fresh.access)
        }
        Err(e @ (AuthError::Reconnect | AuthError::MissingScopes)) => {
            clear_tokens();
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// ShellExecute wants COM on the calling thread; a short-lived STA thread
/// keeps that apartment choice away from the blocking sync worker.
fn open_browser(url: &str) -> Result<(), AuthError> {
    let url = HSTRING::from(url);
    let opened = std::thread::spawn(move || unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = ShellExecuteW(None, w!("open"), &url, None, None, SW_SHOWNORMAL);
        if com.is_ok() {
            CoUninitialize();
        }
        // ShellExecute reports success as a value above 32.
        result.0 as isize > 32
    })
    .join()
    .unwrap_or(false);
    if opened {
        Ok(())
    } else {
        Err(AuthError::Network("could not open the browser".into()))
    }
}

/// Accepts loopback connections until the OAuth redirect arrives or the
/// login times out. Other requests (favicon, empty preconnects) are skipped.
/// The redirect's code goes through `exchange` before the browser gets its
/// answer, so the page says "connected" only when the login really worked.
/// `own`: the login uses the user's own client, not Vale's built-in one.
fn wait_for_code<T>(
    listener: &TcpListener,
    expected_state: &str,
    own: bool,
    mut exchange: impl FnMut(String) -> Result<T, AuthError>,
) -> Result<T, AuthError> {
    listener.set_nonblocking(true).map_err(io_error)?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(code) = read_redirect(&stream, expected_state) {
                    let outcome = code.and_then(&mut exchange);
                    let message = page_key(&outcome, own);
                    respond(&stream, "200 OK", &i18n::oauth_page(i18n::lang(), message));
                    return outcome;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(AuthError::Network("login timed out".into()));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(io_error(e)),
        }
    }
}

/// What the browser page after a login says. A rejected client is advice,
/// like `sync::rejected`: the user's own → check Settings; Vale's built-in
/// one → try later or use an own Client ID.
fn page_key<T>(outcome: &Result<T, AuthError>, own: bool) -> Key {
    match outcome {
        Ok(_) => Key::OauthConnected,
        Err(AuthError::MissingScopes) => Key::OauthScopes,
        Err(AuthError::NeedsLogin) => Key::OauthCancelled,
        Err(AuthError::NotConfigured) if own => Key::OauthRejected,
        Err(AuthError::NotConfigured) => Key::OauthBuiltInRejected,
        Err(_) => Key::OauthFailed,
    }
}

/// The redirect's code, or why the login failed; the caller answers the
/// browser. `None` (already answered 404) when the request was not the
/// redirect and waiting should go on: that includes an error whose `state`
/// is not this login's, so a forged `?error=` cannot end it.
fn read_redirect(stream: &TcpStream, expected_state: &str) -> Option<Result<String, AuthError>> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT)).ok()?;
    let mut reader = BufReader::new(stream.take(16 * 1024));
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    // Drain the headers: closing with unread data resets the connection and
    // the browser would show an error instead of the page.
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|n| n > 0) && !header.trim_end().is_empty() {
        header.clear();
    }

    Some(match parse_redirect(request_line.trim_end()) {
        Err((e, state)) if e == NO_CODE || state.as_deref() != Some(expected_state) => {
            respond(stream, "404 Not Found", "");
            return None;
        }
        Err((e, _)) if e == "access_denied" => Err(AuthError::NeedsLogin),
        Err((e, _)) => Err(AuthError::Network(format!(
            "login error: {}",
            e.chars().take(64).collect::<String>()
        ))),
        // A code with another login's state is not ours either: ignored,
        // so a page probing the port cannot end the real login.
        Ok((_, state)) if state != expected_state => {
            respond(stream, "404 Not Found", "");
            return None;
        }
        Ok((code, _)) => Ok(code),
    })
}

fn respond(mut stream: &TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

/// `%APPDATA%\Vale\google-token.bin`, next to the config file.
fn token_path() -> PathBuf {
    config::path().with_file_name("google-token.bin")
}

pub fn load_tokens() -> Option<Tokens> {
    load_tokens_from(&token_path())
}

pub fn save_tokens(tokens: &Tokens) -> io::Result<()> {
    save_tokens_to(&token_path(), tokens)
}

pub fn clear_tokens() {
    let _ = std::fs::remove_file(token_path());
}

fn load_tokens_from(path: &Path) -> Option<Tokens> {
    let sealed = std::fs::read(path).ok()?;
    let plain = dpapi::unprotect(&sealed).ok()?;
    serde_json::from_slice(&plain).ok()
}

/// Written to a temp file and renamed, so a crash never leaves half a token file.
fn save_tokens_to(path: &Path, tokens: &Tokens) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let plain = serde_json::to_vec(tokens).map_err(io::Error::other)?;
    // DPAPI, current-user scope: only this Windows account can decrypt it.
    let sealed = dpapi::protect(&plain, w!("Vale Google token"))?;
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, sealed)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::client::Client;

    #[test]
    fn pkce_rfc7636_vector() {
        assert_eq!(
            // RFC 7636 Appendix B (the plan's copy of the verifier had a typo).
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn base64url_has_no_padding() {
        assert_eq!(base64url(b"ab"), "YWI");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn auth_url_has_required_params() {
        let url = auth_url(
            "cid.apps.googleusercontent.com",
            "http://127.0.0.1:5555",
            "chal",
            "xyz",
        );
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        for part in [
            "client_id=cid.apps.googleusercontent.com",
            "response_type=code",
            "code_challenge=chal",
            "code_challenge_method=S256",
            "access_type=offline",
            "prompt=consent",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A5555",
            "state=xyz",
        ] {
            assert!(url.contains(part), "missing {part} in {url}");
        }
        let scope = format!(
            "scope={}",
            SCOPES
                .replace(':', "%3A")
                .replace('/', "%2F")
                .replace(' ', "%20")
        );
        assert!(url.contains(&scope), "missing encoded scope in {url}");
    }

    #[test]
    fn parse_redirect_ok_and_error() {
        assert_eq!(
            parse_redirect("GET /?state=s1&code=4%2F0Ab HTTP/1.1"),
            Ok(("4/0Ab".to_string(), "s1".to_string()))
        );
        assert_eq!(
            parse_redirect("GET /?error=access_denied&state=s1 HTTP/1.1"),
            Err(("access_denied".to_string(), Some("s1".to_string())))
        );
        assert_eq!(
            parse_redirect("GET /?error=access_denied HTTP/1.1"),
            Err(("access_denied".to_string(), None))
        );
        assert_eq!(
            parse_redirect("GET /favicon.ico HTTP/1.1"),
            Err((NO_CODE.to_string(), None))
        );
    }

    #[test]
    fn token_response_without_refresh_keeps_old_refresh() {
        let old = Tokens {
            access: "old-a".into(),
            refresh: "old-r".into(),
            expires_at_unix: 10,
            client: "mine".into(),
        };
        let resp: TokenResponse = serde_json::from_str(
            r#"{"access_token":"new-a","expires_in":3599,"token_type":"Bearer"}"#,
        )
        .unwrap();
        let merged = merge_tokens(Some(&old), resp, 1_000).unwrap();
        assert_eq!(
            merged,
            Tokens {
                access: "new-a".into(),
                refresh: "old-r".into(),
                expires_at_unix: 4_599,
                client: "mine".into(),
            }
        );

        let fresh: TokenResponse =
            serde_json::from_str(r#"{"access_token":"a","expires_in":60}"#).unwrap();
        assert!(
            merge_tokens(None, fresh, 0).is_none(),
            "no refresh token at all"
        );
    }

    #[test]
    fn tokens_are_used_only_with_the_client_that_issued_them() {
        let own = Client {
            id: "mine".into(),
            secret: "s".into(),
            own: true,
        };
        let tokens = |client: &str| Tokens {
            access: "a".into(),
            refresh: "r".into(),
            expires_at_unix: 0,
            client: client.into(),
        };
        assert_eq!(usable(None, &own), Err(AuthError::NeedsLogin));
        assert_eq!(usable(Some(tokens("mine")), &own), Ok(tokens("mine")));
        // Signed in with Vale's client, now the user's own: log in again.
        assert_eq!(
            usable(Some(tokens("vale")), &own),
            Err(AuthError::ClientChanged)
        );
        // A token file from before the client was recorded still works.
        assert_eq!(usable(Some(tokens("")), &own), Ok(tokens("")));
        let old: Tokens =
            serde_json::from_str(r#"{"access":"a","refresh":"r","expires_at_unix":0}"#).unwrap();
        assert_eq!(old, tokens(""));
    }

    #[test]
    fn invalid_grant_maps_to_reconnect() {
        assert!(matches!(
            classify_token_error(400, r#"{"error":"invalid_grant"}"#),
            AuthError::Reconnect
        ));
    }

    #[test]
    fn refresh_only_within_a_minute_of_expiry() {
        let t = Tokens {
            access: "a".into(),
            refresh: "r".into(),
            expires_at_unix: 1_000,
            client: String::new(),
        };
        assert!(!needs_refresh(&t, 939));
        assert!(needs_refresh(&t, 940));
        assert!(needs_refresh(&t, 2_000));
    }

    /// Sends one raw request to the loopback listener and returns the raw response.
    fn send_raw(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    /// Runs the loopback wait against `requests`; `exchange` stands in for the
    /// token exchange. The client records whether the exchange had finished
    /// when each response arrived.
    fn loopback_run(
        requests: &'static [&'static str],
        exchange: fn(String) -> Result<String, AuthError>,
    ) -> (Result<String, AuthError>, Vec<(String, bool)>) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let exchanged = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&exchanged);
        let client = std::thread::spawn(move || {
            requests
                .iter()
                .map(|r| {
                    let response = send_raw(port, r);
                    (response, seen.load(Ordering::SeqCst))
                })
                .collect::<Vec<_>>()
        });
        let outcome = wait_for_code(&listener, "s1", true, |code| {
            // Slow enough that an early answer would beat it.
            std::thread::sleep(Duration::from_millis(200));
            let result = exchange(code);
            exchanged.store(true, Ordering::SeqCst);
            result
        });
        (outcome, client.join().unwrap())
    }

    #[test]
    fn loopback_skips_other_requests_and_answers_the_redirect() {
        let (outcome, responses) = loopback_run(
            &[
                "GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                "GET /?state=s1&code=4%2F0Ab&scope=x HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: */*\r\n\r\n",
            ],
            |code| Ok(format!("tokens for {code}")),
        );
        assert_eq!(outcome, Ok("tokens for 4/0Ab".to_string()));
        assert!(responses[0].0.starts_with("HTTP/1.1 404"));
        assert!(responses[1].0.starts_with("HTTP/1.1 200 OK"));
        assert!(
            responses[1]
                .0
                .contains("Berhasil terhubung. Kembali ke island ya.")
        );
        assert!(responses[1].1, "answered only after the exchange");
    }

    #[test]
    fn loopback_shows_failure_when_the_exchange_fails() {
        let (outcome, responses) =
            loopback_run(&["GET /?state=s1&code=c HTTP/1.1\r\n\r\n"], |_| {
                Err(AuthError::Google("invalid_request".into()))
            });
        assert_eq!(outcome, Err(AuthError::Google("invalid_request".into())));
        assert!(responses[0].1, "answered only after the exchange");
        assert!(
            responses[0]
                .0
                .contains("Gagal terhubung — coba lagi dari island")
        );
        assert!(!responses[0].0.contains("Berhasil"));

        let (outcome, responses) =
            loopback_run(&["GET /?state=s1&code=c HTTP/1.1\r\n\r\n"], |_| {
                Err(AuthError::MissingScopes)
            });
        assert_eq!(outcome, Err(AuthError::MissingScopes));
        assert!(responses[0].0.contains("Izin Google belum lengkap"));
    }

    #[test]
    fn a_rejected_client_page_depends_on_whose_client_it_is() {
        let rejected: Result<(), AuthError> = Err(AuthError::NotConfigured);
        assert_eq!(page_key(&rejected, true), Key::OauthRejected);
        assert_eq!(page_key(&rejected, false), Key::OauthBuiltInRejected);
        // Everything else reads the same for both clients.
        for own in [true, false] {
            assert_eq!(page_key(&Ok(()), own), Key::OauthConnected);
            assert_eq!(
                page_key::<()>(&Err(AuthError::MissingScopes), own),
                Key::OauthScopes
            );
            assert_eq!(
                page_key::<()>(&Err(AuthError::NeedsLogin), own),
                Key::OauthCancelled
            );
            let other = Err(AuthError::Google("invalid_request".into()));
            assert_eq!(page_key::<()>(&other, own), Key::OauthFailed);
        }
        for lang in [i18n::Lang::Id, i18n::Lang::En] {
            let page = i18n::oauth_page(lang, Key::OauthBuiltInRejected);
            assert!(page.contains("Client ID"), "{page}");
        }
    }

    #[test]
    fn loopback_ignores_a_code_with_another_state() {
        // The forged code is never exchanged; the real redirect still works.
        let (outcome, responses) = loopback_run(
            &[
                "GET /?state=evil&code=bad HTTP/1.1\r\n\r\n",
                "GET /?state=s1&code=good HTTP/1.1\r\n\r\n",
            ],
            |code| {
                assert_eq!(code, "good", "only this login's code is exchanged");
                Ok(format!("tokens for {code}"))
            },
        );
        assert_eq!(outcome, Ok("tokens for good".to_string()));
        assert!(responses[0].0.starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn loopback_ignores_an_error_from_another_login() {
        // A forged error (wrong or no state) does not end the login; the
        // real redirect after it still goes through.
        let (outcome, responses) = loopback_run(
            &[
                "GET /?error=access_denied&state=evil HTTP/1.1\r\n\r\n",
                "GET /?error=access_denied HTTP/1.1\r\n\r\n",
                "GET /?state=s1&code=c HTTP/1.1\r\n\r\n",
            ],
            |code| Ok(format!("tokens for {code}")),
        );
        assert_eq!(outcome, Ok("tokens for c".to_string()));
        assert!(responses[0].0.starts_with("HTTP/1.1 404"));
        assert!(responses[1].0.starts_with("HTTP/1.1 404"));
        // This login's own error still ends it.
        let (outcome, _) = loopback_run(
            &["GET /?error=access_denied&state=s1 HTTP/1.1\r\n\r\n"],
            |_| panic!("no exchange after an error"),
        );
        assert_eq!(outcome, Err(AuthError::NeedsLogin));
    }

    const BOTH: &str = "https://www.googleapis.com/auth/calendar.events \
        https://www.googleapis.com/auth/calendar.calendarlist.readonly";

    #[test]
    fn both_scopes_are_required() {
        assert_eq!(check_scopes(Some(BOTH)), Ok(()));
        assert_eq!(check_scopes(Some(SCOPES)), Ok(()));
        assert_eq!(
            check_scopes(Some("https://www.googleapis.com/auth/calendar")),
            Ok(()),
            "the full calendar scope covers both"
        );
        assert_eq!(check_scopes(None), Ok(()), "absent field: cannot judge");
        for granted in [
            "https://www.googleapis.com/auth/calendar.events",
            "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
            "",
            "openid email",
        ] {
            assert_eq!(
                check_scopes(Some(granted)),
                Err(AuthError::MissingScopes),
                "{granted:?}"
            );
        }
    }

    #[test]
    fn a_token_without_both_scopes_is_not_kept() {
        let one = r#"{"access_token":"a","expires_in":3599,"refresh_token":"r",
            "scope":"https://www.googleapis.com/auth/calendar.events"}"#;
        assert_eq!(token_result(200, one, None), Err(AuthError::MissingScopes));
        let both = format!(
            r#"{{"access_token":"a","expires_in":3599,"refresh_token":"r","scope":"{BOTH}"}}"#
        );
        assert_eq!(token_result(200, &both, None).unwrap().access, "a");
        // A refresh answer without `scope` keeps working.
        let old = Tokens {
            access: "old".into(),
            refresh: "r".into(),
            expires_at_unix: 0,
            client: String::new(),
        };
        let plain = r#"{"access_token":"b","expires_in":3599}"#;
        assert_eq!(token_result(200, plain, Some(&old)).unwrap().access, "b");
    }

    #[test]
    fn token_endpoint_refusals_are_google_errors() {
        assert_eq!(
            classify_token_error(
                400,
                r#"{"error":"invalid_request","error_description":"Missing code verifier."}"#
            ),
            AuthError::Google("invalid_request (Missing code verifier.)".into())
        );
        assert_eq!(
            classify_token_error(401, r#"{"error":"invalid_client"}"#),
            AuthError::NotConfigured
        );
        assert_eq!(
            classify_token_error(503, r#"{"error":"backend_error"}"#),
            AuthError::Network("HTTP 503: backend_error".into())
        );
        assert_eq!(
            classify_token_error(502, "<html>"),
            AuthError::Network("HTTP 502".into())
        );
        let long = format!(
            r#"{{"error":"x","error_description":"{}"}}"#,
            "y".repeat(300)
        );
        let AuthError::Google(m) = classify_token_error(400, &long) else {
            panic!("not a Google error")
        };
        assert_eq!(m.chars().count(), MESSAGE_MAX);
    }

    /// Live request with bogus credentials: checks the WinRT client, form
    /// encoding and error mapping. Run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs network"]
    fn live_bogus_credentials_are_classified() {
        let http = Http::new().unwrap();
        let bogus = Tokens {
            access: "x".into(),
            refresh: "x".into(),
            expires_at_unix: 0,
            client: String::new(),
        };
        assert_eq!(
            refresh(&http, "bogus-client", "bogus-secret", &bogus),
            Err(AuthError::NotConfigured)
        );
        let (status, _) = http
            .get_json(
                "https://www.googleapis.com/calendar/v3/users/me/calendarList",
                Some("bogus"),
            )
            .unwrap();
        assert_eq!(status, 401);
    }

    #[test]
    fn dpapi_round_trip() {
        let path = std::env::temp_dir().join(format!("vale-token-test-{}.bin", std::process::id()));
        let tokens = Tokens {
            access: "ya29.access-token-text".into(),
            refresh: "1//refresh-token-text".into(),
            expires_at_unix: 1_790_000_000,
            client: String::new(),
        };
        save_tokens_to(&path, &tokens).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let loaded = load_tokens_from(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded, Some(tokens));
        let needle = b"access-token-text";
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle),
            "token stored in plain text"
        );
    }
}
