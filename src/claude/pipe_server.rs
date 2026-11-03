use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_NO_DATA, ERROR_PIPE_CONNECTED, HANDLE};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_INBOUND,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::core::HSTRING;

use super::PIPE_NAME;
use super::hook::{HookKind, HookMessage, send_line, to_event};
use super::origin;
use crate::model::Event;

const BUFFER_SIZE: u32 = 4096;
/// Longest line accepted from a client; anything past it is dropped.
const MAX_LINE: u64 = 64 * 1024;

/// How long the first instance is retried while another process (the old
/// island, still closing after an update) holds the name.
const FIRST_INSTANCE_WAIT: Duration = Duration::from_secs(60);
/// Pause between two tries at the first instance.
const FIRST_INSTANCE_STEP: Duration = Duration::from_millis(250);
/// How long `stop` waits for the serve thread to end.
const STOP_WAIT: Duration = Duration::from_secs(1);

/// The pipe server's thread; `stop` ends it.
pub struct PipeServer {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    name: &'static str,
}

/// Listens on `\\.\pipe\vale` on a background thread and turns each
/// hook message into an `Event`. While another process owns the pipe (the old
/// island after an update) it retries for a minute, then gives up quietly.
pub fn spawn_pipe_server(tx: Sender<Event>, wake: impl Fn() + Send + 'static) -> PipeServer {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let join = std::thread::Builder::new()
        .name("claude-pipe".to_string())
        .spawn(move || serve(PIPE_NAME, tx, wake, &flag))
        .ok();
    PipeServer {
        stop,
        join,
        name: PIPE_NAME,
    }
}

impl PipeServer {
    /// Stops serving: the island is ending and must free the pipe name before
    /// it lets go of its mutex. The thread sits in `ConnectNamedPipe`, so it
    /// is woken by a connection of our own (an empty line). A hook message
    /// that arrived before the stop is still read and delivered. Waits about
    /// a second for the thread; if it does not end, the caller goes on.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let Some(join) = self.join.take() else {
            return;
        };
        let deadline = Instant::now() + STOP_WAIT;
        // The wake-up may find every instance busy for a moment: try again
        // until the thread ends.
        while !join.is_finished() && Instant::now() < deadline {
            send_line(self.name, b"\n");
            std::thread::sleep(Duration::from_millis(10));
        }
        if join.is_finished() {
            let _ = join.join();
        }
    }
}

/// The delay before the next try at creating the FIRST instance, `elapsed`
/// after the first try; `None` once the minute is up.
fn first_instance_retry(elapsed: Duration) -> Option<Duration> {
    (elapsed < FIRST_INSTANCE_WAIT).then_some(FIRST_INSTANCE_STEP)
}

/// The first instance of the pipe. The old process may still hold the name
/// for a moment, so a failed try is repeated on `first_instance_retry`'s
/// plan; `None` when the plan runs out or `stop` is set.
fn create_first_instance(name: &HSTRING, stop: &AtomicBool) -> Option<File> {
    let started = Instant::now();
    loop {
        if let Some(pipe) = create_instance(name, true) {
            return Some(pipe);
        }
        let delay = first_instance_retry(started.elapsed())?;
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        std::thread::sleep(delay);
    }
}

/// Accepts one connection at a time, reading one JSON line from each. Returns
/// when the pipe cannot be created, the receiver is gone, or `stop` is set and
/// a connection with nothing to deliver (the stop's own wake-up) came in:
/// every client that connected before it is read first.
fn serve(pipe_name: &str, tx: Sender<Event>, wake: impl Fn(), stop: &AtomicBool) {
    let name = HSTRING::from(pipe_name);
    let Some(mut listening) = create_first_instance(&name, stop) else {
        return;
    };
    loop {
        let connected = wait_for_client(&listening);
        // Open the next instance before reading, so a client arriving now
        // finds a busy pipe rather than a missing one.
        let Some(next) = create_instance(&name, false) else {
            return;
        };
        let client = std::mem::replace(&mut listening, next);
        match connected.then(|| read_event(client)).flatten() {
            Some(event) => {
                if tx.send(event).is_err() {
                    return;
                }
                wake();
            }
            // Nothing to deliver: the wake-up of `stop`, or noise.
            None if stop.load(Ordering::SeqCst) => return,
            None => {}
        }
    }
}

