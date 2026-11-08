//! transcribe.cpp, loaded at run time from the unpacked native package
//! (`store::Store::engine_dir`) with `LoadLibraryExW` and bound over its C
//! API with `GetProcAddress`. Nothing of it is linked into `vale.exe`.
//!
//! Safety net, in order: the package's SHA-256 (download), its
//! `contract.json` (version and header hash, `store::check_contract`),
//! `transcribe_version()` at load, and `transcribe_abi_struct_size` /
//! `_align` for every struct crossing the boundary, compared with the
//! `#[repr(C)]` mirrors below before any is used. Every params struct is
//! filled by the library's own `*_init` function, never zeroed by hand.
//!
//! Once loaded the library stays loaded for the life of the process: ggml
//! keeps a global backend registry (and the Vulkan driver its own state),
//! so unloading is not safe. Loading it costs nothing at idle (no thread
//! runs between transcriptions; ggml joins its CPU workers after each
//! graph). The backend mask is fixed by the first load, per process.

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{HSTRING, PCSTR};

use super::catalog::ENGINE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// No `transcribe.dll` in the folder: the engine is not installed.
    NotInstalled,
    /// The Visual C++ runtime the package needs is not on this PC
    /// (`msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll`).
    RuntimeMissing(Vec<&'static str>),
    /// The library would not load or lacks an export this binding needs.
    BadLibrary(String),
    /// The library is another version or its structs differ from ours.
    Mismatch(String),
    /// Loaded once already from another folder (one engine per process).
    OtherFolder(PathBuf),
    /// `transcribe_init_backends` found no device to run on.
    NoBackend,
    /// The model file failed to load (`TRANSCRIBE_ERR_*` text).
    Model(String),
    /// The audio is longer than this model takes in one run.
    TooLong,
    /// The GPU backend failed mid-run; reloading on the CPU recovers.
    Backend,
    Cancelled,
    /// Any other failed call: its status code and text.
    Status(i32, String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::NotInstalled => write!(f, "the engine is not installed"),
            EngineError::RuntimeMissing(dlls) => {
                write!(f, "the Visual C++ runtime is missing ({})", dlls.join(", "))
            }
            EngineError::BadLibrary(why) => write!(f, "the engine library: {why}"),
            EngineError::Mismatch(why) => write!(f, "the engine does not match: {why}"),
            EngineError::OtherFolder(dir) => {
                write!(f, "the engine is already loaded from {}", dir.display())
            }
            EngineError::NoBackend => write!(f, "the engine found no CPU or GPU to run on"),
            EngineError::Model(why) => write!(f, "the model: {why}"),
            EngineError::TooLong => write!(f, "the audio is too long for this model"),
            EngineError::Backend => write!(f, "the GPU failed"),
            EngineError::Cancelled => write!(f, "cancelled"),
            EngineError::Status(code, text) => write!(f, "engine status {code}: {text}"),
        }
    }
}

// Status codes used here (transcribe.h, `transcribe_status`).
const OK: i32 = 0;
const ERR_BACKEND: i32 = 8;
const ERR_ABORTED: i32 = 13;
const ERR_INPUT_TOO_LONG: i32 = 17;
const ERR_OUTPUT_TRUNCATED: i32 = 18;
const ERR_OUTPUT_REPETITION: i32 = 19;

// `transcribe_backend_request`.
const BACKEND_AUTO: i32 = 0;
const BACKEND_CPU: i32 = 1;
// `TRANSCRIBE_BACKEND_MASK_*`.
const MASK_CPU: u32 = 0x1;
const MASK_ALL: u32 = 0xFFFF_FFFF;
// `transcribe_timestamp_kind`.
const TIMESTAMPS_AUTO: i32 = 1;
// `transcribe_role`.
const ROLE_ASR: u32 = 1;
const ROLE_VAD: u32 = 1 << 3;

#[repr(C)]
struct RawModel(c_void);
#[repr(C)]
struct RawSession(c_void);
#[repr(C)]
struct RawVadSession(c_void);

