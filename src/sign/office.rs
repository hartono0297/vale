//! Office to PDF conversion for the signing module. Microsoft Office and WPS
//! are driven through late-bound COM (`IDispatch`); LibreOffice runs headless.
//! Nothing here touches an office instance the user started: `Quit` and
//! `TerminateProcess` only ever go to a process our own `CoCreateInstance`
//! launched.

use std::ffi::OsString;
use std::io;
use std::mem::ManuallyDrop;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, VARIANT_BOOL};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CLSIDFromProgID, COINIT_APARTMENTTHREADED, CoCreateInstance,
    CoInitializeEx, CoUninitialize, DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET,
    DISPATCH_PROPERTYPUT, DISPPARAMS, EXCEPINFO, IDispatch,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Ole::DISPID_PROPERTYPUT;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey,
    RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, OpenProcess, PROCESS_TERMINATE, TerminateProcess,
};
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_ERROR, VT_I4,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::{GUID, PCWSTR};

/// How long one conversion may take before the instance we started is killed.
const TIMEOUT: Duration = Duration::from_secs(90);
/// Opens password protected files without a prompt: the wrong password makes
/// the open fail, which we report as `Locked`.
const NO_PASSWORD: &str = "vale-no-password";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Suite {
    Microsoft,
    Wps,
    LibreOffice(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Writer,
    Sheets,
    Slides,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertError {
    NoSuite,
    Locked,
    Failed,
    TimedOut,
}

pub fn app_for(path: &Path) -> Option<App> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "doc" | "docx" => Some(App::Writer),
        "xls" | "xlsx" => Some(App::Sheets),
        "ppt" | "pptx" => Some(App::Slides),
        _ => None,
    }
}

// ---------------------------------------------------------------- detection

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn key_exists(root: HKEY, subkey: &str) -> bool {
    let sub = wide(subkey);
    let mut key = HKEY::default();
    unsafe {
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), None, KEY_READ, &mut key).is_ok() {
            let _ = RegCloseKey(key);
            true
        } else {
            false
        }
    }
}

