//! The phone link's rules, with no sockets and no clock of their own: the
//! token and how long it lives, the phones that joined (each with a cookie
//! of its own), what a cookie opens, the idle end and the batches the owner
//! cancelled. There is no approval: whoever opens the link while its token
//! is good joins at once, and the token is the owner's QR code on screen.
//! `now` is passed in and the random bytes come from an injected filler, so
//! every rule is tested without time or a network.

use std::collections::HashSet;

use std::time::{Duration, Instant};

use super::share::SharedSet;

/// A token is good for this long, whoever joined with it: it never lives
/// longer, so a code that was photographed stops working by itself. Phones
/// that joined keep their own cookie until the session ends.
pub const TOKEN_TTL: Duration = Duration::from_secs(300);
/// A session with no meaningful request for this long ends.
pub const IDLE_TTL: Duration = Duration::from_secs(600);
/// A sanity cap on the phones one session lets in. It is the only limit on
/// how many may join; past it a valid link is a bare 404.
pub const MAX_DEVICES: usize = 64;
/// Batch ids remembered, so a queued upload of a cancelled batch is refused.
const MAX_BATCHES: usize = 256;

/// Fills the buffer with cryptographically random bytes; `false` if it
/// cannot (nothing is then made from weak bytes).
pub type Fill = fn(&mut [u8]) -> bool;

#[derive(Clone, Debug)]
struct Device {
    id: u64,
    /// Its own random session cookie (`vs`).
    cookie: String,
}

/// What `join` made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Joined {
    pub device_id: u64,
    pub cookie: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinError {
    /// Wrong, expired or replaced token.
    BadToken,
    /// `MAX_DEVICES` phones are in already.
    Full,
    /// The system could not give random bytes.
    NoRandom,
}

/// The token's standing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenState {
    Valid,
    Expired,
}

/// Something `tick` found out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tick {
    TokenExpired,
    Idle,
}

pub struct Session {
    fill: Fill,
    token: String,
    token_made: Instant,
    expiry_told: bool,
    devices: Vec<Device>,
    next_id: u64,
    last_activity: Instant,
    seen_batches: Vec<String>,
    cancelled: HashSet<String>,
    /// The shelf files shared with the phones.
    shared: SharedSet,
}

/// Equal without stopping at the first difference.
pub(super) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn random_cookie(fill: Fill) -> Option<String> {
    let mut b = [0u8; 16];
    fill(&mut b).then(|| super::base64url(&b))
}