// Mirrors of the public structs (transcribe.h and transcribe/vad.h of
// v0.4.1). C enums are `int`, `bool` is one byte, x64 alignment.

#[repr(C)]
struct BackendInitParams {
    struct_size: u64,
    artifact_dir: *const c_char,
    allowed_backends: u32,
}

#[repr(C)]
struct ModelLoadParams {
    struct_size: u64,
    backend: i32,
    device: *mut c_void,
    no_mmap: bool,
}

#[repr(C)]
struct SessionParams {
    struct_size: u64,
    n_threads: i32,
    kv_type: i32,
    n_ctx: i32,
}

#[repr(C)]
struct RunParams {
    struct_size: u64,
    task: i32,
    timestamps: i32,
    pnc: i32,
    itn: i32,
    diarize: i32,
    language: *const c_char,
    target_language: *const c_char,
    keep_special_tags: bool,
    family: *const c_void,
    spec_k_drafts: i32,
    vocabulary: *const *const c_char,
    n_vocabulary: i32,
    prompt: *const c_char,
    prefix: *const c_char,
}

#[repr(C)]
struct Capabilities {
    struct_size: u64,
    native_sample_rate: i32,
    n_languages: i32,
    languages: *const *const c_char,
    max_timestamp_kind: i32,
    supports_language_detect: bool,
    supports_translate: bool,
    supports_streaming: bool,
    supports_spec_decode: bool,
    max_audio_ms: i64,
    n_translate_target_languages: i32,
    translate_target_languages: *const *const c_char,
}

#[repr(C)]
struct SegmentRow {
    struct_size: u64,
    t0_ms: i64,
    t1_ms: i64,
    first_word: i32,
    n_words: i32,
    first_token: i32,
    n_tokens: i32,
    text: *const c_char,
    speaker_id: i32,
}

#[repr(C)]
struct VadInfo {
    struct_size: u64,
    sample_rate: i32,
    frame_samples: i32,
}

#[repr(C)]
struct VadSessionParams {
    struct_size: u64,
    n_threads: i32,
}

#[repr(C)]
struct VadResult {
    struct_size: u64,
    n_probs: i32,
    first_frame: i64,
    n_segments: i32,
}

