//! The phone link's rules, with no sockets and no clock of their own: the
//! token and how long it lives, the pairing of each phone (a code and a
//! cookie, answered by the owner), what each cookie opens, the idle end and
//! the batches the owner cancelled. `now` is passed in and the random bytes
//! come from an injected filler, so every rule is tested without time or
//! a network.

use std::collections::HashSet;

use std::time::{Duration, Instant};

use super::Outcome;
use super::share::SharedSet;


/// A token nobody has used is good for this long.
pub const TOKEN_TTL: Duration = Duration::from_secs(300);
/// A pairing nobody answered is turned down after this long.
pub const APPROVE_TTL: Duration = Duration::from_secs(60);
/// A session with no meaningful request for this long ends.
pub const IDLE_TTL: Duration = Duration::from_secs(600);
/// Rejections (Tolak or time out) in one session that void the token.
pub const MAX_REJECTIONS: u32 = 3;
/// Phones waiting for an answer at once; more scans are ignored.
const MAX_PENDING: usize = 3;
/// Batch ids remembered, so a queued upload of a cancelled batch is refused.
const MAX_BATCHES: usize = 256;

/// Fills the buffer with cryptographically random bytes; `false` if it
/// cannot (nothing is then made from weak bytes).
pub type Fill = fn(&mut [u8]) -> bool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Pending(Instant),
    Approved,
    Rejected,
    TimedOut,
}

#[derive(Clone, Debug)]
struct Device {
    id: u64,
    code: String,
    pairing_cookie: String,
    session_cookie: Option<String>,
    status: Status,
    /// The token this phone paired with (`Session::token_gen` then).
    token_gen: u64,
}

/// What `begin_pairing` made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPairing {
    pub device_id: u64,
    pub code: String,
    pub cookie: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairError {
    /// Wrong, expired or voided token.
    BadToken,
    /// Too many phones are already waiting.
    Busy,
    /// The system could not give random bytes.
    NoRandom,
}

/// The token's standing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenState {
    Valid,
    Expired,
    Void,
}

/// What `/api/state` answers a cookie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateAnswer {
    /// Nothing known about this cookie: a bare 404.
    Unknown,
    Waiting { code: String },
    /// Approved. `session_cookie` is the new `vs` to set; it is `None` when
    /// the request already came with `vs`, or when this phone's `vs` was
    /// made before (a second tab, a reload racing a poll): the pairing
    /// cookie never makes another one. `clear_pairing`: the request came
    /// with the pairing cookie, which is then cleared on the phone.
    Approved {
        session_cookie: Option<String>,
        clear_pairing: bool,
    },
    Rejected,
}

/// Something `tick` found out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tick {
    PairingEnded { device_id: u64, outcome: Outcome },
    TokenExpired,
    TokenVoid,
    Idle,
}

/// What `approve` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Approval {
    /// The phone paired with the current token while it was still good:
    /// the token now lives as long as the session. Not for a phone that
    /// paired with a token replaced since, nor once the token expired.
    pub token_kept: bool,
}

/// What `reject` caused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rejected {
    /// The device was waiting (an answer was due).
    pub was_pending: bool,
    /// This was the rejection that voided the token.
    pub voided: bool,
}

