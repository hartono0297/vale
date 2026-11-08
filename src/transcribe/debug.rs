//! `vale --transcribe <file.m4a|file.wav> [--model <gguf path or catalog
//! key>] [--engine <dir>] [--vad <gguf path>|energy] [--lang id|en] [--cpu]`
//! (debug builds): transcribes one file on this thread, prints each segment
//! as it comes and the timing, and writes `.md` / `.srt` next to the input
//! (and the sidecar's `"transcript"` when there is one). Defaults: the
//! engine, the default model and Silero from `%APPDATA%\Vale\transcribe`.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use super::catalog;
use super::gpu;
use super::output::{self, LABELS_ID};
use super::runner::{self, Event, Job};
use super::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub audio: PathBuf,
    /// A `.gguf` path or a catalog key; `None`: the default model.
    pub model: Option<String>,
    pub engine: Option<PathBuf>,
    /// `Some("energy")` forces the energy detector.
    pub vad: Option<String>,
    pub lang: Option<String>,
    pub cpu: bool,
}

/// Parses the arguments after `--transcribe`.
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut iter = args.iter();
    let audio = iter.next().ok_or("missing <file>")?;
    let mut parsed = Args {
        audio: PathBuf::from(audio),
        model: None,
        engine: None,
        vad: None,
        lang: None,
        cpu: false,
    };
    while let Some(flag) = iter.next() {
        let mut value = || iter.next().cloned().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--model" => parsed.model = Some(value()?),
            "--engine" => parsed.engine = Some(PathBuf::from(value()?)),
            "--vad" => parsed.vad = Some(value()?),
            "--lang" => parsed.lang = Some(value()?),
            "--cpu" => parsed.cpu = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(parsed)
}

/// The job the arguments describe, against `store` for the defaults.
pub fn job(args: &Args, store: &Store) -> Result<Job, String> {
    let (model_path, model) = match args.model.as_deref() {
        Some(key) if catalog::model(key).is_some() => {
            let m = catalog::model(key).unwrap();
            (store.model_path(m), Some(m))
        }
        Some(path) => {
            let path = PathBuf::from(path);
            // A file of a catalog model still gets its name in the header.
            let known = catalog::MODELS.iter().find(|m| {
                path.file_name().is_some_and(|f| f.to_string_lossy() == m.asset.file)
            });
            (path, known)
        }
        None => {
            let m = catalog::default_model(gpu::has_hardware_gpu());
            (store.model_path(m), Some(m))
        }
    };
    if !model_path.is_file() {
        return Err(format!("no model at {}", model_path.display()));
    }
    let vad_path = match args.vad.as_deref() {
        Some("energy") => None,
        Some(path) => Some(PathBuf::from(path)),
        None => store.vad_installed().then(|| store.vad_path()),
    };
    Ok(Job {
        audio: args.audio.clone(),
        model_path,
        model,
        engine_dir: args.engine.clone().unwrap_or_else(|| store.engine_dir()),
        vad_path,
        language: args.lang.clone(),
        gpu: !args.cpu,
        threads: runner::default_threads(),
        labels: LABELS_ID,
    })
}

