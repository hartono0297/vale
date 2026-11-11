//! `vale --captions-test <seconds> [--mic] [--cpu]` (debug builds): a real
//! caption session on what the PC plays (with `--mic`, the microphone too)
//! for that long, each line printed to this console as it comes, with how
//! long after its last word it came. Uses the engine, the saved model (else
//! the default for this PC) and Silero from `%APPDATA%\Vale\transcribe`;
//! nothing is downloaded and nothing is written to a file.

use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::live::Out;
use super::worker::{self, Event, Spec};
use crate::transcribe::gpu;
use crate::transcribe::output::Speaker;
use crate::transcribe::runner;
use crate::transcribe::store::Store;

const USAGE: &str = "usage: vale --captions-test <seconds> [--mic] [--cpu]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Args {
    pub seconds: u64,
    pub mic: bool,
    pub cpu: bool,
}

/// Parses the arguments after `--captions-test`.
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut iter = args.iter();
    let seconds = iter.next().ok_or("missing <seconds>")?;
    let seconds = seconds
        .parse()
        .map_err(|_| format!("`{seconds}` is not a number of seconds"))?;
    let mut parsed = Args {
        seconds,
        mic: false,
        cpu: false,
    };
    for flag in iter {
        match flag.as_str() {
            "--mic" => parsed.mic = true,
            "--cpu" => parsed.cpu = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(parsed)
}

/// `[mm:ss]` of a time in the session.
fn stamp(ms: u64) -> String {
    format!("[{:02}:{:02}]", ms / 60_000, ms / 1000 % 60)
}

fn who(speaker: Option<Speaker>) -> &'static str {
    match speaker {
        Some(Speaker::Me) => "Saya",
        Some(Speaker::Others) => "Peserta",
        None => "-",
    }
}

/// Runs the flag; the process exit code.
pub fn run(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(why) => {
            eprintln!("vale --captions-test: {why}");
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let store = Store::default_location();
    let prefs = crate::config::load().transcript;
    let model = prefs.model_for(gpu::has_hardware_gpu());
    if !store.engine_installed() || !store.model_installed(model) {
        println!(
            "no engine or no model `{}` in {} (turn Transkrip on in Settings first)",
            model.key,
            store.root().display()
        );
        return 1;
    }
    let spec = Spec {
        engine_dir: store.engine_dir(),
        model_path: store.model_path(model),
        vad_path: store.vad_installed().then(|| store.vad_path()),
        gpu: !parsed.cpu,
        threads: runner::default_threads(),
        mic: parsed.mic,
        devices: [None, None],
        language: Arc::new(AtomicU8::new(worker::lang_to_u8(prefs.language))),
    };
    println!("model:   {}", spec.model_path.display());
    println!(
        "vad:     {}   lang: {}   threads: {}   gpu: {}   mic: {}",
        if spec.vad_path.is_some() {
            "silero"
        } else {
            "energy"
        },
        prefs.language.code().unwrap_or("auto"),
        spec.threads,
        spec.gpu,
        spec.mic
    );
    let asked = Instant::now();
    let (tx, rx) = mpsc::channel();
    let running = match worker::spawn(spec, move |event| {
        let _ = tx.send((Instant::now(), event));
    }) {
        Ok(running) => running,
        Err(e) => {
            eprintln!("vale --captions-test: {e}");
            return 1;
        }
    };

    // The session clock starts about when `Started` comes.
    let mut started: Option<Instant> = None;
    let mut lags: Vec<u64> = Vec::new();
    let (mut greys, mut gaps, mut failed) = (0, 0, false);
    let mut stop_at: Option<Instant> = None;
    let mut running = Some(running);
    loop {
        let wait = stop_at.map_or(Duration::from_secs(120), |at| {
            at.saturating_duration_since(Instant::now())
        });
        let (at, event) = match rx.recv_timeout(wait) {
            Ok(got) => got,
            Err(mpsc::RecvTimeoutError::Timeout) if running.is_some() => {
                // Time is up (or the model never loaded): stop, then read
                // what is left up to `Ended`.
                if let Some(running) = running.take() {
                    running.stop();
                }
                continue;
            }
            Err(_) => break,
        };
        let now_ms = started.map_or(0, |s| at.duration_since(s).as_millis() as u64);
        match event {
            Event::Started { backend } => {
                started = Some(at);
                stop_at = Some(at + Duration::from_secs(parsed.seconds));
                println!(
                    "started: on {backend}, {:.1} s after the start; listening for {} s",
                    at.duration_since(asked).as_secs_f32(),
                    parsed.seconds
                );
            }
            Event::Out(Out::Final(segment)) => {
                let lag = now_ms.saturating_sub(segment.end_ms);
                lags.push(lag);
                println!(
                    "{} FINAL {} {}   (+{lag} ms)",
                    stamp(segment.start_ms),
                    who(segment.speaker),
                    segment.text
                );
            }
            Event::Out(Out::Interim { channel, text }) => {
                greys += 1;
                println!("{} GREY  ch{channel} {text}", stamp(now_ms));
            }
            Event::Out(Out::ClearInterim { .. }) => {}
            Event::Out(Out::Gap) => {
                gaps += 1;
                println!("{} (gap: speech was skipped)", stamp(now_ms));
            }
            Event::Out(Out::Slow) => {
                println!("{} (slow: the model is heavy for this PC)", stamp(now_ms))
            }
            Event::SourceLost(source) => println!("{} lost: {source}", stamp(now_ms)),
            Event::Failed(kind) => {
                failed = true;
                println!("failed: {kind}");
            }
            Event::Ended => break,
        }
    }
    drop(running);
    lags.sort_unstable();
    match lags.as_slice() {
        [] => println!(
            "no line in {} s (was anything playing?); {greys} grey",
            parsed.seconds
        ),
        lags => println!(
            "{} lines, {greys} grey, {gaps} gaps; a line came {} ms after its last word (median), {} to {} ms",
            lags.len(),
            lags[lags.len() / 2],
            lags[0],
            lags[lags.len() - 1]
        ),
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_arguments_are_read() {
        assert_eq!(
            parse(&args(&["20"])),
            Ok(Args {
                seconds: 20,
                mic: false,
                cpu: false
            })
        );
        assert_eq!(
            parse(&args(&["5", "--cpu", "--mic"])),
            Ok(Args {
                seconds: 5,
                mic: true,
                cpu: true
            })
        );
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["soon"])).is_err());
        assert!(parse(&args(&["5", "--loud"])).is_err());
    }

    #[test]
    fn times_and_speakers_read_well() {
        assert_eq!(stamp(0), "[00:00]");
        assert_eq!(stamp(83_400), "[01:23]");
        assert_eq!(
            (
                who(Some(Speaker::Me)),
                who(Some(Speaker::Others)),
                who(None)
            ),
            ("Saya", "Peserta", "-")
        );
    }
}