pub struct Session {
    fill: Fill,
    token: String,
    token_made: Instant,
    /// A phone was approved with this token while it was good: it then
    /// lives until the session ends (more phones may come), not just
    /// `TOKEN_TTL`.
    approved_once: bool,
    /// Which token is current: each new one takes the next number, so an
    /// approval tells the token its phone paired with from a later one.
    token_gen: u64,
    voided: bool,
    expiry_told: bool,
    rejections: u32,
    devices: Vec<Device>,
    next_id: u64,
    last_activity: Instant,
    seen_batches: Vec<String>,
    cancelled: HashSet<String>,
    /// The shelf files the owner ticked for the phone.
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

/// A four-digit code without bias: values from the top of the range that
/// would make low digits likelier are thrown away and drawn again.
fn random_code(fill: Fill) -> Option<String> {
    for _ in 0..32 {
        let mut b = [0u8; 2];
        if !fill(&mut b) {
            return None;
        }
        let v = u32::from(u16::from_le_bytes(b));
        // 65536 does not divide by 10000: only 0..60000 is uniform.
        if v < 60_000 {
            return Some(format!("{:04}", v % 10_000));
        }
    }
    None
}

impl Session {
    pub fn new(now: Instant, fill: Fill) -> Option<Self> {
        let token = random_cookie(fill)?;
        Some(Self {
            fill,
            token,
            token_made: now,
            approved_once: false,
            token_gen: 0,
            voided: false,
            expiry_told: false,
            rejections: 0,
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

    /// A new token for the QR code ("Buat QR baru"): the old one stops
    /// working, the count of rejections starts again. Phones already
    /// approved stay approved. `false` if the system gave no random bytes.
    pub fn new_token(&mut self, now: Instant) -> bool {
        let Some(token) = random_cookie(self.fill) else {
            return false;
        };
        self.token = token;
        self.token_made = now;
        self.token_gen += 1;
        self.approved_once = false;
        self.voided = false;
        self.expiry_told = false;
        self.rejections = 0;
        true
    }

    pub fn token_state(&self, now: Instant) -> TokenState {
        if self.voided {
            TokenState::Void
        } else if !self.approved_once && now.saturating_duration_since(self.token_made) >= TOKEN_TTL
        {
            TokenState::Expired
        } else {
            TokenState::Valid
        }
    }

    /// How many phones wait for an answer.
    pub fn pending(&self) -> usize {
        self.devices
            .iter()
            .filter(|d| matches!(d.status, Status::Pending(_)))
            .count()
    }

    /// A phone opened the link with `presented`: a pairing is made (a code
    /// to show on both screens and a cookie that proves it is the same
    /// phone). The owner answers it with `approve` or `reject`.
    pub fn begin_pairing(
        &mut self,
        presented: &str,
        now: Instant,
    ) -> Result<NewPairing, PairError> {
        if !same(presented, &self.token) || self.token_state(now) != TokenState::Valid {
            return Err(PairError::BadToken);
        }
        if self.pending() >= MAX_PENDING {
            return Err(PairError::Busy);
        }
        let code = random_code(self.fill).ok_or(PairError::NoRandom)?;
        let cookie = random_cookie(self.fill).ok_or(PairError::NoRandom)?;
        let id = self.next_id;
        self.next_id += 1;
        self.devices.push(Device {
            id,
            code: code.clone(),
            pairing_cookie: cookie.clone(),
            session_cookie: None,
            status: Status::Pending(now),
            token_gen: self.token_gen,
        });
        self.last_activity = now;
        Ok(NewPairing {
            device_id: id,
            code,
            cookie,
        })
    }

    fn by_pairing(&self, cookie: &str) -> Option<usize> {
        self.devices
            .iter()
            .position(|d| same(&d.pairing_cookie, cookie))
    }

    fn by_session(&self, cookie: &str) -> Option<usize> {
        self.devices.iter().position(|d| {
            d.status == Status::Approved
                && d.session_cookie
                    .as_deref()
                    .is_some_and(|s| same(s, cookie))
        })
    }

    /// The device a page load or `/api/state` belongs to: its session
    /// cookie, or its pairing cookie (which opens only these two).
    fn device_for_view(&self, vp: Option<&str>, vs: Option<&str>) -> Option<usize> {
        vs.and_then(|c| self.by_session(c))
            .or_else(|| vp.and_then(|c| self.by_pairing(c)))
    }

    /// The phone's pairing is over without a yes (the owner turned it down,
    /// or nobody answered in time) and it has no session cookie of an
    /// approved phone: it may scan the QR again for a new pairing.
    pub fn pairing_over(&self, vp: Option<&str>, vs: Option<&str>) -> bool {
        self.device_for_view(vp, vs)
            .is_some_and(|i| matches!(self.devices[i].status, Status::Rejected | Status::TimedOut))
    }

    /// May this request see the page? A page load counts as activity.
    pub fn page_access(&mut self, vp: Option<&str>, vs: Option<&str>, now: Instant) -> bool {
        let ok = self.device_for_view(vp, vs).is_some();
        if ok {
            self.last_activity = now;
        }
        ok
    }

    /// The answer to `/api/state`. A poll of a phone that waits for its
    /// answer counts as activity; a poll of one already approved does not,
    /// so a phone tab left open cannot keep the session alive.
    pub fn answer_state(
        &mut self,
        vp: Option<&str>,
        vs: Option<&str>,
        now: Instant,
    ) -> StateAnswer {
        let Some(i) = self.device_for_view(vp, vs) else {
            return StateAnswer::Unknown;
        };
        let via_session = vs.is_some_and(|c| self.by_session(c) == Some(i));
        match self.devices[i].status {
            Status::Pending(_) => {
                self.last_activity = now;
                StateAnswer::Waiting {
                    code: self.devices[i].code.clone(),
                }
            }
            Status::Approved if via_session => StateAnswer::Approved {
                session_cookie: None,
                clear_pairing: false,
            },
            // The session cookie was made already: the pairing cookie only
            // says "approved" and is cleared. A new cookie here would void
            // the one the phone holds, and the pairing cookie would stay a
            // way into the API.
            Status::Approved if self.devices[i].session_cookie.is_some() => {
                StateAnswer::Approved {
                    session_cookie: None,
                    clear_pairing: true,
                }
            }
            Status::Approved => {
                // The first look after the owner's yes: the phone gets its
                // own session cookie (and drops the pairing one).
                let Some(cookie) = random_cookie(self.fill) else {
                    return StateAnswer::Unknown;
                };
                self.devices[i].session_cookie = Some(cookie.clone());
                self.last_activity = now;
                StateAnswer::Approved {
                    session_cookie: Some(cookie),
                    clear_pairing: true,
                }
            }
            Status::Rejected | Status::TimedOut => StateAnswer::Rejected,
        }
    }

    /// The device an upload (or any other API) belongs to: only an approved
    /// phone's session cookie opens it. Counts as activity.
    pub fn device_for_api(&mut self, vs: Option<&str>, now: Instant) -> Option<u64> {
        let i = vs.and_then(|c| self.by_session(c))?;
        self.last_activity = now;
        Some(self.devices[i].id)
    }

    /// The device a read-only API request (the file list, a download, a
    /// thumbnail) belongs to: only an approved phone's session cookie opens
    /// it. Unlike `device_for_api` this is not activity: whoever wants a
    /// download to count calls `touch`.
    pub fn device_for_view_api(&self, vs: Option<&str>) -> Option<u64> {
        let i = vs.and_then(|c| self.by_session(c))?;
        Some(self.devices[i].id)
    }

    /// The files ticked for the phone.
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

    /// Progress of an upload in flight keeps the session alive.
    pub fn touch(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// The owner said yes. `None` if the device was not waiting.
    pub fn approve(&mut self, device_id: u64, now: Instant) -> Option<Approval> {
        let d = self.devices.iter_mut().find(|d| d.id == device_id)?;
        if !matches!(d.status, Status::Pending(_)) {
            return None;
        }
        d.status = Status::Approved;
        let current = d.token_gen == self.token_gen;
        // An expired or replaced token is not brought back: the PC already
        // shows it as gone.
        let token_kept = current && self.token_state(now) == TokenState::Valid;
        if token_kept {
            self.approved_once = true;
        }
        Some(Approval { token_kept })
    }

    /// The owner said no. Three of these (and time outs) in one session void
    /// the token.
    pub fn reject(&mut self, device_id: u64) -> Rejected {
        let Some(d) = self.devices.iter_mut().find(|d| d.id == device_id) else {
            return Rejected {
                was_pending: false,
                voided: false,
            };
        };
        if !matches!(d.status, Status::Pending(_)) {
            return Rejected {
                was_pending: false,
                voided: false,
            };
        }
        d.status = Status::Rejected;
        Rejected {
            was_pending: true,
            voided: self.count_rejection(),
        }
    }

    /// Counts one rejection; `true` when it voided the token.
    fn count_rejection(&mut self) -> bool {
        self.rejections += 1;
        if self.rejections >= MAX_REJECTIONS && !self.voided {
            self.voided = true;
            true
        } else {
            false
        }
    }

    /// Time moves on: unanswered pairings time out (a rejection each), the
    /// token expires, the session may have gone idle. Each is told once.
    pub fn tick(&mut self, now: Instant) -> Vec<Tick> {
        let mut out = Vec::new();
        let mut timed_out = Vec::new();
        for d in &mut self.devices {
            if let Status::Pending(since) = d.status
                && now.saturating_duration_since(since) >= APPROVE_TTL
            {
                d.status = Status::TimedOut;
                timed_out.push(d.id);
            }
        }
        for device_id in timed_out {
            out.push(Tick::PairingEnded {
                device_id,
                outcome: Outcome::TimedOut,
            });
            if self.count_rejection() {
                out.push(Tick::TokenVoid);
            }
        }
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

    fn ones(buf: &mut [u8]) -> bool {
        buf.fill(0xff);
        true
    }

    fn session() -> (Session, Instant) {
        let t0 = Instant::now();
        (Session::new(t0, fake).expect("session"), t0)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn pair(s: &mut Session, now: Instant) -> NewPairing {
        let t = s.token().to_owned();
        s.begin_pairing(&t, now).expect("pairing")
    }

    #[test]
    fn a_phone_whose_pairing_is_over_may_pair_again_but_not_an_approved_one() {
        let (mut s, t0) = session();
        let first = pair(&mut s, t0);
        assert!(!s.pairing_over(Some(&first.cookie), None), "still waiting");
        s.reject(first.device_id);
        assert!(s.pairing_over(Some(&first.cookie), None));
        // The old cookie still sees the "rejected" page, a new pairing
        // is made with the valid token.
        assert!(s.page_access(Some(&first.cookie), None, t0));
        let second = pair(&mut s, t0);
        assert_ne!(second.device_id, first.device_id);
        assert!(!s.pairing_over(Some(&second.cookie), None));
        assert!(s.approve(second.device_id, t0).is_some());
        let StateAnswer::Approved { session_cookie: Some(vs), .. } =
            s.answer_state(Some(&second.cookie), None, t0)
        else {
            panic!("approved");
        };
        assert!(!s.pairing_over(Some(&second.cookie), Some(&vs)));
        // Timed out counts the same.
        let third = pair(&mut s, t0);
        s.tick(t0 + APPROVE_TTL);
        assert!(s.pairing_over(Some(&third.cookie), None));
        // An unknown cookie is nobody.
        assert!(!s.pairing_over(Some("nope"), None));
    }

    #[test]
    fn the_token_is_22_url_safe_characters_and_a_new_one_differs() {
        let (mut s, t0) = session();
        let first = s.token().to_owned();
        assert_eq!(first.len(), 22);
        assert!(first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(s.new_token(t0));
        assert_ne!(s.token(), first);
        assert!(Session::new(t0, none).is_none());
    }

    #[test]
    fn an_unused_token_expires_after_five_minutes() {
        let (mut s, t0) = session();
        assert_eq!(s.token_state(t0 + secs(299)), TokenState::Valid);
        assert_eq!(s.token_state(t0 + secs(300)), TokenState::Expired);
        let t = s.token().to_owned();
        assert_eq!(
            s.begin_pairing(&t, t0 + secs(301)),
            Err(PairError::BadToken)
        );
        // Told once.
        assert!(s.tick(t0 + secs(301)).contains(&Tick::TokenExpired));
        assert!(!s.tick(t0 + secs(302)).contains(&Tick::TokenExpired));
    }

    #[test]
    fn after_the_first_approval_the_token_lives_until_the_session_ends() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert!(s.approve(p.device_id, t0).is_some());
        assert_eq!(s.token_state(t0 + secs(3000)), TokenState::Valid);
        let t = s.token().to_owned();
        // A second phone can come much later (within the idle time).
        assert!(s.begin_pairing(&t, t0 + secs(400)).is_ok());
        // A new token starts over.
        assert!(s.new_token(t0 + secs(500)));
        assert_eq!(s.token_state(t0 + secs(500) + secs(300)), TokenState::Expired);
    }

    #[test]
    fn a_wrong_token_is_refused_and_the_right_one_pairs() {
        let (mut s, t0) = session();
        assert_eq!(s.begin_pairing("nope", t0), Err(PairError::BadToken));
        assert_eq!(s.begin_pairing("", t0), Err(PairError::BadToken));
        let p = pair(&mut s, t0);
        assert_eq!(p.code.len(), 4);
        assert!(p.code.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(p.cookie.len(), 22);
        assert_eq!(s.pending(), 1);
    }

    #[test]
    fn the_code_is_drawn_without_bias() {
        // A draw from the biased top of the range is thrown away.
        assert_eq!(random_code(ones), None, "all draws rejected gives up");
        // Values below 60000 map straight on.
        fn low(buf: &mut [u8]) -> bool {
            let v: u16 = 4821;
            buf.copy_from_slice(&v.to_le_bytes());
            true
        }
        assert_eq!(random_code(low).as_deref(), Some("4821"));
        fn zero(buf: &mut [u8]) -> bool {
            buf.fill(0);
            true
        }
        assert_eq!(random_code(zero).as_deref(), Some("0000"));
        fn edge(buf: &mut [u8]) -> bool {
            buf.copy_from_slice(&59_999u16.to_le_bytes());
            true
        }
        assert_eq!(random_code(edge).as_deref(), Some("9999"));
        // Every digit shows up over many draws.
        let mut seen = [false; 10];
        for _ in 0..2000 {
            let c = random_code(fake).expect("code");
            seen[(c.as_bytes()[0] - b'0') as usize] = true;
        }
        assert!(seen.iter().all(|&b| b));
    }

    #[test]
    fn at_most_three_phones_wait_at_once() {
        let (mut s, t0) = session();
        for _ in 0..3 {
            pair(&mut s, t0);
        }
        let t = s.token().to_owned();
        assert_eq!(s.begin_pairing(&t, t0), Err(PairError::Busy));
    }

    #[test]
    fn an_unanswered_pairing_times_out_after_a_minute_and_counts_as_a_rejection() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert!(s.tick(t0 + secs(59)).is_empty());
        let ticks = s.tick(t0 + secs(60));
        assert_eq!(
            ticks,
            vec![Tick::PairingEnded {
                device_id: p.device_id,
                outcome: Outcome::TimedOut
            }]
        );
        assert_eq!(s.pending(), 0);
        assert!(s.approve(p.device_id, t0 + secs(60)).is_none(), "too late");
        assert_eq!(
            s.answer_state(Some(&p.cookie), None, t0 + secs(61)),
            StateAnswer::Rejected
        );
    }

    #[test]
    fn three_rejections_void_the_token() {
        let (mut s, t0) = session();
        for i in 0..2 {
            let p = pair(&mut s, t0 + secs(i));
            assert_eq!(
                s.reject(p.device_id),
                Rejected {
                    was_pending: true,
                    voided: false
                }
            );
        }
        let p = pair(&mut s, t0 + secs(3));
        // The third is a time out: it counts the same.
        let ticks = s.tick(t0 + secs(3) + APPROVE_TTL);
        assert!(ticks.contains(&Tick::TokenVoid), "{ticks:?}");
        assert_eq!(s.token_state(t0 + secs(100)), TokenState::Void);
        let t = s.token().to_owned();
        assert_eq!(s.begin_pairing(&t, t0 + secs(100)), Err(PairError::BadToken));
        // Rejecting again changes nothing.
        assert!(!s.reject(p.device_id).was_pending);
        // A new QR makes it work again.
        assert!(s.new_token(t0 + secs(101)));
        assert_eq!(s.token_state(t0 + secs(101)), TokenState::Valid);
    }

    #[test]
    fn a_rejection_by_the_owner_that_is_the_third_voids_it_at_once() {
        let (mut s, t0) = session();
        let mut last = None;
        for i in 0..3 {
            let p = pair(&mut s, t0 + secs(i));
            last = Some(s.reject(p.device_id));
        }
        assert_eq!(
            last,
            Some(Rejected {
                was_pending: true,
                voided: true
            })
        );
    }

    #[test]
    fn the_pairing_cookie_opens_only_the_page_and_the_state() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert!(s.page_access(Some(&p.cookie), None, t0));
        assert_eq!(
            s.answer_state(Some(&p.cookie), None, t0),
            StateAnswer::Waiting {
                code: p.code.clone()
            }
        );
        // Not the API, even once approved.
        assert_eq!(s.device_for_api(Some(&p.cookie), t0), None);
        assert!(s.approve(p.device_id, t0).is_some());
        assert_eq!(s.device_for_api(Some(&p.cookie), t0), None);
        // A made-up cookie opens nothing.
        assert!(!s.page_access(Some("zzz"), Some("yyy"), t0));
        assert_eq!(s.answer_state(None, None, t0), StateAnswer::Unknown);
    }

    #[test]
    fn approval_hands_over_a_new_session_cookie_once() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert!(s.approve(p.device_id, t0).is_some());
        let StateAnswer::Approved {
            session_cookie: Some(vs),
            ..
        } = s.answer_state(Some(&p.cookie), None, t0)
        else {
            panic!("expected a session cookie");
        };
        assert_ne!(vs, p.cookie);
        assert_eq!(vs.len(), 22);
        // With the new cookie: approved, nothing more to set, API open.
        assert_eq!(
            s.answer_state(None, Some(&vs), t0),
            StateAnswer::Approved {
                session_cookie: None,
                clear_pairing: false,
            }
        );
        assert_eq!(s.device_for_api(Some(&vs), t0), Some(p.device_id));
        assert!(s.page_access(None, Some(&vs), t0));
        // Two phones get two different cookies.
        let q = pair(&mut s, t0);
        s.approve(q.device_id, t0);
        let StateAnswer::Approved {
            session_cookie: Some(vs2),
            ..
        } = s.answer_state(Some(&q.cookie), None, t0)
        else {
            panic!("second cookie");
        };
        assert_ne!(vs, vs2);
        assert_eq!(s.device_for_api(Some(&vs2), t0), Some(q.device_id));
    }

    #[test]
    fn the_pairing_cookie_never_makes_a_second_session_cookie() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert!(s.approve(p.device_id, t0).is_some());
        let StateAnswer::Approved {
            session_cookie: Some(vs),
            clear_pairing: true,
        } = s.answer_state(Some(&p.cookie), None, t0)
        else {
            panic!("first cookie");
        };
        // A second tab, or a reload racing a poll, still sends only `vp`:
        // it hears "approved", gets no cookie of its own, and `vp` goes.
        assert_eq!(
            s.answer_state(Some(&p.cookie), None, t0),
            StateAnswer::Approved {
                session_cookie: None,
                clear_pairing: true,
            }
        );
        // The cookie the phone holds still works.
        assert_eq!(s.device_for_api(Some(&vs), t0), Some(p.device_id));
        // Both cookies at once: the session one counts, nothing is made.
        assert_eq!(
            s.answer_state(Some(&p.cookie), Some(&vs), t0),
            StateAnswer::Approved {
                session_cookie: None,
                clear_pairing: false,
            }
        );
        assert_eq!(s.device_for_api(Some(&vs), t0), Some(p.device_id));
    }

    #[test]
    fn only_a_phone_of_the_current_good_token_keeps_the_token() {
        // Approved in time with the current token: kept.
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert_eq!(
            s.approve(p.device_id, t0 + secs(10)),
            Some(Approval { token_kept: true })
        );
        assert_eq!(s.token_state(t0 + secs(3000)), TokenState::Valid);

        // Scanned at 4:50, approved at 5:10: the token stays expired.
        let (mut s, t0) = session();
        let p = pair(&mut s, t0 + secs(290));
        assert!(s.tick(t0 + secs(300)).contains(&Tick::TokenExpired));
        assert_eq!(
            s.approve(p.device_id, t0 + secs(310)),
            Some(Approval { token_kept: false })
        );
        assert_eq!(s.token_state(t0 + secs(310)), TokenState::Expired);
        let t = s.token().to_owned();
        assert_eq!(s.begin_pairing(&t, t0 + secs(311)), Err(PairError::BadToken));

        // Paired with the old token, approved after "Buat QR baru": the
        // phone is in, the new token still runs out.
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        assert!(s.new_token(t0 + secs(20)));
        assert_eq!(
            s.approve(p.device_id, t0 + secs(30)),
            Some(Approval { token_kept: false })
        );
        assert_eq!(
            s.token_state(t0 + secs(20) + TOKEN_TTL),
            TokenState::Expired
        );
        let StateAnswer::Approved {
            session_cookie: Some(vs),
            ..
        } = s.answer_state(Some(&p.cookie), None, t0 + secs(31))
        else {
            panic!("the phone is approved");
        };
        assert_eq!(s.device_for_api(Some(&vs), t0 + secs(31)), Some(p.device_id));
    }

    #[test]
    fn a_rejected_phone_sees_rejected_and_gets_no_api() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        s.reject(p.device_id);
        assert_eq!(
            s.answer_state(Some(&p.cookie), None, t0),
            StateAnswer::Rejected
        );
        assert_eq!(s.device_for_api(Some(&p.cookie), t0), None);
    }

    #[test]
    fn a_session_cookie_is_not_valid_before_approval() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        // The pairing cookie is not a session cookie.
        assert_eq!(s.device_for_api(Some(&p.cookie), t0), None);
        assert!(!s.page_access(None, Some(&p.cookie), t0));
    }

    #[test]
    fn the_session_ends_after_ten_idle_minutes() {
        let (mut s, t0) = session();
        assert!(s.tick(t0 + secs(599)).iter().all(|t| *t != Tick::Idle));
        assert!(s.tick(t0 + secs(600)).contains(&Tick::Idle));
    }

    #[test]
    fn pages_uploads_and_pending_polls_reset_the_idle_clock_but_approved_polls_do_not() {
        let (mut s, t0) = session();
        let p = pair(&mut s, t0);
        // A pending poll counts.
        let t1 = t0 + secs(500);
        assert!(matches!(
            s.answer_state(Some(&p.cookie), None, t1),
            StateAnswer::Waiting { .. }
        ));
        assert!(!s.idle(t1 + secs(599)));
        s.approve(p.device_id, t1);
        let t2 = t1 + secs(10);
        let StateAnswer::Approved {
            session_cookie: Some(vs),
            ..
        } = s.answer_state(Some(&p.cookie), None, t2)
        else {
            panic!("cookie")
        };
        // From now only the polls of an approved phone: idle runs out.
        let mut now = t2;
        for _ in 0..200 {
            now += secs(5);
            assert_eq!(
                s.answer_state(None, Some(&vs), now),
                StateAnswer::Approved {
                    session_cookie: None,
                    clear_pairing: false,
                }
            );
        }
        assert!(s.idle(now), "polls alone did not keep it alive");
        // A page load counts.
        assert!(s.page_access(None, Some(&vs), now));
        assert!(!s.idle(now + secs(599)));
        // An upload counts.
        let later = now + secs(590);
        assert!(s.device_for_api(Some(&vs), later).is_some());
        assert!(!s.idle(later + secs(599)));
        s.touch(later + secs(500));
        assert!(!s.idle(later + secs(1000)));
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