/// `transcribe_abi_struct` ids and our size and alignment of each.
fn abi_table() -> [(i32, &'static str, usize, usize); 9] {
    use std::mem::{align_of, size_of};
    [
        (0, "model_load_params", size_of::<ModelLoadParams>(), align_of::<ModelLoadParams>()),
        (1, "session_params", size_of::<SessionParams>(), align_of::<SessionParams>()),
        (2, "run_params", size_of::<RunParams>(), align_of::<RunParams>()),
        (4, "capabilities", size_of::<Capabilities>(), align_of::<Capabilities>()),
        (6, "segment", size_of::<SegmentRow>(), align_of::<SegmentRow>()),
        (15, "backend_init_params", size_of::<BackendInitParams>(), align_of::<BackendInitParams>()),
        (24, "vad_info", size_of::<VadInfo>(), align_of::<VadInfo>()),
        (25, "vad_session_params", size_of::<VadSessionParams>(), align_of::<VadSessionParams>()),
        (27, "vad_result", size_of::<VadResult>(), align_of::<VadResult>()),
    ]
}

type LogCallback = unsafe extern "C" fn(level: i32, msg: *const c_char, user: *mut c_void);
type AbortCallback = unsafe extern "C" fn(user: *mut c_void) -> bool;

/// Declares the export table and its resolver.
macro_rules! api {
    ($($field:ident = $name:literal : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        struct Api {
            $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        /// Every export, or the name of the first one missing.
        unsafe fn resolve(module: HMODULE) -> Result<Api, String> {
            Ok(Api {
                $($field: {
                    let name = concat!($name, "\0");
                    let proc = unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }
                        .ok_or_else(|| format!("no export {}", $name))?;
                    // SAFETY: the type is the export's prototype in transcribe.h.
                    unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, _>(proc) }
                },)*
            })
        }
    };
}

api! {
    version = "transcribe_version": fn() -> *const c_char;
    status_string = "transcribe_status_string": fn(i32) -> *const c_char;
    abi_struct_size = "transcribe_abi_struct_size": fn(i32) -> usize;
    abi_struct_align = "transcribe_abi_struct_align": fn(i32) -> usize;
    log_set = "transcribe_log_set": fn(Option<LogCallback>, *mut c_void);
    backend_init_params_init = "transcribe_backend_init_params_init": fn(*mut BackendInitParams);
    init_backends_ex = "transcribe_init_backends_ex": fn(*const BackendInitParams) -> i32;
    model_load_params_init = "transcribe_model_load_params_init": fn(*mut ModelLoadParams);
    session_params_init = "transcribe_session_params_init": fn(*mut SessionParams);
    run_params_init = "transcribe_run_params_init": fn(*mut RunParams);
    capabilities_init = "transcribe_capabilities_init": fn(*mut Capabilities);
    segment_init = "transcribe_segment_init": fn(*mut SegmentRow);
    model_load_file = "transcribe_model_load_file":
        fn(*const c_char, *const ModelLoadParams, *mut *mut RawModel) -> i32;
    model_free = "transcribe_model_free": fn(*mut RawModel);
    model_roles = "transcribe_model_roles": fn(*const RawModel) -> u32;
    model_get_capabilities = "transcribe_model_get_capabilities":
        fn(*const RawModel, *mut Capabilities) -> i32;
    model_backend = "transcribe_model_backend": fn(*const RawModel) -> *const c_char;
    session_init = "transcribe_session_init":
        fn(*mut RawModel, *const SessionParams, *mut *mut RawSession) -> i32;
    session_free = "transcribe_session_free": fn(*mut RawSession);
    run = "transcribe_run": fn(*mut RawSession, *const f32, i32, *const RunParams) -> i32;
    set_abort_callback = "transcribe_set_abort_callback":
        fn(*mut RawSession, Option<AbortCallback>, *mut c_void);
    full_text = "transcribe_full_text": fn(*const RawSession) -> *const c_char;
    n_segments = "transcribe_n_segments": fn(*const RawSession) -> i32;
    get_segment = "transcribe_get_segment": fn(*const RawSession, i32, *mut SegmentRow) -> i32;
    detected_language = "transcribe_detected_language": fn(*const RawSession) -> *const c_char;
    vad_info_init = "transcribe_vad_info_init": fn(*mut VadInfo);
    vad_get_info = "transcribe_vad_get_info": fn(*const RawModel, *mut VadInfo) -> i32;
    vad_session_params_init = "transcribe_vad_session_params_init": fn(*mut VadSessionParams);
    vad_result_init = "transcribe_vad_result_init": fn(*mut VadResult);
    vad_session_init = "transcribe_vad_session_init":
        fn(*mut RawModel, *const VadSessionParams, *mut *mut RawVadSession) -> i32;
    vad_session_free = "transcribe_vad_session_free": fn(*mut RawVadSession);
    vad_set_abort_callback = "transcribe_vad_set_abort_callback":
        fn(*mut RawVadSession, Option<AbortCallback>, *mut c_void);
    vad_stream_feed = "transcribe_vad_stream_feed": fn(*mut RawVadSession, *const f32, i32) -> i32;
    vad_stream_flush = "transcribe_vad_stream_flush": fn(*mut RawVadSession) -> i32;
    vad_get_result = "transcribe_vad_get_result": fn(*const RawVadSession, *mut VadResult) -> i32;
    vad_probs = "transcribe_vad_probs": fn(*const RawVadSession) -> *const f32;
}

/// The loaded library. One per process, never unloaded.
pub struct Engine {
    api: Api,
    dir: PathBuf,
}

// SAFETY: the API is a table of function pointers into a library that
// stays loaded; its model functions are thread-safe (transcribe.h,
// "Threading"), sessions are used by one thread at a time (`&mut`).
unsafe impl Send for Engine {}
unsafe impl Sync for Engine {}

static LOADED: Mutex<Option<&'static Engine>> = Mutex::new(None);

/// The last engine messages (warnings and errors), for a failure report.
static LOG: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const LOG_LINES: usize = 64;

unsafe extern "C" fn on_log(level: i32, msg: *const c_char, _user: *mut c_void) {
    // INFO = 1, WARN = 2, ERROR = 3; DEBUG and continuations are dropped.
    if !(2..=3).contains(&level) || msg.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    let line = text.trim_end().to_string();
    if let Ok(mut log) = LOG.lock() {
        if log.len() == LOG_LINES {
            log.pop_front();
        }
        log.push_back(line);
    }
}

/// Takes the warnings and errors the engine logged since the last call.
pub fn take_log() -> Vec<String> {
    LOG.lock().map(|mut log| log.drain(..).collect()).unwrap_or_default()
}

unsafe extern "C" fn on_abort(user: *mut c_void) -> bool {
    // SAFETY: `user` is the `AtomicBool` the caller keeps alive for the run.
    unsafe { (*(user as *const AtomicBool)).load(Ordering::Relaxed) }
}

fn text(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// The Visual C++ runtime DLLs the package links against, and the ones
/// `present` cannot find.
pub fn missing_runtime(present: impl Fn(&str) -> bool) -> Vec<&'static str> {
    ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"]
        .into_iter()
        .filter(|dll| !present(dll))
        .collect()
}

/// The runtime DLLs of `missing_runtime` that neither System32 nor `dir`
/// (an app-local copy) has.
pub fn runtime_missing(dir: &Path) -> Vec<&'static str> {
    missing_runtime(|name| runtime_present(name, dir))
}