/// Runs the flag; the process exit code.
pub fn run(args: &[String]) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(why) => {
            eprintln!("vale --transcribe: {why}");
            eprintln!(
                "usage: vale --transcribe <file.m4a|file.wav> [--model <gguf|key>] [--engine <dir>] [--vad <gguf>|energy] [--lang id|en] [--cpu]"
            );
            return 2;
        }
    };
    let job = match job(&parsed, &Store::default_location()) {
        Ok(job) => job,
        Err(why) => {
            eprintln!("vale --transcribe: {why}");
            return 2;
        }
    };
    println!("audio:   {}", job.audio.display());
    println!("model:   {}", job.model_path.display());
    println!("engine:  {}", job.engine_dir.display());
    println!(
        "vad:     {}",
        job.vad_path
            .as_ref()
            .map_or("energy".to_string(), |p| p.display().to_string())
    );
    println!(
        "lang:    {}   threads: {}   gpu: {}",
        job.language.as_deref().unwrap_or("auto"),
        job.threads,
        job.gpu
    );
    let started = Instant::now();
    let mut last_tenth = None;
    let mut audio_ms = None;
    let result = runner::run_job(&job, &AtomicBool::new(false), &mut |event| match event {
        Event::Started { backend, vad } => {
            println!(
                "loaded in {:.1} s on {backend}, cutting with {vad}",
                started.elapsed().as_secs_f32()
            );
        }
        Event::Progress { done_ms, total_ms } => {
            audio_ms = Some(total_ms.unwrap_or(done_ms));
            if let Some(total) = total_ms.filter(|&t| t > 0) {
                let tenth = done_ms * 10 / total;
                if last_tenth != Some(tenth) {
                    last_tenth = Some(tenth);
                    eprintln!(
                        "  {:>3} %  ({:.0} s)",
                        tenth * 10,
                        started.elapsed().as_secs_f32()
                    );
                }
            }
        }
        Event::Segment(s) => {
            let who = s.speaker.map_or("", |w| LABELS_ID.of(w));
            println!(
                "[{} - {}] {who:<7} {}",
                output::short_time(s.start_ms),
                output::short_time(s.end_ms),
                s.text
            );
        }
        Event::Finished(_) => {}
    });
    for line in runner::engine_log() {
        eprintln!("engine: {line}");
    }
    match result {
        Ok(report) => {
            let seconds = report.elapsed_ms as f64 / 1000.0;
            println!("segments: {}  language: {}", report.segments, report.language.as_deref().unwrap_or("?"));
            if let Some(ms) = audio_ms.filter(|&ms| ms > 0) {
                println!(
                    "took {seconds:.1} s for {:.1} s of audio ({:.1}x realtime) on {}",
                    ms as f64 / 1000.0,
                    ms as f64 / 1000.0 / seconds.max(0.001),
                    report.backend
                );
            }
            if report.partial_chunks > 0 {
                println!("{} chunk(s) stopped early (truncated or looping)", report.partial_chunks);
            }
            println!("wrote {}", report.md.display());
            println!("wrote {}", report.srt.display());
            if let Some(sidecar) = report.sidecar {
                println!("updated {}", sidecar.display());
            }
            0
        }
        Err(error) => {
            eprintln!("vale --transcribe: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::store::tests::temp_store;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments_parse() {
        let args = parse(&strings(&[
            "meet.m4a", "--model", "whisper-small", "--lang", "id", "--cpu", "--vad", "energy",
        ]))
        .unwrap();
        assert_eq!(args.audio, PathBuf::from("meet.m4a"));
        assert_eq!(args.model.as_deref(), Some("whisper-small"));
        assert_eq!(args.lang.as_deref(), Some("id"));
        assert_eq!(args.vad.as_deref(), Some("energy"));
        assert!(args.cpu);
        assert!(parse(&[]).is_err());
        assert!(parse(&strings(&["a.m4a", "--model"])).is_err());
        assert!(parse(&strings(&["a.m4a", "--bogus"])).is_err());
    }

    #[test]
    fn the_job_resolves_models_against_the_store() {
        let store = temp_store("debug-job");
        let args = parse(&strings(&["meet.m4a"])).unwrap();
        assert!(job(&args, &store).unwrap_err().contains("no model"));

        let qwen = catalog::default_model(gpu::has_hardware_gpu());
        std::fs::create_dir_all(store.models_dir()).unwrap();
        std::fs::write(store.model_path(qwen), b"gguf").unwrap();
        let default = job(&args, &store).unwrap();
        assert_eq!(default.model_path, store.model_path(qwen));
        assert_eq!(default.model, Some(qwen));
        assert_eq!(default.engine_dir, store.engine_dir());
        assert_eq!(default.vad_path, None, "no Silero installed: energy");
        assert!(default.gpu);

        // A path to a catalog file keeps the catalog name.
        let by_path = parse(&strings(&[
            "meet.m4a",
            "--model",
            &store.model_path(qwen).to_string_lossy(),
            "--engine",
            r"D:\engine",
            "--vad",
            r"D:\silero.gguf",
        ]))
        .unwrap();
        let job = job(&by_path, &store).unwrap();
        assert_eq!(job.model, Some(qwen));
        assert_eq!(job.engine_dir, PathBuf::from(r"D:\engine"));
        assert_eq!(job.vad_path, Some(PathBuf::from(r"D:\silero.gguf")));
    }
}