/// The default (unnamed) string value of `subkey`.
fn default_string(root: HKEY, subkey: &str) -> Option<String> {
    let sub = wide(subkey);
    let mut len = 0u32;
    unsafe {
        RegGetValueW(root, PCWSTR(sub.as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ, None, None, Some(&mut len))
            .ok()
            .ok()?;
        if len < 2 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        RegGetValueW(
            root,
            PCWSTR(sub.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .ok()
        .ok()?;
        let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

fn ms_prog_id(app: App) -> &'static str {
    match app {
        App::Writer => "Word.Application",
        App::Sheets => "Excel.Application",
        App::Slides => "PowerPoint.Application",
    }
}

fn wps_prog_ids(app: App) -> [&'static str; 2] {
    match app {
        App::Writer => ["KWps.Application", "Kwps.Application"],
        App::Sheets => ["KET.Application", "Ket.Application"],
        App::Slides => ["KWPP.Application", "Kwpp.Application"],
    }
}

fn prog_id_exists(prog_id: &str) -> bool {
    key_exists(HKEY_CLASSES_ROOT, &format!("{prog_id}\\CLSID"))
}

fn libreoffice() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = default_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\LibreOffice\UNO\InstallPath") {
        candidates.push(Path::new(&dir).join("soffice.com"));
    }
    if let Some(exe) = default_string(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\soffice.exe",
    ) && let Some(dir) = Path::new(&exe).parent()
    {
        candidates.push(dir.join("soffice.com"));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(var) {
            candidates.push(Path::new(&base).join(r"LibreOffice\program\soffice.com"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// The suite that will convert `app`'s files on this PC, Microsoft first.
pub fn detect(app: App) -> Option<Suite> {
    if prog_id_exists(ms_prog_id(app)) {
        return Some(Suite::Microsoft);
    }
    if wps_prog_ids(app).iter().any(|id| prog_id_exists(id)) {
        return Some(Suite::Wps);
    }
    libreoffice().map(Suite::LibreOffice)
}

// ------------------------------------------------------------- temp folders

pub fn temp_root() -> PathBuf {
    std::env::temp_dir().join("Vale").join("sign")
}

pub fn sweep(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

static JOB: AtomicU32 = AtomicU32::new(0);

/// A fresh folder `temp_root()\<pid>-<counter>` for one conversion.
pub fn job_dir() -> io::Result<PathBuf> {
    let n = JOB.fetch_add(1, Ordering::Relaxed);
    let dir = temp_root().join(format!("{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

// ---------------------------------------------------------------- processes

fn exe_names(suite: &Suite, app: App) -> &'static [&'static str] {
    match (suite, app) {
        (Suite::Wps, App::Writer) => &["wps.exe"],
        (Suite::Wps, App::Sheets) => &["et.exe"],
        (Suite::Wps, App::Slides) => &["wpp.exe"],
        (_, App::Writer) => &["winword.exe"],
        (_, App::Sheets) => &["excel.exe"],
        (_, App::Slides) => &["powerpnt.exe"],
    }
}

fn pids_of(names: &[&str]) -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let n = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..n]).to_lowercase();
                if names.contains(&exe.as_str()) {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

/// The pid we may quit or kill: the one our `CoCreateInstance` started, never
/// one that was already running. `app_pid` is the pid behind the application's
/// window when it has one.
fn should_quit(before: &[u32], after: &[u32], app_pid: Option<u32>) -> Option<u32> {
    if let Some(pid) = app_pid {
        return if before.contains(&pid) { None } else { Some(pid) };
    }
    let mut fresh = after.iter().filter(|p| !before.contains(p));
    match (fresh.next(), fresh.next()) {
        (Some(p), None) => Some(*p),
        _ => None,
    }
}

fn terminate(pid: u32) {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(h, 1);
            let _ = CloseHandle(h);
        }
    }
}

// ---------------------------------------------------------------------- COM

struct ComGuard(bool);

impl ComGuard {
    fn new() -> ComGuard {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        ComGuard(hr.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

#[derive(Debug)]
struct DispErr {
    description: String,
}

impl DispErr {
    fn plain(msg: &str) -> DispErr {
        DispErr { description: msg.to_string() }
    }

    /// The open failed because of the password we passed (office text is
    /// localised, so match the few words it uses).
    fn is_password(&self) -> bool {
        let d = self.description.to_lowercase();
        ["password", "kata sandi", "sandi", "protected", "encrypt"].iter().any(|w| d.contains(w))
    }
}

fn variant(vt: VARENUM, value: VARIANT_0_0_0) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: value,
            }),
        },
    }
}

fn vi(v: i32) -> VARIANT {
    variant(VT_I4, VARIANT_0_0_0 { lVal: v })
}

fn vb(v: bool) -> VARIANT {
    variant(VT_BOOL, VARIANT_0_0_0 { boolVal: VARIANT_BOOL::from(v) })
}

fn vs(v: &str) -> VARIANT {
    VARIANT::from(v)
}

/// A skipped optional argument.
fn missing() -> VARIANT {
    variant(VT_ERROR, VARIANT_0_0_0 { scode: 0x8002_0004_u32 as i32 })
}

/// The application is busy for a moment (RPC_E_CALL_REJECTED or
/// RPC_E_SERVERCALL_RETRYLATER); asking again for a few seconds usually works.
fn is_busy(e: &windows::core::Error) -> bool {
    matches!(e.code().0 as u32, 0x8001_0001 | 0x8001_010A)
}

struct Disp(IDispatch);

impl Disp {
    fn id(&self, name: &str) -> Result<i32, DispErr> {
        let n = wide(name);
        let names = [PCWSTR(n.as_ptr())];
        let mut tries = 0;
        loop {
            let mut id = 0i32;
            match unsafe { self.0.GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, 0x0409, &mut id) } {
                Ok(()) => return Ok(id),
                Err(e) if is_busy(&e) && tries < 50 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(DispErr { description: format!("{name}: {e}") }),
            }
        }
    }

    fn invoke(&self, name: &str, flags: DISPATCH_FLAGS, args: &[VARIANT]) -> Result<VARIANT, DispErr> {
        let id = self.id(name)?;
        // Invoke wants the arguments last to first.
        let mut rg: Vec<VARIANT> = args.iter().rev().cloned().collect();
        let mut named = DISPID_PROPERTYPUT;
        let put = flags == DISPATCH_PROPERTYPUT;
        let params = DISPPARAMS {
            rgvarg: if rg.is_empty() { std::ptr::null_mut() } else { rg.as_mut_ptr() },
            rgdispidNamedArgs: if put { &mut named } else { std::ptr::null_mut() },
            cArgs: rg.len() as u32,
            cNamedArgs: u32::from(put),
        };
        let mut tries = 0;
        loop {
            let mut result = VARIANT::default();
            let mut excep = EXCEPINFO::default();
            let r = unsafe {
                self.0.Invoke(id, &GUID::zeroed(), 0x0409, flags, &params, Some(&mut result), Some(&mut excep), None)
            };
            let description = ManuallyDrop::into_inner(std::mem::take(&mut excep.bstrDescription)).to_string();
            drop(ManuallyDrop::into_inner(std::mem::take(&mut excep.bstrSource)));
            drop(ManuallyDrop::into_inner(std::mem::take(&mut excep.bstrHelpFile)));
            match r {
                Ok(()) => return Ok(result),
                Err(e) if is_busy(&e) && tries < 50 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(DispErr { description: format!("{name}: {description} ({e})") }),
            }
        }
    }

    fn get(&self, name: &str) -> Result<VARIANT, DispErr> {
        self.invoke(name, DISPATCH_PROPERTYGET, &[])
    }

    fn get_disp(&self, name: &str) -> Result<Disp, DispErr> {
        to_disp(&self.get(name)?)
    }

    fn put(&self, name: &str, value: VARIANT) -> Result<(), DispErr> {
        self.invoke(name, DISPATCH_PROPERTYPUT, &[value]).map(|_| ())
    }

    /// A method, or an indexed property such as `Range("A1")`.
    fn call(&self, name: &str, args: &[VARIANT]) -> Result<VARIANT, DispErr> {
        self.invoke(name, DISPATCH_FLAGS(DISPATCH_METHOD.0 | DISPATCH_PROPERTYGET.0), args)
    }

    fn call_disp(&self, name: &str, args: &[VARIANT]) -> Result<Disp, DispErr> {
        to_disp(&self.call(name, args)?)
    }
}

fn to_disp(v: &VARIANT) -> Result<Disp, DispErr> {
    IDispatch::try_from(v).map(Disp).map_err(|_| DispErr::plain("not an object"))
}

fn to_i32(v: &VARIANT) -> Option<i32> {
    i32::try_from(v).ok()
}

/// A running office application of ours (or the user's, which we then leave
/// alone). `Drop` quits it only when `ours` is set.
struct Session {
    app: Disp,
    ours: Option<u32>,
}

impl Session {
    fn start(suite: &Suite, app_kind: App) -> Result<Session, ConvertError> {
        let prog_id = match suite {
            Suite::Microsoft => ms_prog_id(app_kind),
            Suite::Wps => wps_prog_ids(app_kind)
                .into_iter()
                .find(|id| prog_id_exists(id))
                .ok_or(ConvertError::NoSuite)?,
            Suite::LibreOffice(_) => return Err(ConvertError::NoSuite),
        };
        let names = exe_names(suite, app_kind);
        let p = wide(prog_id);
        let clsid = unsafe { CLSIDFromProgID(PCWSTR(p.as_ptr())) }.map_err(|_| ConvertError::NoSuite)?;
        let before = pids_of(names);
        let dispatch: IDispatch = unsafe { CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|_| ConvertError::Failed)?;
        let app = Disp(dispatch);
        let after = pids_of(names);
        let window_pid = if app_kind == App::Slides {
            None
        } else {
            app.get("Hwnd").ok().and_then(|v| to_i32(&v)).and_then(|h| {
                let mut pid = 0u32;
                unsafe { GetWindowThreadProcessId(HWND(h as isize as *mut _), Some(&mut pid)) };
                (pid != 0).then_some(pid)
            })
        };
        let ours = should_quit(&before, &after, window_pid);
        Ok(Session { app, ours })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.ours.is_some() {
            let _ = self.app.call("Quit", &[]);
        }
    }
}

fn open_error(e: &DispErr) -> ConvertError {
    if e.is_password() { ConvertError::Locked } else { ConvertError::Failed }
}

/// Quiet settings for the instance we started. A user's instance keeps its
/// window and its alerts; only macros are switched off while we open the file,
/// and the old setting comes back through `unquiet`.
fn quiet(app: &Disp, ours: bool, alerts_off: VARIANT) -> Result<Option<VARIANT>, ConvertError> {
    let step = |_: DispErr| ConvertError::Failed;
    let mut old = None;
    if ours {
        app.put("Visible", vb(false)).map_err(step)?;
        app.put("DisplayAlerts", alerts_off).map_err(step)?;
    } else {
        old = app.get("AutomationSecurity").ok();
    }
    app.put("AutomationSecurity", vi(3)).map_err(step)?;
    Ok(old)
}

fn unquiet(app: &Disp, old: Option<VARIANT>) {
    if let Some(v) = old {
        let _ = app.put("AutomationSecurity", v);
    }
}

fn drive_writer(app: &Disp, src: &Path, out: &Path, ours: bool) -> Result<(), ConvertError> {
    let step = |_: DispErr| ConvertError::Failed;
    let old = quiet(app, ours, vi(0))?;
    let r = (|| {
        let docs = app.get_disp("Documents").map_err(step)?;
        let doc = docs
            .call_disp(
                "Open",
                &[vs(&src.to_string_lossy()), vb(false), vb(true), vb(false), vs(NO_PASSWORD)],
            )
            .map_err(|e| open_error(&e))?;
        let exported = doc.call("ExportAsFixedFormat", &[vs(&out.to_string_lossy()), vi(17)]);
        let _ = doc.call("Close", &[vi(0)]);
        exported.map(|_| ()).map_err(step)
    })();
    unquiet(app, old);
    r
}

fn drive_sheets(app: &Disp, src: &Path, out: &Path, ours: bool) -> Result<(), ConvertError> {
    let step = |_: DispErr| ConvertError::Failed;
    let old = quiet(app, ours, vb(false))?;
    let r = (|| {
        let books = app.get_disp("Workbooks").map_err(step)?;
        let book = books
            .call_disp(
                "Open",
                &[vs(&src.to_string_lossy()), vi(0), vb(true), missing(), vs(NO_PASSWORD)],
            )
            .map_err(|e| open_error(&e))?;
        let exported = book.call("ExportAsFixedFormat", &[vi(0), vs(&out.to_string_lossy())]);
        let _ = book.call("Close", &[vb(false)]);
        exported.map(|_| ()).map_err(step)
    })();
    unquiet(app, old);
    r
}

fn drive_slides(app: &Disp, src: &Path, out: &Path) -> Result<(), ConvertError> {
    let step = |_: DispErr| ConvertError::Failed;
    let decks = app.get_disp("Presentations").map_err(step)?;
    let name = format!("{}::{NO_PASSWORD}", src.to_string_lossy());
    let deck = decks
        .call_disp("Open", &[vs(&name), vi(-1), vi(0), vi(0)])
        .map_err(|e| open_error(&e))?;
    let result = (|| {
        let slides = deck.get_disp("Slides")?;
        let count = to_i32(&slides.get("Count")?).unwrap_or(0);
        if count == 0 {
            return Err(DispErr::plain("no slides"));
        }
        deck.call("SaveAs", &[vs(&out.to_string_lossy()), vi(32)]).map(|_| ())
    })();
    let _ = deck.call("Close", &[]);
    result.map_err(step)
}

fn convert_com(
    suite: &Suite,
    app_kind: App,
    src: &Path,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<(), ConvertError> {
    let _com = ComGuard::new();
    let done = AtomicBool::new(false);
    let timed_out = AtomicBool::new(false);
    let ours_pid = AtomicU32::new(0);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            let start = Instant::now();
            while !done.load(Ordering::Acquire) {
                let expired = start.elapsed() >= TIMEOUT;
                if expired || cancel.load(Ordering::Relaxed) {
                    if expired {
                        timed_out.store(true, Ordering::Release);
                    }
                    cancel.store(true, Ordering::Release);
                    let pid = ours_pid.load(Ordering::Acquire);
                    if pid != 0 {
                        terminate(pid);
                    }
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let r = (|| {
            let session = Session::start(suite, app_kind)?;
            if let Some(pid) = session.ours {
                ours_pid.store(pid, Ordering::Release);
            }
            match app_kind {
                App::Writer => drive_writer(&session.app, src, out, session.ours.is_some()),
                App::Sheets => drive_sheets(&session.app, src, out, session.ours.is_some()),
                App::Slides => drive_slides(&session.app, src, out),
            }
        })();
        done.store(true, Ordering::Release);
        r
    });
    if timed_out.load(Ordering::Acquire) {
        return Err(ConvertError::TimedOut);
    }
    result
}

// -------------------------------------------------------------- LibreOffice

fn soffice_args(src: &Path, job: &Path) -> Vec<OsString> {
    let profile = format!("{}/profile", job.to_string_lossy().replace('\\', "/"));
    vec![
        "--headless".into(),
        "--norestore".into(),
        "--nolockcheck".into(),
        format!("-env:UserInstallation=file:///{profile}").into(),
        "--convert-to".into(),
        "pdf".into(),
        "--outdir".into(),
        job.as_os_str().to_owned(),
        src.as_os_str().to_owned(),
    ]
}

fn convert_soffice(exe: &Path, src: &Path, job: &Path, cancel: &AtomicBool) -> Result<(), ConvertError> {
    let mut child = Command::new(exe)
        .args(soffice_args(src, job))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|_| ConvertError::Failed)?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(_) => return Err(ConvertError::Failed),
        }
        let timed_out = start.elapsed() >= TIMEOUT;
        if timed_out || cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if timed_out { ConvertError::TimedOut } else { ConvertError::Failed });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ------------------------------------------------------------------- public

/// Converts one Office file to `<out_dir>\<stem>.pdf`. Blocks the caller's
/// thread (up to 90 s); `cancel` aborts it.
pub fn convert(src: &Path, out_dir: &Path, cancel: &AtomicBool) -> Result<PathBuf, ConvertError> {
    let app = app_for(src).ok_or(ConvertError::Failed)?;
    let suite = detect(app).ok_or(ConvertError::NoSuite)?;
    std::fs::create_dir_all(out_dir).map_err(|_| ConvertError::Failed)?;
    let stem = src.file_stem().ok_or(ConvertError::Failed)?;
    let mut name = stem.to_os_string();
    name.push(".pdf");
    let out = out_dir.join(name);
    let _ = std::fs::remove_file(&out);
    match &suite {
        Suite::LibreOffice(exe) => convert_soffice(exe, src, out_dir, cancel)?,
        _ => convert_com(&suite, app, src, &out, cancel)?,
    }
    if out.is_file() { Ok(out) } else { Err(ConvertError::Failed) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn app_for_extensions() {
        assert_eq!(app_for(Path::new("a.DOCX")), Some(App::Writer));
        assert_eq!(app_for(Path::new("a.doc")), Some(App::Writer));
        assert_eq!(app_for(Path::new("a.xls")), Some(App::Sheets));
        assert_eq!(app_for(Path::new("a.xlsx")), Some(App::Sheets));
        assert_eq!(app_for(Path::new("a.ppt")), Some(App::Slides));
        assert_eq!(app_for(Path::new("a.pptx")), Some(App::Slides));
        assert_eq!(app_for(Path::new("a.pdf")), None);
        assert_eq!(app_for(Path::new("noext")), None);
    }

    #[test]
    fn quit_only_when_we_started_the_process() {
        assert_eq!(should_quit(&[10], &[10, 20], None), Some(20));
        assert_eq!(should_quit(&[10], &[10, 20], Some(20)), Some(20));
        assert_eq!(should_quit(&[10], &[10, 20], Some(10)), None);
        assert_eq!(should_quit(&[10], &[10], None), None);
        assert_eq!(should_quit(&[10], &[10], Some(10)), None);
        assert_eq!(should_quit(&[], &[1, 2], None), None);
    }

    #[test]
    fn soffice_args_use_private_profile() {
        let args = soffice_args(Path::new(r"C:\a b\x.docx"), Path::new(r"C:\t\job-1"));
        let args: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.contains(&"-env:UserInstallation=file:///C:/t/job-1/profile".to_string()));
        let i = args.iter().position(|a| a == "--convert-to").unwrap();
        assert_eq!(args[i + 1], "pdf");
        assert!(args.contains(&"--headless".to_string()));
        assert_eq!(args.last().unwrap(), r"C:\a b\x.docx");
    }

    #[test]
    fn sweep_removes_folder() {
        let dir = crate::phone::files::tests::temp_dir("sign-office-sweep");
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a").join("f.txt"), b"x").unwrap();
        sweep(&dir);
        assert!(!dir.exists());
    }

    // Live tests: run by hand with `cargo test --locked -- --ignored sign::office`.
    // They skip when the user already has that office app running, so they never
    // hide, close or touch an instance they did not start.

    fn busy(names: &[&str]) -> bool {
        // A Quit from an earlier test can take a few seconds to finish.
        let start = Instant::now();
        while !pids_of(names).is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(250));
        }
        let running = !pids_of(names).is_empty();
        if running {
            eprintln!("skipped: {names:?} is already running");
        }
        running
    }

    fn pdf_ok(path: &Path) {
        let bytes = std::fs::read(path).expect("pdf exists");
        assert!(bytes.starts_with(b"%PDF"), "not a pdf");
    }

    fn make_docx(path: &Path, password: Option<&str>) {
        let s = Session::start(&Suite::Microsoft, App::Writer).expect("word");
        s.app.put("DisplayAlerts", vi(0)).unwrap();
        let docs = s.app.get_disp("Documents").unwrap();
        let doc = docs.call_disp("Add", &[]).unwrap();
        doc.get_disp("Content").unwrap().put("Text", vs("Vale test")).unwrap();
        let mut args = vec![vs(&path.to_string_lossy()), vi(16)];
        if let Some(p) = password {
            args.push(vb(false));
            args.push(vs(p));
        }
        doc.call("SaveAs2", &args).unwrap();
        doc.call("Close", &[vi(0)]).unwrap();
    }

    fn make_xlsx(path: &Path) {
        let s = Session::start(&Suite::Microsoft, App::Sheets).expect("excel");
        s.app.put("DisplayAlerts", vb(false)).unwrap();
        let books = s.app.get_disp("Workbooks").unwrap();
        let book = books.call_disp("Add", &[]).unwrap();
        let sheet = book.get_disp("ActiveSheet").unwrap();
        sheet.call_disp("Range", &[vs("A1")]).unwrap().put("Value", vs("Vale test")).unwrap();
        book.call("SaveAs", &[vs(&path.to_string_lossy()), vi(51)]).unwrap();
        book.call("Close", &[vb(false)]).unwrap();
    }

    fn make_pptx(path: &Path) {
        let s = Session::start(&Suite::Microsoft, App::Slides).expect("powerpoint");
        let decks = s.app.get_disp("Presentations").unwrap();
        let deck = decks.call_disp("Add", &[vi(0)]).unwrap();
        deck.get_disp("Slides").unwrap().call("Add", &[vi(1), vi(12)]).unwrap();
        deck.call("SaveAs", &[vs(&path.to_string_lossy()), vi(24)]).unwrap();
        deck.call("Close", &[]).unwrap();
    }

    #[test]
    #[ignore]
    fn live_word_to_pdf() {
        if busy(&["winword.exe"]) {
            return;
        }
        let _com = ComGuard::new();
        let dir = crate::phone::files::tests::temp_dir("sign-office-word");
        let src = dir.join("t.docx");
        make_docx(&src, None);
        let out = convert(&src, &dir.join("job"), &AtomicBool::new(false)).expect("convert");
        pdf_ok(&out);
    }

    #[test]
    #[ignore]
    fn live_word_locked_file() {
        if busy(&["winword.exe"]) {
            return;
        }
        let _com = ComGuard::new();
        let dir = crate::phone::files::tests::temp_dir("sign-office-locked");
        let src = dir.join("secret.docx");
        make_docx(&src, Some("hunter2"));
        let r = convert(&src, &dir.join("job"), &AtomicBool::new(false));
        assert_eq!(r, Err(ConvertError::Locked));
    }

    #[test]
    #[ignore]
    fn live_excel_to_pdf() {
        if busy(&["excel.exe"]) {
            return;
        }
        let _com = ComGuard::new();
        let dir = crate::phone::files::tests::temp_dir("sign-office-excel");
        let src = dir.join("t.xlsx");
        make_xlsx(&src);
        let out = convert(&src, &dir.join("job"), &AtomicBool::new(false)).expect("convert");
        pdf_ok(&out);
    }

    #[test]
    #[ignore]
    fn live_powerpoint_to_pdf() {
        if busy(&["powerpnt.exe"]) {
            return;
        }
        let _com = ComGuard::new();
        let dir = crate::phone::files::tests::temp_dir("sign-office-ppt");
        let src = dir.join("t.pptx");
        make_pptx(&src);
        let out = convert(&src, &dir.join("job"), &AtomicBool::new(false)).expect("convert");
        pdf_ok(&out);
    }

    #[test]
    #[ignore]
    fn live_word_keeps_users_instance() {
        if busy(&["winword.exe"]) {
            return;
        }
        let _com = ComGuard::new();
        let dir = crate::phone::files::tests::temp_dir("sign-office-user");
        let src = dir.join("other.docx");
        // Stand in for the user: our own Word with a document open.
        let user = Session::start(&Suite::Microsoft, App::Writer).expect("word");
        let pid = user.ours.expect("we started it");
        // Hidden: a visible Word on an unlicensed PC opens a product key dialog
        // that blocks every call.
        user.app.put("Visible", vb(false)).unwrap();
        let docs = user.app.get_disp("Documents").unwrap();
        let _users_doc = docs.call_disp("Add", &[]).unwrap();
        let other = docs.call_disp("Add", &[]).unwrap();
        other.get_disp("Content").unwrap().put("Text", vs("Vale test")).unwrap();
        other.call("SaveAs2", &[vs(&src.to_string_lossy()), vi(16)]).unwrap();
        other.call("Close", &[vi(0)]).unwrap();
        let count = |d: &Disp| to_i32(&d.get("Count").unwrap()).unwrap();
        let before = count(&docs);
        assert_eq!(before, 1);

        let out = convert(&src, &dir.join("job"), &AtomicBool::new(false)).expect("convert");
        pdf_ok(&out);

        assert!(pids_of(&["winword.exe"]).contains(&pid), "the user's Word was closed");
        assert_eq!(count(&docs), before, "the user's documents changed");
        // Close the stand-in document so Quit does not wait on a save prompt.
        _users_doc.call("Close", &[vi(0)]).unwrap();
        // `user` drops here and quits the instance this test started.
    }
}