/// Whether `dll` loads from System32 or from `dir` (an app-local copy).
fn runtime_present(dll: &str, dir: &Path) -> bool {
    unsafe {
        for candidate in [HSTRING::from(dll), HSTRING::from(dir.join(dll).as_os_str())] {
            let flags = if candidate == HSTRING::from(dll) {
                LOAD_LIBRARY_SEARCH_SYSTEM32
            } else {
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
            };
            if let Ok(module) = LoadLibraryExW(&candidate, None, flags) {
                let _ = FreeLibrary(module);
                return true;
            }
        }
    }
    false
}

impl Engine {
    /// Loads the engine from `dir` (once per process; later calls with the
    /// same folder return the same engine). `gpu: false` limits ggml to the
    /// CPU for this process: a broken GPU driver then never runs.
    pub fn load(dir: &Path, gpu: bool) -> Result<&'static Engine, EngineError> {
        let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(engine) = *loaded {
            if engine.dir == dir {
                return Ok(engine);
            }
            return Err(EngineError::OtherFolder(engine.dir.clone()));
        }
        let dll = dir.join(ENGINE.dll);
        if !dll.is_file() {
            return Err(EngineError::NotInstalled);
        }
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        let module = match unsafe { LoadLibraryExW(&HSTRING::from(dll.as_os_str()), None, flags) } {
            Ok(module) => module,
            Err(error) => {
                let missing = runtime_missing(dir);
                return Err(if missing.is_empty() {
                    EngineError::BadLibrary(error.message())
                } else {
                    EngineError::RuntimeMissing(missing)
                });
            }
        };
        let api = match unsafe { resolve(module) } {
            Ok(api) => api,
            Err(why) => {
                unsafe {
                    let _ = FreeLibrary(module);
                }
                return Err(EngineError::BadLibrary(why));
            }
        };
        if let Err(error) = check(&api) {
            unsafe {
                let _ = FreeLibrary(module);
            }
            return Err(error);
        }
        // From here on ggml has global state: the library stays loaded
        // even when the backends fail.
        let engine: &'static Engine = Box::leak(Box::new(Engine {
            api,
            dir: dir.to_path_buf(),
        }));
        unsafe { (engine.api.log_set)(Some(on_log), std::ptr::null_mut()) };
        let artifact = CString::new(dir.to_string_lossy().as_bytes())
            .map_err(|_| EngineError::BadLibrary("folder name".into()))?;
        let status = unsafe {
            let mut params: BackendInitParams = std::mem::zeroed();
            (engine.api.backend_init_params_init)(&mut params);
            params.artifact_dir = artifact.as_ptr();
            params.allowed_backends = if gpu { MASK_ALL } else { MASK_CPU };
            (engine.api.init_backends_ex)(&params)
        };
        if status != OK {
            return Err(EngineError::NoBackend);
        }
        *loaded = Some(engine);
        Ok(engine)
    }

    fn status_text(&self, status: i32) -> String {
        text(unsafe { (self.api.status_string)(status) })
    }
}