/// A new inbound instance of the pipe, owned as a `File` so it closes on drop.
/// `first` fails if any process already serves this name.
fn create_instance(name: &HSTRING, first: bool) -> Option<File> {
    let mut open_mode = PIPE_ACCESS_INBOUND;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let handle = unsafe {
        CreateNamedPipeW(
            name,
            FILE_FLAGS_AND_ATTRIBUTES(open_mode.0),
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER_SIZE,
            BUFFER_SIZE,
            0,
            None,
        )
    };
    if handle.is_invalid() {
        return None;
    }
    Some(unsafe { File::from_raw_handle(handle.0) })
}

/// Blocks until a client opens the instance. True when there may be data to
/// read, including a client that already wrote and closed.
fn wait_for_client(pipe: &File) -> bool {
    match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
        Ok(()) => true,
        Err(e) => {
            e.code() == ERROR_PIPE_CONNECTED.to_hresult() || e.code() == ERROR_NO_DATA.to_hresult()
        }
    }
}

/// Reads the client's single line. Malformed or oversized input yields `None`.
fn read_event(client: File) -> Option<Event> {
    let mut line = Vec::new();
    BufReader::new(client.take(MAX_LINE))
        .read_until(b'\n', &mut line)
        .ok()?;
    let msg: HookMessage = serde_json::from_slice(&line).ok()?;
    note_origin(&msg);
    Some(to_event(msg))
}