impl Session {
    pub fn new(now: Instant, fill: Fill) -> Option<Self> {
        let token = random_cookie(fill)?;
        Some(Self {
            fill,
            token,
            token_made: now,
            expiry_told: false,
            devices: Vec::new(),
            next_id: 1,
            last_activity: now,
            seen_batches: Vec::new(),
            cancelled: HashSet::new(),
            shared: SharedSet::new(),
        })
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn token_made(&self) -> Instant {
        self.token_made
    }

    /// A new token for the QR code: the old one stops working at once.
    /// Phones that joined stay in. `false` if the system gave no random
    /// bytes (the old token stays).
    pub fn new_token(&mut self, now: Instant) -> bool {
        let Some(token) = random_cookie(self.fill) else {
            return false;
        };
        self.token = token;
        self.token_made = now;
        self.expiry_told = false;
        true
    }

    pub fn token_state(&self, now: Instant) -> TokenState {
        if now.saturating_duration_since(self.token_made) >= TOKEN_TTL {
            TokenState::Expired
        } else {
            TokenState::Valid
        }
    }

    /// A phone opened the link with `presented`: with a good token it joins
    /// at once and gets a cookie of its own, and that is activity. The token
    /// is not touched: it still expires `TOKEN_TTL` after it was made.
    pub fn join(&mut self, presented: &str, now: Instant) -> Result<Joined, JoinError> {
        if !same(presented, &self.token) || self.token_state(now) != TokenState::Valid {
            return Err(JoinError::BadToken);
        }
        if self.devices.len() >= MAX_DEVICES {
            return Err(JoinError::Full);
        }
        let cookie = random_cookie(self.fill).ok_or(JoinError::NoRandom)?;
        let id = self.next_id;
        self.next_id += 1;
        self.devices.push(Device {
            id,
            cookie: cookie.clone(),
        });
        self.last_activity = now;
        Ok(Joined {
            device_id: id,
            cookie,
        })
    }

    /// How many phones joined.
    #[cfg(test)]
    pub fn devices(&self) -> usize {
        self.devices.len()
    }

    /// The device `cookie` belongs to. Every device is compared, so the time
    /// does not tell where a matching cookie sits.
    fn by_cookie(&self, cookie: &str) -> Option<usize> {
        let mut at = None;
        for (i, d) in self.devices.iter().enumerate() {
            if same(&d.cookie, cookie) {
                at = Some(i);
            }
        }
        at
    }

    /// May this request see the page? A page load counts as activity.
    pub fn page_access(&mut self, vs: Option<&str>, now: Instant) -> bool {
        let ok = vs.and_then(|c| self.by_cookie(c)).is_some();
        if ok {
            self.last_activity = now;
        }
        ok
    }

    /// The device an upload belongs to: only a joined phone's cookie opens
    /// it. Counts as activity.
    pub fn device_for_api(&mut self, vs: Option<&str>, now: Instant) -> Option<u64> {
        let i = vs.and_then(|c| self.by_cookie(c))?;
        self.last_activity = now;
        Some(self.devices[i].id)
    }

    /// The device a read-only request (`/api/state`, the file list, a
    /// download, a thumbnail) belongs to: only a joined phone's cookie opens
    /// it. Unlike `device_for_api` this is not activity: whoever wants a
    /// download to count calls `touch`.
    pub fn device_for_view_api(&self, vs: Option<&str>) -> Option<u64> {
        let i = vs.and_then(|c| self.by_cookie(c))?;
        Some(self.devices[i].id)
    }

    /// The files shared with the phones.
    pub fn shared(&self) -> &SharedSet {
        &self.shared
    }

    pub fn shared_mut(&mut self) -> &mut SharedSet {
        &mut self.shared
    }

    /// Shares `path` (already seen to be a regular file): its id.
    pub fn share(&mut self, path: &std::path::Path) -> Option<String> {
        self.shared.share(self.fill, path)
    }

    /// Progress of an upload or a download in flight keeps the session alive.
    pub fn touch(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// Time moves on: the token expires, the session may have gone idle.
    /// Each is told once.
    pub fn tick(&mut self, now: Instant) -> Vec<Tick> {
        let mut out = Vec::new();
        if !self.expiry_told && self.token_state(now) == TokenState::Expired {
            self.expiry_told = true;
            out.push(Tick::TokenExpired);
        }
        if self.idle(now) {
            out.push(Tick::Idle);
        }
        out
    }

    pub fn idle(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_activity) >= IDLE_TTL
    }

    /// An upload of `batch` begins: it is remembered so that a later cancel
    /// reaches it. `false` if the owner already cancelled it.
    pub fn start_batch(&mut self, batch: &str) -> bool {
        if self.cancelled.contains(batch) {
            return false;
        }
        if !self.seen_batches.iter().any(|b| b == batch) {
            if self.seen_batches.len() >= MAX_BATCHES {
                self.seen_batches.remove(0);
            }
            self.seen_batches.push(batch.to_owned());
        }
        true
    }

    pub fn is_cancelled(&self, batch: &str) -> bool {
        self.cancelled.contains(batch)
    }

    /// The cancelled batch ids, sorted: the page stops the rest of such a
    /// pick and names the reason when an upload's answer was lost.
    pub fn cancelled_batches(&self) -> Vec<String> {
        let mut v: Vec<String> = self.cancelled.iter().cloned().collect();
        v.sort();
        v
    }

    /// "Batal": every batch begun so far is cancelled, so the upload in
    /// flight stops and the rest of each queue is refused (409).
    pub fn cancel_batches(&mut self) {
        self.cancelled.extend(self.seen_batches.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEED: AtomicU64 = AtomicU64::new(1);

    /// Not random: a counter run through a mixer, so each call differs.
    fn fake(buf: &mut [u8]) -> bool {
        for b in buf.iter_mut() {
            let mut x = SEED.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
            x ^= x >> 30;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^= x >> 27;
            *b = (x >> 11) as u8;
        }
        true
    }

    fn none(_: &mut [u8]) -> bool {
        false
    }

    fn session() -> (Session, Instant) {
        let t0 = Instant::now();
        (Session::new(t0, fake).expect("session"), t0)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn join(s: &mut Session, now: Instant) -> Joined {
        let t = s.token().to_owned();
        s.join(&t, now).expect("joined")
    }

    #[test]
    fn the_token_is_22_url_safe_characters_and_a_new_one_differs() {
        let (mut s, t0) = session();
        let first = s.token().to_owned();
        assert_eq!(first.len(), 22);
        assert!(first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(s.new_token(t0));
        assert_ne!(s.token(), first);
        assert_eq!(s.token().len(), 22);
        assert!(Session::new(t0, none).is_none());
        // With no random bytes a new token is refused and the old one stays.
        let before = s.token().to_owned();
        s.fill = none;
        assert!(!s.new_token(t0));
        assert_eq!(s.token(), before);
    }

    #[test]
    fn a_valid_token_joins_at_once_with_a_cookie_of_its_own() {
        let (mut s, t0) = session();
        let j = join(&mut s, t0);
        assert_eq!(j.device_id, 1);
        assert_eq!(j.cookie.len(), 22);
        assert!(j.cookie.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(j.cookie, s.token(), "the cookie is not the token");
        assert_eq!(s.devices(), 1);
        // No answer is waited for: the page, the state and the API are open.
        assert!(s.page_access(Some(&j.cookie), t0));
        assert_eq!(s.device_for_view_api(Some(&j.cookie)), Some(j.device_id));
        assert_eq!(s.device_for_api(Some(&j.cookie), t0), Some(j.device_id));
    }

    #[test]
    fn two_phones_with_one_token_are_two_devices_with_different_cookies() {
        let (mut s, t0) = session();
        let a = join(&mut s, t0);
        let b = join(&mut s, t0 + secs(5));
        assert_ne!(a.device_id, b.device_id);
        assert_ne!(a.cookie, b.cookie);
        assert_eq!(s.devices(), 2);
        assert_eq!(s.device_for_api(Some(&a.cookie), t0), Some(a.device_id));
        assert_eq!(s.device_for_api(Some(&b.cookie), t0), Some(b.device_id));
        assert!(s.page_access(Some(&a.cookie), t0));
        assert!(s.page_access(Some(&b.cookie), t0));
    }

    #[test]
    fn a_wrong_an_expired_and_a_replaced_token_are_refused() {
        let (mut s, t0) = session();
        assert_eq!(s.join("nope", t0), Err(JoinError::BadToken));
        assert_eq!(s.join("", t0), Err(JoinError::BadToken));
        // Good for 299 s, not for 300.
        let t = s.token().to_owned();
        assert_eq!(s.token_state(t0 + secs(299)), TokenState::Valid);
        assert!(s.join(&t, t0 + secs(299)).is_ok());
        assert_eq!(s.token_state(t0 + secs(300)), TokenState::Expired);
        assert_eq!(s.join(&t, t0 + secs(300)), Err(JoinError::BadToken));
        assert_eq!(s.join(&t, t0 + secs(301)), Err(JoinError::BadToken));
        // A new token replaces the old one at once.
        let (mut s, t0) = session();
        let old = s.token().to_owned();
        assert!(s.new_token(t0 + secs(10)));
        assert_eq!(s.join(&old, t0 + secs(11)), Err(JoinError::BadToken));
        let new = s.token().to_owned();
        assert!(s.join(&new, t0 + secs(11)).is_ok());
    }

    #[test]
    fn joining_does_not_keep_the_token_alive() {
        let (mut s, t0) = session();
        join(&mut s, t0 + secs(10));
        // Five minutes after it was made, whoever joined with it.
        assert_eq!(s.token_state(t0 + secs(299)), TokenState::Valid);
        assert_eq!(s.token_state(t0 + secs(300)), TokenState::Expired);
        let t = s.token().to_owned();
        assert_eq!(s.join(&t, t0 + secs(400)), Err(JoinError::BadToken));
        // A new token starts its five minutes over.
        assert!(s.new_token(t0 + secs(500)));
        assert_eq!(s.token_state(t0 + secs(799)), TokenState::Valid);
        assert_eq!(s.token_state(t0 + secs(800)), TokenState::Expired);
    }

    #[test]
    fn a_new_token_keeps_the_phones_that_joined_working() {
        let (mut s, t0) = session();
        let a = join(&mut s, t0);
        assert!(s.new_token(t0 + secs(30)));
        assert!(s.page_access(Some(&a.cookie), t0 + secs(31)));
        assert_eq!(s.device_for_api(Some(&a.cookie), t0 + secs(31)), Some(a.device_id));
        // Even long after the token that let it in has run out.
        assert!(s.page_access(Some(&a.cookie), t0 + secs(2000)));
        assert_eq!(s.device_for_view_api(Some(&a.cookie)), Some(a.device_id));
    }

    #[test]
    fn no_cookie_or_an_unknown_one_opens_nothing() {
        let (mut s, t0) = session();
        let a = join(&mut s, t0);
        assert!(!s.page_access(None, t0));
        assert!(!s.page_access(Some("zzz"), t0));
        assert!(!s.page_access(Some(""), t0));
        assert_eq!(s.device_for_api(None, t0), None);
        assert_eq!(s.device_for_api(Some("zzz"), t0), None);
        assert_eq!(s.device_for_view_api(None), None);
        assert_eq!(s.device_for_view_api(Some("zzz")), None);
        // The token is not a cookie.
        let t = s.token().to_owned();
        assert!(!s.page_access(Some(&t), t0));
        assert_eq!(s.device_for_view_api(Some(&t)), None);
        // Another session never knows this one's cookie.
        let (mut other, _) = session();
        assert!(!other.page_access(Some(&a.cookie), t0));
    }

    #[test]
    fn a_phone_with_no_random_bytes_is_not_let_in() {
        let (mut s, t0) = session();
        let t = s.token().to_owned();
        s.fill = none;
        assert_eq!(s.join(&t, t0), Err(JoinError::NoRandom));
        assert_eq!(s.devices(), 0);
    }

    #[test]
    fn at_most_64_phones_join_and_the_65th_is_refused() {
        let (mut s, t0) = session();
        let t = s.token().to_owned();
        for n in 0..MAX_DEVICES {
            assert!(s.join(&t, t0 + secs(n as u64 % 200)).is_ok(), "phone {n}");
        }
        assert_eq!(s.devices(), MAX_DEVICES);
        assert_eq!(s.join(&t, t0 + secs(10)), Err(JoinError::Full));
        assert_eq!(s.devices(), MAX_DEVICES, "none was added");
        // A new token does not make room: the cap is on the session.
        assert!(s.new_token(t0 + secs(20)));
        let t2 = s.token().to_owned();
        assert_eq!(s.join(&t2, t0 + secs(21)), Err(JoinError::Full));
    }

    #[test]
    fn a_token_that_ran_out_is_told_once_and_the_session_ends_after_ten_idle_minutes() {
        let (mut s, t0) = session();
        assert!(s.tick(t0 + secs(299)).is_empty());
        assert_eq!(s.tick(t0 + secs(300)), vec![Tick::TokenExpired]);
        assert!(!s.tick(t0 + secs(301)).contains(&Tick::TokenExpired), "told once");
        assert!(s.tick(t0 + secs(599)).iter().all(|t| *t != Tick::Idle));
        assert!(s.tick(t0 + secs(600)).contains(&Tick::Idle));
        // A new token is told again when it runs out.
        let (mut s, t0) = session();
        s.touch(t0 + secs(290));
        assert_eq!(s.tick(t0 + secs(300)), vec![Tick::TokenExpired]);
        assert!(s.new_token(t0 + secs(310)));
        assert!(s.tick(t0 + secs(609)).is_empty());
        assert_eq!(s.tick(t0 + secs(610)), vec![Tick::TokenExpired]);
    }

    #[test]
    fn a_join_and_a_page_load_are_activity_but_state_polls_the_list_and_thumbnails_are_not() {
        let (mut s, t0) = session();
        // A join counts.
        let t1 = t0 + secs(200);
        let t = s.token().to_owned();
        let a = s.join(&t, t1).expect("joined");
        assert!(!s.idle(t1 + secs(599)));
        assert!(s.idle(t1 + secs(600)));
        // From now only the polls of a joined phone (the state, the list, a
        // thumbnail): idle runs out.
        let mut now = t1;
        for _ in 0..200 {
            now += secs(5);
            assert!(s.device_for_view_api(Some(&a.cookie)).is_some());
        }
        assert!(s.idle(now), "polls alone did not keep it alive");
        // A page load counts.
        assert!(s.page_access(Some(&a.cookie), now));
        assert!(!s.idle(now + secs(599)));
        // An upload counts.
        let later = now + secs(590);
        assert!(s.device_for_api(Some(&a.cookie), later).is_some());
        assert!(!s.idle(later + secs(599)));
        // The start of a download and each chunk of it (`touch`) count.
        s.touch(later + secs(500));
        assert!(!s.idle(later + secs(1000)));
        assert!(s.idle(later + secs(1100)));
    }

    #[test]
    fn the_shared_files_live_in_the_session_and_go_with_it() {
        let (mut s, _) = session();
        let a = s.share(std::path::Path::new(r"C:\x\a.pdf")).expect("id");
        assert_eq!(s.shared().len(), 1);
        assert_eq!(s.shared().find(&a).expect("a").name, "a.pdf");
        assert!(s.shared_mut().unshare(&a));
        assert!(s.shared().is_empty());
        // With no random bytes nothing is shared.
        let mut dead = Session::new(Instant::now(), fake).expect("session");
        dead.fill = none;
        assert_eq!(dead.share(std::path::Path::new(r"C:\x\b.pdf")), None);
        // Another session never knows an id of this one.
        let (mut other, _) = session();
        let b = other.share(std::path::Path::new(r"C:\x\a.pdf")).expect("id");
        assert_ne!(a, b);
        assert!(s.shared().find(&b).is_none());
    }

    #[test]
    fn a_cancelled_batch_stays_cancelled_and_a_new_one_is_free() {
        let (mut s, _) = session();
        assert!(s.start_batch("b1"));
        assert!(s.start_batch("b1"));
        assert!(!s.is_cancelled("b1"));
        s.cancel_batches();
        assert!(s.is_cancelled("b1"));
        assert!(!s.start_batch("b1"), "queued uploads of it get 409");
        assert!(s.start_batch("b2"), "a new pick is a new batch");
        assert!(!s.is_cancelled("b2"));
        assert_eq!(s.cancelled_batches(), ["b1"]);
    }

    #[test]
    fn comparison_is_exact() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "abcd"));
        assert!(!same("", "a"));
    }
}