/// The version and every struct layout, before anything else is called.
fn check(api: &Api) -> Result<(), EngineError> {
    let version = text(unsafe { (api.version)() });
    if version != ENGINE.version {
        return Err(EngineError::Mismatch(format!(
            "version {version}, expected {}",
            ENGINE.version
        )));
    }
    for (id, name, size, align) in abi_table() {
        let theirs = unsafe { ((api.abi_struct_size)(id), (api.abi_struct_align)(id)) };
        if theirs != (size, align) {
            return Err(EngineError::Mismatch(format!(
                "{name}: library {}/{}, binding {size}/{align}",
                theirs.0, theirs.1
            )));
        }
    }
    Ok(())
}

/// A loaded model file (speech recognition or VAD).
pub struct Model {
    engine: &'static Engine,
    raw: *mut RawModel,
}

// SAFETY: transcribe_model_* functions are thread-safe; the handle is
// freed once, in `drop`, after every session borrowing it is gone.
unsafe impl Send for Model {}

impl Model {
    /// Loads `path`: on the best GPU when the engine allows one (AUTO), or
    /// strictly on the CPU.
    pub fn load(engine: &'static Engine, path: &Path, cpu: bool) -> Result<Model, EngineError> {
        let utf8 = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| EngineError::Model("path".into()))?;
        let mut raw = std::ptr::null_mut();
        let status = unsafe {
            let mut params: ModelLoadParams = std::mem::zeroed();
            (engine.api.model_load_params_init)(&mut params);
            params.backend = if cpu { BACKEND_CPU } else { BACKEND_AUTO };
            (engine.api.model_load_file)(utf8.as_ptr(), &params, &mut raw)
        };
        if status != OK || raw.is_null() {
            let mut why = engine.status_text(status);
            if let Some(last) = take_log().pop() {
                why = format!("{why} ({last})");
            }
            return Err(EngineError::Model(why));
        }
        Ok(Model { engine, raw })
    }

    pub fn is_asr(&self) -> bool {
        unsafe { (self.engine.api.model_roles)(self.raw) & ROLE_ASR != 0 }
    }

    pub fn is_vad(&self) -> bool {
        unsafe { (self.engine.api.model_roles)(self.raw) & ROLE_VAD != 0 }
    }

    /// "cpu", "vulkan", ...: where the weights ended up.
    pub fn backend(&self) -> String {
        text(unsafe { (self.engine.api.model_backend)(self.raw) })
    }

    /// The longest audio one run takes, when the model has a limit.
    pub fn max_audio_ms(&self) -> Option<u64> {
        let caps = self.capabilities()?;
        (caps.max_audio_ms > 0).then_some(caps.max_audio_ms as u64)
    }

    /// Whether the model gives timestamps of its own.
    pub fn has_timestamps(&self) -> bool {
        self.capabilities()
            .is_some_and(|caps| caps.max_timestamp_kind > TIMESTAMPS_AUTO)
    }

    fn capabilities(&self) -> Option<Capabilities> {
        unsafe {
            let mut caps: Capabilities = std::mem::zeroed();
            (self.engine.api.capabilities_init)(&mut caps);
            ((self.engine.api.model_get_capabilities)(self.raw, &mut caps) == OK).then_some(caps)
        }
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        unsafe { (self.engine.api.model_free)(self.raw) };
    }
}

/// One timed piece of a run, relative to the audio passed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub text: String,
    /// Empty from a model without timestamps (Qwen3-ASR).
    pub segments: Vec<RawSegment>,
    /// The detected language ("id", "en"), unless one was asked for.
    pub language: Option<String>,
    /// The decode stopped early (its budget ran out, or it started looping):
    /// the text is what came before.
    pub partial: bool,
}