/// A notice's origin goes to the side registry (the click looks it up by
/// session id); a notice from an older hook binary, with none, clears the
/// session's entry so its click only dismisses, as before. Activity leaves it.
fn note_origin(msg: &HookMessage) {
    if msg.kind == HookKind::Activity {
        return;
    }
    match &msg.origin {
        Some(o) => origin::remember(&msg.session_id, o.clone(), &msg.project),
        None => origin::forget(&msg.session_id),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::claude::hook::{HookKind, HookMessage, send_line};
    use crate::claude::origin::ClaudeOrigin;

    /// Sends `line`, retrying while the server thread is still creating its pipe.
    fn send_when_ready(name: &str, line: &[u8]) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !send_line(name, line) {
            assert!(Instant::now() < deadline, "pipe server never came up");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_first_instance_is_retried_for_a_minute() {
        // The first tries retry, with a pause that never grows past a second.
        for secs in [0, 1, 10, 59] {
            let delay = first_instance_retry(Duration::from_secs(secs)).expect("retries");
            assert!(delay > Duration::ZERO && delay <= Duration::from_secs(1));
        }
        assert!(first_instance_retry(Duration::from_millis(59_999)).is_some());
        // After a minute, it gives up.
        assert_eq!(first_instance_retry(Duration::from_secs(60)), None);
        assert_eq!(first_instance_retry(Duration::from_secs(600)), None);
    }

    #[test]
    fn a_pipe_held_by_another_is_taken_once_it_is_free() {
        let name = format!(r"\\.\pipe\vale-test-retry-{}", std::process::id());
        let hname = HSTRING::from(name.as_str());
        // The "old island": holds the first instance.
        let old = create_instance(&hname, true).expect("old owner");
        let (tx, rx) = mpsc::channel();
        let server_name = name.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || serve(&server_name, tx, || {}, &flag));
        // It cannot create its own first instance yet, and does not give up.
        std::thread::sleep(Duration::from_millis(600));
        assert!(!thread.is_finished());
        drop(old);
        let mut line = serde_json::to_vec(&HookMessage {
            kind: HookKind::Activity,
            session_id: "retry-1".into(),
            project: "p".into(),
            message: None,
            origin: None,
        })
        .unwrap();
        line.push(b'\n');
        send_when_ready(&name, &line);
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
        stop.store(true, Ordering::SeqCst);
        send_line(&name, b"\n");
        thread.join().unwrap();
    }

    #[test]
    fn stop_ends_the_server_and_frees_the_name_but_delivers_what_was_sent() {
        let name = format!(r"\\.\pipe\vale-test-stop-{}", std::process::id());
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let server_name = name.clone();
        let join = std::thread::spawn(move || serve(&server_name, tx, || {}, &flag));
        let server = PipeServer {
            stop,
            join: Some(join),
            name: Box::leak(name.clone().into_boxed_str()),
        };
        let mut line = serde_json::to_vec(&HookMessage {
            kind: HookKind::Activity,
            session_id: "stop-1".into(),
            project: "p".into(),
            message: None,
            origin: None,
        })
        .unwrap();
        line.push(b'\n');
        send_when_ready(&name, &line);
        let started = Instant::now();
        server.stop();
        assert!(started.elapsed() < Duration::from_millis(900), "{:?}", started.elapsed());
        // The event sent before the stop is not lost.
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            Event::ClaudeActivity {
                session_id: "stop-1".into()
            }
        );
        // The name is free: nobody serves it any more, and a new owner can
        // create the first instance at once.
        assert!(!send_line(&name, b"\n"));
        assert!(create_instance(&HSTRING::from(name.as_str()), true).is_some());
    }

    #[test]
    fn round_trip_ignores_malformed_lines() {
        let name = format!(r"\\.\pipe\vale-test-{}", std::process::id());
        let (tx, rx) = mpsc::channel();
        let (wake_tx, wake_rx) = mpsc::channel();
        let server_name = name.clone();
        std::thread::spawn(move || {
            serve(
                &server_name,
                tx,
                move || {
                    let _ = wake_tx.send(());
                },
                &AtomicBool::new(false),
            )
        });

        send_when_ready(&name, b"not json\n");
        let msg = HookMessage {
            kind: HookKind::Activity,
            session_id: "s9".into(),
            project: "p".into(),
            message: None,
            origin: None,
        };
        let mut line = serde_json::to_vec(&msg).unwrap();
        line.push(b'\n');
        send_when_ready(&name, &line);

        let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            event,
            Event::ClaudeActivity {
                session_id: "s9".into()
            }
        );
        assert!(wake_rx.recv_timeout(Duration::from_secs(1)).is_ok());
        assert!(
            rx.try_recv().is_err(),
            "malformed line must not produce an event"
        );
    }

    #[test]
    fn origin_goes_to_the_registry_and_an_older_line_forgets_it() {
        let name = format!(r"\\.\pipe\vale-test-origin-{}", std::process::id());
        let (tx, rx) = mpsc::channel();
        let server_name = name.clone();
        std::thread::spawn(move || serve(&server_name, tx, || {}, &AtomicBool::new(false)));

        let send = |line: String| {
            let mut line = line.into_bytes();
            line.push(b'\n');
            send_when_ready(&name, &line);
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        };
        let with_origin = HookMessage {
            kind: HookKind::Done,
            session_id: "pipe-origin-1".into(),
            project: "proj".into(),
            message: None,
            origin: Some(ClaudeOrigin {
                host_pid: 42,
                host_exe: "windowsterminal.exe".into(),
                ..Default::default()
            }),
        };
        send(serde_json::to_string(&with_origin).unwrap());
        let got = origin::get("pipe-origin-1").unwrap();
        assert_eq!(got.origin.host_pid, 42);
        assert_eq!(got.project, "proj");

        // Activity leaves the entry alone.
        send(
            r#"{"kind":"Activity","session_id":"pipe-origin-1","project":"proj","message":null}"#
                .into(),
        );
        assert!(origin::get("pipe-origin-1").is_some());

        // A line from an older hook binary has no `origin`: it parses, and
        // the session's entry is gone.
        send(
            r#"{"kind":"Waiting","session_id":"pipe-origin-1","project":"proj","message":"m"}"#
                .into(),
        );
        assert_eq!(origin::get("pipe-origin-1"), None);
    }
}