/// A recognition session on a model; one thread at a time.
pub struct Session<'m> {
    model: &'m Model,
    raw: *mut RawSession,
}

impl<'m> Session<'m> {
    /// `threads` CPU threads for the ops that run on the CPU (0: the
    /// library's choice).
    pub fn new(model: &'m Model, threads: i32) -> Result<Session<'m>, EngineError> {
        let api = &model.engine.api;
        let mut raw = std::ptr::null_mut();
        let status = unsafe {
            let mut params: SessionParams = std::mem::zeroed();
            (api.session_params_init)(&mut params);
            params.n_threads = threads.max(0);
            (api.session_init)(model.raw, &params, &mut raw)
        };
        if status != OK || raw.is_null() {
            return Err(EngineError::Status(status, model.engine.status_text(status)));
        }
        Ok(Session { model, raw })
    }

    /// Transcribes `pcm` (16 kHz mono, finite, at least one sample).
    /// `language` is a hint ("id", "en") or `None` to detect it. `cancel`
    /// stops the run between decode steps.
    pub fn run(
        &mut self,
        pcm: &[f32],
        language: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<RunOutput, EngineError> {
        let engine = self.model.engine;
        let api = &engine.api;
        let language = language.and_then(|l| CString::new(l).ok());
        let n = i32::try_from(pcm.len()).map_err(|_| EngineError::TooLong)?;
        let status = unsafe {
            let mut params: RunParams = std::mem::zeroed();
            (api.run_params_init)(&mut params);
            params.timestamps = TIMESTAMPS_AUTO;
            params.language = language.as_ref().map_or(std::ptr::null(), |l| l.as_ptr());
            (api.set_abort_callback)(
                self.raw,
                Some(on_abort),
                cancel as *const AtomicBool as *mut c_void,
            );
            let status = (api.run)(self.raw, pcm.as_ptr(), n, &params);
            (api.set_abort_callback)(self.raw, None, std::ptr::null_mut());
            status
        };
        let partial = match outcome(status) {
            Outcome::Done => false,
            Outcome::Partial => true,
            Outcome::Fail(error) => {
                return Err(match error {
                    EngineError::Status(code, _) => {
                        EngineError::Status(code, engine.status_text(code))
                    }
                    other => other,
                });
            }
        };
        let segments = unsafe {
            (0..(api.n_segments)(self.raw))
                .filter_map(|i| {
                    let mut row: SegmentRow = std::mem::zeroed();
                    (api.segment_init)(&mut row);
                    ((api.get_segment)(self.raw, i, &mut row) == OK && !row.text.is_null())
                        .then(|| RawSegment {
                            start_ms: row.t0_ms.max(0) as u64,
                            end_ms: row.t1_ms.max(row.t0_ms).max(0) as u64,
                            text: text(row.text).trim().to_string(),
                        })
                })
                .collect()
        };
        let detected = text(unsafe { (api.detected_language)(self.raw) });
        Ok(RunOutput {
            text: text(unsafe { (api.full_text)(self.raw) }).trim().to_string(),
            segments,
            language: (!detected.is_empty()).then_some(detected),
            partial,
        })
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        unsafe { (self.model.engine.api.session_free)(self.raw) };
    }
}

#[derive(Debug, PartialEq)]
enum Outcome {
    Done,
    Partial,
    Fail(EngineError),
}

/// What a `transcribe_run` status means for the caller.
fn outcome(status: i32) -> Outcome {
    match status {
        OK => Outcome::Done,
        ERR_OUTPUT_TRUNCATED | ERR_OUTPUT_REPETITION => Outcome::Partial,
        ERR_ABORTED => Outcome::Fail(EngineError::Cancelled),
        ERR_INPUT_TOO_LONG => Outcome::Fail(EngineError::TooLong),
        ERR_BACKEND => Outcome::Fail(EngineError::Backend),
        other => Outcome::Fail(EngineError::Status(other, String::new())),
    }
}

/// Silero VAD, streamed: speech probabilities per frame.
pub struct VadSession<'m> {
    model: &'m Model,
    raw: *mut RawVadSession,
    frame_samples: usize,
}

impl<'m> VadSession<'m> {
    pub fn new(model: &'m Model) -> Result<VadSession<'m>, EngineError> {
        let api = &model.engine.api;
        let mut raw = std::ptr::null_mut();
        let (status, frame) = unsafe {
            let mut info: VadInfo = std::mem::zeroed();
            (api.vad_info_init)(&mut info);
            let info_status = (api.vad_get_info)(model.raw, &mut info);
            if info_status != OK {
                (info_status, 0)
            } else {
                let mut params: VadSessionParams = std::mem::zeroed();
                (api.vad_session_params_init)(&mut params);
                params.n_threads = 1;
                ((api.vad_session_init)(model.raw, &params, &mut raw), info.frame_samples)
            }
        };
        if status != OK || raw.is_null() || frame <= 0 {
            return Err(EngineError::Status(status, model.engine.status_text(status)));
        }
        Ok(VadSession {
            model,
            raw,
            frame_samples: frame as usize,
        })
    }

    /// Samples per probability (512 for Silero at 16 kHz).
    pub fn frame_samples(&self) -> usize {
        self.frame_samples
    }

    /// Feeds audio; the probability of every frame it completed.
    pub fn feed(&mut self, pcm: &[f32]) -> Result<Vec<f32>, EngineError> {
        let n = i32::try_from(pcm.len()).map_err(|_| EngineError::TooLong)?;
        let status = unsafe { (self.model.engine.api.vad_stream_feed)(self.raw, pcm.as_ptr(), n) };
        self.probs(status)
    }

    /// The last, zero-padded frame.
    pub fn flush(&mut self) -> Result<Vec<f32>, EngineError> {
        let status = unsafe { (self.model.engine.api.vad_stream_flush)(self.raw) };
        self.probs(status)
    }

    fn probs(&self, status: i32) -> Result<Vec<f32>, EngineError> {
        let api = &self.model.engine.api;
        if status != OK {
            return Err(EngineError::Status(status, self.model.engine.status_text(status)));
        }
        unsafe {
            let mut result: VadResult = std::mem::zeroed();
            (api.vad_result_init)(&mut result);
            if (api.vad_get_result)(self.raw, &mut result) != OK || result.n_probs <= 0 {
                return Ok(Vec::new());
            }
            let probs = (api.vad_probs)(self.raw);
            if probs.is_null() {
                return Ok(Vec::new());
            }
            Ok(std::slice::from_raw_parts(probs, result.n_probs as usize).to_vec())
        }
    }
}

impl Drop for VadSession<'_> {
    fn drop(&mut self) {
        unsafe { (self.model.engine.api.vad_session_free)(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    /// The offsets a C compiler gives these structs on x64 (transcribe.h of
    /// v0.4.1, worked out by hand); the run-time check compares the sizes
    /// with the library's own.
    #[test]
    fn the_struct_mirrors_have_the_c_layout() {
        assert_eq!((size_of::<BackendInitParams>(), align_of::<BackendInitParams>()), (24, 8));
        assert_eq!(offset_of!(BackendInitParams, allowed_backends), 16);

        assert_eq!(size_of::<ModelLoadParams>(), 32);
        assert_eq!(offset_of!(ModelLoadParams, device), 16);
        assert_eq!(offset_of!(ModelLoadParams, no_mmap), 24);

        assert_eq!(size_of::<SessionParams>(), 24);
        assert_eq!(offset_of!(SessionParams, n_ctx), 16);

        assert_eq!(size_of::<RunParams>(), 104);
        assert_eq!(offset_of!(RunParams, diarize), 24);
        assert_eq!(offset_of!(RunParams, language), 32);
        assert_eq!(offset_of!(RunParams, keep_special_tags), 48);
        assert_eq!(offset_of!(RunParams, family), 56);
        assert_eq!(offset_of!(RunParams, spec_k_drafts), 64);
        assert_eq!(offset_of!(RunParams, vocabulary), 72);
        assert_eq!(offset_of!(RunParams, n_vocabulary), 80);
        assert_eq!(offset_of!(RunParams, prefix), 96);

        assert_eq!(size_of::<Capabilities>(), 56);
        assert_eq!(offset_of!(Capabilities, max_timestamp_kind), 24);
        assert_eq!(offset_of!(Capabilities, supports_spec_decode), 31);
        assert_eq!(offset_of!(Capabilities, max_audio_ms), 32);
        assert_eq!(offset_of!(Capabilities, translate_target_languages), 48);

        assert_eq!(size_of::<SegmentRow>(), 56);
        assert_eq!(offset_of!(SegmentRow, text), 40);
        assert_eq!(offset_of!(SegmentRow, speaker_id), 48);

        assert_eq!(size_of::<VadInfo>(), 16);
        assert_eq!(size_of::<VadSessionParams>(), 16);
        assert_eq!(size_of::<VadResult>(), 32);
        assert_eq!(offset_of!(VadResult, first_frame), 16);
        assert_eq!(offset_of!(VadResult, n_segments), 24);
        assert!(abi_table().iter().all(|&(_, _, size, align)| size > 0 && align == 8));
    }

    #[test]
    fn run_statuses_map_to_outcomes() {
        assert_eq!(outcome(OK), Outcome::Done);
        assert_eq!(outcome(ERR_OUTPUT_TRUNCATED), Outcome::Partial);
        assert_eq!(outcome(ERR_OUTPUT_REPETITION), Outcome::Partial);
        assert_eq!(outcome(ERR_ABORTED), Outcome::Fail(EngineError::Cancelled));
        assert_eq!(outcome(ERR_INPUT_TOO_LONG), Outcome::Fail(EngineError::TooLong));
        assert_eq!(outcome(ERR_BACKEND), Outcome::Fail(EngineError::Backend));
        assert!(matches!(outcome(4), Outcome::Fail(EngineError::Status(4, _))));
    }

    #[test]
    fn the_runtime_check_names_what_is_missing() {
        assert!(missing_runtime(|_| true).is_empty());
        assert_eq!(
            missing_runtime(|dll| dll != "msvcp140.dll"),
            vec!["msvcp140.dll"]
        );
        // kernel32 always loads from System32; a made-up DLL never does.
        let dir = Path::new(".");
        assert!(runtime_present("kernel32.dll", dir));
        assert!(!runtime_present("vale-no-such-runtime.dll", dir));
    }

    #[test]
    fn a_folder_without_the_library_is_not_installed() {
        let store = crate::transcribe::store::tests::temp_store("engine-missing");
        assert_eq!(
            Engine::load(&store.engine_dir(), true).err(),
            Some(EngineError::NotInstalled)
        );
    }

    #[test]
    fn a_file_that_is_not_a_library_fails_cleanly() {
        let store = crate::transcribe::store::tests::temp_store("engine-garbage");
        let dir = store.engine_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(ENGINE.dll), b"MZ not really a library").unwrap();
        assert!(matches!(
            Engine::load(&dir, false),
            Err(EngineError::BadLibrary(_) | EngineError::RuntimeMissing(_))
        ));
    }

    #[test]
    fn the_abort_callback_reads_the_flag() {
        let flag = AtomicBool::new(false);
        let user = &flag as *const AtomicBool as *mut c_void;
        assert!(!unsafe { on_abort(user) });
        flag.store(true, Ordering::Relaxed);
        assert!(unsafe { on_abort(user) });
    }

    #[test]
    fn the_log_keeps_warnings_and_errors_only() {
        let _ = take_log();
        let warn = CString::new("vulkan: no device\n").unwrap();
        let info = CString::new("loaded").unwrap();
        unsafe {
            on_log(2, warn.as_ptr(), std::ptr::null_mut());
            on_log(1, info.as_ptr(), std::ptr::null_mut());
        }
        let log = take_log();
        assert!(log.contains(&"vulkan: no device".to_string()), "{log:?}");
        assert!(!log.contains(&"loaded".to_string()));
    }
}
